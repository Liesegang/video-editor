use crate::model::frame::color::Color;
use crate::model::frame::entity::FrameObject;
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct FrameInfo {
    pub width: u64,
    pub height: u64,
    pub background_color: Color,
    pub color_profile: String,
    pub objects: Vec<FrameObject>,
}
