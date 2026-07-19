use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::ui_types::{GizmoHandle, TimelineDisplayMode, Vec2Def};
use crate::model::vector::VectorEditorState;

use library::PropertyOwner;
use library::animation::EasingFunction; // Added import
use library::model::project::PortOwner;
use library::model::property::KeyframeId;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum KeyframeValueComponent {
    #[default]
    Scalar,
    X,
    Y,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyframeDialogEditControl {
    Time,
    Value,
    Easing,
    Overshoot,
    Period,
    BounceAmplitude,
    BounceDuration,
    Expression,
}

#[derive(Clone, Debug)]
pub(crate) struct KeyframeDialogValues {
    pub time: f64,
    pub value: f64,
    pub easing: EasingFunction,
}

impl KeyframeDialogValues {
    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.time == other.time
            && self.value == other.value
            && serde_json::to_value(&self.easing).ok() == serde_json::to_value(&other.easing).ok()
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct KeyframeDialogTransaction {
    pub baseline: Option<KeyframeDialogValues>,
    pub active_control: Option<KeyframeDialogEditControl>,
    pub dirty: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct KeyframeDialogState {
    pub is_open: bool,
    pub track_id: Option<Uuid>,
    pub entity_id: Option<Uuid>,
    pub property_name: String,
    #[serde(skip)]
    pub owner: Option<PropertyOwner>,
    pub property_key: String,
    #[serde(skip)]
    pub keyframe_id: Option<KeyframeId>,
    #[serde(skip)]
    pub component: KeyframeValueComponent,
    /// Display time in global Timeline seconds. Conversion to Clip-local
    /// source time happens only when the edit is committed.
    pub time: f64,
    pub value: f64,
    pub easing: EasingFunction,
    #[serde(skip)]
    pub(crate) transaction: KeyframeDialogTransaction,
}

impl KeyframeDialogState {
    pub(crate) fn values(&self) -> KeyframeDialogValues {
        KeyframeDialogValues {
            time: self.time,
            value: self.value,
            easing: self.easing.clone(),
        }
    }

    pub(crate) fn begin_transaction(&mut self) {
        self.transaction = KeyframeDialogTransaction {
            baseline: Some(self.values()),
            active_control: None,
            dirty: false,
        };
    }
}

impl Default for KeyframeDialogState {
    fn default() -> Self {
        Self {
            is_open: false,
            track_id: None,
            entity_id: None,
            property_name: String::new(),
            owner: None,
            property_key: String::new(),
            keyframe_id: None,
            component: KeyframeValueComponent::Scalar,
            time: 0.0,
            value: 0.0,
            easing: EasingFunction::Linear,
            transaction: KeyframeDialogTransaction::default(),
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

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub enum PreviewTool {
    #[default]
    Select,
    Pan,
    Zoom,
    Text,
    Shape,
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
    /// Graph selection is view state for one authoritative Node. It must not
    /// leak when a different Clip/Node becomes the active Graph owner.
    #[serde(skip)]
    pub active_entity_id: Option<Uuid>,
    #[serde(skip)]
    pub selected_keyframes: HashSet<(String, KeyframeId)>,
    /// Absolute gesture snapshot. egui's `drag_delta` is per-frame while
    /// `total_drag_delta` is cumulative from the press, so every frame is
    /// evaluated against these original values plus the total delta.
    #[serde(skip)]
    pub keyframe_drag: Option<GraphKeyframeDragState>,
}

impl Default for GraphEditorState {
    fn default() -> Self {
        Self {
            pan: egui::Vec2::ZERO,
            zoom_x: 100.0, // Default 100 pixels per second
            zoom_y: 1.0,   // Default 1 pixel per unit
            visible_properties: HashSet::new(),
            active_entity_id: None,
            selected_keyframes: HashSet::new(),
            keyframe_drag: None,
        }
    }
}

impl GraphEditorState {
    pub fn begin_entity(&mut self, entity_id: Uuid) -> bool {
        if self.active_entity_id != Some(entity_id) {
            self.active_entity_id = Some(entity_id);
            self.selected_keyframes.clear();
            self.keyframe_drag = None;
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphKeyframeDragOrigin {
    pub property_name: String,
    pub keyframe_id: KeyframeId,
    pub global_time: f64,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphKeyframeDragState {
    pub entity_id: Uuid,
    pub anchor: (String, KeyframeId),
    pub origins: Vec<GraphKeyframeDragOrigin>,
    pub changed: bool,
}

use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq)]
pub enum DragStateItem {
    Asset {
        asset_id: Uuid,
        pos: Option<egui::Pos2>,
    },
    Composition {
        id: Uuid,
        pos: Option<egui::Pos2>,
    },
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct SelectionState {
    pub composition_id: Option<Uuid>,
    pub selected_entities: HashSet<Uuid>,
    pub last_selected_entity_id: Option<Uuid>,
    pub last_selected_track_id: Option<Uuid>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct InteractionState {
    #[serde(skip)]
    pub dragged_item: Option<DragStateItem>,
    #[serde(skip)]
    pub active_confirmation: Option<crate::ui::dialogs::confirmation::ConfirmationDialog>,
    #[serde(skip)]
    pub active_modal_error: Option<String>,

    // Drag/Drop specifics
    pub dragged_entity_original_track_id: Option<Uuid>,
    pub dragged_entity_hovered_track_id: Option<Uuid>,
    pub dragged_entity_has_moved: bool,

    /// Runtime-only state for reordering top-level Timeline tracks. It stores
    /// identifiers and an insertion slot, never a mutable Project copy.
    #[serde(skip)]
    pub timeline_track_reorder: Option<TimelineTrackReorderState>,

    // Manipulation
    pub is_resizing_entity: bool,
    pub is_moving_selected_entity: bool,

    // We can't import GizmoState here easily if it depends on something else or circular dep,
    // but GizmoState is defined in context.rs.
    // Ideally we should move GizmoState here or to a separate file.
    // For now, let's assume we will move GizmoState here or import it.
    // Based on previous file read, GizmoState is in context.rs.
    // I will MOVE GizmoState to this file to avoid circular dependency.
    #[serde(skip)]
    pub gizmo_state: Option<GizmoState>,

    // Vector Editor State
    #[serde(skip)]
    pub vector_editor_state: Option<VectorEditorState>,

    // Text Input
    #[serde(skip)]
    pub current_time_text_input: String,
    #[serde(skip)]
    pub is_editing_current_time: bool,

    // Context Menu
    #[serde(skip)]
    pub context_menu_open_pos: Option<egui::Pos2>,

    // Graph Editor Selection: encoded property scope plus persistent model ID.
    pub selected_keyframe: Option<(String, KeyframeId)>,
    #[allow(
        dead_code,
        reason = "keyframe edit selection is serialized separately while the graph dialog integration is completed"
    )]
    #[serde(skip)]
    pub editing_keyframe: Option<(String, KeyframeId)>,

    // Body Drag State for absolute delta calculation
    pub body_drag_state: Option<BodyDragState>,

    // Drag-to-select state
    #[serde(skip)]
    pub timeline_selection_drag_start: Option<egui::Pos2>,
    #[serde(skip)]
    pub preview_selection_drag_start: Option<egui::Pos2>,

    /// Render-branch path for the primary Preview selection. The persistent
    /// selection remains a Project Node ID; this transient path distinguishes
    /// fan-out of that Node through multiple Merge/Reference branches.
    #[serde(skip)]
    pub preview_selected_instance_path: Option<Vec<Uuid>>,

    // Hand Tool Logic
    #[serde(skip)]
    pub handled_hand_tool_drag: bool,

    // Preview-only viewport state. This is derived UI state and must never be
    // persisted into, or used as a second source of truth for, Project data.
    #[serde(skip)]
    pub preview_viewport: PreviewViewportRuntimeState,

    // Caching for Text/Shape bounds
    #[serde(skip)]
    pub bounds_cache: BoundsCache,

    // Text Editing State
    #[serde(skip)]
    pub editing_text_entity_id: Option<uuid::Uuid>,
    #[serde(skip)]
    pub text_edit_buffer: String,

    // Import Reporting
    #[serde(skip)]
    pub import_report: Option<ImportReport>,

    // Track Rename State
    #[serde(skip)]
    pub renaming_track_id: Option<Uuid>,
    #[serde(skip)]
    pub rename_buffer: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelineTrackReorderState {
    pub composition_id: Uuid,
    pub track_id: Uuid,
    pub source_index: usize,
    /// Slot in the original order: 0 is before the first Track and `len` is
    /// after the last Track.
    pub hover_insertion_slot: Option<usize>,
}

/// Owner of the current primary-pointer gesture in the Preview panel.
///
/// `Pending` covers the small interval between a press and egui deciding that
/// it is a drag. This lets Space claim a press that happened just before the
/// key was held, without stealing an edit gesture that has already started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreviewPrimaryGesture {
    #[default]
    Idle,
    Pending,
    Pan,
    Content,
}

#[derive(Clone, Debug)]
pub struct PreviewViewportRuntimeState {
    pub fitted_composition_id: Option<Uuid>,
    pub fitted_canvas_size: [u64; 2],
    pub last_viewport_size: egui::Vec2,
    pub auto_fit: bool,
    pub primary_gesture: PreviewPrimaryGesture,
}

impl Default for PreviewViewportRuntimeState {
    fn default() -> Self {
        Self {
            fitted_composition_id: None,
            fitted_canvas_size: [0, 0],
            last_viewport_size: egui::Vec2::ZERO,
            auto_fit: true,
            primary_gesture: PreviewPrimaryGesture::Idle,
        }
    }
}

impl PreviewViewportRuntimeState {
    pub fn request_fit(&mut self) {
        self.fitted_composition_id = None;
        self.fitted_canvas_size = [0, 0];
        self.last_viewport_size = egui::Vec2::ZERO;
        self.auto_fit = true;
        self.primary_gesture = PreviewPrimaryGesture::Idle;
    }
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
    pub bounds: std::collections::HashMap<Uuid, CachedPreviewBounds>,
}

pub type PreviewBounds = (f32, f32, f32, f32);
pub type CachedPreviewBounds = (u64, PreviewBounds);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BodyDragState {
    #[serde(with = "crate::model::ui_types::Pos2Def")]
    pub start_mouse_pos: egui::Pos2,
    // Map of Entity ID -> Original Position [x, y]
    pub original_positions: std::collections::HashMap<Uuid, [f32; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GizmoState {
    #[serde(with = "crate::model::ui_types::Pos2Def")]
    pub start_mouse_pos: egui::Pos2,
    pub active_handle: GizmoHandle,
    pub original_position: [f32; 2],
    pub original_scale_x: f32,
    pub original_scale_y: f32,
    pub original_rotation: f32,
    /// Final evaluated values at gesture start. These may differ from the
    /// direct source values above when a downstream Shape Effector contributes
    /// to the rendered transform.
    pub original_visual_position: [f32; 2],
    pub original_visual_scale_x: f32,
    pub original_visual_scale_y: f32,
    pub original_visual_rotation: f32,
    pub original_anchor_x: f32,
    pub original_anchor_y: f32,
    pub original_width: f32,
    pub original_height: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMenuState {
    #[serde(with = "crate::model::ui_types::Pos2Def")]
    pub position: egui::Pos2,
    pub open_time: f64,
    pub search_query: String,
    // Add other state if needed, e.g., expanded sections
    #[serde(default)]
    pub expanded_sections: std::collections::HashSet<String>,
}

impl ContextMenuState {
    pub fn new(position: egui::Pos2, open_time: f64) -> Self {
        Self {
            position,
            open_time,
            search_query: String::new(),
            expanded_sections: std::collections::HashSet::from([
                "Input".to_string(),
                "Geometry".to_string(),
                "Composition".to_string(),
                "Effect".to_string(),
            ]),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeEditorState {
    #[serde(skip)]
    pub pending_navigation: Option<Uuid>,
    #[serde(skip)]
    pub layout_changed_during_drag: bool,
    /// Nodes whose Snarl positions changed during the current pointer drag.
    /// Membership is resolved once, at pointer release, from the final drop
    /// position so geometry and containment share one history transaction.
    #[serde(skip)]
    pub moved_node_ids: std::collections::HashSet<Uuid>,
    /// Compositions whose legacy/fixture layout has already received its one
    /// automatic repair. Manual drags must remain exactly where the user left
    /// them on subsequent frames.
    #[serde(skip)]
    pub repaired_compositions: std::collections::HashSet<Uuid>,
    /// Snapshot held for the complete pointer gesture so edge/corner resize
    /// uses absolute drag delta and produces one coalesced history entry.
    #[serde(skip)]
    pub container_resize: Option<ContainerResizeState>,
    /// Dirty inline edit waiting for its gesture/focus boundary. Project
    /// values update on every frame, but history is committed once for this
    /// owner/property pair when the control finishes or the editing context
    /// changes.
    #[serde(skip)]
    pub pending_continuous_edit: Option<NodeEditorPendingEdit>,
    /// Canonical connection selected through a real wire hit in the canvas.
    #[serde(skip)]
    pub selected_connection_id: Option<Uuid>,
    /// Pointer gesture captured by a wire or one of its endpoint handles.
    #[serde(skip)]
    pub wire_gesture: Option<NodeEditorWireGesture>,
    /// Alt+primary stroke started on empty canvas; every intersected canonical
    /// wire is removed in one history transaction when the stroke ends.
    #[serde(skip)]
    pub wire_knife: Option<NodeEditorWireKnifeGesture>,
    /// Persistent right-click menu for a canonical wire.
    #[serde(skip)]
    pub wire_context_menu: Option<NodeEditorWireContextMenu>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeEditorWireDragKind {
    Disconnect,
    ReconnectSource,
    ReconnectTarget,
}

#[derive(Clone, Debug)]
pub struct NodeEditorWireGesture {
    pub connection_id: Uuid,
    pub kind: NodeEditorWireDragKind,
    pub start: egui::Pos2,
    pub current: egui::Pos2,
}

#[derive(Clone, Debug)]
pub struct NodeEditorWireKnifeGesture {
    pub points: Vec<egui::Pos2>,
    pub crossed_connection_ids: std::collections::HashSet<Uuid>,
}

#[derive(Clone, Debug)]
pub struct NodeEditorWireContextMenu {
    pub connection_id: Uuid,
    pub position: egui::Pos2,
    pub open_time: f64,
    pub inserting: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeEditorPendingEdit {
    pub owner: PortOwner,
    pub key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerResizeEdge {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone)]
pub struct ContainerResizeState {
    pub owner: PortOwner,
    pub edge: ContainerResizeEdge,
    pub start_pointer: egui::Pos2,
    pub start_position: [f32; 2],
    pub start_size: [f32; 2],
}
