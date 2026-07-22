use crate::action::HistoryManager;
use crate::model::ui_types::Tab;
use crate::state::context::EditorContext;
use crate::state::context_types::{NodeEditorEditableWire, SelectionTarget};
use egui_dock::DockState;
use library::core::framing::FrameEvaluator;
use library::model::project::connection::DATA_VALUE_OUTPUT_PORT;
use library::model::project::{EvalOutput, NodeContainer, PortAddress, PortOwner, Project};
use library::model::NodeContent;
use library::plugin::PluginManager;
use library::PropertyOwner;
use serde_json::{json, Value};
use std::sync::mpsc::SyncSender;

fn node_container_key(container: NodeContainer) -> String {
    match container {
        NodeContainer::Composition(id) => format!("composition:{id}"),
        NodeContainer::Track(id) => format!("track:{id}"),
        NodeContainer::Clip(id) => format!("clip:{id}"),
    }
}

fn property_owner_json(owner: PropertyOwner) -> Value {
    match owner {
        PropertyOwner::Node(id) => json!({"kind": "node", "id": id}),
        PropertyOwner::Clip(id) => json!({"kind": "clip", "id": id}),
    }
}

/// A one-shot request from the loopback HTTP thread to the UI thread.
///
/// The response is deliberately produced on demand from the authoritative
/// Project and EditorContext.  The QA bridge never keeps a synchronized model
/// or a cached copy of editor state.
pub struct StateQuery {
    pub response: SyncSender<Result<Value, String>>,
}

