//! Compositing nodes — transform, blend, composite, mask, preview output.

pub mod blend;
pub mod definitions;
pub mod preview_output;
pub mod transform;

pub use blend::BlendEvaluator;
pub use preview_output::PreviewOutputEvaluator;
pub use transform::TransformEvaluator;

use crate::plugin::node_types::NodeTypeDefinition;

/// All compositing node definitions.
pub fn definitions() -> Vec<NodeTypeDefinition> {
    definitions::compositing_nodes()
}
