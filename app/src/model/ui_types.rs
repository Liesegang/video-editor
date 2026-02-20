use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum GizmoHandle {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,
    Bottom,
    Left,
    Right,
    Rotation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DraggedItem {
    Asset(Uuid),
    Composition(Uuid),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tab {
    Preview,
    Timeline,
    Inspector,
    Assets,
    GraphEditor,
    NodeGraph,
    // Add other tabs as needed
}

impl Tab {
    pub(crate) fn all() -> &'static [Tab] {
        &[
            Tab::Preview,
            Tab::Timeline,
            Tab::Inspector,
            Tab::Assets,
            Tab::GraphEditor,
            Tab::NodeGraph,
        ]
    }

    pub(crate) fn name(&self) -> &'static str {
        match self {
            Tab::Preview => "Preview",
            Tab::Timeline => "Timeline",
            Tab::Inspector => "Inspector",
            Tab::Assets => "Assets",
            Tab::GraphEditor => "Graph Editor",
            Tab::NodeGraph => "Node Editor",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CompositionPreset {
    pub(crate) name: String,
    pub(crate) width: u64,
    pub(crate) height: u64,
    pub(crate) fps: f64,
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
pub(crate) enum TimelineDisplayMode {
    Seconds,
    Frames,
    SecondsAndFrames,
}

use library::plugin::PluginCategory;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SettingsTab {
    Shortcuts,
    PluginPaths,
    PluginList(PluginCategory, Option<String>), // Category, Type Filter
    Theme,
}
