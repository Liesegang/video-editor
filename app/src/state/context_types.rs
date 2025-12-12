use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::ui_types::{DraggedItem, GizmoHandle, TimelineDisplayMode, Vec2Def};

#[derive(Serialize, Deserialize, Clone)]
pub struct TimelineState {
    pub current_time: f32,
    pub is_playing: bool,
    pub pixels_per_second: f32,
    pub display_mode: TimelineDisplayMode,
    pub v_zoom: f32,
    pub h_zoom: f32,
    #[serde(skip)]
    pub scroll_offset: egui::Vec2,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            current_time: 0.0,
            is_playing: false,
            pixels_per_second: 50.0,
            display_mode: TimelineDisplayMode::Seconds,
            v_zoom: 1.0,
            h_zoom: 1.0,
            scroll_offset: egui::Vec2::ZERO,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ViewState {
    #[serde(with = "Vec2Def")]
    pub pan: egui::Vec2,
    pub zoom: f32,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            pan: egui::vec2(20.0, 20.0),
            zoom: 0.3,
        }
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct SelectionState {
    pub composition_id: Option<Uuid>,
    pub track_id: Option<Uuid>,
    pub entity_id: Option<Uuid>,
}

#[derive(Default, Clone)]
pub struct InteractionState {
    pub dragged_item: Option<DraggedItem>,
    pub asset_delete_candidate: Option<Uuid>,
    pub comp_delete_candidate: Option<Uuid>,
    pub active_modal_error: Option<String>,

    // Drag/Drop specifics
    pub dragged_entity_original_track_id: Option<Uuid>,
    pub dragged_entity_hovered_track_id: Option<Uuid>,
    pub dragged_entity_has_moved: bool,

    // Manipulation
    pub is_resizing_entity: bool,
    pub is_moving_selected_entity: bool,

    // We can't import GizmoState here easily if it depends on something else or circular dep,
    // but GizmoState is defined in context.rs.
    // Ideally we should move GizmoState here or to a separate file.
    // For now, let's assume we will move GizmoState here or import it.
    // Based on previous file read, GizmoState is in context.rs.
    // I will MOVE GizmoState to this file to avoid circular dependency.
    pub gizmo_state: Option<GizmoState>,

    // Text Input
    pub current_time_text_input: String,
    pub is_editing_current_time: bool,
}

#[derive(Debug, Clone)]
pub struct GizmoState {
    pub start_mouse_pos: egui::Pos2,
    pub active_handle: GizmoHandle,
    pub original_position: [f32; 2],
    pub original_scale_x: f32,
    pub original_scale_y: f32,
    pub original_rotation: f32,
    pub original_anchor_x: f32,
    pub original_anchor_y: f32,
    pub original_width: f32,
    pub original_height: f32,
}
