use egui_dock::DockState;
use library::editor::TimelineEditorService;
use library::model::authoring::AuthoringProject;
use serde_json::{json, Value};

use crate::model::ui_types::Tab;
use crate::state::authoring::{AuthoringSelection, AuthoringUiState};
use crate::state::module_node_editor::{ModuleEditorHost, ModuleNodeEditorDocument};

pub fn snapshot(
    frame: u64,
    project: &AuthoringProject,
    editor: &AuthoringUiState,
    dock_state: &DockState<Tab>,
    service: &TimelineEditorService,
) -> Result<Value, String> {
    let active_tabs = dock_state
        .iter_leaves()
        .filter_map(|(_, leaf)| leaf.tabs.get(leaf.active.0))
        .map(Tab::name)
        .collect::<Vec<_>>();
    let selection = editor.selection.primary().map(|selection| match selection {
        AuthoringSelection::Timeline(id) => json!({"kind": "timeline", "id": id}),
        AuthoringSelection::Track(id) => json!({"kind": "track", "id": id}),
        AuthoringSelection::Item(id) => json!({"kind": "timeline_item", "id": id}),
        AuthoringSelection::Asset(id) => json!({"kind": "asset", "id": id}),
        AuthoringSelection::ModuleDefinition(id) => {
            json!({"kind": "module_definition", "id": id})
        }
    });
    let document = editor
        .node_editor
        .active_document
        .as_ref()
        .map(|document| match document {
            ModuleNodeEditorDocument::ModuleDefinition {
                definition_id,
                host,
            } => {
                let (kind, instance_id) = match host {
                    ModuleEditorHost::NodeClip {
                        module_instance_id, ..
                    } => ("node_clip", module_instance_id),
                    ModuleEditorHost::Attachment {
                        module_instance_id, ..
                    } => ("attachment", module_instance_id),
                };
                json!({
                    "kind": "module_definition",
                    "definition_id": definition_id,
                    "host": kind,
                    "instance_id": instance_id,
                })
            }
        });
    let serialized_project = serde_json::to_value(project)
        .map_err(|error| format!("failed to serialize authoritative Project: {error}"))?;
    let revision = service
        .revision()
        .map_err(|error| format!("failed to read authoring revision: {error}"))?;
    let can_undo = service
        .can_undo()
        .map_err(|error| format!("failed to read Undo state: {error}"))?;
    let can_redo = service
        .can_redo()
        .map_err(|error| format!("failed to read Redo state: {error}"))?;
    Ok(json!({
        "frame": frame,
        "project": serialized_project,
        "editor": {
            "navigation": {
                "active_timeline_id": editor.active_timeline_id,
                "instance_path": editor.active_instance_path,
                "definition_scope": editor.active_instance_path.is_none(),
            },
            "selection": {"primary": selection},
            "timeline": {
                "current_frame": editor.timeline.current_frame,
                "is_playing": editor.timeline.is_playing,
                "pixels_per_second": editor.timeline.pixels_per_second,
                "item_gesture_active": editor.timeline.item_gesture.is_some(),
                "library_drag_active": editor.timeline.library_drag.is_some(),
            },
            "preview": {
                "pan": {"x": editor.preview.pan.x, "y": editor.preview.pan.y},
                "zoom": editor.preview.zoom,
                "show_grid": editor.preview.show_grid,
                "auto_fit": editor.preview.auto_fit,
                "rendered_revision": editor.preview.rendered_revision,
                "rendered_frame": editor.preview.rendered_frame,
                "texture_width": editor.preview.texture_width,
                "texture_height": editor.preview.texture_height,
                "nontransparent_pixels": editor.preview.nontransparent_pixels,
                "pixel_hash": editor.preview.pixel_hash,
            },
            "node_editor": {
                "document": document,
                "selected_node_count": editor.node_editor.selected_nodes.len(),
                "selected_connection": editor.node_editor.selected_connection,
            },
            "curve": {"drag_active": editor.curve.drag.is_some()},
            "status": editor.status,
            "error": editor.error,
        },
        "dock": {"active_tabs": active_tabs},
        "history": {
            "can_undo": can_undo,
            "can_redo": can_redo,
            "revision": revision.get(),
        },
    }))
}
