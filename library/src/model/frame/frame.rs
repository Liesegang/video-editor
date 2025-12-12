use crate::model::frame::color::Color;
use crate::model::frame::entity::FrameObject;
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
pub struct FrameInfo {
    pub width: u64,
    pub height: u64,
    pub background_color: Color,
    pub color_profile: String,
    pub render_scale: ordered_float::OrderedFloat<f64>,
    pub now_time: ordered_float::OrderedFloat<f64>,
    pub objects: Vec<FrameObject>,
}
