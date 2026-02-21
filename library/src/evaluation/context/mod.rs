//! Evaluation context — carries all state needed during a single frame evaluation.

mod property_resolve;

use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;

use super::evaluator::NodeEvaluator;
use super::output::PinValue;
use crate::error::LibraryError;
use crate::plugin::PluginManager;
use crate::plugin::PropertyEvaluatorRegistry;
use crate::project::connection::PinId;
use crate::project::node::Node;
use crate::project::project::{Composition, Project};
use crate::rendering::cache::CacheManager;
use crate::rendering::skia_renderer::SkiaRenderer;
use crate::runtime::frame::Region;

/// Context for a single frame evaluation pass.
///
/// Created fresh for each frame. Provides pull-based input resolution,
/// property evaluation, and per-frame memoization of node outputs.
pub struct EvalContext<'a> {
    pub project: &'a Project,
    pub composition: &'a Composition,
    pub plugin_manager: &'a PluginManager,
    pub renderer: &'a mut SkiaRenderer,
    pub cache_manager: &'a CacheManager,
    pub property_evaluators: Arc<PropertyEvaluatorRegistry>,
    pub time: f64,
    pub frame_number: u64,
    pub render_scale: f64,
    pub region: Option<Region>,

    /// Reference to the registered evaluators for recursive dispatch.
    evaluators: &'a [Box<dyn NodeEvaluator>],

    /// Per-frame memoization cache: (node_id, pin_name) → evaluated value.
    node_cache: HashMap<(Uuid, String), PinValue>,
}

impl<'a> EvalContext<'a> {
    pub fn new(
        project: &'a Project,
        composition: &'a Composition,
        plugin_manager: &'a PluginManager,
        renderer: &'a mut SkiaRenderer,
        cache_manager: &'a CacheManager,
        property_evaluators: Arc<PropertyEvaluatorRegistry>,
        evaluators: &'a [Box<dyn NodeEvaluator>],
        frame_number: u64,
        render_scale: f64,
        region: Option<Region>,
    ) -> Self {
        let time = frame_number as f64 / composition.fps;
        Self {
            project,
            composition,
            plugin_manager,
            renderer,
            cache_manager,
            property_evaluators,
            evaluators,
            time,
            frame_number,
            render_scale,
            region,
            node_cache: HashMap::new(),
        }
    }

    /// Evaluate a specific node's output pin (with caching and dispatch).
    ///
    /// This is the core recursive dispatch method. Evaluators call this
    /// to pull values from upstream nodes.
    pub fn evaluate_pin(
        &mut self,
        node_id: Uuid,
        pin_name: &str,
    ) -> Result<PinValue, LibraryError> {
        // Check cache
        if let Some(cached) = self.node_cache.get(&(node_id, pin_name.to_string())) {
            return Ok(cached.clone());
        }

        let node = self
            .project
            .get_node(node_id)
            .ok_or_else(|| LibraryError::render(format!("Node not found: {}", node_id)))?
            .clone();

        // Copy the evaluators slice reference out of self so we can borrow self mutably
        let evaluators = self.evaluators;

        let result = match &node {
            Node::Clip(_clip) => {
                let evaluator = find_evaluator(evaluators, "clip.")
                    .ok_or_else(|| LibraryError::render("No clip evaluator registered"))?;
                evaluator.evaluate(node_id, pin_name, self)?
            }
            Node::Graph(graph_node) => {
                let evaluator =
                    find_evaluator(evaluators, &graph_node.type_id).ok_or_else(|| {
                        LibraryError::render(format!(
                            "No evaluator for node type: {}",
                            graph_node.type_id
                        ))
                    })?;
                evaluator.evaluate(node_id, pin_name, self)?
            }
            Node::Track(_) => {
                return Err(LibraryError::render(
                    "Track nodes should be evaluated via evaluate_track",
                ));
            }
        };

        // Cache the result
        self.node_cache
            .insert((node_id, pin_name.to_string()), result.clone());
        Ok(result)
    }

    /// Pull the evaluated value from an input pin by following connections backwards.
    ///
    /// Finds the connection where `to == (node_id, pin_name)`, evaluates the
    /// upstream node's output pin, and returns the result.
    /// Returns `PinValue::None` if the pin is unconnected.
    pub fn pull_input_value(
        &mut self,
        node_id: Uuid,
        pin_name: &str,
    ) -> Result<PinValue, LibraryError> {
        match self.find_upstream(node_id, pin_name) {
            Some((source_node_id, source_pin_name)) => {
                self.evaluate_pin(source_node_id, &source_pin_name)
            }
            None => Ok(PinValue::None),
        }
    }

    /// Find the upstream connection for an input pin (node_id, pin_name of source).
    pub fn find_upstream(&self, node_id: Uuid, pin_name: &str) -> Option<(Uuid, String)> {
        let target = PinId::new(node_id, pin_name);
        self.project
            .connections
            .iter()
            .find(|c| c.to == target)
            .map(|c| (c.from.node_id, c.from.pin_name.clone()))
    }

    /// Find all connections FROM a given pin (fan-out).
    pub fn find_downstream(&self, node_id: Uuid, pin_name: &str) -> Vec<(Uuid, String)> {
        let source = PinId::new(node_id, pin_name);
        self.project
            .connections
            .iter()
            .filter(|c| c.from == source)
            .map(|c| (c.to.node_id, c.to.pin_name.clone()))
            .collect()
    }
}

/// Find the evaluator that handles a given type_id (free function to avoid borrow conflicts).
fn find_evaluator<'a>(
    evaluators: &'a [Box<dyn NodeEvaluator>],
    type_id: &str,
) -> Option<&'a dyn NodeEvaluator> {
    evaluators
        .iter()
        .find(|e| e.handles().iter().any(|prefix| type_id.starts_with(prefix)))
        .map(|e| e.as_ref())
}
