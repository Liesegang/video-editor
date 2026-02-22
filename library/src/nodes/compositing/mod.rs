//! Compositing nodes — transform, blend, composite, mask.

pub mod blend;
pub mod definitions;
pub mod transform;

pub use blend::BlendEvaluator;
pub use transform::TransformEvaluator;

use crate::plugin::node_types::NodeTypeDefinition;

/// All compositing node definitions.
pub fn definitions() -> Vec<NodeTypeDefinition> {
    definitions::compositing_nodes()
}
