use crate::runtime::color::Color;
use crate::runtime::entity::FrameObject;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct FrameInfo {
    pub width: u64,
    pub height: u64,
    pub background_color: Color,
    pub color_profile: String,
    pub render_scale: ordered_float::OrderedFloat<f64>,
    pub now_time: ordered_float::OrderedFloat<f64>,
    pub region: Option<Region>,
    pub objects: Vec<FrameObject>,
}

// Implement Hash manually for Region since f64 doesn't implement Hash
impl std::hash::Hash for Region {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        ordered_float::OrderedFloat(self.x).hash(state);
        ordered_float::OrderedFloat(self.y).hash(state);
        ordered_float::OrderedFloat(self.width).hash(state);
        ordered_float::OrderedFloat(self.height).hash(state);
    }
}

// Implement Hash for FrameInfo
impl std::hash::Hash for FrameInfo {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.width.hash(state);
        self.height.hash(state);
        self.background_color.hash(state);
        self.color_profile.hash(state);
        self.render_scale.hash(state);
        self.now_time.hash(state);
        self.region.hash(state);
        self.objects.hash(state);
    }
}

impl Eq for FrameInfo {}
