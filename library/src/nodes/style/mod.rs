//! Style nodes — rasterize shapes with fill/stroke styles.

pub mod definitions;
pub mod evaluator;

pub use evaluator::StyleEvaluator;

use crate::plugin::node_types::NodeTypeDefinition;

/// All style node definitions.
pub fn definitions() -> Vec<NodeTypeDefinition> {
    definitions::style_nodes()
}
