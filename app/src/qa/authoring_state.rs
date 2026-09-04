use egui_dock::DockState;
use library::editor::TimelineEditorService;
use library::model::authoring::AuthoringProject;
use serde_json::{json, Value};

use crate::model::ui_types::Tab;
use crate::state::authoring::{AuthoringSelection, AuthoringUiState};
use crate::state::node_editor::{ModuleEditorHost, NodeEditorDocument};

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
            NodeEditorDocument::ModuleDefinition {
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
            "assets": {
                "view_mode": editor.assets.view_mode.qa_name(),
            },
            "timeline": {
                "current_frame": editor.timeline.current_frame,
                "is_playing": editor.timeline.is_playing,
                "pixels_per_second": editor.timeline.pixels_per_second,
                "vertical_zoom": editor.timeline.vertical_zoom,
                "horizontal_scroll": editor.timeline.horizontal_scroll,
                "vertical_scroll": editor.timeline.vertical_scroll,
                "item_gesture_active": editor.timeline.item_gesture.is_some(),
                "keyframe_gesture_active": editor.timeline.keyframe_gesture.is_some(),
                "library_drag_active": editor.timeline.library_drag.is_some(),
                "expanded_items": editor.timeline.expanded_items,
                "track_display_modes": editor.timeline.track_display_modes.iter().map(|(id, mode)| {
                    (id.to_string(), mode.qa_name())
                }).collect::<std::collections::HashMap<_, _>>(),
                "item_display_modes": editor.timeline.item_display_modes.iter().map(|(id, mode)| {
                    (id.to_string(), mode.qa_name())
                }).collect::<std::collections::HashMap<_, _>>(),
            },
            "preview": {
                "pan": {"x": editor.preview.canvas.pan.x, "y": editor.preview.canvas.pan.y},
                "zoom": editor.preview.canvas.zoom.x,
                "show_grid": editor.preview.show_grid,
                "auto_fit": editor.preview.auto_fit,
                "rendered_revision": editor.preview.rendered_revision,
                "rendered_frame": editor.preview.rendered_frame,
                "texture_width": editor.preview.texture_width,
                "texture_height": editor.preview.texture_height,
                "nontransparent_pixels": editor.preview.nontransparent_pixels,
                "pixel_hash": editor.preview.pixel_hash,
                "active_tool": match editor.preview.active_tool {
                    crate::state::authoring::PreviewTool::Select => "select",
                    crate::state::authoring::PreviewTool::Text => "text",
                    crate::state::authoring::PreviewTool::Path => "path",
                    crate::state::authoring::PreviewTool::Pan => "pan",
                    crate::state::authoring::PreviewTool::Zoom => "zoom",
                },
                "text_editor": {
                    "target_item_id": editor.preview.text_editor.target_item,
                    "editing": editor.preview.text_editor.editing,
                    "changed": editor.preview.text_editor.changed(),
                },
                "path_editor": {
                    "target_item_id": editor.preview.path_editor.target_item,
                    "selected_point_indices": editor.preview.path_editor.selected_point_indices,
                    "drag_active": editor.preview.path_editor.drag.is_some(),
                },
            },
            "node_editor": {
                "document": document,
                "selected_node_count": editor.node_editor.selected_nodes.len(),
                "selected_connection": editor.node_editor.selected_connection,
                "pan": {
                    "x": editor.node_editor.canvas.pan.x,
                    "y": editor.node_editor.canvas.pan.y,
                },
                "zoom": editor.node_editor.canvas.zoom.x,
                "surface": "egui_snarl",
            },
            "curve_editor": {
                "pan": {"x": editor.curve_editor.canvas.pan.x, "y": editor.curve_editor.canvas.pan.y},
                "zoom": {"x": editor.curve_editor.canvas.zoom.x, "y": editor.curve_editor.canvas.zoom.y},
                "drag_active": editor.curve_editor.drag.is_some(),
            },
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
