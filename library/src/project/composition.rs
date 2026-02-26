use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::runtime::color::Color;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Composition {
    pub id: Uuid,
    pub name: String,
    pub width: u64,
    pub height: u64,
    pub fps: f64,
    pub duration: f64,
    pub background_color: Color,
    pub color_profile: String,
    #[serde(default)]
    pub work_area_in: u64,
    #[serde(default)]
    pub work_area_out: u64,
    /// Child nodes (tracks) owned by this composition.
    #[serde(default)]
    pub child_ids: Vec<Uuid>,
    /// Deprecated: kept for backward-compatible deserialization of old projects.
    /// On load, `Project::migrate()` converts this into `child_ids`.
    #[serde(default, skip_serializing)]
    pub root_track_id: Option<Uuid>,
}

impl Composition {
    /// Create a new empty composition.
    pub fn new(name: &str, width: u64, height: u64, fps: f64, duration: f64) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            width,
            height,
            fps,
            duration,
            background_color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            color_profile: "sRGB".to_string(),
            work_area_in: 0,
            work_area_out: (duration * fps).ceil() as u64,
            child_ids: Vec::new(),
            root_track_id: None,
        }
    }
}
