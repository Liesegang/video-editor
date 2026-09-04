use crate::model::frame::frame::FrameInfo;

pub mod color;
pub mod draw_type;
pub mod effect;
pub mod entity;
#[allow(
    clippy::module_inception,
    reason = "FrameInfo's established public path is model::frame::frame::FrameInfo"
)]
pub mod frame;
pub mod image;
pub mod particle;
pub mod runtime_shape;
pub mod transform;

pub use image::Image;

pub fn parse_frame_info(json_str: &str) -> Result<FrameInfo, serde_json::Error> {
    serde_json::from_str(json_str)
}
