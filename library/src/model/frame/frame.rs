use crate::model::frame::color::Color;
use crate::model::frame::entity::FrameEntity;
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FrameInfo {
    pub width: u64,
    pub height: u64,
    pub background_color: Color,
    pub color_profile: String,
    pub objects: Vec<FrameEntity>,
}
