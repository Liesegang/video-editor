//! Evaluation engine — orchestrates pull-based node graph evaluation.

mod image_chain;

use std::sync::Arc;

use uuid::Uuid;

use super::context::{EvalContext, TrackEvaluator};
use super::evaluator::NodeEvaluator;
use crate::error::LibraryError;
use crate::plugin::PluginManager;
use crate::plugin::PropertyEvaluatorRegistry;
use crate::project::node::Node;
use crate::project::project::{Composition, Project};
use crate::project::source::SourceKind;
use crate::rendering::cache::CacheManager;
use crate::rendering::renderer::{RenderOutput, Renderer};
use crate::rendering::skia_renderer::SkiaRenderer;
use crate::runtime::frame::Region;

/// The evaluation engine holds all registered node evaluators and drives
/// the pull-based rendering pipeline.
pub struct EvalEngine {
    evaluators: Vec<Box<dyn NodeEvaluator>>,
}

impl TrackEvaluator for EvalEngine {
    fn evaluate_track(
        &self,
        track_id: Uuid,
        ctx: &mut EvalContext,
    ) -> Result<RenderOutput, LibraryError> {
        EvalEngine::evaluate_track(self, track_id, ctx)
    }
}

impl EvalEngine {
    pub fn new() -> Self {
        Self {
            evaluators: Vec::new(),
        }
    }

    /// Create an engine with all built-in evaluators registered.
    pub fn with_default_evaluators() -> Self {
        let mut engine = Self::new();
        for evaluator in crate::nodes::all_evaluators() {
            engine.register(evaluator);
        }
        engine
    }

    /// Register a node evaluator.
    pub fn register(&mut self, evaluator: Box<dyn NodeEvaluator>) {
        self.evaluators.push(evaluator);
    }

    /// Evaluate a composition, producing the final rendered output.
    ///
    /// This is the entry point for rendering a single frame.
    /// If a `compositing.preview_output` node exists in the project graph with a
    /// connected `image_in`, the engine pulls from it. Otherwise falls back to
    /// root track compositing.
    pub fn evaluate_composition(
        &self,
        project: &Project,
        composition: &Composition,
        plugin_manager: &PluginManager,
        renderer: &mut SkiaRenderer,
        cache_manager: &CacheManager,
        property_evaluators: Arc<PropertyEvaluatorRegistry>,
        frame_number: u64,
        render_scale: f64,
        region: Option<Region>,
    ) -> Result<RenderOutput, LibraryError> {
        let mut ctx = EvalContext::new(
            project,
            composition,
            plugin_manager,
            renderer,
            cache_manager,
            property_evaluators,
            &self.evaluators,
            self,
            frame_number,
            render_scale,
            region,
        );

        // Look for a preview output node in the project graph.
        if let Some(output_node_id) = Self::find_preview_output_node(project) {
            if let Some((source_id, source_pin)) = ctx.find_upstream(output_node_id, "image_in") {
                log::debug!(
                    "[EvalEngine] Pulling from preview output node {} via {}.{}",
                    output_node_id,
                    source_id,
                    source_pin
                );
                let value = ctx.evaluate_pin(source_id, &source_pin)?;
                return match value.into_image() {
                    Some(img) => {
                        let identity = crate::runtime::transform::Transform::default();
                        ctx.renderer.draw_layer(&img, &identity)?;
                        ctx.renderer.finalize()
                    }
                    None => {
                        log::warn!(
                            "[EvalEngine] Preview output node {} upstream returned None",
                            output_node_id
                        );
                        ctx.renderer.finalize()
                    }
                };
            }
            log::debug!(
                "[EvalEngine] Preview output node {} has no connected input, falling back to root track",
                output_node_id
            );
        }

        // Fallback: composite all direct children of the composition
        for child_id in &composition.child_ids {
            match ctx.project.get_node(*child_id).cloned() {
                Some(Node::Track(_)) | Some(Node::Layer(_)) => {
                    let sub_output = self.evaluate_track(*child_id, &mut ctx)?;
                    let identity = crate::runtime::transform::Transform::default();
                    ctx.renderer.draw_layer(&sub_output, &identity)?;
                }
                _ => {}
            }
        }
        ctx.renderer.finalize()
    }

    /// Find the first `compositing.preview_output` node in the project graph.
    fn find_preview_output_node(project: &Project) -> Option<Uuid> {
        project
            .all_graph_nodes()
            .find(|g| g.type_id == "compositing.preview_output")
            .map(|g| g.id)
    }

