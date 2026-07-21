use crate::action::HistoryManager;
use crate::state::context::EditorContext;
use crate::state::context_types::{
    NodeEditorEditableWire, NodeEditorState, NodeEditorWireContextMenu, SelectionState,
    SelectionTarget,
};
use eframe::egui;
use library::model::project::PortOwner;
use library::model::Project;
use library::EditorService;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};
use uuid::Uuid;

use super::{
    apply_auto_layout, apply_edit, apply_layout_edit, apply_queued_node_edits, build_snarl,
    capture_container_resize_before_canvas, captured_snarl_drag_node, captured_snarl_drag_target,
    collect_layout_edits_for_selection, compute_auto_layout, compute_full_composition_layout,
    container_inactive, container_resize_interactions, final_node_positions, finish_node_reparent,
    flush_pending_continuous_edit, handle_context_menu, layout_needs_reflow,
    merge_images_target_node_id, node_can_splice_connection, node_drop_intents,
    node_editor_canvas_metadata, node_editor_details_visible,
    node_editor_port_interactions_enabled, node_editor_snarl_style_for, non_selectable_label,
    paint_container_foreground, port_owner_composition, port_owner_for_node_container,
    primary_node_drop_intent, push_history_snapshot, record_node_reparent_origins,
    register_container_chrome, register_implicit_time_context_wires, register_rendered_edges,
    register_reparent_drop_targets, rendered_edge_at_position, select_logical_item,
    selection_target_for_owner, show_wire_context_menu, splice_node_for_release, wire_interactions,
    wire_port_drop_rect, wire_secondary_click_hit, AutoLayoutScope, GraphItem,
    NodeContextMenuFrame, NodeEdit, OverviewWirePainter, ProjectNodeViewer, ReparentReleaseOutcome,
    SurfaceCapture, SurfaceProjection, TimeContextNode, WireInteractionFrame,
    WireSecondaryClickHit,
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

pub fn node_editor_panel(
    ui: &mut egui::Ui,
    project_lock: &Arc<RwLock<Project>>,
    project_service: &EditorService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
) {
    let comp_id = editor_context.active_composition_id;
    let current_time = f64::from(editor_context.timeline.current_time);
    let context_menu_state = &mut editor_context.node_editor_context_menu;
    let node_editor_state = &mut editor_context.node_editor_state;
    let Some(comp_id) = comp_id else {
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
    let (primary_down, primary_released) = ui.input(|input| {
        (
            input.pointer.primary_down(),
            input.pointer.primary_released(),
        )
    });
    let stale_merge_reorder =
        node_editor_state
            .merge_layer_reorder
            .as_ref()
            .is_some_and(|gesture| {
                let connection_exists = project_lock.read().is_ok_and(|project| {
                    project.connections.iter().any(|connection| {
                        connection.id == gesture.connection_id
                            && merge_images_target_node_id(&project, &connection.to)
                                == Some(gesture.merge_id)
                    })
                });
                !connection_exists || (!primary_down && !primary_released && !gesture.finished)
            });
    if stale_merge_reorder {
        node_editor_state.merge_layer_reorder = None;
    }

    let mut requested_layout = None;
    let mut selected_nodes = editor_context
        .selection
        .targets()
        .iter()
        .filter_map(|target| target.node_id())
        .collect::<Vec<_>>();
    selected_nodes.sort_unstable();
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
    ui.horizontal(|ui| {
        non_selectable_label(ui, "Clean layout");
        let all = ui
            .button("All")
            .on_hover_text("Lay out every track and node in this composition");
        register_layout_button(ui, &all, "node_editor.layout.all", "all");
        if all.clicked() {
            requested_layout = Some(AutoLayoutScope::All);
        }
        let selection = ui
            .add_enabled(!selected_nodes.is_empty(), egui::Button::new("Selection"))
            .on_hover_text("Lay out selected nodes without moving unselected nodes");
        register_layout_button(ui, &selection, "node_editor.layout.selection", "selection");
        if selection.clicked() {
            requested_layout = Some(AutoLayoutScope::Selection(selected_nodes.clone()));
        }
        let container = ui.button("Container").on_hover_text(
            "Lay out the selected track, or the composition if no track is selected",
        );
        register_layout_button(ui, &container, "node_editor.layout.container", "container");
        if container.clicked() {
            requested_layout = Some(AutoLayoutScope::Container(selected_container));
        }
    });
    ui.separator();

    if requested_layout.is_some() {
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

    let explicit_layout_changed = requested_layout.is_some_and(|scope| {
        let Ok(mut project) = project_lock.write() else {
            return false;
        };
        let Some(plan) = compute_auto_layout(&project, comp_id, scope) else {
            return false;
        };
        apply_auto_layout(&mut project, comp_id, &plan)
    });

    let mut snarl;
    let layout_edits;
    let rendered_edges;
    let mut suppress_wire_secondary_click = false;
    let mut edits = Vec::new();
    let mut drop_intents = Vec::new();
    let mut selection_changed = false;
    let mut context_menu_exclusion_rects = Vec::new();
    let mut wire_context_request = None;
    let mut snarl_selected_node_ids: Vec<Uuid>;
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
        let (built_snarl, containers) = build_snarl(&project, comp_id);
        snarl = built_snarl;

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
            current_time,
            context_menu_exclusion_rects: &mut context_menu_exclusion_rects,
            wire_context_request: &mut wire_context_request,
            suppress_wire_connect: node_editor_state.wire_gesture.is_some()
                || node_editor_state.normal_connect_gesture.is_some()
                || node_editor_state.normal_connect_cancel_pending_release
                || node_editor_state.merge_layer_reorder.is_some(),
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
        snarl.show(&mut viewer, &snarl_style, graph_id, ui);
        drop(viewer);
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
        snarl_selected_node_ids = egui_snarl::ui::get_selected_nodes(snarl_id, ui.ctx())
            .into_iter()
            .filter_map(|snarl_node_id| match snarl.get_node(snarl_node_id) {
                Some(GraphItem::Node(node_id)) => Some(*node_id),
                Some(GraphItem::Container(_) | GraphItem::PortAnchor { .. }) | None => None,
            })
            .collect();
        snarl_selected_node_ids.sort_unstable();
        let captured_drag_node_id = captured_snarl_drag_node(ui.ctx(), &snarl, snarl_id);
        let captured_drag_target = captured_snarl_drag_target(ui.ctx(), &snarl, snarl_id);
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
        let mut time_context_by_node = HashMap::<Uuid, (bool, bool)>::new();
        for node_id in selected_nodes
            .iter()
            .chain(snarl_selected_node_ids.iter())
            .copied()
        {
            time_context_by_node.entry(node_id).or_default().0 = true;
        }
        if let Some(node_id) = hovered_node_id {
            time_context_by_node.entry(node_id).or_default().1 = true;
        }
        let mut time_context_nodes = time_context_by_node
            .into_iter()
            .map(|(node_id, (selected, hovered))| TimeContextNode {
                node_id,
                selected,
                hovered,
            })
            .collect::<Vec<_>>();
        time_context_nodes.sort_by_key(|node| node.node_id);
        let context_wire_painter = ui.painter().with_clip_rect(canvas_clip);
        register_implicit_time_context_wires(
            &project,
            &rendered_ports,
            &time_context_nodes,
            canvas_clip,
            &context_wire_painter,
        );
        for container in &containers {
            paint_container_foreground(
                &foreground,
                &project,
                container,
                container_inactive(&project, container.owner, current_time),
                to_global.scaling,
            );
            register_container_chrome(container, to_global, canvas_clip, &project, current_time);
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
        let wire_owned_layout_before = wire_pointer_owns_layout(node_editor_state);
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

        let resize_owned_layout_before = node_editor_state.container_resize.is_some();
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
        let pointer_is_specialized = pointer_on_port
            || wire_owned_layout
            || resize_owned_layout
            || node_editor_state.wire_knife.is_some()
            || node_editor_state.merge_layer_reorder.is_some();

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

        let surface_was_active = node_editor_state.surface_interaction.is_active();
        let surface_outputs = if let (Ok(node_rects), Ok(port_rects), Ok(capture)) = (
            rendered_node_rects.lock(),
            rendered_ports.lock(),
            surface_capture.lock(),
        ) {
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
            let options = if node_editor_details_visible(to_global.scaling) {
                node_editor_ui::InteractionOptions::SELECTION
            } else {
                node_editor_ui::InteractionOptions::OVERVIEW_SELECTION
            };
            node_editor_ui::Editor::interact(
                ui,
                &projection.frame(),
                &mut node_editor_state.surface_interaction,
                options,
                pointer_is_specialized,
            )
        } else {
            Vec::new()
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
        let surface_owned_layout =
            surface_was_active || node_editor_state.surface_interaction.is_active();

        let layout_pointer_owned = wire_owned_layout || resize_owned_layout || surface_owned_layout;
        if primary_down && !layout_pointer_owned {
            if let Some(target) = captured_drag_target {
                if node_editor_state.active_drag_selection != Some(target) {
                    selection_changed |=
                        select_logical_item(&mut editor_context.selection, target, modifiers.shift);
                    node_editor_state.active_drag_selection = Some(target);
                }
            }
        }

        let mut collected = collect_layout_edits_for_selection(
            &project,
            &snarl,
            node_editor_state.active_drag_selection,
            editor_context.selection.targets(),
        );
        // A specialized gesture or marquee owns the physical pointer. If a
        // backend batches press and motion into one RawInput, Snarl may have
        // calculated a stale competing move earlier in this same frame.
        if layout_pointer_owned {
            collected.clear();
        }
        collected.extend(resize_edits);
        let gesture_allowed = (primary_down || primary_released)
            && node_editor_state.container_resize.is_none()
            && !node_editor_state.surface_interaction.is_marquee_active()
            && node_editor_state.wire_gesture.is_none()
            && node_editor_state.normal_connect_gesture.is_none()
            && node_editor_state.wire_knife.is_none()
            && node_editor_state.merge_layer_reorder.is_none();
        record_node_reparent_origins(&project, &collected, node_editor_state, gesture_allowed);
        if let (Some(pointer_position), Ok(node_rects)) =
            (pointer_position, rendered_node_rects.lock())
        {
            let graph_drop_point = to_global.inverse() * pointer_position;
            if let Some(state) = node_editor_state.node_reparent.as_mut() {
                if state.primary_node_id.is_none() {
                    state.primary_node_id =
                        captured_drag_node_id.filter(|node_id| state.origins.contains_key(node_id));
                }
            }
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
        if !primary_down && !primary_released {
            node_editor_state.node_reparent = None;
            node_editor_state.moved_node_ids.clear();
            node_editor_state.active_drag_selection = None;
        }
        if node_editor_state.merge_layer_reorder.is_some() {
            collected.clear();
        }
        layout_edits = collected;
    }

    if selection_changed {
        editor_context.interaction.preview_edit_target = None;
        // Inspector is normally laid out before Node Editor in the dock tree.
        // Wake one follow-up frame so a Node click replaces the previous
        // Inspector owner even when playback and all animations are idle.
        ui.ctx().request_repaint();
    }

    let primary_released = ui.input(|input| input.pointer.primary_released());
    if primary_released {
        node_editor_state.active_drag_selection = None;
    }

    let mut layout_changed = false;
    if let Ok(mut project) = project_lock.write() {
        if apply_queued_node_edits(&mut project, edits, history_manager, node_editor_state) {
            // Render completion is asynchronous. Wake the UI immediately so
            // a paused Preview observes this authoritative graph mutation
            // without waiting for unrelated pointer input.
            ui.ctx().request_repaint();
        }
        for edit in layout_edits {
            layout_changed |= apply_layout_edit(&mut project, edit);
        }
        if primary_released {
            let reparent_gesture = node_editor_state.node_reparent.take();
            let moved_node_ids = reparent_gesture
                .as_ref()
                .map(|gesture| gesture.origins.keys().copied().collect::<HashSet<_>>())
                .unwrap_or_default();
            let captured_primary_node_id = reparent_gesture
                .as_ref()
                .and_then(|gesture| gesture.primary_node_id);
            node_editor_state.moved_node_ids.clear();
            let reparent_outcome =
                finish_node_reparent(&mut project, &drop_intents, reparent_gesture.as_ref());
            layout_changed |= reparent_outcome != ReparentReleaseOutcome::NoIntent;
            let dropped_wire = ui
                .input(|input| input.pointer.interact_pos())
                .and_then(|position| rendered_edge_at_position(&rendered_edges, position))
                .and_then(|edge| edge.kind.connection_id());
            if let (Some(connection_id), Some(node_id)) = (
                dropped_wire,
                splice_node_for_release(
                    reparent_outcome,
                    &moved_node_ids,
                    captured_primary_node_id,
                ),
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
        }
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
    let layout_finished = primary_released && node_editor_state.layout_changed_during_drag;
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

fn register_layout_button(ui: &egui::Ui, response: &egui::Response, id: &str, scope: &str) {
    crate::qa::register_component_with_metadata(
        id,
        "node_editor_layout_button",
        response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "scope": scope,
            "visible": ui.is_rect_visible(response.rect),
        })),
    );
}
