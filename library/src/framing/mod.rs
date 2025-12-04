mod frame;
mod property;

pub use frame::{FrameEvaluator, evaluate_composition_frame, get_frame_from_project};
pub use property::{EvaluationContext, PropertyEvaluator, PropertyEvaluatorRegistry};
