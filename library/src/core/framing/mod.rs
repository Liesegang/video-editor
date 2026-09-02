#[path = "frame/authoring_frame.rs"]
mod authoring_frame;

pub use authoring_frame::{
    evaluate_authoring_frame, evaluate_authoring_timeline_frame,
    evaluate_authoring_timeline_frame_with_signals,
};
