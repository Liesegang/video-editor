use serde::{Deserialize, Serialize};
use uuid::Uuid;

// GUI-specific Clip representation (derived from TrackEntity for display)
// This struct holds display-only properties, actual data resides in Project
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineClip {
    pub id: Uuid,
    pub name: String,
    pub track_id: Uuid,
    pub in_frame: u64,
    pub out_frame: u64,
    pub timeline_duration_frames: u64,
    pub source_begin_frame: u64,
    pub duration_frame: Option<u64>,
    pub color: egui::Color32, // Changed from Color32 to egui::Color32 to match original context
    pub position: [f32; 2],
    pub scale: f32,
    pub opacity: f32,
    pub rotation: f32,
    pub asset_id: Option<Uuid>, // Changed from asset_index to asset_id
                                // Add other UI-specific properties here (is_selected, etc. if needed in future)
}

#[derive(Debug, Clone, PartialEq)]
pub enum DraggedItem {
    Asset(Uuid),
    Composition(Uuid),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tab {
    Preview,
    Timeline,
    Inspector,
    Assets,
    // Add other tabs as needed
}

impl Tab {
    pub fn all() -> &'static [Tab] {
        &[Tab::Preview, Tab::Timeline, Tab::Inspector, Tab::Assets]
    }

    pub fn name(&self) -> &'static str {
        match self {
            Tab::Preview => "Preview",
            Tab::Timeline => "Timeline",
            Tab::Inspector => "Inspector",
            Tab::Assets => "Assets",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompositionPreset {
    pub name: String,
    pub width: u64,
    pub height: u64,
    pub fps: f64,
}

impl CompositionPreset {
    /*pub fn resolution(&self) -> String {
        format!("{}x{}", self.width, self.height)
    }*/
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimelineDisplayMode {
    Seconds,
    Frames,
    SecondsAndFrames,
}
