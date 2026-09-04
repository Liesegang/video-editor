use super::support::*;

#[test]
fn move_adapter_preserves_typed_ids_and_prunes_selected_ancestors() {
    let (mut project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
    let delta = egui::vec2(13.0, -7.0);
    let nested = [SurfaceOutput::Move {
        items: vec![
            ItemId::Node(solid_id),
            ItemId::Group(ProjectPortOwner::Clip(clip_id)),
            ItemId::Group(ProjectPortOwner::Track(track_id)),
            ItemId::Node(merge_id),
            ItemId::Group(ProjectPortOwner::Composition(composition_id)),
        ],
        grabbed: ItemId::Node(solid_id),
        delta,
    }];
    let change = move_change(&project, &nested).expect("generic Move adapter output");
    assert_eq!(change.grabbed_node, Some(solid_id));
    assert!(matches!(
        change.edits.as_slice(),
        [LayoutEdit::MoveContainer { owner, delta: edit_delta }]
            if *owner == ProjectPortOwner::Composition(composition_id)
                && *edit_delta == [delta.x, delta.y]
    ));

    let mut same_id_node = Node::new_merge("Same UUID as Clip");
    same_id_node.id = clip_id;
    same_id_node.ui_position = [90.0, 60.0];
    project.add_node(same_id_node);
    project
        .attach_node_to_container(NodeContainer::Composition(composition_id), clip_id)
        .expect("cross-kind UUID remains a valid typed owner");
    let typed = [SurfaceOutput::Move {
        items: vec![
            ItemId::Group(ProjectPortOwner::Clip(clip_id)),
            ItemId::Node(clip_id),
        ],
        grabbed: ItemId::Node(clip_id),
        delta,
    }];
    let change = move_change(&project, &typed).expect("typed movement");
    assert_eq!(change.edits.len(), 2);
    assert!(matches!(
        change.edits[0],
        LayoutEdit::MoveContainer {
            owner: ProjectPortOwner::Clip(id),
            delta: edit_delta,
        } if id == clip_id && edit_delta == [delta.x, delta.y]
    ));
    assert!(matches!(
        change.edits[1],
        LayoutEdit::MoveNode { node_id, position }
            if node_id == clip_id && position == [103.0, 53.0]
    ));
}

#[test]
fn move_end_adapter_preserves_released_and_cancelled_outcomes() {
    assert_eq!(
        move_end(&[SurfaceOutput::MoveEnd {
            outcome: MoveEndOutcome::Released,
        }]),
        Some(MoveEndOutcome::Released)
    );
    assert_eq!(
        move_end(&[SurfaceOutput::MoveEnd {
            outcome: MoveEndOutcome::Cancelled,
        }]),
        Some(MoveEndOutcome::Cancelled)
    );
    assert_eq!(move_end(&[]), None);
}

#[test]
fn production_pointer_drag_moves_selected_nodes_and_emits_final_release_delta() {
    let (mut project, _composition_id, _track_id, _clip_id, solid_id, merge_id) = fixture();
    let initial_solid = project.get_node(solid_id).unwrap().ui_position;
    let initial_merge = project.get_node(merge_id).unwrap().ui_position;
    let mut node_rects = HashMap::from([
        (
            solid_id,
            egui::Rect::from_min_size(
                egui::pos2(initial_solid[0], initial_solid[1]),
                egui::vec2(180.0, 120.0),
            ),
        ),
        (
            merge_id,
            egui::Rect::from_min_size(
                egui::pos2(initial_merge[0], initial_merge[1]),
                egui::vec2(180.0, 120.0),
            ),
        ),
    ]);
    let mut capture = SurfaceCapture::default();
    for (node_id, rect) in &node_rects {
        capture.record_selectable(SelectionTarget::Node(*node_id));
        capture.record_node_header(
            *node_id,
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 28.0)),
        );
    }
    let selected = [
        SelectionTarget::Node(solid_id),
        SelectionTarget::Node(merge_id),
    ];
    let transform = egui::emath::TSTransform::new(egui::vec2(35.0, 45.0), 1.5);
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_400.0, 1_000.0));
    let context = egui::Context::default();
    let mut state = SurfaceState::default();
    let start = transform
        * egui::Rect::from_min_size(
            node_rects[&solid_id].min,
            egui::vec2(node_rects[&solid_id].width(), 28.0),
        )
        .center();

    let projection = SurfaceProjection::from_project(
        &project,
        &[],
        &node_rects,
        &HashMap::new(),
        &capture,
        &[],
        &selected,
        Some(SelectionTarget::Node(merge_id)),
        None,
        viewport,
        transform,
    );
    let _ = run_pointer_frame(
        &context,
        &projection,
        &mut state,
        node_editor_ui::InteractionOptions::SELECTION_AND_MOVE,
        vec![
            egui::Event::PointerMoved(start),
            pointer_button(start, true),
        ],
    );
    drop(projection);
    assert!(state.is_move_active());

    let first_delta = egui::vec2(20.0, 12.0);
    let projection = SurfaceProjection::from_project(
        &project,
        &[],
        &node_rects,
        &HashMap::new(),
        &capture,
        &[],
        &selected,
        Some(SelectionTarget::Node(merge_id)),
        None,
        viewport,
        transform,
    );
    let moved = run_pointer_frame(
        &context,
        &projection,
        &mut state,
        node_editor_ui::InteractionOptions::SELECTION_AND_MOVE,
        vec![egui::Event::PointerMoved(
            start + first_delta * transform.scaling,
        )],
    );
    drop(projection);
    let change = move_change(&project, &moved).expect("first production Move");
    assert_eq!(change.grabbed_node, Some(solid_id));
    assert_eq!(change.edits.len(), 2);
    for edit in change.edits {
        assert!(apply_layout_edit(&mut project, edit));
    }
    for rect in node_rects.values_mut() {
        *rect = rect.translate(first_delta);
    }

    let final_increment = egui::vec2(10.0, 6.0);
    let final_delta = first_delta + final_increment;
    let projection = SurfaceProjection::from_project(
        &project,
        &[],
        &node_rects,
        &HashMap::new(),
        &capture,
        &[],
        &selected,
        Some(SelectionTarget::Node(merge_id)),
        None,
        viewport,
        transform,
    );
    let released = run_pointer_frame(
        &context,
        &projection,
        &mut state,
        node_editor_ui::InteractionOptions::SELECTION_AND_MOVE,
        vec![pointer_button(
            start + final_delta * transform.scaling,
            false,
        )],
    );
    drop(projection);
    let change = move_change(&project, &released).expect("release-frame Move");
    assert_eq!(move_end(&released), Some(MoveEndOutcome::Released));
    assert!(change.edits.iter().all(|edit| matches!(
        edit,
        LayoutEdit::MoveNode { node_id, position }
            if [solid_id, merge_id].contains(node_id)
                && (*position == [initial_solid[0] + final_delta.x, initial_solid[1] + final_delta.y]
                    || *position == [initial_merge[0] + final_delta.x, initial_merge[1] + final_delta.y])
    )));
    for edit in change.edits {
        assert!(apply_layout_edit(&mut project, edit));
    }
    assert!(!state.is_active());
    assert_eq!(
        project.get_node(solid_id).unwrap().ui_position,
        [
            initial_solid[0] + final_delta.x,
            initial_solid[1] + final_delta.y,
        ]
    );
    assert_eq!(
        project.get_node(merge_id).unwrap().ui_position,
        [
            initial_merge[0] + final_delta.x,
            initial_merge[1] + final_delta.y,
        ]
    );
}