    /// Evaluate a track node, compositing its children onto an offscreen surface.
    ///
    /// For layer sub-tracks that have a connection to their `image_out` pin
    /// (e.g. `transform.image_out → layer.image_out`), the engine pulls from
    /// that connection instead of compositing children.
    ///
    /// For root tracks (no `image_out` connection), children are evaluated
    /// in order (painter's algorithm).
    fn evaluate_track(
        &self,
        track_id: Uuid,
        ctx: &mut EvalContext,
    ) -> Result<RenderOutput, LibraryError> {
        // Resolve child_ids, visible, name from either Track or Layer
        let (child_ids, visible, name) = if let Some(track) = ctx.project.get_track(track_id) {
            (track.child_ids.clone(), track.visible, track.name.clone())
        } else if let Some(layer) = ctx.project.get_layer(track_id) {
            (layer.child_ids.clone(), layer.visible, layer.name.clone())
        } else {
            return Err(LibraryError::render(format!(
                "Track/Layer not found: {}",
                track_id
            )));
        };

        if !visible {
            log::debug!("[EvalEngine] Track {} '{}' hidden, skip", track_id, name);
            return ctx.renderer.finalize();
        }

        // Layer container output: if there's a connection TO this track's image_out
        // (e.g. transform.image_out → layer.image_out), pull from the connected source.
        if let Some((source_id, source_pin)) = ctx.find_upstream(track_id, "image_out") {
            log::debug!(
                "[EvalEngine] Track {} '{}' pulling from {}.{}",
                track_id,
                name,
                source_id,
                source_pin
            );
            let value = ctx.evaluate_pin(source_id, &source_pin)?;
            return match value.into_image() {
                Some(img) => {
                    log::debug!("[EvalEngine] Track {} got image from upstream", track_id);
                    let identity = crate::runtime::transform::Transform::default();
                    ctx.renderer.draw_layer(&img, &identity)?;
                    ctx.renderer.finalize()
                }
                None => {
                    log::warn!("[EvalEngine] Track {} upstream returned None", track_id);
                    ctx.renderer.finalize()
                }
            };
        }

        // No connection → composite children (root track behavior)
        log::debug!(
            "[EvalEngine] Track {} '{}' compositing {} children (frame={})",
            track_id,
            name,
            child_ids.len(),
            ctx.frame_number
        );

        for child_id in &child_ids {
            match ctx.project.get_node(*child_id).cloned() {
                Some(Node::Source(clip)) => {
                    if clip.kind == SourceKind::Audio {
                        continue;
                    }
                    if ctx.frame_number < clip.in_frame || ctx.frame_number > clip.out_frame {
                        log::debug!(
                            "[EvalEngine] Clip {} ({:?}) out of range: frame={} clip=[{}..{}]",
                            child_id,
                            clip.kind,
                            ctx.frame_number,
                            clip.in_frame,
                            clip.out_frame
                        );
                        continue;
                    }

                    log::debug!(
                        "[EvalEngine] Evaluating clip {} ({:?}) [{}..{}]",
                        child_id,
                        clip.kind,
                        clip.in_frame,
                        clip.out_frame
                    );

                    let output = self.resolve_image_chain(*child_id, &clip.kind, ctx)?;
                    if let Some(image) = output {
                        log::debug!("[EvalEngine] Clip {} produced image", child_id);
                        let identity = crate::runtime::transform::Transform::default();
                        ctx.renderer.draw_layer(&image, &identity)?;
                    } else {
                        log::warn!("[EvalEngine] Clip {} produced no image", child_id);
                    }
                }
                Some(Node::Track(_)) => {
                    log::debug!("[EvalEngine] Evaluating sub-track {}", child_id);
                    let sub_output = self.evaluate_track(*child_id, ctx)?;
                    let identity = crate::runtime::transform::Transform::default();
                    ctx.renderer.draw_layer(&sub_output, &identity)?;
                }
                Some(Node::Layer(layer)) => {
                    // Check Layer timing before evaluating
                    if ctx.frame_number < layer.in_frame || ctx.frame_number > layer.out_frame {
                        log::debug!(
                            "[EvalEngine] Layer {} out of range: frame={} layer=[{}..{}]",
                            child_id,
                            ctx.frame_number,
                            layer.in_frame,
                            layer.out_frame
                        );
                        continue;
                    }
                    log::debug!(
                        "[EvalEngine] Evaluating layer {} [{}..{}]",
                        child_id,
                        layer.in_frame,
                        layer.out_frame
                    );
                    let sub_output = self.evaluate_track(*child_id, ctx)?;
                    let identity = crate::runtime::transform::Transform::default();
                    ctx.renderer.draw_layer(&sub_output, &identity)?;
                }
                Some(Node::Graph(g)) => {
                    log::trace!("[EvalEngine] Skip graph node {} ({})", child_id, g.type_id);
                }
                Some(Node::Composition(_)) => {
                    log::trace!("[EvalEngine] Skip composition node {}", child_id);
                }
                None => {
                    log::warn!("[EvalEngine] Child {} not found in nodes", child_id);
                }
            }
        }

        ctx.renderer.finalize()
    }
}
