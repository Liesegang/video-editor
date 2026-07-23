use crate::action::{request_node_layout_command, HistoryManager};
use crate::command::{CommandId, CommandRegistry};
use crate::state::context::EditorContext;
use crate::state::context_types::{
    NodeEditorEditableWire, NodeEditorLayoutExecution, NodeEditorState, NodeEditorWireContextMenu,
    SelectionState, SelectionTarget,
};
use eframe::egui;
use library::model::project::PortOwner;
use library::model::Project;
use library::EditorService;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};
use uuid::Uuid;

use super::layout::{
    apply_directional_layout_commit, apply_directional_layout_preview,
    finish_directional_layout_release_guard, finish_edits_before_directional_layout_start,
    handle_directional_layout_outputs, recover_directional_layout_release_guard,
    DirectionalLayoutFrameOutcome,
};
use crate::utils::lock::{mutex_lock_or_recover, write_or_recover};

use super::{
    apply_auto_layout, apply_edit, apply_layout_edit, apply_queued_node_edits, build_snarl,
    capture_container_resize_before_canvas, compute_auto_layout, compute_full_composition_layout,
    container_inactive, container_resize_interactions, final_node_positions, finish_node_reparent,
    flush_pending_continuous_edit, handle_context_menu, implicit_time_overlay_requested,
    layout_needs_reflow, layout_toolbar, native_variadic_merge_target, node_can_splice_connection,
    move_change, move_end, node_drop_intents, node_editor_canvas_metadata, node_editor_details_visible,
    node_editor_port_interactions_enabled, node_editor_snarl_style_for, paint_container_foreground,
    port_owner_composition, port_owner_for_node_container, primary_node_drop_intent,
    push_history_snapshot, record_node_reparent_origins, register_container_chrome,
    register_implicit_time_context_wires, register_implicit_time_overlay, register_rendered_edges,
    register_reparent_drop_targets, rendered_edge_at_position, select_logical_item,
    selected_container_owners, selection_target_for_owner, show_wire_context_menu,
    splice_node_for_release, wire_interactions, wire_port_drop_rect, wire_secondary_click_hit,
    AutoLayoutScope, NodeContextMenuFrame, NodeEdit, OverviewWirePainter, ProjectNodeViewer,
    QueuedNodeEdit, ReparentReleaseOutcome, SurfaceCapture, SurfaceProjection, TimeContextNode,
    WireInteractionFrame, WireSecondaryClickHit,
};

fn wire_pointer_owns_layout(state: &NodeEditorState) -> bool {
    state.wire_gesture.is_some()
        || state.normal_connect_gesture.is_some()
        || state.normal_wire_drag_active
        || state.normal_connect_cancel_pending_release
}

fn replace_selection_if_changed(
    selection: &mut SelectionState,
    targets: Vec<SelectionTarget>,
    primary: Option<SelectionTarget>,
) -> bool {
    if selection.targets() == targets && selection.primary() == primary {
        return false;
    }
    selection.replace(targets, primary);
    true
}

pub(super) fn retain_merge_reorder_release_edits(edits: &mut Vec<QueuedNodeEdit>) {
    edits.retain(|queued| {
        queued.finishes_continuous_edit()
            || matches!(
                queued,
                QueuedNodeEdit::Atomic(
                    NodeEdit::ReorderConnection { .. } | NodeEdit::ReorderStructuralChild { .. }
                )
            )
    });
}

fn discard_layout_request_without_composition(state: &mut NodeEditorState) -> bool {
    state.pending_layout_command.take().is_some()
}

