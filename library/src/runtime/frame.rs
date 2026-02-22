use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
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
