//! Hierarchical derived execution plan for Timeline authoring.
//!
//! The compiler never turns ordinary Timeline items, Tracks, or nested
//! Timelines into user-facing Nodes. Only explicit Module invocations enter a
//! compiled Node graph, and all instances share the same compiled definition.

mod cache;
mod compiler;
mod model;
mod particle;
mod runtime;

pub use cache::{RenderPlanCache, RenderPlanCacheStats};
pub use compiler::RenderPlanCompiler;
pub use model::*;
pub(crate) use runtime::time_map::map_composition_time;
pub use runtime::{
    evaluate_render_plan_frame, evaluate_timeline_render_plan_frame,
    evaluate_timeline_render_plan_frame_at_instance,
};

#[cfg(test)]
mod module_property_tests;
#[cfg(test)]
mod particle_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod transition_tests;
