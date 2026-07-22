//! Transient state for one Node Editor directional-layout gesture.
//!
//! This module stores only a frozen UI projection and sparse preview
//! positions. The authoritative graph and positions remain in `Project`.

use std::collections::BTreeMap;

use library::model::NodeContainer;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DirectionalLayoutGestureMode {
    Layout,
    Align,
    Distribute,
    AlignAndDistribute,
}

impl DirectionalLayoutGestureMode {
    pub(crate) const fn from_modifiers(modifiers: egui::Modifiers) -> Self {
        match (modifiers.shift, modifiers.alt) {
            (false, false) => Self::Layout,
            (true, false) => Self::Align,
            (false, true) => Self::Distribute,
            (true, true) => Self::AlignAndDistribute,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Layout => "layout",
            Self::Align => "align",
            Self::Distribute => "distribute",
            Self::AlignAndDistribute => "align_and_distribute",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DirectionalLayoutGestureDirection {
    Upstream,
    Downstream,
}

impl DirectionalLayoutGestureDirection {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Upstream => "upstream",
            Self::Downstream => "downstream",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FrozenNodeGeometry {
    /// Exact outer rectangle reported by Snarl at pointer press.
    pub(crate) rect: egui::Rect,
    /// Difference between Snarl's outer top-left and persisted node position.
    pub(crate) render_offset: egui::Vec2,
    pub(crate) measured: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DirectionalLayoutGestureDiagnostics {
    pub(crate) reachable_node_ids: Vec<Uuid>,
    pub(crate) eligible_node_ids: Vec<Uuid>,
    pub(crate) moved_node_ids: Vec<Uuid>,
    pub(crate) blocked_node_ids: Vec<Uuid>,
}

#[derive(Clone, Debug)]
pub(crate) struct NodeEditorDirectionalLayoutGesture {
    pub(crate) gesture_id: u64,
    pub(crate) composition_id: Uuid,
    pub(crate) direct_owner: NodeContainer,
    pub(crate) anchor_node_id: Uuid,
    pub(crate) frozen_selected_node_ids: Vec<Uuid>,
    pub(crate) baseline_positions: BTreeMap<Uuid, [f32; 2]>,
    pub(crate) frozen_geometry: BTreeMap<Uuid, FrozenNodeGeometry>,
    /// Sparse persisted-position projection painted by Snarl during preview.
    pub(crate) preview_positions: BTreeMap<Uuid, [f32; 2]>,
    pub(crate) start: egui::Pos2,
    pub(crate) current: egui::Pos2,
    pub(crate) axis: Option<node_editor_ui::LayoutSwipeAxis>,
    pub(crate) direction: Option<DirectionalLayoutGestureDirection>,
    pub(crate) mode: DirectionalLayoutGestureMode,
    pub(crate) modifiers: egui::Modifiers,
    pub(crate) canvas_transform: egui::emath::TSTransform,
    /// Canonical SHA-256 of the authoritative Project at gesture start.
    pub(crate) project_revision: String,
    pub(crate) history_undo_depth: usize,
    pub(crate) history_redo_depth: usize,
    pub(crate) diagnostics: DirectionalLayoutGestureDiagnostics,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DirectionalLayoutGestureOutcome {
    Committed,
    Cancelled,
    Rejected,
}

impl DirectionalLayoutGestureOutcome {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Committed => "committed",
            Self::Cancelled => "cancelled",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct NodeEditorDirectionalLayoutExecution {
    pub(crate) gesture_id: u64,
    pub(crate) outcome: DirectionalLayoutGestureOutcome,
    pub(crate) reason: Option<String>,
    pub(crate) composition_id: Uuid,
    pub(crate) direct_owner: NodeContainer,
    pub(crate) anchor_node_id: Uuid,
    pub(crate) axis: Option<node_editor_ui::LayoutSwipeAxis>,
    pub(crate) direction: Option<DirectionalLayoutGestureDirection>,
    pub(crate) mode: DirectionalLayoutGestureMode,
    pub(crate) moved_node_ids: Vec<Uuid>,
    pub(crate) project_revision_before: String,
    pub(crate) project_revision_after: String,
    pub(crate) history_undo_before: usize,
    pub(crate) history_undo_after: usize,
    pub(crate) history_redo_before: usize,
    pub(crate) history_redo_after: usize,
}
