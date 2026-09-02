use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Workspace {
    Beginner,
    #[default]
    Edit,
    Motion,
    Data,
    Logic,
    Diagnostics,
}

impl Workspace {
    pub const fn all() -> &'static [Self] {
        &[
            Self::Beginner,
            Self::Edit,
            Self::Motion,
            Self::Data,
            Self::Logic,
            Self::Diagnostics,
        ]
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Beginner => "Beginner",
            Self::Edit => "Edit",
            Self::Motion => "Motion",
            Self::Data => "Data",
            Self::Logic => "Logic",
            Self::Diagnostics => "Diagnostics",
        }
    }

    pub const fn depth(self) -> u8 {
        match self {
            Self::Beginner => 0,
            Self::Edit => 1,
            Self::Motion | Self::Data => 2,
            Self::Logic => 3,
            Self::Diagnostics => 4,
        }
    }
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

    pub fn all_for_workspace(workspace: Workspace) -> &'static [Tab] {
        match workspace {
            Workspace::Beginner => &[Tab::Preview, Tab::Timeline, Tab::Assets],
            Workspace::Edit | Workspace::Data => {
                &[Tab::Preview, Tab::Timeline, Tab::Inspector, Tab::Assets]
            }
            Workspace::Motion => &[
                Tab::Preview,
                Tab::Timeline,
                Tab::Inspector,
                Tab::Assets,
                Tab::GraphEditor,
            ],
            Workspace::Logic | Workspace::Diagnostics => Self::all(),
        }
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
        egui::Color32::from_rgba_premultiplied(def.0.0, def.0.1, def.0.2, def.0.3)
    }
}

// Serde helper for egui::Vec2
#[derive(Serialize, Deserialize)]
#[serde(remote = "egui::Vec2")]
pub struct Vec2Def {
    pub x: f32,
    pub y: f32,
}

// Serde helper for egui::Pos2
#[derive(Serialize, Deserialize)]
#[serde(remote = "egui::Pos2")]
pub struct Pos2Def {
    pub x: f32,
    pub y: f32,
}

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
