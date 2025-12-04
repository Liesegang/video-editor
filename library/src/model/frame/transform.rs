use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize, Debug, Clone)]
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

impl Default for Position {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Scale {
    pub x: f64,
    pub y: f64,
}

impl Default for Scale {
    fn default() -> Self {
        Self { x: 1.0, y: 1.0 }
    }
}
