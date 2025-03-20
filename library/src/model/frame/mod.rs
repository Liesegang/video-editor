use crate::model::frame::frame::FrameInfo;

pub mod color;
pub mod draw_type;
pub mod entity;
pub mod frame;
pub mod transform;

pub fn parse_frame_info(json_str: &str) -> Result<FrameInfo, serde_json::Error> {
    serde_json::from_str(json_str)
}
