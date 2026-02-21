use crate::runtime::frame::FrameInfo;

pub mod color;
pub mod draw_type;
pub mod effect;
pub mod entity;
pub mod frame;
pub mod image;
pub mod transform;

pub use image::Image;

pub fn parse_frame_info(json_str: &str) -> Result<FrameInfo, serde_json::Error> {
    serde_json::from_str(json_str)
}