#[test]
fn production_group_header_drag_uses_generic_move_and_prunes_child_group() {
    let (project, composition_id, track_id, clip_id, _, _) = fixture();
    let (_, containers) = build_snarl(&project, composition_id);
    let mut capture = SurfaceCapture::default();
    for target in [
        SelectionTarget::Composition(composition_id),
        SelectionTarget::Track(track_id),
        SelectionTarget::Clip(clip_id),
    ] {
        capture.record_selectable(target);
    }
    let selected = [
        SelectionTarget::Track(track_id),
        SelectionTarget::Clip(clip_id),
    ];
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_600.0, 1_100.0));
    let projection = SurfaceProjection::from_project(
        &project,
        &containers,
        &HashMap::new(),
        &HashMap::new(),
        &capture,
        &[],
        &selected,
        Some(SelectionTarget::Clip(clip_id)),
        None,
        viewport,
        egui::emath::TSTransform::IDENTITY,
    );
    let track = containers
        .iter()
        .find(|container| container.owner == ProjectPortOwner::Track(track_id))
        .unwrap();
    let start = egui::Rect::from_min_size(
        track.rect().min,
        egui::vec2(track.rect().width(), CONTAINER_HEADER_HEIGHT),
    )
    .center();
    let context = egui::Context::default();
    let mut state = SurfaceState::default();
    let _ = run_pointer_frame(
        &context,
        &projection,
        &mut state,
        node_editor_ui::InteractionOptions::SELECTION_AND_MOVE,
        vec![
            egui::Event::PointerMoved(start),
            pointer_button(start, true),
        ],
    );
    let delta = egui::vec2(22.0, 14.0);
    let outputs = run_pointer_frame(
        &context,
        &projection,
        &mut state,
        node_editor_ui::InteractionOptions::SELECTION_AND_MOVE,
        vec![egui::Event::PointerMoved(start + delta)],
    );
    assert!(outputs.iter().any(|output| matches!(
        output,
        SurfaceOutput::Move {
            grabbed: ItemId::Group(ProjectPortOwner::Track(id)),
            ..
        } if *id == track_id
    )));
    let change = move_change(&project, &outputs).expect("Group Move adapter");
    assert!(matches!(
        change.edits.as_slice(),
        [LayoutEdit::MoveContainer { owner, delta: edit_delta }]
            if *owner == ProjectPortOwner::Track(track_id)
                && *edit_delta == [delta.x, delta.y]
    ));
}

