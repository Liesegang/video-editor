use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::ui_types::{DraggedItem, GizmoHandle, TimelineDisplayMode, Vec2Def};
use crate::model::vector::VectorEditorState;

use library::animation::EasingFunction; // Added import

#[derive(Clone, Serialize, Deserialize)]
pub struct KeyframeDialogState {
    pub is_open: bool,
    pub track_id: Option<Uuid>,
    pub entity_id: Option<Uuid>,
    pub property_name: String,
    pub keyframe_index: usize,
    pub time: f64,
    pub value: f64,
    pub easing: EasingFunction,
}

impl Default for KeyframeDialogState {
    fn default() -> Self {
        Self {
            is_open: false,
            track_id: None,
            entity_id: None,
            property_name: String::new(),
            keyframe_index: 0,
            time: 0.0,
            value: 0.0,
            easing: EasingFunction::Linear,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TimelineState {
    pub current_time: f32,
    pub is_playing: bool,
    pub pixels_per_second: f32,
    pub display_mode: TimelineDisplayMode,
    pub v_zoom: f32,
    pub h_zoom: f32,
    #[serde(skip)]
    pub playback_accumulator: f32,
    #[serde(skip)]
    pub scroll_offset: egui::Vec2,
    #[serde(default)]
    pub expanded_tracks: HashSet<Uuid>,
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
            playback_accumulator: 0.0,
            scroll_offset: egui::Vec2::ZERO,
            expanded_tracks: HashSet::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum PreviewTool {
    Select,
    Pan,
    Zoom,
    Text,
    Shape,
}

impl Default for PreviewTool {
    fn default() -> Self {
        Self::Select
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ViewState {
    #[serde(with = "Vec2Def")]
    pub pan: egui::Vec2,
    pub zoom: f32,
    #[serde(default = "default_preview_resolution")]
    pub preview_resolution: f32,
    #[serde(default)]
    pub active_tool: PreviewTool,
}

fn default_preview_resolution() -> f32 {
    1.0
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            pan: egui::vec2(20.0, 20.0),
            zoom: 0.3,
            preview_resolution: 1.0,
            active_tool: PreviewTool::default(),
        }
    }
}

// Added GraphEditorState
#[derive(Serialize, Deserialize, Clone)]
pub struct GraphEditorState {
    #[serde(with = "Vec2Def")]
    pub pan: egui::Vec2, // Pan offset
    pub zoom_x: f32, // Pixels per second
    pub zoom_y: f32, // Pixels per unit value
    #[serde(default)]
    pub visible_properties: HashSet<String>,
}

impl Default for GraphEditorState {
    fn default() -> Self {
        Self {
            pan: egui::Vec2::ZERO,
            zoom_x: 100.0, // Default 100 pixels per second
            zoom_y: 1.0,   // Default 1 pixel per unit
            visible_properties: HashSet::new(),
        }
    }
}

use std::collections::HashSet;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct SelectionState {
    pub composition_id: Option<Uuid>,
    pub selected_entities: HashSet<Uuid>,
    pub last_selected_entity_id: Option<Uuid>,
    pub last_selected_track_id: Option<Uuid>,
}

// --- Sub-states split by panel responsibility ---

/// Preview panel interaction state (gizmo, vector editor, text editing, selection drag)
#[derive(Default, Clone)]
pub struct PreviewInteractionState {
    pub gizmo_state: Option<GizmoState>,
    pub vector_editor_state: Option<VectorEditorState>,
    pub body_drag_state: Option<BodyDragState>,
    pub preview_selection_drag_start: Option<egui::Pos2>,
    pub handled_hand_tool_drag: bool,
    pub bounds_cache: BoundsCache,
    pub editing_text_entity_id: Option<uuid::Uuid>,
    pub text_edit_buffer: String,
    pub is_moving_selected_entity: bool,
}

/// Timeline panel interaction state (clip drag/drop, resize, selection, track rename)
#[derive(Default, Clone)]
pub struct TimelineInteractionState {
    pub dragged_item: Option<DraggedItem>,
    pub dragged_entity_original_track_id: Option<Uuid>,
    pub dragged_entity_hovered_track_id: Option<Uuid>,
    pub dragged_entity_has_moved: bool,
    pub is_resizing_entity: bool,
    pub timeline_selection_drag_start: Option<egui::Pos2>,
    pub current_time_text_input: String,
    pub is_editing_current_time: bool,
    pub context_menu_open_pos: Option<egui::Pos2>,
    pub renaming_track_id: Option<Uuid>,
    pub rename_buffer: String,
}

/// Graph editor interaction state (keyframe selection)
#[derive(Default, Clone)]
pub struct GraphEditorInteractionState {
    pub selected_keyframe: Option<(String, usize)>,
    #[allow(dead_code)]
    pub editing_keyframe: Option<(String, usize)>,
}

/// General interaction state (dialogs, modals, import reports)
#[derive(Default, Clone)]
pub struct GeneralInteractionState {
    pub active_confirmation: Option<crate::ui::dialogs::confirmation::ConfirmationDialog>,
    pub active_modal_error: Option<String>,
    pub import_report: Option<ImportReport>,
}

/// Combined interaction state that holds all sub-states.
/// Individual panels should access only their relevant sub-state.
#[derive(Default, Clone)]
pub struct InteractionState {
    pub preview: PreviewInteractionState,
    pub timeline: TimelineInteractionState,
    pub graph_editor: GraphEditorInteractionState,
    pub general: GeneralInteractionState,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct ImportReport {
    pub successful_count: usize,
    pub duplicates: Vec<String>,
    pub errors: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct BoundsCache {
    // Key: Entity ID
    // Value: (Property Hash, (X, Y, Width, Height))
    pub bounds: std::collections::HashMap<Uuid, (u64, (f32, f32, f32, f32))>,
}

#[derive(Debug, Clone, Default)]
pub struct BodyDragState {
    pub start_mouse_pos: egui::Pos2,
    // Map of Entity ID -> Original Position [x, y]
    pub original_positions: std::collections::HashMap<Uuid, [f32; 2]>,
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
