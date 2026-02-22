//! Decorator nodes — add decoration shapes to text (backplates, etc.).

pub mod definitions;
pub mod evaluator;
pub mod svg_builder;

pub use evaluator::DecoratorEvaluator;

use crate::plugin::node_types::NodeTypeDefinition;

/// All decorator node definitions.
pub fn definitions() -> Vec<NodeTypeDefinition> {
    definitions::decorator_nodes()
}