pub fn snapshot(
    frame: u64,
    project: &Project,
    editor_context: &EditorContext,
    dock_state: &DockState<Tab>,
    history_manager: &HistoryManager,
    plugin_manager: &PluginManager,
) -> Result<Value, String> {
    let preview_render = editor_context.preview_render_scheduler.diagnostics();
    let mut expanded_tracks = editor_context
        .timeline
        .expanded_tracks
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    expanded_tracks.sort();

    let active_tabs = dock_state
        .iter_leaves()
        .filter_map(|(_, leaf)| leaf.tabs.get(leaf.active.0))
        .map(|tab| tab.name())
        .collect::<Vec<_>>();

    let track_reorder = editor_context
        .interaction
        .timeline_track_reorder
        .map(|state| {
            json!({
                "composition_id": state.composition_id,
                "track_id": state.track_id,
                "source_index": state.source_index,
                "hover_insertion_slot": state.hover_insertion_slot,
            })
        });

    let mut graph_keyframes = editor_context
        .graph_editor
        .selected_keyframes
        .iter()
        .map(|(property, keyframe_id)| {
            json!({
                "property": property,
                "keyframe_id": keyframe_id,
            })
        })
        .collect::<Vec<_>>();
    graph_keyframes.sort_by_key(|selection| {
        (
            selection["property"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            selection["keyframe_id"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        )
    });
    let graph_drag = editor_context
        .graph_editor
        .keyframe_drag
        .as_ref()
        .map(|drag| {
            json!({
                "target": drag.target,
                "anchor": {
                    "property": drag.anchor.0,
                    "keyframe_id": drag.anchor.1,
                },
                "selected_count": drag.origins.len(),
                "changed": drag.changed,
            })
        });

    let selected_metadata_output = selected_data_runtime_snapshot(
        project,
        editor_context.selection.primary(),
        editor_context.active_composition_id,
        f64::from(editor_context.timeline.current_time),
        plugin_manager,
    );
    let project = serde_json::to_value(project)
        .map_err(|error| format!("failed to serialize authoritative Project: {error}"))?;
    Ok(json!({
        "frame": frame,
        "project": project,
        "runtime": {
            "selected_metadata_output": selected_metadata_output,
        },
        "editor": {
            "navigation": {
                "active_composition_id": editor_context.active_composition_id,
            },
            "selection": {
                "targets": editor_context.selection.targets(),
                "primary": editor_context.selection.primary(),
            },
            "timeline": {
                "current_time": editor_context.timeline.current_time,
                "is_playing": editor_context.timeline.is_playing,
                "seek_revision": editor_context.timeline.transport_seek_revision,
                "pixels_per_second": editor_context.timeline.pixels_per_second,
                "expanded_tracks": expanded_tracks,
                "track_reorder": track_reorder,
            },
            "preview": {
                "pan": {
                    "x": editor_context.view.pan.x,
                    "y": editor_context.view.pan.y,
                },
                "zoom": editor_context.view.zoom,
                "texture_id": editor_context.preview_texture_id,
                "texture_width": editor_context.preview_texture_width,
                "texture_height": editor_context.preview_texture_height,
                "render_revision": editor_context.preview_render_revision,
                "scheduler": {
                    "generation": preview_render.generation,
                    "in_flight_request": preview_render.in_flight_request,
                    "desired_pending": preview_render.desired_pending,
                    "submitted": preview_render.submitted,
                    "published": preview_render.published,
                    "discarded": preview_render.discarded,
                    "coalesced": preview_render.coalesced,
                },
                "nontransparent_pixels": editor_context.preview_nontransparent_pixels,
                "pixel_hash": editor_context.preview_pixel_hash,
                "region": editor_context.preview_region,
                "auto_fit": editor_context.interaction.preview_viewport.auto_fit,
                "primary_gesture": format!(
                    "{:?}",
                    editor_context.interaction.preview_viewport.primary_gesture
                ),
                "is_moving_selected_entity": editor_context.interaction.is_moving_selected_entity,
                "selection_drag_active": editor_context
                    .interaction
                    .preview_selection_drag_start
                    .is_some(),
                "body_drag_active": editor_context.interaction.body_drag_state.is_some(),
                "gizmo_active": editor_context.interaction.gizmo_state.is_some(),
                "vector_handle_active": editor_context
                    .interaction
                    .vector_editor_state
                    .as_ref()
                    .is_some_and(|state| state.selected_handle.is_some()),
                "vector_editor": editor_context.interaction.vector_editor_state.as_ref().map(|state| {
                    let mut selected_point_indices = state
                        .selected_point_indices
                        .iter()
                        .copied()
                        .collect::<Vec<_>>();
                    selected_point_indices.sort_unstable();
                    serde_json::json!({
                        "selected_point_indices": selected_point_indices,
                        "active_handle": state.selected_handle.map(|(index, handle)| {
                            serde_json::json!({
                                "point_index": index,
                                "handle": format!("{handle:?}").to_lowercase(),
                            })
                        }),
                        "focused_handle": state.focused_handle.map(|(index, handle)| {
                            serde_json::json!({
                                "point_index": index,
                                "handle": format!("{handle:?}").to_lowercase(),
                            })
                        }),
                        "drag_changed": state.has_changed_drag(),
                    })
                }),
                "edit_target": editor_context.interaction.preview_edit_target.as_ref().map(|target| {
                    serde_json::json!({
                        "owner": target.owner,
                        "content_node_id": target.content_node_id,
                        "spatial_node_id": target.spatial_node_id,
                        "instance_path": &target.instance_path,
                    })
                }),
                "modal_error": editor_context.interaction.active_modal_error,
            },
            "node_editor": {
                "context_menu_open": editor_context.node_editor_context_menu.is_some(),
                "pending_layout_command": editor_context.node_editor_state.pending_layout_command,
                "layout_execution_serial": editor_context.node_editor_state.layout_execution_serial,
                "last_layout_execution": editor_context
                    .node_editor_state
                    .last_layout_execution
                    .as_ref()
                    .map(|execution| serde_json::json!({
                        "execution_id": execution.execution_id,
                        "command": execution.command,
                        "scope": execution.scope,
                        "changed": execution.changed,
                    })),
                "pending_navigation": editor_context.node_editor_state.pending_navigation,
                "selected_connection_id": editor_context.node_editor_state.selected_connection_id,
                "reparent_gesture": editor_context.node_editor_state.node_reparent.as_ref().map(|gesture| {
                    let mut node_ids = gesture.origins.keys().copied().collect::<Vec<_>>();
                    node_ids.sort_unstable();
                    serde_json::json!({
                        "node_ids": node_ids,
                        "primary_node_id": gesture.primary_node_id,
                        "hovered_target": gesture.hovered_target.map(node_container_key),
                        "hovered_node_id": gesture.hovered_node_id,
                        "hovered_score": gesture.hovered_score,
                    })
                }),
                "wire_context_menu_open": editor_context
                    .node_editor_state
                    .wire_context_menu
                    .is_some(),
                "wire_context_menu_target": editor_context
                    .node_editor_state
                    .wire_context_menu
                    .as_ref()
                    .map(|context| match context.target {
                        NodeEditorEditableWire::ProjectConnection { connection_id } => serde_json::json!({
                            "kind": "explicit",
                            "connection_id": connection_id,
                        }),
                        NodeEditorEditableWire::OutputBinding { owner, node_id, data_type } => serde_json::json!({
                            "kind": "output_binding",
                            "owner": owner,
                            "node_id": node_id,
                            "output_type": format!("{data_type:?}").to_lowercase(),
                        }),
                    }),
                "wire_gesture": editor_context.node_editor_state.wire_gesture.as_ref().map(|gesture| {
                    let (connection_id, target) = match gesture.wire {
                        NodeEditorEditableWire::ProjectConnection { connection_id } => (
                            Some(connection_id),
                            serde_json::json!({
                                "kind": "explicit",
                                "connection_id": connection_id,
                            }),
                        ),
                        NodeEditorEditableWire::OutputBinding { owner, node_id, data_type } => (
                            None,
                            serde_json::json!({
                                "kind": "output_binding",
                                "owner": owner,
                                "node_id": node_id,
                                "output_type": format!("{data_type:?}").to_lowercase(),
                            }),
                        ),
                    };
                    serde_json::json!({
                        "connection_id": connection_id,
                        "target": target,
                        "kind": format!("{:?}", gesture.kind),
                        "start": {"x": gesture.start.x, "y": gesture.start.y},
                        "current": {"x": gesture.current.x, "y": gesture.current.y},
                    })
                }),
                "merge_layer_reorder": editor_context
                    .node_editor_state
                    .merge_layer_reorder
                    .as_ref()
                    .map(|gesture| serde_json::json!({
                        "merge_id": gesture.merge_id,
                        "connection_id": gesture.connection_id,
                        "start_index": gesture.start_index,
                        "target_index": gesture.target_index,
                        "layer_count": gesture.layer_count,
                        "reorder_min_index": gesture.reorder_min_index,
                        "reorder_max_index": gesture.reorder_max_index,
                        "measured_row_count": gesture
                            .row_rects
                            .iter()
                            .filter(|rect| rect.is_positive())
                            .count(),
                        "finished": gesture.finished,
                    })),
                "wire_knife": editor_context.node_editor_state.wire_knife.as_ref().map(|gesture| {
                    let mut crossed = gesture.crossed_wires.iter().copied().collect::<Vec<_>>();
                    crossed.sort_by_key(|target| match target {
                        NodeEditorEditableWire::ProjectConnection { connection_id } => {
                            format!("explicit:{connection_id}")
                        }
                        NodeEditorEditableWire::OutputBinding { owner, node_id, data_type } => {
                            format!("output_binding:{owner:?}:{data_type:?}:{node_id}")
                        }
                    });
                    let crossed_connection_ids = crossed.iter().filter_map(|target| match target {
                        NodeEditorEditableWire::ProjectConnection { connection_id } => Some(*connection_id),
                        NodeEditorEditableWire::OutputBinding { .. } => None,
                    }).collect::<Vec<_>>();
                    let crossed_wires = crossed.into_iter().map(|target| match target {
                        NodeEditorEditableWire::ProjectConnection { connection_id } => serde_json::json!({
                            "kind": "explicit",
                            "connection_id": connection_id,
                        }),
                        NodeEditorEditableWire::OutputBinding { owner, node_id, data_type } => serde_json::json!({
                            "kind": "output_binding",
                            "owner": owner,
                            "node_id": node_id,
                            "output_type": format!("{data_type:?}").to_lowercase(),
                        }),
                    }).collect::<Vec<_>>();
                    serde_json::json!({
                        "point_count": gesture.points.len(),
                        "crossed_connection_ids": crossed_connection_ids,
                        "crossed_wires": crossed_wires,
                    })
                }),
            },
            "graph": {
                "active_target": editor_context.graph_editor.active_target,
                "selected_keyframes": graph_keyframes,
                "drag": graph_drag,
                "pan": {
                    "x": editor_context.graph_editor.pan.x,
                    "y": editor_context.graph_editor.pan.y,
                },
                "zoom_x": editor_context.graph_editor.zoom_x,
                "zoom_y": editor_context.graph_editor.zoom_y,
            },
            "keyframe_dialog": {
                "is_open": editor_context.keyframe_dialog.is_open,
                "owner": editor_context.keyframe_dialog.owner.map(property_owner_json),
                "graph_address": editor_context.keyframe_dialog.graph_address.as_ref().map(|address| json!({
                    "target": address.target,
                    "section_id": address.section_id,
                    "stable_id": address.stable_id,
                    "owner": format!("{:?}", address.owner),
                    "property_key": address.property_key,
                    "component": format!("{:?}", address.component),
                })),
                "property": editor_context.keyframe_dialog.property_key,
                "keyframe_id": editor_context.keyframe_dialog.keyframe_id,
                "global_time": editor_context.keyframe_dialog.time,
                "value": editor_context.keyframe_dialog.value,
            },
        },
        "dock": {
            "active_tabs": active_tabs,
        },
        "history": {
            "undo_depth": history_manager.undo_depth(),
            "redo_depth": history_manager.redo_depth(),
        },
    }))
}

fn selected_data_runtime_snapshot(
    project: &Project,
    selection: Option<SelectionTarget>,
    active_composition_id: Option<uuid::Uuid>,
    global_time: f64,
    plugin_manager: &PluginManager,
) -> Value {
    let Some(node_id) = selection.and_then(SelectionTarget::node_id) else {
        return Value::Null;
    };
    let Some(node) = project.get_node(node_id) else {
        return Value::Null;
    };
    if !matches!(node.content(), NodeContent::Data(_)) {
        return Value::Null;
    }
    let Some(composition_id) = project
        .find_containing_composition(node_id)
        .or(active_composition_id)
    else {
        return json!({
            "node_id": node_id,
            "port": DATA_VALUE_OUTPUT_PORT,
            "time": global_time,
            "status": "error",
            "error": "Data Node has no containing or active composition",
        });
    };
    let Some(composition) = project.get_composition(composition_id) else {
        return json!({
            "node_id": node_id,
            "port": DATA_VALUE_OUTPUT_PORT,
            "time": global_time,
            "status": "error",
            "error": "Data Node composition is absent",
        });
    };
    let evaluator = FrameEvaluator::new(
        project,
        composition,
        plugin_manager.get_property_evaluators(),
        plugin_manager,
    );
    let source = PortAddress::new(PortOwner::Node(node_id), DATA_VALUE_OUTPUT_PORT);
    let output = match evaluator.evaluate_metadata_output(&source, global_time) {
        Ok(EvalOutput::Produced(value)) => json!({
            "status": "produced",
            "value": Value::from(&value),
        }),
        Ok(EvalOutput::NoOutput) => json!({"status": "no_output"}),
        Err(error) => json!({
            "status": "error",
            "error": error.to_string(),
        }),
    };
    json!({
        "node_id": node_id,
        "port": DATA_VALUE_OUTPUT_PORT,
        "global_time": global_time,
        "evaluation_source": "authoritative_project",
        "result": output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::context_types::SelectionTarget;
    use crate::ui::tab_viewer::create_initial_dock_state;
    use library::model::project::connection::DATA_VALUE_PROPERTY;
    use library::model::project::Composition;
    use library::model::property::{ColorSpaceRef, ColorValue, Property, PropertyValue};
    use library::model::{DataContent, Node};
    use library::plugin::PluginManager;

    #[test]
    fn snapshot_reads_project_and_transient_editor_state() {
        let mut project = Project::new("qa");
        let (composition, track) = Composition::new("Main", 320, 180, 30.0, 2.0);
        let composition_id = composition.id;
        let track_id = track.id;
        assert!(
            project.add_track(track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_composition(composition).is_ok(),
            "container structural Merge insertion must succeed"
        );
        let color = ColorValue::new(
            ColorSpaceRef::new("scene_linear_ap1").unwrap(),
            [-0.5, 4.25, 0.125, 0.375],
        )
        .unwrap();
        let mut data_node = Node::new_data("HDR Color", DataContent::Color);
        data_node
            .set_property(
                DATA_VALUE_PROPERTY.to_string(),
                Property::constant(PropertyValue::ColorValue(color.clone())),
            )
            .unwrap();
        let data_node_id = data_node.id;
        project.add_node(data_node);
        project
            .attach_node_to_container(NodeContainer::Composition(composition_id), data_node_id)
            .unwrap();
        let mut context = EditorContext::new(composition_id);
        context.timeline.current_time = 1.25;
        context.timeline.expanded_tracks.insert(track_id);
        context.view.pan = egui::vec2(12.0, 34.0);
        let shared_id = data_node_id;
        context.add_selection(SelectionTarget::Clip(shared_id));
        context.add_selection(SelectionTarget::Node(shared_id));
        context.keyframe_dialog.owner = Some(PropertyOwner::Node(shared_id));

        let mut history = HistoryManager::new();
        history.push_project_state(project.clone());
        let value = snapshot(
            9,
            &project,
            &context,
            &create_initial_dock_state(),
            &history,
            &PluginManager::default(),
        )
        .unwrap();
        assert_eq!(value["frame"], 9);
        assert_eq!(value["project"]["name"], "qa");
        assert_eq!(value["editor"]["timeline"]["current_time"], 1.25);
        assert_eq!(value["editor"]["preview"]["pan"]["x"], 12.0);
        assert_eq!(
            value["editor"]["navigation"]["active_composition_id"],
            composition_id.to_string()
        );
        assert_eq!(
            value["editor"]["selection"]["targets"],
            json!([
                {"kind": "clip", "id": shared_id},
                {"kind": "node", "id": shared_id},
            ])
        );
        assert_eq!(
            value["editor"]["selection"]["primary"],
            json!({"kind": "node", "id": shared_id})
        );
        assert_eq!(
            value["editor"]["keyframe_dialog"]["owner"],
            json!({"kind": "node", "id": shared_id})
        );
        assert_eq!(
            value["editor"]["timeline"]["expanded_tracks"][0],
            track_id.to_string()
        );
        assert_eq!(value["history"]["undo_depth"], 1);
        assert_eq!(
            value["runtime"]["selected_metadata_output"]["evaluation_source"],
            "authoritative_project"
        );
        assert_eq!(
            value["runtime"]["selected_metadata_output"]["result"]["value"],
            Value::from(&PropertyValue::ColorValue(color))
        );
    }
}
