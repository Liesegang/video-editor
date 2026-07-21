use crate::state::context_types::{NodeEditorEditableWire, NodeEditorMergeLayerReorderGesture};
use eframe::egui;
use library::model::Project;
use library::plugin::PluginManager;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use super::{ContainerVisual, QueuedNodeEdit, RenderedPortKey, SurfaceCapture};

mod body;
mod selection;
mod snarl;

/// The Snarl payload contains only stable IDs. All labels, pins, positions and
/// connections are read from `Project` for this frame, so Snarl never becomes
/// another editable project model.
pub(in crate::ui::panels::node_editor) struct ProjectNodeViewer<'a> {
    pub(in crate::ui::panels::node_editor) project: &'a Project,
    pub(in crate::ui::panels::node_editor) plugin_manager: Option<&'a PluginManager>,
    pub(in crate::ui::panels::node_editor) containers: &'a [ContainerVisual],
    pub(in crate::ui::panels::node_editor) edits: &'a mut Vec<QueuedNodeEdit>,
    pub(in crate::ui::panels::node_editor) pending_navigation: &'a mut Option<Uuid>,
    /// Authoritative Project selection from `EditorContext`, sorted by UUID.
    /// egui-snarl's transient selection remains an input gesture detail only.
    pub(in crate::ui::panels::node_editor) selected_node_ids: &'a [Uuid],
    pub(in crate::ui::panels::node_editor) current_time: f64,
    pub(in crate::ui::panels::node_editor) context_menu_exclusion_rects: &'a mut Vec<egui::Rect>,
    pub(in crate::ui::panels::node_editor) wire_context_request:
        &'a mut Option<NodeEditorEditableWire>,
    pub(in crate::ui::panels::node_editor) suppress_wire_connect: bool,
    pub(in crate::ui::panels::node_editor) locked_canvas_transform:
        Option<egui::emath::TSTransform>,
    /// Last transform observed outside egui-snarl. It is transition input,
    /// not a second authoritative viewport state.
    pub(in crate::ui::panels::node_editor) previous_canvas_transform:
        Option<egui::emath::TSTransform>,
    pub(in crate::ui::panels::node_editor) to_global: &'a mut egui::emath::TSTransform,
    pub(in crate::ui::panels::node_editor) canvas_clip: &'a mut egui::Rect,
    pub(in crate::ui::panels::node_editor) rendered_ports:
        Arc<Mutex<HashMap<RenderedPortKey, egui::Rect>>>,
    pub(in crate::ui::panels::node_editor) merge_layer_reorder:
        &'a mut Option<NodeEditorMergeLayerReorderGesture>,
    /// Exact graph-space rectangles reported by Snarl after layout. These are
    /// the geometry authority for drop targeting; estimated Node sizes are not
    /// precise enough around nested container boundaries.
    pub(in crate::ui::panels::node_editor) rendered_node_rects:
        Arc<Mutex<HashMap<Uuid, egui::Rect>>>,
    /// Actual back-to-front Snarl callback order and exact Node header hits.
    pub(in crate::ui::panels::node_editor) surface_capture: Arc<Mutex<SurfaceCapture>>,
}
