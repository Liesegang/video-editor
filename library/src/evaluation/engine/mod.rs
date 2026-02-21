//! Evaluation engine — orchestrates pull-based node graph evaluation.

mod image_chain;

use std::sync::Arc;

use uuid::Uuid;

use super::context::EvalContext;
use super::evaluator::NodeEvaluator;
use crate::error::LibraryError;
use crate::plugin::PluginManager;
use crate::plugin::PropertyEvaluatorRegistry;
use crate::project::clip::TrackClipKind;
use crate::project::node::Node;
use crate::project::project::{Composition, Project};
use crate::rendering::cache::CacheManager;
use crate::rendering::renderer::{RenderOutput, Renderer};
use crate::rendering::skia_renderer::SkiaRenderer;
use crate::runtime::frame::Region;

/// The evaluation engine holds all registered node evaluators and drives
/// the pull-based rendering pipeline.
pub struct EvalEngine {
    evaluators: Vec<Box<dyn NodeEvaluator>>,
}

impl EvalEngine {
    pub fn new() -> Self {
        Self {
            evaluators: Vec::new(),
        }
    }

    /// Create an engine with all built-in evaluators registered.
    pub fn with_default_evaluators() -> Self {
        use super::evaluators::clip::ClipEvaluator;
        use super::evaluators::decorator::DecoratorEvaluator;
        use super::evaluators::effect::EffectEvaluator;
        use super::evaluators::effector::EffectorEvaluator;
        use super::evaluators::style::StyleEvaluator;
        use super::evaluators::transform::TransformEvaluator;

        let mut engine = Self::new();
        engine.register(Box::new(ClipEvaluator));
        engine.register(Box::new(EffectEvaluator));
        engine.register(Box::new(StyleEvaluator));
        engine.register(Box::new(EffectorEvaluator));
        engine.register(Box::new(DecoratorEvaluator));
        engine.register(Box::new(TransformEvaluator));
        engine
    }

    /// Register a node evaluator.
    pub fn register(&mut self, evaluator: Box<dyn NodeEvaluator>) {
        self.evaluators.push(evaluator);
    }

    /// Evaluate a composition, producing the final rendered output.
    ///
    /// This is the entry point for rendering a single frame.
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
            frame_number,
            render_scale,
            region,
        );

        self.evaluate_track(composition.root_track_id, &mut ctx)
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
        let track = ctx
            .project
            .get_track(track_id)
            .ok_or_else(|| LibraryError::render(format!("Track not found: {}", track_id)))?
            .clone();

        if !track.visible {
            return ctx.renderer.finalize();
        }

        // Layer container output: if there's a connection TO this track's image_out
        // (e.g. transform.image_out → layer.image_out), pull from the connected source.
        if let Some((source_id, source_pin)) = ctx.find_upstream(track_id, "image_out") {
            let value = ctx.evaluate_pin(source_id, &source_pin)?;
            return match value.into_image() {
                Some(img) => {
                    let identity = crate::runtime::transform::Transform::default();
                    ctx.renderer.draw_layer(&img, &identity)?;
                    ctx.renderer.finalize()
                }
                None => ctx.renderer.finalize(),
            };
        }

        // No connection → composite children (root track behavior)
        let child_ids = track.child_ids.clone();

        for child_id in &child_ids {
            match ctx.project.get_node(*child_id).cloned() {
                Some(Node::Clip(clip)) => {
                    // Skip audio clips and clips outside the current frame range
                    if clip.kind == TrackClipKind::Audio {
                        continue;
                    }
                    if ctx.frame_number < clip.in_frame || ctx.frame_number > clip.out_frame {
                        continue;
                    }

                    // Evaluate the clip's primary output, then follow the image chain
                    // (clip → fill → transform → effects) via downstream connections.
                    let output = self.resolve_image_chain(*child_id, &clip.kind, ctx)?;
                    if let Some(image) = output {
                        let identity = crate::runtime::transform::Transform::default();
                        ctx.renderer.draw_layer(&image, &identity)?;
                    }
                }
                Some(Node::Track(_)) => {
                    // Recursive track evaluation
                    let sub_output = self.evaluate_track(*child_id, ctx)?;
                    // TODO: Apply track.blend_mode and track.opacity when compositing
                    let identity = crate::runtime::transform::Transform::default();
                    ctx.renderer.draw_layer(&sub_output, &identity)?;
                }
                Some(Node::Graph(_)) => {
                    // Graph nodes in a track's child_ids are pulled by their connected clips,
                    // not evaluated independently during track traversal.
                }
                None => {}
            }
        }

        ctx.renderer.finalize()
    }
}
