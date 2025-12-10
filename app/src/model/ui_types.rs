use serde::{Deserialize, Serialize};
use uuid::Uuid;

// GUI-specific Clip representation (derived from TrackEntity for display)
// This struct holds display-only properties, actual data resides in Project
#[derive(Debug, Clone, PartialEq)]
pub struct GuiClip {
    pub id: Uuid,
    pub name: String,
    pub track_id: Uuid,
    pub start_time: f32,
    pub duration: f32,
    pub color: egui::Color32,
    pub position: [f32; 2],
    pub scale: f32,
    pub opacity: f32,
    pub rotation: f32,
    pub asset_index: usize, // To link back to local assets for display info
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tab {
    Preview,
    Timeline,
    Inspector,
    Assets,
    // Add other tabs as needed
}

// Serde helper for egui::Color32
#[derive(Serialize, Deserialize)]
#[serde(remote = "egui::Color32")]
pub struct ColorDef(#[serde(getter = "get_color_tuple")] (u8, u8, u8, u8));
fn get_color_tuple(color: &egui::Color32) -> (u8, u8, u8, u8) {
    color.to_tuple()
}
impl From<ColorDef> for egui::Color32 {
    fn from(def: ColorDef) -> egui::Color32 {
        egui::Color32::from_rgba_premultiplied(def.0 .0, def.0 .1, def.0 .2, def.0 .3)
    }
}

// Serde helper for egui::Vec2
#[derive(Serialize, Deserialize)]
#[serde(remote = "egui::Vec2")]
pub struct Vec2Def {
    pub x: f32,
    pub y: f32,
}