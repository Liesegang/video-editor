//! Effect nodes — post-raster image effects (blur, glow, etc.).

pub mod definitions;
pub mod evaluator;

pub use evaluator::EffectEvaluator;

use crate::plugin::node_types::NodeTypeDefinition;

/// All effect/filter node definitions.
pub fn definitions() -> Vec<NodeTypeDefinition> {
    definitions::filter_nodes()
}
