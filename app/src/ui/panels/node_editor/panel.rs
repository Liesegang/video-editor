use crate::action::HistoryManager;
use crate::state::context::EditorContext;
use crate::state::context_types::{
    NodeEditorEditableWire, NodeEditorWireContextMenu, SelectionTarget,
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
    capture_container_resize_before_canvas, captured_snarl_drag_node, collect_layout_edits,
    compute_auto_layout, compute_full_composition_layout, container_inactive,
    container_resize_interactions, final_node_positions, finish_node_reparent,
    flush_pending_continuous_edit, handle_context_menu, layout_needs_reflow,
    merge_images_target_node_id, node_can_splice_connection, node_drop_intents,
    node_editor_canvas_metadata, node_editor_details_visible, node_editor_snarl_style,
    node_selection_after_snarl_click, non_selectable_label, paint_container_foreground,
    port_owner_composition, port_owner_for_node_container, primary_node_drop_intent,
    push_history_snapshot, record_node_reparent_origins, register_container_chrome,
    register_implicit_time_context_wires, register_rendered_edges, register_reparent_drop_targets,
    rendered_edge_at_position, show_wire_context_menu, splice_node_for_release, wire_interactions,
    wire_secondary_click_hit, AutoLayoutScope, GraphItem, NodeContextMenuFrame, NodeEdit,
    OverviewWirePainter, ProjectNodeViewer, ReparentReleaseOutcome, TimeContextNode,
    WireInteractionFrame, WireSecondaryClickHit,
};

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
        if ui
            .button("All")
            .on_hover_text("Lay out every track and node in this composition")
            .clicked()
        {
            requested_layout = Some(AutoLayoutScope::All);
        }
        if ui
            .add_enabled(!selected_nodes.is_empty(), egui::Button::new("Selection"))
            .on_hover_text("Lay out selected nodes without moving unselected nodes")
            .clicked()
        {
            requested_layout = Some(AutoLayoutScope::Selection(selected_nodes.clone()));
        }
        if ui
            .button("Container")
            .on_hover_text("Lay out the selected track, or the composition if no track is selected")
            .clicked()
        {
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
    let mut pending_selection = None;
    let mut context_menu_exclusion_rects = Vec::new();
    let mut wire_context_request = None;
    let mut snarl_selected_node_ids: Vec<Uuid>;
    let mut to_global = egui::emath::TSTransform::default();
    let mut canvas_clip = canvas_rect;
    let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
    let rendered_node_rects = Arc::new(Mutex::new(HashMap::new()));
    let plugin_manager = project_service.get_plugin_manager();
    {
        let Ok(project) = project_lock.read() else {
            ui.label("Project is unavailable");
            return;
        };
        let (built_snarl, containers) = build_snarl(&project, comp_id);
        snarl = built_snarl;

        if let Some(previous_transform) = node_editor_state.node_editor_canvas_transform {
            capture_container_resize_before_canvas(
                ui,
                &containers,
                previous_transform,
                canvas_rect,
                node_editor_state,
            );
        }

        let mut viewer = ProjectNodeViewer {
            project: &project,
            plugin_manager: Some(plugin_manager.as_ref()),
            containers: &containers,
            edits: &mut edits,
            pending_navigation: &mut node_editor_state.pending_navigation,
            pending_selection: &mut pending_selection,
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
            to_global: &mut to_global,
            canvas_clip: &mut canvas_clip,
            rendered_ports: Arc::clone(&rendered_ports),
            merge_layer_reorder: &mut node_editor_state.merge_layer_reorder,
            rendered_node_rects: Arc::clone(&rendered_node_rects),
        };
        let snarl_style = node_editor_snarl_style();
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
            });
            *context_menu_state = None;
        }
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

        let mut collected = collect_layout_edits(&project, &snarl);
        collected.extend(container_resize_interactions(
            ui,
            &project,
            &containers,
            to_global,
            canvas_clip,
            node_editor_state,
        ));
        let (primary_down, primary_released, pointer_position) = ui.input(|input| {
            (
                input.pointer.primary_down(),
                input.pointer.primary_released(),
                input.pointer.interact_pos(),
            )
        });
        let gesture_allowed = (primary_down || primary_released)
            && node_editor_state.container_resize.is_none()
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
        }
        if node_editor_state.merge_layer_reorder.is_some() {
            collected.clear();
        }
        layout_edits = collected;
    }

    let selection_modifiers = ui.input(|input| input.modifiers);
    let mut selection_changed = false;
    if let Some(owner) = pending_selection {
        if let Ok(project) = project_lock.read() {
            match owner {
                PortOwner::Node(node_id) if project.get_node(node_id).is_some() => {
                    let (targets, primary) = node_selection_after_snarl_click(
                        editor_context.selection.targets(),
                        editor_context.selection.primary(),
                        &snarl_selected_node_ids,
                        node_id,
                        selection_modifiers,
                    );
                    editor_context.selection.replace(targets, primary);
                    selection_changed = true;
                }
                PortOwner::Clip(clip_id) if project.get_clip(clip_id).is_some() => {
                    editor_context.selection.replace(
                        [SelectionTarget::Clip(clip_id)],
                        Some(SelectionTarget::Clip(clip_id)),
                    );
                    selection_changed = true;
                }
                PortOwner::Track(track_id) if project.get_track(track_id).is_some() => {
                    editor_context.selection.replace(
                        [SelectionTarget::Track(track_id)],
                        Some(SelectionTarget::Track(track_id)),
                    );
                    selection_changed = true;
                }
                PortOwner::Composition(composition_id) if composition_id == comp_id => {
                    editor_context.selection.replace(
                        [SelectionTarget::Composition(composition_id)],
                        Some(SelectionTarget::Composition(composition_id)),
                    );
                    selection_changed = true;
                }
                _ => {}
            }
        }
    }

    if !selection_changed
        && ui.input(|input| input.pointer.primary_released())
        && snarl_selected_node_ids.len() > 1
    {
        let targets = snarl_selected_node_ids
            .iter()
            .copied()
            .map(SelectionTarget::Node)
            .collect::<Vec<_>>();
        let primary = editor_context
            .selection
            .primary()
            .filter(|target| targets.contains(target))
            .or_else(|| targets.first().copied());
        editor_context.selection.replace(targets, primary);
        selection_changed = true;
    }
    if selection_changed {
        editor_context.interaction.preview_edit_target = None;
    }

    let primary_released = ui.input(|input| input.pointer.primary_released());

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
