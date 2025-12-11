use serde::{Deserialize, Serialize};
use ordered_float::OrderedFloat;
use std::hash::{Hash, Hasher};
#[derive(Serialize, Deserialize, Debug, Clone)] // Removed PartialEq, Eq (manual impl below)
pub struct Transform {
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub scale: Scale,
    #[serde(default)]
    pub anchor: Position,
    #[serde(default)]

    pub rotation: f64,
}

impl Hash for Transform {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.position.hash(state);
        self.scale.hash(state);
        self.anchor.hash(state);
        OrderedFloat(self.rotation).hash(state);
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: Default::default(),
            scale: Default::default(),
            anchor: Default::default(),
            rotation: 0.0,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)] // Removed PartialEq, Eq
pub struct Position {
    pub x: f64,
    pub y: f64,
}

impl Hash for Position {
    fn hash<H: Hasher>(&self, state: &mut H) {
        OrderedFloat(self.x).hash(state);
        OrderedFloat(self.y).hash(state);
    }
}

impl Default for Position {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)] // Removed PartialEq, Eq
pub struct Scale {
    pub x: f64,
    pub y: f64,
}

impl Hash for Scale {
    fn hash<H: Hasher>(&self, state: &mut H) {
        OrderedFloat(self.x).hash(state);
        OrderedFloat(self.y).hash(state);
    }
}

impl Default for Scale {
    fn default() -> Self {
        Self { x: 1.0, y: 1.0 }
    }
}

impl PartialEq for Transform {
    fn eq(&self, other: &Self) -> bool {
        self.position == other.position && 
        self.scale == other.scale && 
        OrderedFloat(self.rotation) == OrderedFloat(other.rotation) && 
        self.anchor == other.anchor
    }
}
impl Eq for Transform {}

impl PartialEq for Position {
    fn eq(&self, other: &Self) -> bool {
        OrderedFloat(self.x) == OrderedFloat(other.x) && OrderedFloat(self.y) == OrderedFloat(other.y)
    }
}
impl Eq for Position {}

impl PartialEq for Scale {
    fn eq(&self, other: &Self) -> bool {
        OrderedFloat(self.x) == OrderedFloat(other.x) && OrderedFloat(self.y) == OrderedFloat(other.y)
    }
}
impl Eq for Scale {}
