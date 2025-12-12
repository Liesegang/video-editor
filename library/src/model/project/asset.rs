use crate::model::frame::color::Color;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Asset {
    pub id: Uuid,
    pub name: String,
    pub path: String, // Path to the file
    pub kind: AssetKind,
    pub duration: Option<f64>, // Duration in seconds, if applicable

    // Metadata
    #[serde(default)]
    pub color: Color,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum AssetKind {
    Video,
    Audio,
    Image,
    Model3D,
    Other,
}

impl Asset {
    pub fn new(name: &str, path: &str, kind: AssetKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            path: path.to_string(),
            kind,
            duration: None,
            color: Color {
                r: 100,
                g: 100,
                b: 100,
                a: 255,
            }, // Default gray
        }
    }
}
