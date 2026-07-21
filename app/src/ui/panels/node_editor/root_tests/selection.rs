use super::*;

#[test]
fn real_egui_capture_selects_the_top_overlapping_node_for_a_multi_drag() {
    let (mut project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
    assert!(project.remove_node(solid_id).unwrap().is_some());
    assert!(project.remove_node(merge_id).unwrap().is_some());
    if let Some(clip) = project.get_clip_mut(clip_id) {
        clip.ui_size = [1_300.0, 760.0];
    }
    if let Some(track) = project.get_track_mut(track_id) {
        track.ui_size = [1_800.0, 1_050.0];
    }
    if let Some(composition) = project.get_composition_mut(composition_id) {
        composition.ui_size = [2_200.0, 1_400.0];
    }
    let clip_content = project.get_clip(clip_id).map(|clip| {
        nested_content_rect(
            container_rect(clip.ui_position, clip.ui_size),
            AUTO_LAYOUT_CLIP_TOP,
        )
    });
    assert!(clip_content.is_some());
    let Some(clip_content) = clip_content else {
        return;
    };
    let mut lower = Node::new_merge("Capture Lower");
    lower.id = Uuid::from_u128(1);
    lower.ui_position = [clip_content.min.x + 40.0, clip_content.min.y + 40.0];
    let lower_id = lower.id;
    let mut upper = Node::new_merge("Capture Upper");
    upper.id = Uuid::from_u128(2);
    upper.ui_position = [clip_content.min.x + 520.0, clip_content.min.y + 40.0];
    let upper_id = upper.id;
    project.add_node(lower);
    project.add_node(upper);
    assert!(project
        .attach_node_to_container(NodeContainer::Clip(clip_id), lower_id)
        .is_ok());
    assert!(project
        .attach_node_to_container(NodeContainer::Clip(clip_id), upper_id)
        .is_ok());

    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 1200.0));
    let graph_id = egui::Id::new(("captured-overlap-drag", composition_id));
    let (mut snarl, containers) = build_snarl(&project, composition_id);
    let rendered_node_rects = Arc::new(Mutex::new(HashMap::new()));
    reset_test_rects();
    let render_frame = |project: &Project,
                        snarl: &mut Snarl<GraphItem>,
                        frame: usize,
                        events: Vec<egui::Event>,
                        modifiers: egui::Modifiers| {
        let mut layout_edits = Vec::new();
        let mut captured = None;
        let mut persistent_snarl_id = None;
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(frame as f64 / 60.0),
                events,
                modifiers,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let mut edits = Vec::new();
                    let mut navigation = None;
                    let mut wire_context_request = None;
                    let mut exclusions = Vec::new();
                    let mut to_global = egui::emath::TSTransform::IDENTITY;
                    let mut canvas_clip = ui.clip_rect();
                    let mut merge_layer_reorder = None;
                    let mut viewer = ProjectNodeViewer {
                        project,
                        plugin_manager: None,
                        containers: &containers,
                        edits: &mut edits,
                        pending_navigation: &mut navigation,
                        selected_node_ids: &[],
                        current_time: 0.0,
                        context_menu_exclusion_rects: &mut exclusions,
                        wire_context_request: &mut wire_context_request,
                        suppress_wire_connect: false,
                        locked_canvas_transform: None,
                        previous_canvas_transform: None,
                        to_global: &mut to_global,
                        canvas_clip: &mut canvas_clip,
                        rendered_ports: Arc::new(Mutex::new(HashMap::new())),
                        merge_layer_reorder: &mut merge_layer_reorder,
                        rendered_node_rects: Arc::clone(&rendered_node_rects),
                    };
                    snarl.show(&mut viewer, &node_editor_snarl_style(), graph_id, ui);
                    drop(viewer);
                    let snarl_id = ui.make_persistent_id(graph_id);
                    persistent_snarl_id = Some(snarl_id);
                    captured = captured_snarl_drag_node(ui.ctx(), snarl, snarl_id);
                    layout_edits = collect_layout_edits(project, snarl);
                });
            },
        ));
        (layout_edits, captured, persistent_snarl_id)
    };

    let mut persistent_snarl_id = None;
    for frame in 0..4 {
        let (_, _, frame_snarl_id) = render_frame(
            &project,
            &mut snarl,
            frame,
            Vec::new(),
            egui::Modifiers::NONE,
        );
        persistent_snarl_id = frame_snarl_id;
    }
    let lower_rect = test_rect(&format!("node_editor.node:{lower_id}"));
    let upper_rect = test_rect(&format!("node_editor.node:{upper_id}"));
    assert!(lower_rect.is_some_and(|rect| rect.is_positive()));
    assert!(upper_rect.is_some_and(|rect| rect.is_positive()));
    assert!(test_rect(&format!("node_editor.node_header:{upper_id}"))
        .is_some_and(|rect| rect.is_positive()));
    let Some(persistent_snarl_id) = persistent_snarl_id else {
        return;
    };
    let snarl_node_ids = snarl
        .node_ids()
        .filter_map(|(snarl_id, item)| match item {
            GraphItem::Node(project_id) if [lower_id, upper_id].contains(project_id) => {
                Some((*project_id, snarl_id))
            }
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let (Some(lower_snarl_id), Some(upper_snarl_id)) = (
        snarl_node_ids.get(&lower_id).copied(),
        snarl_node_ids.get(&upper_id).copied(),
    ) else {
        return;
    };
    // Establish the group through Snarl's real rectangle-selection path.
    // Starting in the canvas margin avoids invisible container controls;
    // selecting the other graph items too is harmless for this capture
    // test and exercises the same group-drag path.
    let shift = egui::Modifiers {
        shift: true,
        ..Default::default()
    };
    let mut frame = 4;
    let selection_start = screen.min + egui::vec2(20.0, 20.0);
    let selection_drag_start = selection_start + egui::vec2(10.0, 10.0);
    let selection_end = screen.max - egui::vec2(20.0, 20.0);
    for events in [
        vec![egui::Event::PointerMoved(selection_start)],
        vec![egui::Event::PointerButton {
            pos: selection_start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: shift,
        }],
        vec![egui::Event::PointerMoved(selection_drag_start)],
        vec![egui::Event::PointerMoved(selection_end)],
        vec![egui::Event::PointerButton {
            pos: selection_end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: shift,
        }],
    ] {
        render_frame(&project, &mut snarl, frame, events, shift);
        frame += 1;
    }
    let selected = egui_snarl::ui::get_selected_nodes(persistent_snarl_id, &context);
    assert!(
        selected.contains(&lower_snarl_id),
        "lower {lower_snarl_id:?} selection missing from {selected:?}; upper is {upper_snarl_id:?}"
    );
    assert!(
        selected.contains(&upper_snarl_id),
        "upper selection missing from {selected:?}"
    );
    // A newly visible body changes Snarl's measured Node width while its
    // open animation settles. Coordinate input must use geometry from a
    // settled frame, just like the HTTP QA client does.
    let mut previous_upper_header = None;
    let mut stable_header_frames = 0;
    let mut settled_upper_header = None;
    for _ in 0..30 {
        render_frame(
            &project,
            &mut snarl,
            frame,
            Vec::new(),
            egui::Modifiers::NONE,
        );
        frame += 1;
        let current = test_rect(&format!("node_editor.node_header:{upper_id}"));
        if current.is_some_and(|rect| rect.is_positive()) && current == previous_upper_header {
            stable_header_frames += 1;
            if stable_header_frames >= 2 {
                settled_upper_header = current;
                break;
            }
        } else {
            stable_header_frames = 0;
        }
        previous_upper_header = current;
    }
    let Some(upper_header) = settled_upper_header else {
        panic!("Merge header geometry did not settle before coordinate input");
    };
    assert!(upper_header.is_positive());
    let upper_header_center = upper_header.center();
    let command = egui::Modifiers {
        command: true,
        mac_cmd: cfg!(target_os = "macos"),
        ctrl: !cfg!(target_os = "macos"),
        ..Default::default()
    };
    for events in [
        vec![egui::Event::PointerMoved(upper_header_center)],
        vec![egui::Event::PointerButton {
            pos: upper_header_center,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: command,
        }],
        vec![egui::Event::PointerButton {
            pos: upper_header_center,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: command,
        }],
    ] {
        render_frame(&project, &mut snarl, frame, events, command);
        frame += 1;
    }
    let selected_after_command = egui_snarl::ui::get_selected_nodes(persistent_snarl_id, &context);
    assert!(selected_after_command.contains(&lower_snarl_id));
    assert!(!selected_after_command.contains(&upper_snarl_id));
    let post_show_project_ids = selected_after_command
        .iter()
        .filter_map(|snarl_id| match snarl.get_node(*snarl_id) {
            Some(GraphItem::Node(node_id)) => Some(*node_id),
            Some(GraphItem::Container(_) | GraphItem::PortAnchor { .. }) | None => None,
        })
        .collect::<Vec<_>>();
    let (typed_targets, typed_primary) = node_selection_after_snarl_click(
        &[
            SelectionTarget::Node(lower_id),
            SelectionTarget::Node(upper_id),
        ],
        Some(SelectionTarget::Node(upper_id)),
        &post_show_project_ids,
        upper_id,
        command,
    );
    assert_eq!(typed_targets, vec![SelectionTarget::Node(lower_id)]);
    assert_eq!(typed_primary, Some(SelectionTarget::Node(lower_id)));

    // Restore the group for the overlapping multi-drag assertion below.
    let upper_header = test_rect(&format!("node_editor.node_header:{upper_id}"));
    assert!(upper_header.is_some_and(|rect| rect.is_positive()));
    let Some(upper_header) = upper_header else {
        return;
    };
    let upper_header_center = upper_header.center();
    for events in [
        vec![egui::Event::PointerMoved(upper_header_center)],
        vec![egui::Event::PointerButton {
            pos: upper_header_center,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: shift,
        }],
        vec![egui::Event::PointerButton {
            pos: upper_header_center,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: shift,
        }],
    ] {
        render_frame(&project, &mut snarl, frame, events, shift);
        frame += 1;
    }

    let overlap_position = egui::pos2(clip_content.min.x + 260.0, clip_content.min.y + 220.0);
    for node_id in [lower_id, upper_id] {
        if let Some(node) = project.get_node_mut(node_id) {
            node.ui_position = [overlap_position.x, overlap_position.y];
        }
    }
    for node_id in [lower_id, upper_id] {
        if let Some(snarl_id) = snarl_node_ids.get(&node_id).copied() {
            if let Some(node) = snarl.get_node_info_mut(snarl_id) {
                node.pos = overlap_position;
            }
        }
    }
    for _ in 0..2 {
        render_frame(
            &project,
            &mut snarl,
            frame,
            Vec::new(),
            egui::Modifiers::NONE,
        );
        frame += 1;
    }
    let top_header = test_rect(&format!("node_editor.node_header:{upper_id}"));
    assert!(top_header.is_some_and(|rect| rect.is_positive()));
    let Some(top_header) = top_header else {
        return;
    };
    let start = top_header.center();
    render_frame(
        &project,
        &mut snarl,
        frame,
        vec![egui::Event::PointerMoved(start)],
        egui::Modifiers::NONE,
    );
    frame += 1;
    render_frame(
        &project,
        &mut snarl,
        frame,
        vec![egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
        egui::Modifiers::NONE,
    );
    frame += 1;
    let end = start + egui::vec2(48.0, 24.0);
    let (layout_edits, captured, _) = render_frame(
        &project,
        &mut snarl,
        frame,
        vec![egui::Event::PointerMoved(end)],
        egui::Modifiers::NONE,
    );
    assert_eq!(captured, Some(upper_id));
    assert_ne!(captured, Some(lower_id));
    let moved_nodes = layout_edits
        .iter()
        .filter_map(|edit| match edit {
            LayoutEdit::MoveNode { node_id, .. }
                if *node_id == lower_id || *node_id == upper_id =>
            {
                Some(*node_id)
            }
            _ => None,
        })
        .collect::<HashSet<_>>();
    assert_eq!(moved_nodes, HashSet::from([lower_id, upper_id]));
    let mut state = NodeEditorState::default();
    record_node_reparent_origins(&project, &layout_edits, &mut state, true);
    if let Some(gesture) = state.node_reparent.as_mut() {
        gesture.primary_node_id = captured.filter(|node_id| gesture.origins.contains_key(node_id));
    }
    assert_eq!(
        state
            .node_reparent
            .as_ref()
            .and_then(|gesture| gesture.primary_node_id),
        Some(upper_id)
    );
}

#[test]
fn cmd_deselect_uses_post_snarl_state_without_reselecting_clicked_node() {
    let first = Uuid::from_u128(1);
    let second = Uuid::from_u128(2);
    let current = [SelectionTarget::Node(first), SelectionTarget::Node(second)];
    let command = egui::Modifiers {
        command: true,
        mac_cmd: cfg!(target_os = "macos"),
        ctrl: !cfg!(target_os = "macos"),
        ..Default::default()
    };

    let (targets, primary) = node_selection_after_snarl_click(
        &current,
        Some(SelectionTarget::Node(second)),
        &[first],
        second,
        command,
    );

    assert_eq!(targets, vec![SelectionTarget::Node(first)]);
    assert_eq!(primary, Some(SelectionTarget::Node(first)));
}

#[test]
fn shift_node_selection_keeps_same_uuid_non_node_target() {
    let shared_id = Uuid::from_u128(1);
    let other_node_id = Uuid::from_u128(2);
    let shift = egui::Modifiers {
        shift: true,
        ..Default::default()
    };

    let (targets, primary) = node_selection_after_snarl_click(
        &[SelectionTarget::Clip(shared_id)],
        Some(SelectionTarget::Clip(shared_id)),
        &[other_node_id, shared_id, shared_id],
        shared_id,
        shift,
    );

    assert_eq!(
        targets,
        vec![
            SelectionTarget::Clip(shared_id),
            SelectionTarget::Node(other_node_id),
            SelectionTarget::Node(shared_id),
        ]
    );
    assert_eq!(primary, Some(SelectionTarget::Node(shared_id)));
}

#[test]
fn real_egui_node_header_drag_reparents_once_from_final_snarl_rect() {
    let (mut project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
    if let Some(track) = project.get_track_mut(track_id) {
        track.ui_size = [1_800.0, 1_000.0];
    }
    if let Some(composition) = project.get_composition_mut(composition_id) {
        composition.ui_size = [2_200.0, 1_400.0];
    }
    let track_structural_merge_id = project
        .get_track(track_id)
        .expect("fixture Track")
        .structural_merge_node_id;
    if let Some(structural_merge) = project.get_node_mut(track_structural_merge_id) {
        // This test exercises the Solid header gesture itself. Keep the
        // generated Track sink clear of that header; production performs
        // the same collision repair before the first interactive frame.
        structural_merge.ui_position = [1_450.0, 760.0];
    }
    assert!(project
        .set_output_node(NodeContainer::Clip(clip_id), Some(solid_id))
        .is_ok());
    let explicit_wire = project
        .connections
        .iter()
        .find(|connection| {
            connection.from.owner == PortOwner::Node(solid_id)
                && connection.to.owner == PortOwner::Node(merge_id)
        })
        .cloned();
    assert!(explicit_wire.is_some());
    let initial = project.clone();
    let initial_position = project.get_node(solid_id).map(|node| node.ui_position);
    assert!(initial_position.is_some());
    let mut history = HistoryManager::new();
    history.push_project_state(initial.clone());

    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 1200.0));
    let (mut snarl, containers) = build_snarl(&project, composition_id);
    let rendered_node_rects = Arc::new(Mutex::new(HashMap::new()));
    let mut state = NodeEditorState::default();
    let mut final_transform = egui::emath::TSTransform::IDENTITY;
    reset_test_rects();

    for frame in 0..5 {
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(frame as f64 / 60.0),
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let mut edits = Vec::new();
                    let mut navigation = None;
                    let mut wire_context_request = None;
                    let mut exclusions = Vec::new();
                    let mut to_global = egui::emath::TSTransform::IDENTITY;
                    let mut canvas_clip = ui.clip_rect();
                    let mut merge_layer_reorder = None;
                    let mut viewer = ProjectNodeViewer {
                        project: &project,
                        plugin_manager: None,
                        containers: &containers,
                        edits: &mut edits,
                        pending_navigation: &mut navigation,
                        selected_node_ids: &[],
                        current_time: 0.0,
                        context_menu_exclusion_rects: &mut exclusions,
                        wire_context_request: &mut wire_context_request,
                        suppress_wire_connect: false,
                        locked_canvas_transform: None,
                        previous_canvas_transform: None,
                        to_global: &mut to_global,
                        canvas_clip: &mut canvas_clip,
                        rendered_ports: Arc::new(Mutex::new(HashMap::new())),
                        merge_layer_reorder: &mut merge_layer_reorder,
                        rendered_node_rects: Arc::clone(&rendered_node_rects),
                    };
                    snarl.show(
                        &mut viewer,
                        &node_editor_snarl_style(),
                        egui::Id::new(("real-reparent-drag", composition_id)),
                        ui,
                    );
                    final_transform = to_global;
                });
            },
        ));
    }

    let header = test_rect(&format!("node_editor.node_header:{solid_id}"));
    assert!(header.is_some_and(|rect| rect.is_positive()));
    let Some(header) = header else {
        return;
    };
    let Some(initial_position) = initial_position else {
        return;
    };
    let start = header.center();
    let desired_position = [1_120.0, 470.0];
    let graph_delta = egui::vec2(
        desired_position[0] - initial_position[0],
        desired_position[1] - initial_position[1],
    );
    let end = start + graph_delta * final_transform.scaling;
    assert!(screen.contains(end));
    let drag_start = start + (end - start).normalized() * 12.0;
    let event_frames = [
        vec![egui::Event::PointerMoved(start)],
        vec![egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
        vec![egui::Event::PointerMoved(drag_start)],
        vec![egui::Event::PointerMoved(end)],
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    ];
    let mut history_commits = 0;
    let mut release_outcome = ReparentReleaseOutcome::NoIntent;
    for (offset, events) in event_frames.into_iter().enumerate() {
        let mut frame_layout_edits = Vec::new();
        let mut frame_drop_intents = Vec::new();
        let mut frame_released = false;
        if let Ok(mut rects) = rendered_node_rects.lock() {
            rects.clear();
        }
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some((offset + 5) as f64 / 60.0),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let mut edits = Vec::new();
                    let mut navigation = None;
                    let mut wire_context_request = None;
                    let mut exclusions = Vec::new();
                    let mut to_global = egui::emath::TSTransform::IDENTITY;
                    let mut canvas_clip = ui.clip_rect();
                    let mut merge_layer_reorder = None;
                    let mut viewer = ProjectNodeViewer {
                        project: &project,
                        plugin_manager: None,
                        containers: &containers,
                        edits: &mut edits,
                        pending_navigation: &mut navigation,
                        selected_node_ids: &[],
                        current_time: 0.0,
                        context_menu_exclusion_rects: &mut exclusions,
                        wire_context_request: &mut wire_context_request,
                        suppress_wire_connect: false,
                        locked_canvas_transform: None,
                        previous_canvas_transform: None,
                        to_global: &mut to_global,
                        canvas_clip: &mut canvas_clip,
                        rendered_ports: Arc::new(Mutex::new(HashMap::new())),
                        merge_layer_reorder: &mut merge_layer_reorder,
                        rendered_node_rects: Arc::clone(&rendered_node_rects),
                    };
                    let graph_id = egui::Id::new(("real-reparent-drag", composition_id));
                    snarl.show(&mut viewer, &node_editor_snarl_style(), graph_id, ui);
                    drop(viewer);
                    let captured_drag_node_id =
                        captured_snarl_drag_node(ui.ctx(), &snarl, ui.make_persistent_id(graph_id));
                    frame_layout_edits = collect_layout_edits(&project, &snarl);
                    let (primary_down, primary_released, pointer) = ui.input(|input| {
                        (
                            input.pointer.primary_down(),
                            input.pointer.primary_released(),
                            input.pointer.interact_pos(),
                        )
                    });
                    frame_released = primary_released;
                    record_node_reparent_origins(
                        &project,
                        &frame_layout_edits,
                        &mut state,
                        primary_down || primary_released,
                    );
                    let Some(pointer) = pointer else {
                        return;
                    };
                    let graph_point = to_global.inverse() * pointer;
                    let Ok(rects) = rendered_node_rects.lock() else {
                        return;
                    };
                    if let Some(gesture) = state.node_reparent.as_mut() {
                        if gesture.primary_node_id.is_none() {
                            gesture.primary_node_id = captured_drag_node_id
                                .filter(|node_id| gesture.origins.contains_key(node_id));
                        }
                    }
                    if let Some(gesture) = state.node_reparent.as_ref() {
                        frame_drop_intents = node_drop_intents(
                            &project,
                            composition_id,
                            gesture,
                            &rects,
                            &final_node_positions(&project, gesture, &frame_layout_edits),
                            graph_point,
                            to_global.scaling,
                        );
                    }
                });
            },
        ));
        let mut frame_changed = false;
        for edit in frame_layout_edits {
            frame_changed |= apply_layout_edit(&mut project, edit);
        }
        if frame_released {
            let reparent_gesture = state.node_reparent.take();
            release_outcome =
                finish_node_reparent(&mut project, &frame_drop_intents, reparent_gesture.as_ref());
            frame_changed |= release_outcome != ReparentReleaseOutcome::NoIntent;
            state.moved_node_ids.clear();
            if frame_changed {
                history.push_project_state(project.clone());
                history_commits += 1;
            }
        }
    }

    assert_eq!(release_outcome, ReparentReleaseOutcome::Applied);
    assert_eq!(history_commits, 1);
    assert_eq!(history.undo_depth(), 2);
    assert_eq!(
        project.find_node_container(solid_id),
        Some(NodeContainer::Track(track_id))
    );
    assert_eq!(
        project
            .get_clip(clip_id)
            .and_then(|clip| clip.output_node_id),
        None
    );
    assert_eq!(
        explicit_wire.as_ref().and_then(|wire| {
            project
                .connections
                .iter()
                .find(|connection| connection.id == wire.id)
        }),
        explicit_wire.as_ref(),
    );
    assert!(project
        .get_node(solid_id)
        .is_some_and(|node| node.ui_position != initial_position));
    assert!(project.validate_containment().is_empty());
    assert!(project.validate_connections().is_empty());
    let edited = project.clone();
    assert_single_gesture_undo_redo(&mut history, &initial, &edited);
}
