//! Effector nodes — per-character transform modifiers for text.

pub mod definitions;
pub mod evaluator;

pub use evaluator::EffectorEvaluator;

use crate::plugin::node_types::NodeTypeDefinition;

/// All effector node definitions.
pub fn definitions() -> Vec<NodeTypeDefinition> {
    definitions::effector_nodes()
}
