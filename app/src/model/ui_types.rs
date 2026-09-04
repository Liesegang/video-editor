#![cfg_attr(
    test,
    allow(
        dead_code,
        reason = "recovery-tag UI types remain available only to focused serialization tests"
    )
)]

use serde::{Deserialize, Serialize};

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GizmoHandle {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tab {
    Preview,
    Timeline,
    Inspector,
    Assets,
    GraphEditor,
    NodeEditor,
    // Add other tabs as needed
}

impl Tab {
    pub fn all() -> &'static [Tab] {
        &[
            Tab::Preview,
            Tab::Timeline,
            Tab::Inspector,
            Tab::Assets,
            Tab::GraphEditor,
            Tab::NodeEditor,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            Tab::Preview => "Preview",
            Tab::Timeline => "Timeline",
            Tab::Inspector => "Inspector",
            Tab::Assets => "Assets",
            Tab::GraphEditor => "Graph Editor",
            Tab::NodeEditor => "Node Editor",
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub struct CompositionPreset {
    pub name: String,
    pub width: u64,
    pub height: u64,
    pub fps: f64,
}

#[cfg(test)]
impl CompositionPreset {
    /*pub fn resolution(&self) -> String {
        format!("{}x{}", self.width, self.height)
    }*/
}

// Serde helper for egui::Color32
#[cfg(test)]
#[derive(Serialize, Deserialize)]
#[serde(remote = "egui::Color32")]
pub struct ColorDef(#[serde(getter = "get_color_tuple")] (u8, u8, u8, u8));
#[cfg(test)]
fn get_color_tuple(color: &egui::Color32) -> (u8, u8, u8, u8) {
    color.to_tuple()
}
#[cfg(test)]
impl From<ColorDef> for egui::Color32 {
    fn from(def: ColorDef) -> egui::Color32 {
        egui::Color32::from_rgba_premultiplied(def.0 .0, def.0 .1, def.0 .2, def.0 .3)
    }
}

// Serde helper for egui::Vec2
#[cfg(test)]
#[derive(Serialize, Deserialize)]
#[serde(remote = "egui::Vec2")]
pub struct Vec2Def {
    pub x: f32,
    pub y: f32,
}

// Serde helper for egui::Pos2
#[cfg(test)]
#[derive(Serialize, Deserialize)]
#[serde(remote = "egui::Pos2")]
pub struct Pos2Def {
    pub x: f32,
    pub y: f32,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimelineDisplayMode {
    Seconds,
    Frames,
    SecondsAndFrames,
}

use library::plugin::PluginCategory;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsTab {
    Shortcuts,
    PluginPaths,
    PluginList(PluginCategory, Option<String>), // Category, Type Filter
    Theme,
}
