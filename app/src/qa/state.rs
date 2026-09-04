use crate::action::HistoryManager;
use crate::model::ui_types::Tab;
use crate::state::context::EditorContext;
use crate::state::context_types::NodeEditorEditableWire;
use crate::state::context_types::{ModuleEditorHost, NodeEditorDocument};
use egui_dock::DockState;
use library::PropertyOwner;
use library::model::project::{NodeContainer, Project};
use serde_json::{Value, json};

pub fn authoring_snapshot(
    frame: u64,
    project: &library::model::authoring::AuthoringProject,
    editor: &crate::state::authoring::AuthoringUiState,
    dock_state: &DockState<Tab>,
    service: &library::editor::TimelineEditorService,
) -> Result<Value, String> {
    use crate::state::authoring::AuthoringSelection;

    let active_tabs = dock_state
        .iter_leaves()
        .filter_map(|(_, leaf)| leaf.tabs.get(leaf.active.0))
        .map(|tab| tab.name())
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
    Ok(json!({
        "frame": frame,
        "project": serialized_project,
        "editor": {
            "navigation": {"active_timeline_id": editor.active_timeline_id},
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
                "selected_node_count": editor.node_editor.module_selected_nodes.len(),
                "selected_connection": editor.node_editor.module_selected_connection,
            },
            "curve": {"drag_active": editor.curve.drag.is_some()},
            "status": editor.status,
            "error": editor.error,
        },
        "dock": {"active_tabs": active_tabs},
        "history": {
            "can_undo": service.can_undo().unwrap_or(false),
            "can_redo": service.can_redo().unwrap_or(false),
            "revision": revision.get(),
        },
    }))
}

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

pub fn snapshot(
    frame: u64,
    project: &Project,
    editor_context: &EditorContext,
    dock_state: &DockState<Tab>,
    history_manager: &HistoryManager,
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

    let project = serde_json::to_value(project)
        .map_err(|error| format!("failed to serialize authoritative Project: {error}"))?;
    let node_editor_document = editor_context
        .node_editor_state
        .active_document
        .as_ref()
        .map(|document| match document {
            NodeEditorDocument::ModuleDefinition {
                definition_id,
                host,
            } => {
                let (host_kind, timeline_item_id, attachment_id, instance_path, instance_id) =
                    match host {
                        ModuleEditorHost::NodeClip {
                            timeline_item_id,
                            instance_path,
                            module_instance_id,
                        } => (
                            "node_clip",
                            Some(*timeline_item_id),
                            None,
                            instance_path,
                            *module_instance_id,
                        ),
                        ModuleEditorHost::Attachment {
                            attachment_id,
                            instance_path,
                            module_instance_id,
                        } => (
                            "attachment",
                            None,
                            Some(*attachment_id),
                            instance_path,
                            *module_instance_id,
                        ),
                    };
                json!({
                    "document_kind": "module_definition",
                    "module_definition_id": definition_id,
                    "module_instance_id": instance_id,
                    "instance_path": instance_path,
                    "host_kind": host_kind,
                    "timeline_item_id": timeline_item_id,
                    "attachment_id": attachment_id,
                })
            }
        });
    let directional_layout_swipe = editor_context
        .node_editor_state
        .directional_layout_swipe
        .as_ref()
        .map(|gesture| {
            let measured_geometry_count = gesture
                .frozen_geometry
                .values()
                .filter(|geometry| geometry.measured)
                .count();
            json!({
                "gesture_id": gesture.gesture_id,
                "composition_id": gesture.composition_id,
                "direct_owner": node_container_key(gesture.direct_owner),
                "anchor_node_id": gesture.anchor_node_id,
                "mode": gesture.mode.as_str(),
                "axis": gesture.axis.map(|axis| format!("{axis:?}").to_lowercase()),
                "direction": gesture.direction.map(|direction| direction.as_str()),
                "start": {"x": gesture.start.x, "y": gesture.start.y},
                "current": {"x": gesture.current.x, "y": gesture.current.y},
                "frozen_selected_node_ids": gesture.frozen_selected_node_ids,
                "frozen_geometry_count": gesture.frozen_geometry.len(),
                "measured_geometry_count": measured_geometry_count,
                "estimated_geometry_count": gesture.frozen_geometry.len() - measured_geometry_count,
                "preview_positions": gesture.preview_positions,
                "project_revision": gesture.project_revision,
                "history_undo_depth": gesture.history_undo_depth,
                "history_redo_depth": gesture.history_redo_depth,
                "diagnostics": {
                    "reachable_node_ids": gesture.diagnostics.reachable_node_ids,
                    "eligible_node_ids": gesture.diagnostics.eligible_node_ids,
                    "moved_node_ids": gesture.diagnostics.moved_node_ids,
                    "blocked_node_ids": gesture.diagnostics.blocked_node_ids,
                },
            })
        });
    let last_directional_layout_swipe = editor_context
        .node_editor_state
        .last_directional_layout_swipe
        .as_ref()
        .map(|execution| {
            json!({
                "gesture_id": execution.gesture_id,
                "outcome": execution.outcome.as_str(),
                "reason": execution.reason,
                "composition_id": execution.composition_id,
                "direct_owner": node_container_key(execution.direct_owner),
                "anchor_node_id": execution.anchor_node_id,
                "mode": execution.mode.as_str(),
                "axis": execution.axis.map(|axis| format!("{axis:?}").to_lowercase()),
                "direction": execution.direction.map(|direction| direction.as_str()),
                "moved_node_ids": execution.moved_node_ids,
                "project_revision_before": execution.project_revision_before,
                "project_revision_after": execution.project_revision_after,
                "history_undo_before": execution.history_undo_before,
                "history_undo_after": execution.history_undo_after,
                "history_redo_before": execution.history_redo_before,
                "history_redo_after": execution.history_redo_after,
            })
        });
    Ok(json!({
        "frame": frame,
        "project": project,
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
                "document": node_editor_document,
                "focus_requested": editor_context.node_editor_state.focus_requested,
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
                "directional_layout_swipe": directional_layout_swipe,
                "directional_layout_release_guard": editor_context
                    .node_editor_state
                    .directional_layout_release_guard,
                "last_directional_layout_swipe": last_directional_layout_swipe,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::context_types::SelectionTarget;
    use crate::ui::tab_viewer::create_initial_dock_state;
    use library::model::project::Composition;
    use library::model::project::connection::DATA_VALUE_PROPERTY;
    use library::model::property::{ColorSpaceRef, ColorValue, Property, PropertyValue};
    use library::model::{DataContent, Node};

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
                Property::constant(PropertyValue::ColorValue(color)),
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
        assert!(value.get("runtime").is_none());
    }
}
