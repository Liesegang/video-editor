//! UI state for the node editor.

use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// UI state for the node editor panel.
#[derive(Default)]
pub struct NodeEditorState {
    /// Pan offset in screen pixels.
    pub pan: egui::Vec2,
    /// Zoom level (1.0 = 100%).
    pub zoom: f32,
    /// Node positions in graph space.
    pub node_positions: HashMap<Uuid, egui::Pos2>,
    /// Currently selected nodes.
    pub selected_nodes: HashSet<Uuid>,
    /// Currently selected connections.
    pub selected_connections: HashSet<Uuid>,
    /// Drag state for nodes.
    pub dragging: Option<DragState>,
    /// Connection creation state.
    pub connecting: Option<ConnectingState>,
    /// Context menu state.
    pub context_menu: Option<ContextMenuState>,
    /// Current container being viewed.
    pub current_container: Option<Uuid>,
    /// Containers expanded inline.
    pub expanded_containers: HashSet<Uuid>,
    /// Search text for context menu.
    pub context_search: String,
}

pub struct DragState {
    pub node_ids: Vec<Uuid>,
    pub start_positions: Vec<egui::Pos2>,
    pub mouse_start: egui::Pos2,
}

pub struct ConnectingState {
    pub from_node: Uuid,
    pub from_pin: String,
    pub is_output: bool,
    pub mouse_pos: egui::Pos2,
}

#[derive(Clone)]
pub struct ContextMenuState {
    pub screen_pos: egui::Pos2,
    pub container_id: Uuid,
}
