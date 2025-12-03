mod frame;
mod property;

pub use frame::{evaluate_composition_frame, get_frame_from_project, FrameEvaluator};
pub use property::{EvaluationContext, PropertyEvaluator, PropertyEvaluatorRegistry};
