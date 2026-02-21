use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::ui_types::{DraggedItem, GizmoHandle, TimelineDisplayMode, Vec2Def};
use crate::model::vector::VectorEditorState;

use library::animation::EasingFunction; // Added import

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct KeyframeDialogState {
    pub(crate) is_open: bool,
    pub(crate) track_id: Option<Uuid>,
    pub(crate) entity_id: Option<Uuid>,
    pub(crate) property_name: String,
    pub(crate) keyframe_index: usize,
    pub(crate) time: f64,
    pub(crate) value: f64,
    pub(crate) easing: EasingFunction,
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
pub(crate) struct TimelineState {
    pub(crate) current_time: f32,
    pub(crate) is_playing: bool,
    pub(crate) pixels_per_second: f32,
    pub(crate) display_mode: TimelineDisplayMode,
    pub(crate) v_zoom: f32,
    pub(crate) h_zoom: f32,
    #[serde(skip)]
    pub(crate) playback_accumulator: f32,
    #[serde(skip)]
    pub(crate) scroll_offset: egui::Vec2,
    #[serde(default)]
    pub(crate) expanded_tracks: HashSet<Uuid>,
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
pub(crate) enum PreviewTool {
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
pub(crate) struct ViewState {
    #[serde(with = "Vec2Def")]
    pub(crate) pan: egui::Vec2,
    pub(crate) zoom: f32,
    #[serde(default = "default_preview_resolution")]
    pub(crate) preview_resolution: f32,
    #[serde(default)]
    pub(crate) active_tool: PreviewTool,
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
pub(crate) struct GraphEditorState {
    #[serde(with = "Vec2Def")]
    pub(crate) pan: egui::Vec2, // Pan offset
    pub(crate) zoom_x: f32, // Pixels per second
    pub(crate) zoom_y: f32, // Pixels per unit value
    #[serde(default)]
    pub(crate) visible_properties: HashSet<String>,
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
pub(crate) struct SelectionState {
    pub(crate) composition_id: Option<Uuid>,
    pub(crate) selected_entities: HashSet<Uuid>,
    pub(crate) last_selected_entity_id: Option<Uuid>,
    pub(crate) last_selected_track_id: Option<Uuid>,
}

// --- Sub-states split by panel responsibility ---

/// Preview panel interaction state (gizmo, vector editor, text editing, selection drag)
#[derive(Default, Clone)]
pub(crate) struct PreviewInteractionState {
    pub(crate) gizmo_state: Option<GizmoState>,
    pub(crate) vector_editor_state: Option<VectorEditorState>,
    pub(crate) body_drag_state: Option<BodyDragState>,
    pub(crate) preview_selection_drag_start: Option<egui::Pos2>,
    pub(crate) handled_hand_tool_drag: bool,
    pub(crate) bounds_cache: BoundsCache,
    pub(crate) editing_text_entity_id: Option<uuid::Uuid>,
    pub(crate) text_edit_buffer: String,
    pub(crate) is_moving_selected_entity: bool,
}

/// Timeline panel interaction state (clip drag/drop, resize, selection, track rename)
#[derive(Default, Clone)]
pub(crate) struct TimelineInteractionState {
    pub(crate) dragged_item: Option<DraggedItem>,
    pub(crate) dragged_entity_original_track_id: Option<Uuid>,
    pub(crate) dragged_entity_hovered_track_id: Option<Uuid>,
    pub(crate) dragged_entity_has_moved: bool,
    pub(crate) is_resizing_entity: bool,
    pub(crate) timeline_selection_drag_start: Option<egui::Pos2>,
    pub(crate) current_time_text_input: String,
    pub(crate) is_editing_current_time: bool,
    pub(crate) context_menu_open_pos: Option<egui::Pos2>,
    pub(crate) renaming_track_id: Option<Uuid>,
    pub(crate) rename_buffer: String,
}

/// Graph editor interaction state (keyframe selection)
#[derive(Default, Clone)]
pub(crate) struct GraphEditorInteractionState {
    pub(crate) selected_keyframe: Option<(String, usize)>,
    #[allow(dead_code)]
    pub(crate) editing_keyframe: Option<(String, usize)>,
}

/// General interaction state (dialogs, modals, import reports)
#[derive(Default, Clone)]
pub(crate) struct GeneralInteractionState {
    pub(crate) active_confirmation: Option<crate::ui::dialogs::confirmation::ConfirmationDialog>,
    pub(crate) active_modal_error: Option<String>,
    pub(crate) import_report: Option<ImportReport>,
}

/// Combined interaction state that holds all sub-states.
/// Individual panels should access only their relevant sub-state.
#[derive(Default, Clone)]
pub(crate) struct InteractionState {
    pub(crate) preview: PreviewInteractionState,
    pub(crate) timeline: TimelineInteractionState,
    pub(crate) graph_editor: GraphEditorInteractionState,
    pub(crate) general: GeneralInteractionState,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub(crate) struct ImportReport {
    pub(crate) successful_count: usize,
    pub(crate) duplicates: Vec<String>,
    pub(crate) errors: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BoundsCache {
    // Key: Entity ID
    // Value: (Property Hash, (X, Y, Width, Height))
    pub(crate) bounds: std::collections::HashMap<Uuid, (u64, (f32, f32, f32, f32))>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BodyDragState {
    pub(crate) start_mouse_pos: egui::Pos2,
    // Map of Entity ID -> Original Position [x, y]
    pub(crate) original_positions: std::collections::HashMap<Uuid, [f32; 2]>,
}

#[derive(Debug, Clone)]
pub(crate) struct GizmoState {
    pub(crate) start_mouse_pos: egui::Pos2,
    pub(crate) active_handle: GizmoHandle,
    pub(crate) original_position: [f32; 2],
    pub(crate) original_scale_x: f32,
    pub(crate) original_scale_y: f32,
    pub(crate) original_rotation: f32,
    pub(crate) original_anchor_x: f32,
    pub(crate) original_anchor_y: f32,
    pub(crate) original_width: f32,
    pub(crate) original_height: f32,
    /// The compositing.transform graph node ID (if any).
    pub(crate) transform_node_id: Option<uuid::Uuid>,
}
