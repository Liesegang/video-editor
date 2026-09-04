use serde::{Deserialize, Serialize};

/// Direct-manipulation handles shared by Preview painting, hit testing, and
/// gesture state. These are presentation identities, never Project data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GizmoHandle {
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
    Rotation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tab {
    Preview,
    Timeline,
    Inspector,
    Assets,
    CurveEditor,
    NodeEditor,
}

impl Tab {
    pub fn all() -> &'static [Tab] {
        &[
            Tab::Preview,
            Tab::Timeline,
            Tab::Inspector,
            Tab::Assets,
            Tab::CurveEditor,
            Tab::NodeEditor,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            Tab::Preview => "Preview",
            Tab::Timeline => "Timeline",
            Tab::Inspector => "Inspector",
            Tab::Assets => "Assets",
            Tab::CurveEditor => "Curve Editor",
            Tab::NodeEditor => "Node Editor",
        }
    }
}

use library::plugin::PluginCategory;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsTab {
    Shortcuts,
    PluginPaths,
    PluginList(PluginCategory, Option<String>), // Category, Type Filter
    Theme,
}