#[test]
fn production_option_disable_after_delta_cancels_and_commits_current_position_once() {
    let (project, composition_id, _, _, solid_id, _) = fixture();
    let initial = project.clone();
    let node = project.get_node(solid_id).unwrap();
    let rect = egui::Rect::from_min_size(
        egui::pos2(node.ui_position[0], node.ui_position[1]),
        egui::vec2(180.0, 120.0),
    );
    let node_rects = HashMap::from([(solid_id, rect)]);
    let mut capture = SurfaceCapture::default();
    capture.record_selectable(SelectionTarget::Node(solid_id));
    capture.record_node_header(
        solid_id,
        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 28.0)),
    );
    let selected = [SelectionTarget::Node(solid_id)];
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_200.0, 900.0));
    let projection = SurfaceProjection::from_project(
        &project,
        &[],
        &node_rects,
        &HashMap::new(),
        &capture,
        &[],
        &selected,
        Some(selected[0]),
        None,
        viewport,
        egui::emath::TSTransform::IDENTITY,
    );
    let context = egui::Context::default();
    let mut editor_context = EditorContext::new(composition_id);
    let start = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 28.0)).center();
    let _ = run_pointer_frame(
        &context,
        &projection,
        &mut editor_context.node_editor_state.surface_interaction,
        node_editor_ui::InteractionOptions::SELECTION_AND_MOVE,
        vec![
            egui::Event::PointerMoved(start),
            pointer_button(start, true),
        ],
    );
    let moved = run_pointer_frame(
        &context,
        &projection,
        &mut editor_context.node_editor_state.surface_interaction,
        node_editor_ui::InteractionOptions::SELECTION_AND_MOVE,
        vec![egui::Event::PointerMoved(start + egui::vec2(18.0, 9.0))],
    );
    let moved_change = move_change(&project, &moved).expect("production movement before disable");
    assert!(
        editor_context
            .node_editor_state
            .surface_interaction
            .is_move_active()
    );

    let disabled = run_pointer_frame(
        &context,
        &projection,
        &mut editor_context.node_editor_state.surface_interaction,
        node_editor_ui::InteractionOptions::OVERVIEW_SELECTION,
        Vec::new(),
    );
    assert_eq!(move_end(&disabled), Some(MoveEndOutcome::Cancelled));
    assert!(
        disabled
            .iter()
            .all(|output| !matches!(output, SurfaceOutput::Reparent { .. }))
    );
    assert!(
        !editor_context
            .node_editor_state
            .surface_interaction
            .is_active()
    );

    let mut edited = project.clone();
    for edit in moved_change.edits {
        assert!(apply_layout_edit(&mut edited, edit));
    }
    let project_lock = Arc::new(RwLock::new(edited.clone()));
    let mut history = HistoryManager::new();
    history.push_project_state(initial.clone());
    editor_context.node_editor_state.layout_changed_during_drag = true;
    assert!(commit_live_project_edits(
        &mut editor_context,
        &mut history,
        &project_lock,
    ));
    assert_eq!(history.undo_depth(), 2);
    let after_interrupt = run_pointer_frame(
        &context,
        &projection,
        &mut editor_context.node_editor_state.surface_interaction,
        node_editor_ui::InteractionOptions::SELECTION_AND_MOVE,
        vec![
            egui::Event::PointerMoved(start + egui::vec2(36.0, 18.0)),
            pointer_button(start + egui::vec2(36.0, 18.0), false),
        ],
    );
    assert!(move_change(&project, &after_interrupt).is_none());
    assert_eq!(history.undo(&edited), Some(initial));
}