pub fn node_editor_panel(
    ui: &mut egui::Ui,
    project_lock: &Arc<RwLock<Project>>,
    project_service: &EditorService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    command_registry: &CommandRegistry,
) {
    let comp_id = editor_context.active_composition_id;
    let current_time = f64::from(editor_context.timeline.current_time);
    let context_menu_state = &mut editor_context.node_editor_context_menu;
    let node_editor_state = &mut editor_context.node_editor_state;
    let Some(comp_id) = comp_id else {
        node_editor_state.directional_layout_swipe = None;
        node_editor_state.directional_layout_release_guard = false;
        discard_layout_request_without_composition(node_editor_state);
        flush_pending_continuous_edit(project_lock, history_manager, node_editor_state);
        ui.centered_and_justified(|ui| ui.label("No Composition Selected"));
        return;
    };
    let pending_owner_left_composition = node_editor_state
        .pending_continuous_edit
        .as_ref()
        .is_some_and(|pending| {
            project_lock.read().map_or(true, |project| {
                port_owner_composition(&project, pending.owner) != Some(comp_id)
            })
        });
    if pending_owner_left_composition {
        flush_pending_continuous_edit(project_lock, history_manager, node_editor_state);
    }
    let (primary_pressed, primary_down, primary_released) = ui.input(|input| {
        (
            input.pointer.primary_pressed(),
            input.pointer.primary_down(),
            input.pointer.primary_released(),
        )
    });
    recover_directional_layout_release_guard(
        node_editor_state,
        primary_pressed,
        primary_down,
        primary_released,
    );
    let stale_merge_reorder =
        node_editor_state
            .merge_layer_reorder
            .as_ref()
            .is_some_and(|gesture| {
                let connection_exists = project_lock.read().is_ok_and(|project| {
                    project.connections.iter().any(|connection| {
                        connection.id == gesture.connection_id
                            && native_variadic_merge_target(&project, &connection.to)
                                .is_some_and(|target| target.node_id == gesture.merge_id)
                    })
                });
                !connection_exists || (!primary_down && !primary_released && !gesture.finished)
            });
    if stale_merge_reorder {
        node_editor_state.merge_layer_reorder = None;
    }

    let mut selected_nodes = editor_context
        .selection
        .targets()
        .iter()
        .filter_map(|target| target.node_id())
        .collect::<Vec<_>>();
    selected_nodes.sort_unstable();
    let selected_containers = selected_container_owners(&editor_context.selection);
    let selected_container = editor_context
        .selection
        .primary()
        .and_then(|target| match target {
            SelectionTarget::Composition(id) => Some(PortOwner::Composition(id)),
            SelectionTarget::Track(id) => Some(PortOwner::Track(id)),
            SelectionTarget::Clip(id) => Some(PortOwner::Clip(id)),
            SelectionTarget::Node(id) => project_lock
                .read()
                .ok()
                .and_then(|project| project.find_node_container(id))
                .map(port_owner_for_node_container),
        })
        .unwrap_or(PortOwner::Composition(comp_id));
    let container_label = match selected_container {
        PortOwner::Composition(_) => "Current composition",
        PortOwner::Track(_) => "Current track",
        PortOwner::Clip(_) => "Current clip",
        PortOwner::Node(_) => "Current container",
    };
    if let Some(command) = layout_toolbar(
        ui,
        command_registry,
        !selected_nodes.is_empty(),
        container_label,
    ) {
        request_node_layout_command(node_editor_state, command);
    }
    ui.separator();

    let requested_command = node_editor_state.pending_layout_command.take();
    let resolved_layout = requested_command
        .and_then(|command| resolve_layout_scope(command, &selected_nodes, selected_container));
    let requested_scope = resolved_layout.as_ref().map(|(_, scope)| *scope);

    if resolved_layout.is_some() {
        flush_pending_continuous_edit(project_lock, history_manager, node_editor_state);
    }

    let canvas_rect = ui.available_rect_before_wrap();
    crate::qa::register_component_with_metadata(
        "node_editor.canvas",
        "node_editor_canvas",
        canvas_rect,
        true,
        Some(serde_json::json!({"composition_id": comp_id})),
    );

    // Old projects and newly assembled container trees can carry positions
    // that were chosen before the final Snarl node dimensions were known.
    // Repair only invalid layouts (overlap or containment violations); valid
    // user-authored layouts remain untouched.
    let automatic_layout_changed = if node_editor_state.repaired_compositions.insert(comp_id) {
        let plan = project_lock.read().ok().and_then(|project| {
            layout_needs_reflow(&project, comp_id)
                .then(|| compute_full_composition_layout(&project, comp_id))
                .flatten()
        });
        plan.is_some_and(|plan| {
            project_lock
                .write()
                .is_ok_and(|mut project| apply_auto_layout(&mut project, comp_id, &plan))
        })
    } else {
        false
    };

    let explicit_layout_changed = resolved_layout.is_some_and(|(scope, _)| {
        let Ok(mut project) = project_lock.write() else {
            return false;
        };
        let Some(plan) = compute_auto_layout(&project, comp_id, scope) else {
            return false;
        };
        apply_auto_layout(&mut project, comp_id, &plan)
    });
    if let (Some(command), Some(scope)) = (requested_command, requested_scope) {
        node_editor_state.layout_execution_serial =
            node_editor_state.layout_execution_serial.saturating_add(1);
        node_editor_state.last_layout_execution = Some(NodeEditorLayoutExecution {
            execution_id: node_editor_state.layout_execution_serial,
            command,
            scope: scope.to_string(),
            changed: explicit_layout_changed,
        });
    }

    let layout_edits;
    let rendered_edges;
    let mut suppress_wire_secondary_click = false;
    let mut edits = Vec::new();
    let mut drop_intents = Vec::new();
    let mut selection_changed = false;
    let surface_move_emitted;
    let surface_move_end;
    let mut context_menu_exclusion_rects = Vec::new();
    let mut wire_context_request = None;
    let mut directional_layout_frame: DirectionalLayoutFrameOutcome;
    let mut directional_layout_outputs = Vec::new();
    let mut to_global = egui::emath::TSTransform::default();
    let mut canvas_clip = canvas_rect;
    let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
    let rendered_node_rects = Arc::new(Mutex::new(HashMap::new()));
    let surface_capture = Arc::new(Mutex::new(SurfaceCapture::default()));
    let plugin_manager = project_service.get_plugin_manager();
    {
        let Ok(project) = project_lock.read() else {
            ui.label("Project is unavailable");
            return;
        };
        let (mut snarl, containers) = build_snarl(&project, comp_id);
        apply_directional_layout_preview(&mut snarl, node_editor_state);

        let resize_was_active = node_editor_state.container_resize.is_some();
        if let Some(previous_transform) = node_editor_state.node_editor_canvas_transform {
            capture_container_resize_before_canvas(
                ui,
                &project,
                &containers,
                previous_transform,
                canvas_rect,
                node_editor_state,
            );
        }
        let resize_started_owner = (!resize_was_active)
            .then(|| {
                node_editor_state
                    .container_resize
                    .as_ref()
                    .map(|resize| resize.owner)
            })
            .flatten();

        let mut viewer = ProjectNodeViewer {
            project: &project,
            plugin_manager: Some(plugin_manager.as_ref()),
            containers: &containers,
            edits: &mut edits,
            pending_navigation: &mut node_editor_state.pending_navigation,
            selected_node_ids: &selected_nodes,
            selected_container_owners: &selected_containers,
            current_time,
            context_menu_exclusion_rects: &mut context_menu_exclusion_rects,
            wire_context_request: &mut wire_context_request,
            suppress_wire_connect: node_editor_state.wire_gesture.is_some()
                || node_editor_state.normal_connect_gesture.is_some()
                || node_editor_state.normal_connect_cancel_pending_release
                || node_editor_state.merge_layer_reorder.is_some()
                || node_editor_state.directional_layout_swipe.is_some(),
            locked_canvas_transform: node_editor_state
                .container_resize
                .as_ref()
                .map(|resize| resize.canvas_transform)
                .or_else(|| node_editor_state.surface_interaction.locked_transform())
                .or_else(|| {
                    node_editor_state
                        .wire_gesture
                        .as_ref()
                        .map(|gesture| gesture.canvas_transform)
                })
                .or_else(|| {
                    node_editor_state
                        .normal_connect_gesture
                        .as_ref()
                        .map(|gesture| gesture.canvas_transform)
                })
                .or_else(|| {
                    node_editor_state
                        .wire_knife
                        .as_ref()
                        .map(|gesture| gesture.canvas_transform)
                })
                .or_else(|| {
                    node_editor_state
                        .merge_layer_reorder
                        .as_ref()
                        .map(|gesture| gesture.canvas_transform)
                }),
            previous_canvas_transform: node_editor_state.node_editor_canvas_transform,
            to_global: &mut to_global,
            canvas_clip: &mut canvas_clip,
            rendered_ports: Arc::clone(&rendered_ports),
            merge_layer_reorder: &mut node_editor_state.merge_layer_reorder,
            rendered_node_rects: Arc::clone(&rendered_node_rects),
            surface_capture: Arc::clone(&surface_capture),
        };
        let snarl_style = node_editor_snarl_style_for(ui.style());
        let graph_id = egui::Id::new(("project_node_editor", comp_id));
        let popup_was_open = egui::Popup::is_any_open(ui.ctx());
        snarl.show(&mut viewer, &snarl_style, graph_id, ui);
        drop(viewer);
        let popup_owns_pointer = popup_was_open || egui::Popup::is_any_open(ui.ctx());
        node_editor_state.node_editor_canvas_transform = Some(to_global);
        // The early registration makes the canvas discoverable even if graph
        // construction exits before Snarl renders. Replace it in the same
        // frame with the transform observed by Snarl's `final_transform`
        // callback so HTTP QA never reads the identity placeholder.
        crate::qa::register_component_with_metadata(
            "node_editor.canvas",
            "node_editor_canvas",
            canvas_rect,
            true,
            Some(node_editor_canvas_metadata(comp_id, to_global)),
        );

        // `Snarl::show` hashes the supplied salt with the parent Ui id. Using
        // the raw salt here creates an unrelated, untransformed layer and
        // leaks graph-space chrome over the rest of the application.
        let snarl_id = ui.make_persistent_id(graph_id);
        let graph_layer = egui::LayerId::new(ui.layer_id().order, snarl_id);
        // `Context::layer_painter` starts with a *global* content clip. Calling
        // `with_clip_rect` with this graph-space rect would intersect two
        // coordinate systems and reduce the overview to a tiny sliver after
        // the layer transform. Construct the painter with the graph clip
        // directly; egui transforms both the shapes and this clip exactly once.
        let foreground = egui::Painter::new(
            ui.ctx().clone(),
            graph_layer,
            to_global.inverse() * canvas_clip,
        );
        let overview =
            (!node_editor_details_visible(to_global.scaling)).then_some(OverviewWirePainter {
                painter: &foreground,
                to_global,
            });
        rendered_edges = register_rendered_edges(&project, &rendered_ports, canvas_clip, overview);
        if implicit_time_overlay_requested(ui.ctx(), canvas_clip) {
            let hovered_node_id = ui
                .input(|input| input.pointer.hover_pos())
                .filter(|position| canvas_clip.contains(*position))
                .and_then(|position| {
                    let graph_position = to_global.inverse() * position;
                    rendered_node_rects.lock().ok().and_then(|node_rects| {
                        node_rects
                            .iter()
                            .filter(|(_, rect)| rect.contains(graph_position))
                            .min_by_key(|(node_id, _)| **node_id)
                            .map(|(node_id, _)| *node_id)
                    })
                });
            let selected_node_ids = selected_nodes.iter().copied().collect::<HashSet<_>>();
            let mut time_context_nodes = rendered_node_rects
                .lock()
                .map(|node_rects| {
                    node_rects
                        .keys()
                        .copied()
                        .map(|node_id| TimeContextNode {
                            node_id,
                            selected: selected_node_ids.contains(&node_id),
                            hovered: hovered_node_id == Some(node_id),
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            time_context_nodes.sort_by_key(|node| node.node_id);
            let context_wire_painter = ui.painter().with_clip_rect(canvas_clip);
            let wire_count = register_implicit_time_context_wires(
                &project,
                &rendered_ports,
                &time_context_nodes,
                canvas_clip,
                &context_wire_painter,
            );
            register_implicit_time_overlay(canvas_clip, wire_count);
        }
        for container in &containers {
            let selected = selected_containers.contains(&container.owner);
            paint_container_foreground(
                &foreground,
                &project,
                container,
                container_inactive(&project, container.owner, current_time),
                selected,
                to_global.scaling,
            );
            register_container_chrome(
                container,
                to_global,
                canvas_clip,
                &project,
                current_time,
                selected,
            );
        }

        if ui.input(|input| input.pointer.secondary_clicked()) {
            node_editor_state.selected_connection_id = None;
            if let Some(position) = ui.input(|input| input.pointer.interact_pos()) {
                let graph_position = to_global.inverse() * position;
                let over_graph_item = context_menu_exclusion_rects
                    .iter()
                    .any(|rect| rect.contains(graph_position));
                if over_graph_item {
                    wire_context_request = None;
                } else if let Some(hit) = wire_secondary_click_hit(&rendered_edges, position) {
                    // Every rendered wire owns its secondary-click hit area.
                    // Authored wires open commands below; containment-derived
                    // wires are display-only and must not leak the same click
                    // through to the blank-canvas Add menu.
                    suppress_wire_secondary_click = true;
                    match hit {
                        WireSecondaryClickHit::Editable(target) => {
                            if wire_context_request.is_none() {
                                wire_context_request = Some(target);
                            }
                        }
                        WireSecondaryClickHit::DisplayOnly => {
                            wire_context_request = None;
                            node_editor_state.wire_context_menu = None;
                            *context_menu_state = None;
                        }
                    }
                }
            }
        }
        if let Some(target) = wire_context_request {
            let (position, open_time) = ui.input(|input| {
                (
                    input.pointer.interact_pos().unwrap_or(canvas_clip.center()),
                    input.time,
                )
            });
            node_editor_state.selected_connection_id = match target {
                NodeEditorEditableWire::ProjectConnection { connection_id } => Some(connection_id),
                NodeEditorEditableWire::OutputBinding { .. } => None,
            };
            node_editor_state.wire_context_menu = Some(NodeEditorWireContextMenu {
                target,
                position,
                open_time,
                inserting: false,
                actions_armed: false,
            });
            *context_menu_state = None;
        }
        let (primary_pressed, primary_down, primary_released, pointer_position, modifiers) = ui
            .input(|input| {
                (
                    input.pointer.primary_pressed(),
                    input.pointer.primary_down(),
                    input.pointer.primary_released(),
                    input.pointer.interact_pos(),
                    input.modifiers,
                )
            });
        let pointer_on_port = node_editor_port_interactions_enabled(to_global.scaling)
            && pointer_position.is_some_and(|position| {
                canvas_clip.contains(position)
                    && rendered_ports.lock().is_ok_and(|ports| {
                        ports
                            .values()
                            .any(|rect| wire_port_drop_rect(*rect).contains(position))
                    })
            });
        let surface_options = if node_editor_details_visible(to_global.scaling) {
            node_editor_ui::InteractionOptions::SELECTION_AND_MOVE
        } else {
            node_editor_ui::InteractionOptions::OVERVIEW_SELECTION
        };
        let resize_owned_layout_before = node_editor_state.container_resize.is_some();
        let legacy_pointer_already_owned = wire_pointer_owns_layout(node_editor_state)
            || resize_owned_layout_before
            || pointer_on_port
            || node_editor_state.wire_knife.is_some()
            || node_editor_state.merge_layer_reorder.is_some()
            || popup_owns_pointer;
        let layout_swipe_preflight = {
            let node_rects = mutex_lock_or_recover(rendered_node_rects.as_ref());
            let port_rects = mutex_lock_or_recover(rendered_ports.as_ref());
            let capture = mutex_lock_or_recover(surface_capture.as_ref());
            let projection = SurfaceProjection::from_project(
                &project,
                &containers,
                &node_rects,
                &port_rects,
                &capture,
                &rendered_edges,
                editor_context.selection.targets(),
                editor_context.selection.primary(),
                node_editor_state.selected_connection_id,
                canvas_clip,
                to_global,
            );
            node_editor_ui::Editor::layout_swipe_wants_pointer(
                ui,
                &projection.frame(),
                &node_editor_state.surface_interaction,
                surface_options,
                legacy_pointer_already_owned,
            )
        };
        let wire_owned_layout_before = wire_pointer_owns_layout(node_editor_state);
        if !layout_swipe_preflight && !resize_owned_layout_before && !popup_owns_pointer {
            edits.extend(wire_interactions(
                ui,
                node_editor_state,
                WireInteractionFrame {
                    project: &project,
                    edges: &rendered_edges,
                    rendered_ports: &rendered_ports,
                    canvas_clip,
                    graph_item_rects: &context_menu_exclusion_rects,
                    to_global,
                },
            ));
        }
        let wire_owned_layout =
            wire_owned_layout_before || wire_pointer_owns_layout(node_editor_state);
        if let Some(edit) = show_wire_context_menu(
            ui,
            node_editor_state,
            &project,
            plugin_manager.as_ref(),
            comp_id,
            to_global,
        ) {
            edits.push(edit);
        }

        let resize_edits = container_resize_interactions(
            ui,
            &project,
            &containers,
            to_global,
            canvas_clip,
            node_editor_state,
        );
        let resize_owned_layout =
            resize_owned_layout_before || node_editor_state.container_resize.is_some();
        let directional_layout_was_active = node_editor_state.directional_layout_swipe.is_some()
            || node_editor_state.directional_layout_release_guard
            || node_editor_state
                .surface_interaction
                .is_layout_swipe_active();
        let pointer_is_specialized = !directional_layout_was_active
            && (pointer_on_port
                || wire_owned_layout
                || resize_owned_layout
                || node_editor_state.wire_knife.is_some()
                || node_editor_state.merge_layer_reorder.is_some()
                || popup_owns_pointer);

        if primary_pressed {
            if let Some(owner) = resize_started_owner.filter(|owner| {
                node_editor_state
                    .container_resize
                    .as_ref()
                    .is_some_and(|resize| resize.owner == *owner)
            }) {
                let clicked = selection_target_for_owner(owner);
                selection_changed |=
                    select_logical_item(&mut editor_context.selection, clicked, modifiers.shift);
            }
        }

        let surface_outputs = {
            let node_rects = mutex_lock_or_recover(rendered_node_rects.as_ref());
            let port_rects = mutex_lock_or_recover(rendered_ports.as_ref());
            let capture = mutex_lock_or_recover(surface_capture.as_ref());
            let projection = SurfaceProjection::from_project(
                &project,
                &containers,
                &node_rects,
                &port_rects,
                &capture,
                &rendered_edges,
                editor_context.selection.targets(),
                editor_context.selection.primary(),
                node_editor_state.selected_connection_id,
                canvas_clip,
                to_global,
            );
            node_editor_ui::Editor::interact(
                ui,
                &projection.frame(),
                &mut node_editor_state.surface_interaction,
                surface_options,
                pointer_is_specialized || capture.body_pointer_owned(),
            )
        };
        if let Some(change) = super::selection_change(&surface_outputs) {
            selection_changed |= replace_selection_if_changed(
                &mut editor_context.selection,
                change.targets,
                change.primary,
            );
        }
        if super::deselects_wire(&surface_outputs) {
            node_editor_state.selected_connection_id = None;
        }
        directional_layout_outputs.extend(
            surface_outputs
                .iter()
                .filter(|output| matches!(output, node_editor_ui::EditorOutput::LayoutSwipe(_)))
                .cloned(),
        );
        let directional_pointer_owned =
            directional_layout_was_active || !directional_layout_outputs.is_empty();
        surface_move_end = move_end(&surface_outputs);
        let surface_move = move_change(&project, &surface_outputs);
        surface_move_emitted = surface_move.is_some();
        let grabbed_node = surface_move
            .as_ref()
            .and_then(|movement| movement.grabbed_node);
        let mut collected = surface_move.map_or_else(Vec::new, |movement| movement.edits);
        collected.extend(resize_edits);
        record_node_reparent_origins(&project, &collected, grabbed_node, node_editor_state);
        if let (Some(pointer_position), Ok(node_rects)) =
            (pointer_position, rendered_node_rects.lock())
        {
            let graph_drop_point = to_global.inverse() * pointer_position;
            if let Some(gesture) = node_editor_state.node_reparent.as_ref().cloned() {
                let final_positions = final_node_positions(&project, &gesture, &collected);
                drop_intents = node_drop_intents(
                    &project,
                    comp_id,
                    &gesture,
                    &node_rects,
                    &final_positions,
                    graph_drop_point,
                    to_global.scaling,
                );
                let active = primary_node_drop_intent(&drop_intents, graph_drop_point);
                if let Some(state) = node_editor_state.node_reparent.as_mut() {
                    state.hovered_target = active.map(|intent| intent.target.container);
                    state.hovered_node_id = active.map(|intent| intent.node_id);
                    state.hovered_score = active.map(|intent| intent.target.score);
                }
                if let Some(active) = active {
                    register_reparent_drop_targets(
                        &project,
                        comp_id,
                        active,
                        graph_drop_point,
                        to_global,
                        canvas_clip,
                        &foreground,
                    );
                }
            }
        }
        if !primary_down && !primary_released && surface_move_end.is_none() {
            node_editor_state.node_reparent = None;
        }
        if directional_pointer_owned {
            collected.clear();
            edits.retain(QueuedNodeEdit::finishes_continuous_edit);
            drop_intents.clear();
        } else if node_editor_state.merge_layer_reorder.is_some() {
            collected.clear();
            // The Merge body has already resolved the physical release into
            // its one authoritative reorder edit. Suppress competing canvas
            // intents without discarding that release edit itself.
            retain_merge_reorder_release_edits(&mut edits);
            drop_intents.clear();
        }
        layout_edits = collected;
    }

    if surface_move_emitted {
        flush_pending_continuous_edit(project_lock, history_manager, node_editor_state);
    }

    if selection_changed {
        editor_context.interaction.preview_edit_target = None;
        // Inspector is normally laid out before Node Editor in the dock tree.
        // Wake one follow-up frame so a Node click replaces the previous
        // Inspector owner even when playback and all animations are idle.
        ui.ctx().request_repaint();
    }

    let primary_released = ui.input(|input| input.pointer.primary_released());

    let mut layout_changed = false;
    let mut directional_layout_changed = false;
    {
        let mut project = write_or_recover(project_lock.as_ref());
        let pre_start_edits = finish_edits_before_directional_layout_start(
            &mut project,
            &mut edits,
            &directional_layout_outputs,
            node_editor_state,
            history_manager,
        );
        if pre_start_edits.changed {
            ui.ctx().request_repaint();
        }
        let node_rects = mutex_lock_or_recover(rendered_node_rects.as_ref());
        directional_layout_frame = handle_directional_layout_outputs(
            &project,
            comp_id,
            editor_context.selection.targets(),
            &node_rects,
            &directional_layout_outputs,
            node_editor_state,
            history_manager,
        );
        drop(node_rects);
        if let Some(commit) = directional_layout_frame.commit.take() {
            let result = apply_directional_layout_commit(
                &mut project,
                node_editor_state,
                history_manager,
                commit,
            );
            directional_layout_changed = result.changed;
            directional_layout_frame.request_repaint |= result.request_repaint;
        }
        if !pre_start_edits.consumed
            && apply_queued_node_edits(&mut project, edits, history_manager, node_editor_state)
        {
            // Render completion is asynchronous. Wake the UI immediately so
            // a paused Preview observes this authoritative graph mutation
            // without waiting for unrelated pointer input.
            ui.ctx().request_repaint();
        }
        for edit in layout_edits {
            layout_changed |= apply_layout_edit(&mut project, edit);
        }
        if surface_move_end == Some(node_editor_ui::MoveEndOutcome::Released) {
            let reparent_gesture = node_editor_state.node_reparent.take();
            let moved_node_ids = reparent_gesture
                .as_ref()
                .map(|gesture| gesture.origins.keys().copied().collect::<HashSet<_>>())
                .unwrap_or_default();
            let grabbed_node_id = reparent_gesture
                .as_ref()
                .and_then(|gesture| gesture.primary_node_id);
            let reparent_outcome =
                finish_node_reparent(&mut project, &drop_intents, reparent_gesture.as_ref());
            layout_changed |= reparent_outcome != ReparentReleaseOutcome::NoIntent;
            let dropped_wire = ui
                .input(|input| input.pointer.interact_pos())
                .and_then(|position| rendered_edge_at_position(&rendered_edges, position))
                .and_then(|edge| edge.kind.connection_id());
            if let (Some(connection_id), Some(node_id)) = (
                dropped_wire,
                splice_node_for_release(reparent_outcome, &moved_node_ids, grabbed_node_id),
            ) {
                if node_can_splice_connection(&project, connection_id, node_id) {
                    layout_changed |= apply_edit(
                        &mut project,
                        NodeEdit::SpliceExistingNode {
                            connection_id,
                            node_id,
                        },
                    );
                }
            }
        } else if surface_move_end == Some(node_editor_ui::MoveEndOutcome::Cancelled) {
            // Cancellation commits the live positions below as one movement-
            // only transaction. Never evaluate a drop target or splice a wire.
            node_editor_state.node_reparent = None;
        }
    }
    finish_directional_layout_release_guard(node_editor_state, primary_released);
    if directional_layout_frame.request_repaint || directional_layout_changed || layout_changed {
        ui.ctx().request_repaint();
    }
    if node_editor_state
        .merge_layer_reorder
        .as_ref()
        .is_some_and(|gesture| gesture.finished)
    {
        node_editor_state.merge_layer_reorder = None;
    }
    if selection_changed {
        flush_pending_continuous_edit(project_lock, history_manager, node_editor_state);
    }

    node_editor_state.layout_changed_during_drag |= layout_changed;
    if ui.input(|input| input.pointer.secondary_clicked()) {
        flush_pending_continuous_edit(project_lock, history_manager, node_editor_state);
    }
    let created = handle_context_menu(
        ui,
        context_menu_state,
        NodeContextMenuFrame {
            project_lock,
            project_service,
            comp_id,
            exclusion_rects: &context_menu_exclusion_rects,
            to_global,
            suppress_secondary_click: suppress_wire_secondary_click
                || wire_context_request.is_some(),
        },
    );
    // Creation already places its item in a free slot and grows only the
    // necessary ancestors. Connections change dependency semantics, not
    // geometry, and deletion cannot violate containment. Reflowing the entire
    // composition for any of those edits destroys user-authored positions and
    // can move a newly created Node outside the current viewport. Full layout
    // remains available explicitly and for the one-time invalid-layout repair.
    let layout_finished = (primary_released || surface_move_end.is_some())
        && node_editor_state.layout_changed_during_drag;
    if layout_finished {
        node_editor_state.layout_changed_during_drag = false;
    }
    if automatic_layout_changed || explicit_layout_changed || created || layout_finished {
        push_history_snapshot(project_lock, history_manager);
    }
    if let Ok(project) = project_lock.read() {
        editor_context.reconcile_selection(&project);
    }
}

fn resolve_layout_scope(
    command: CommandId,
    selected_nodes: &[Uuid],
    selected_container: PortOwner,
) -> Option<(AutoLayoutScope, &'static str)> {
    match command {
        CommandId::NodeEditorCleanLayout if !selected_nodes.is_empty() => Some((
            AutoLayoutScope::Selection(selected_nodes.to_vec()),
            "selection",
        )),
        CommandId::NodeEditorCleanLayout => {
            Some((AutoLayoutScope::Container(selected_container), "container"))
        }
        CommandId::NodeEditorCleanLayoutSelection if !selected_nodes.is_empty() => Some((
            AutoLayoutScope::Selection(selected_nodes.to_vec()),
            "selection",
        )),
        CommandId::NodeEditorCleanLayoutContainer => {
            Some((AutoLayoutScope::Container(selected_container), "container"))
        }
        CommandId::NodeEditorCleanLayoutAll => Some((AutoLayoutScope::All, "all")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{discard_layout_request_without_composition, resolve_layout_scope};
    use crate::command::CommandId;
    use crate::ui::panels::node_editor::AutoLayoutScope;
    use library::model::project::PortOwner;
    use uuid::Uuid;

    #[test]
    fn smart_layout_prefers_selection_then_falls_back_to_container() {
        let node = Uuid::new_v4();
        let container = PortOwner::Track(Uuid::new_v4());
        let Some((scope, label)) =
            resolve_layout_scope(CommandId::NodeEditorCleanLayout, &[node], container)
        else {
            panic!("selection scope");
        };
        assert!(matches!(scope, AutoLayoutScope::Selection(ids) if ids == vec![node]));
        assert_eq!(label, "selection");

        let Some((scope, label)) =
            resolve_layout_scope(CommandId::NodeEditorCleanLayout, &[], container)
        else {
            panic!("container scope");
        };
        assert!(matches!(scope, AutoLayoutScope::Container(owner) if owner == container));
        assert_eq!(label, "container");
    }

    #[test]
    fn explicit_all_layout_ignores_selection() {
        let Some((scope, label)) = resolve_layout_scope(
            CommandId::NodeEditorCleanLayoutAll,
            &[Uuid::new_v4()],
            PortOwner::Composition(Uuid::new_v4()),
        ) else {
            panic!("all scope");
        };
        assert!(matches!(scope, AutoLayoutScope::All));
        assert_eq!(label, "all");
    }

    #[test]
    fn explicit_selection_layout_is_unavailable_without_selected_nodes() {
        assert!(resolve_layout_scope(
            CommandId::NodeEditorCleanLayoutSelection,
            &[],
            PortOwner::Composition(Uuid::new_v4()),
        )
        .is_none());
    }

    #[test]
    fn layout_request_is_discarded_when_there_is_no_active_composition() {
        let mut state = crate::state::context_types::NodeEditorState {
            pending_layout_command: Some(CommandId::NodeEditorCleanLayoutAll),
            ..Default::default()
        };
        assert!(discard_layout_request_without_composition(&mut state));
        assert_eq!(state.pending_layout_command, None);
    }
}
