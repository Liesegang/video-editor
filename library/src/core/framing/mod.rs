#[path = "frame/authoring_frame.rs"]
mod authoring_frame;

pub use authoring_frame::{evaluate_authoring_frame, evaluate_authoring_timeline_frame};

#[cfg(test)]
mod frame;

#[cfg(test)]
pub(crate) use frame::{
    FrameEvaluator, InputValuePreview, evaluate_composition_frame, get_frame_from_project,
};
