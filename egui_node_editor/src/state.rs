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
    /// Context menu state (right-click on empty space).
    pub context_menu: Option<ContextMenuState>,
    /// Node-specific context menu (right-click on a node).
    pub node_context_menu: Option<NodeContextMenuState>,
    /// Current container being viewed.
    pub current_container: Option<Uuid>,
    /// Containers expanded inline.
    pub expanded_containers: HashSet<Uuid>,
    /// Search text for context menu.
    pub context_search: String,
    /// Box selection state.
    pub box_selecting: Option<BoxSelectState>,
    /// Custom container sizes (overrides auto-calculated size).
    pub container_sizes: HashMap<Uuid, egui::Vec2>,
    /// Resize handle drag state.
    pub resizing: Option<ResizeState>,
    /// Edge-specific context menu (right-click on a connection).
    pub edge_context_menu: Option<EdgeContextMenuState>,
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

#[derive(Clone)]
pub struct NodeContextMenuState {
    pub screen_pos: egui::Pos2,
    pub node_id: Uuid,
}

pub struct BoxSelectState {
    pub start: egui::Pos2,
    pub current: egui::Pos2,
}

pub struct ResizeState {
    pub node_id: Uuid,
    pub start_size: egui::Vec2,
    pub mouse_start: egui::Pos2,
}

/// Context menu for right-clicking on an edge (connection).
#[derive(Clone)]
pub struct EdgeContextMenuState {
    pub screen_pos: egui::Pos2,
    pub connection_id: Uuid,
}
