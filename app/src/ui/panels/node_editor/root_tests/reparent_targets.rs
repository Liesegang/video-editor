use super::*;

#[test]
fn collapsed_reparent_excludes_both_the_header_and_stored_body() {
    let (mut project, composition_id, track_id, clip_id, _, _) = fixture();
    let clip_header = egui::pos2(500.0, 280.0);
    let hidden_clip_body = egui::pos2(500.0, 400.0);

    assert_eq!(
        node_container_at_position(&project, composition_id, clip_header),
        Some(NodeContainer::Clip(clip_id))
    );

    let clip = project.get_clip_mut(clip_id).unwrap();
    clip.ui_collapsed = true;
    assert!(container_rect(clip.ui_position, clip.ui_size).contains(hidden_clip_body));
    let collapsed_geometry = reparent_container_geometries(&project, composition_id)
        .into_iter()
        .find(|geometry| geometry.container == NodeContainer::Clip(clip_id));
    assert!(collapsed_geometry.is_some_and(|geometry| {
        reparent_ineligible_reason(geometry) == Some("collapsed_hidden_content")
    }));

    let node_id = project.get_clip(clip_id).unwrap().node_ids[0];
    // A collapsed owner has no valid content surface. Neither its visible
    // header nor its stored hidden body may become a semantic destination;
    // an explicit expand-on-hover policy can add that later without moving
    // the header or violating exact drop coordinates.
    let node_size = estimated_node_size(&project, node_id);
    let hidden_rect = egui::Rect::from_center_size(hidden_clip_body, node_size);
    assert_eq!(
        deepest_legal_reparent_target(&project, composition_id, hidden_rect, hidden_clip_body,)
            .map(|target| target.container),
        Some(NodeContainer::Track(track_id))
    );
    let header_rect = egui::Rect::from_center_size(clip_header, node_size);
    assert_eq!(
        deepest_legal_reparent_target(&project, composition_id, header_rect, clip_header,)
            .map(|target| target.container),
        Some(NodeContainer::Track(track_id))
    );

    assert!(reparent_test_node_at_drop(
        &mut project,
        composition_id,
        node_id,
        hidden_clip_body,
    ));
    assert_eq!(
        project.find_node_container(node_id),
        Some(NodeContainer::Track(track_id))
    );
    project.get_clip_mut(clip_id).unwrap().ui_collapsed = false;
    assert!(project
        .attach_node_to_container(NodeContainer::Track(track_id), node_id)
        .is_ok());
    let expanded_drop = project.get_clip(clip_id).map(|clip| {
        nested_content_rect(
            container_rect(clip.ui_position, clip.ui_size),
            AUTO_LAYOUT_CLIP_TOP,
        )
        .center()
    });
    assert!(expanded_drop.is_some());
    let Some(expanded_drop) = expanded_drop else {
        return;
    };
    assert!(reparent_test_node_at_drop(
        &mut project,
        composition_id,
        node_id,
        expanded_drop,
    ));
    assert_eq!(
        project.find_node_container(node_id),
        Some(NodeContainer::Clip(clip_id))
    );

    project.get_track_mut(track_id).unwrap().ui_collapsed = true;
    let track = project.get_track(track_id).unwrap();
    let track_header = egui::pos2(
        track.ui_position[0] + 24.0,
        track.ui_position[1] + CONTAINER_HEADER_HEIGHT * 0.5,
    );
    let node_size = estimated_node_size(&project, node_id);
    let track_header_rect = egui::Rect::from_center_size(track_header, node_size);
    assert_eq!(
        deepest_legal_reparent_target(&project, composition_id, track_header_rect, track_header,)
            .map(|target| target.container),
        Some(NodeContainer::Composition(composition_id))
    );
    let hidden_track_body = egui::pos2(track_header.x, track_header.y + 180.0);
    let hidden_track_rect = egui::Rect::from_center_size(hidden_track_body, node_size);
    assert_eq!(
        deepest_legal_reparent_target(
            &project,
            composition_id,
            hidden_track_rect,
            hidden_track_body,
        )
        .map(|target| target.container),
        Some(NodeContainer::Composition(composition_id))
    );
    assert!(project.validate_containment().is_empty());
}

#[test]
fn reparent_overlap_uses_deepest_legal_content_and_authoritative_stacking_order() {
    let (mut project, composition_id, track_id, clip_id, node_id, _) = fixture();
    let Some(first_clip) = project.get_clip(clip_id).cloned() else {
        assert!(project.get_clip(clip_id).is_some());
        return;
    };
    let mut second_clip = Clip::new("Overlapping Clip", 0.0, 5.0);
    second_clip.ui_position = first_clip.ui_position;
    second_clip.ui_size = first_clip.ui_size;
    let second_clip_id = second_clip.id;
    project.add_clip(second_clip);
    assert!(project
        .attach_clip_to_track(track_id, second_clip_id)
        .is_ok());

    let first_rect = container_rect(first_clip.ui_position, first_clip.ui_size);
    let content = nested_content_rect(first_rect, AUTO_LAYOUT_CLIP_TOP);
    let node_size = estimated_node_size(&project, node_id);
    let node_position = content.center() - node_size * 0.5;
    let origin_position = project.get_node(node_id).map(|node| node.ui_position);
    assert!(origin_position.is_some());
    let Some(origin_position) = origin_position else {
        return;
    };
    let Some(node) = project.get_node_mut(node_id) else {
        assert!(project.get_node(node_id).is_some());
        return;
    };
    node.ui_position = [node_position.x, node_position.y];
    let node_rect = egui::Rect::from_min_size(node_position, node_size);
    let drop_point = node_rect.center();
    let selected = deepest_legal_reparent_target(&project, composition_id, node_rect, drop_point);
    assert_eq!(
        selected.map(|target| target.container),
        Some(NodeContainer::Clip(second_clip_id))
    );
    assert!(selected.is_some_and(|target| target.depth == 2 && target.center_inside));
    let gesture = NodeEditorReparentGesture {
        origins: HashMap::from([(
            node_id,
            NodeEditorNodeDragOrigin {
                container: NodeContainer::Clip(clip_id),
                position: origin_position,
            },
        )]),
        primary_node_id: Some(node_id),
        hovered_target: None,
        hovered_node_id: None,
        hovered_score: None,
    };
    let intents = node_drop_intents(
        &project,
        composition_id,
        &gesture,
        &HashMap::from([(node_id, node_rect)]),
        &HashMap::from([(node_id, [node_position.x, node_position.y])]),
        drop_point,
        1.0,
    );
    assert_eq!(intents.len(), 1);
    assert!(intents
        .first()
        .is_some_and(|intent| { intent.target.container == NodeContainer::Clip(second_clip_id) }));

    let Some(second_clip) = project.get_clip_mut(second_clip_id) else {
        assert!(project.get_clip(second_clip_id).is_some());
        return;
    };
    second_clip.ui_collapsed = true;
    assert_eq!(
        deepest_legal_reparent_target(&project, composition_id, node_rect, drop_point,)
            .map(|target| target.container),
        Some(NodeContainer::Clip(clip_id))
    );
}

#[test]
fn reparent_intent_keeps_origin_across_header_padding_until_node_fully_exits() {
    let (mut project, composition_id, track_id, clip_id, node_id, _) = fixture();
    if let Some(track) = project.get_track_mut(track_id) {
        track.ui_size = [1_800.0, 1_000.0];
    }
    if let Some(composition) = project.get_composition_mut(composition_id) {
        composition.ui_size = [2_200.0, 1_400.0];
    }
    let exact_node_size = egui::vec2(120.0, 60.0);
    let origin_position = project.get_clip(clip_id).map(|clip| {
        nested_content_rect(
            container_rect(clip.ui_position, clip.ui_size),
            AUTO_LAYOUT_CLIP_TOP,
        )
        .min + egui::vec2(32.0, 32.0)
    });
    assert!(origin_position.is_some());
    let Some(origin_position) = origin_position else {
        return;
    };
    if let Some(node) = project.get_node_mut(node_id) {
        node.ui_position = [origin_position.x, origin_position.y];
    }
    let Some(origin_node) = project.get_node(node_id) else {
        assert!(project.get_node(node_id).is_some());
        return;
    };
    let origin = NodeEditorNodeDragOrigin {
        container: NodeContainer::Clip(clip_id),
        position: origin_node.ui_position,
    };
    let gesture = NodeEditorReparentGesture {
        origins: HashMap::from([(node_id, origin)]),
        primary_node_id: Some(node_id),
        hovered_target: None,
        hovered_node_id: None,
        hovered_score: None,
    };
    let rendered_rect = egui::Rect::from_min_size(
        egui::pos2(origin.position[0], origin.position[1]),
        exact_node_size,
    );
    let small_min = egui::pos2(origin.position[0] + 4.0, origin.position[1] + 3.0);
    let small_rect = egui::Rect::from_min_size(small_min, exact_node_size);
    assert!(node_drop_intents(
        &project,
        composition_id,
        &gesture,
        &HashMap::from([(node_id, rendered_rect)]),
        &HashMap::from([(node_id, [small_min.x, small_min.y])]),
        small_rect.center(),
        1.0,
    )
    .is_empty());
    let mut non_node_state = NodeEditorState::default();
    record_node_reparent_origins(
        &project,
        &[
            LayoutEdit::MoveContainer {
                owner: PortOwner::Track(track_id),
                delta: [48.0, 24.0],
            },
            LayoutEdit::ResizeContainer {
                owner: PortOwner::Clip(clip_id),
                position: [200.0, 200.0],
                size: [800.0, 500.0],
            },
        ],
        None,
        &mut non_node_state,
    );
    assert!(non_node_state.node_reparent.is_none());
    let Some(clip) = project.get_clip(clip_id) else {
        return;
    };
    let clip_rect = container_rect(clip.ui_position, clip.ui_size);
    let padding_min = clip_rect.min + egui::vec2(12.0, 12.0);
    let padding_rect = egui::Rect::from_min_size(padding_min, exact_node_size);
    let padding_positions = HashMap::from([(node_id, [padding_min.x, padding_min.y])]);
    let padding_intents = node_drop_intents(
        &project,
        composition_id,
        &gesture,
        &HashMap::from([(node_id, rendered_rect)]),
        &padding_positions,
        padding_rect.center(),
        1.0,
    );
    assert_eq!(padding_intents.len(), 1);
    assert_eq!(padding_intents[0].final_rect, padding_rect);
    assert!(padding_intents.first().is_some_and(|intent| {
        intent.target.container == NodeContainer::Clip(clip_id)
            && intent.target.kind == ReparentTargetKind::RetainedOrigin
    }));
    let mut retained_project = project.clone();
    if let Some(node) = retained_project.get_node_mut(node_id) {
        node.ui_position = [padding_min.x, padding_min.y];
    }
    assert!(reparent_nodes_from_intents(
        &mut retained_project,
        &padding_intents,
    ));
    assert_eq!(
        retained_project.find_node_container(node_id),
        Some(NodeContainer::Clip(clip_id))
    );
    let retained_content = retained_project.get_clip(clip_id).map(|clip| {
        nested_content_rect(
            container_rect(clip.ui_position, clip.ui_size),
            AUTO_LAYOUT_CLIP_TOP,
        )
    });
    assert!(retained_content.is_some_and(|content| { rect_contains_rect(content, padding_rect) }));
    assert!(retained_project.validate_containment().is_empty());

    let exited_min = egui::pos2(1_120.0, 470.0);
    let exited_rect = egui::Rect::from_min_size(exited_min, exact_node_size);
    let exited_positions = HashMap::from([(node_id, [exited_min.x, exited_min.y])]);
    let intents = node_drop_intents(
        &project,
        composition_id,
        &gesture,
        &HashMap::from([(node_id, rendered_rect)]),
        &exited_positions,
        exited_rect.center(),
        1.0,
    );
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].final_rect, exited_rect);
    assert!(intents
        .first()
        .is_some_and(|intent| intent.target.container == NodeContainer::Track(track_id)));
}

#[test]
fn subthreshold_screen_drag_repairs_containment_without_changing_owner() {
    let (mut project, composition_id, _, clip_id, _, node_id) = fixture();
    let plan = compute_auto_layout(&project, composition_id, AutoLayoutScope::All);
    assert!(plan.is_some());
    let Some(plan) = plan else {
        return;
    };
    apply_auto_layout(&mut project, composition_id, &plan);
    assert!(!layout_needs_reflow(&project, composition_id));
    let origin_position = project.get_node(node_id).map(|node| node.ui_position);
    assert!(origin_position.is_some());
    let Some(origin_position) = origin_position else {
        return;
    };
    let gesture = NodeEditorReparentGesture {
        origins: HashMap::from([(
            node_id,
            NodeEditorNodeDragOrigin {
                container: NodeContainer::Clip(clip_id),
                position: origin_position,
            },
        )]),
        primary_node_id: Some(node_id),
        hovered_target: None,
        hovered_node_id: None,
        hovered_score: None,
    };
    let screen_delta = NODE_REPARENT_DRAG_THRESHOLD - 1.0;
    let graph_delta = screen_delta / NODE_EDITOR_MIN_SCALE;
    let final_min = egui::pos2(origin_position[0] + graph_delta, origin_position[1]);
    let exact_size = estimated_node_size(&project, node_id) * 0.4;
    let exact_rect = egui::Rect::from_min_size(final_min, exact_size);
    let rendered_rect = egui::Rect::from_min_size(
        egui::pos2(origin_position[0], origin_position[1]),
        exact_size,
    );
    let intents = node_drop_intents(
        &project,
        composition_id,
        &gesture,
        &HashMap::from([(node_id, rendered_rect)]),
        &HashMap::from([(node_id, [final_min.x, final_min.y])]),
        exact_rect.center(),
        NODE_EDITOR_MIN_SCALE,
    );
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].final_rect, exact_rect);
    assert!(intents.first().is_some_and(|intent| {
        intent.target.container == NodeContainer::Clip(clip_id)
            && intent.target.kind == ReparentTargetKind::RetainedOrigin
    }));
    if let Some(node) = project.get_node_mut(node_id) {
        node.ui_position = [final_min.x, final_min.y];
    }
    assert!(reparent_nodes_from_intents(&mut project, &intents));
    assert_eq!(
        project.find_node_container(node_id),
        Some(NodeContainer::Clip(clip_id))
    );
    let estimated_rect = estimated_node_rect(&project, node_id);
    let content = project.get_clip(clip_id).map(|clip| {
        nested_content_rect(
            container_rect(clip.ui_position, clip.ui_size),
            AUTO_LAYOUT_CLIP_TOP,
        )
    });
    assert!(content
        .zip(estimated_rect)
        .is_some_and(|(content, estimated)| rect_contains_rect(content, estimated)));
    assert!(!layout_needs_reflow(&project, composition_id));
}

#[test]
fn retained_growth_rolls_back_when_it_would_overlap_a_sibling_container() {
    let (mut project, composition_id, track_id, clip_id, _, node_id) = fixture();
    let plan = compute_auto_layout(&project, composition_id, AutoLayoutScope::All);
    assert!(plan.is_some());
    let Some(plan) = plan else {
        return;
    };
    apply_auto_layout(&mut project, composition_id, &plan);
    let first_clip = project.get_clip(clip_id).cloned();
    assert!(first_clip.is_some());
    let Some(first_clip) = first_clip else {
        return;
    };
    let first_rect = container_rect(first_clip.ui_position, first_clip.ui_size);
    let mut sibling = Clip::new("Sibling", 0.0, 5.0);
    sibling.ui_position = [
        first_rect.right() + AUTO_LAYOUT_CLIP_GAP,
        first_clip.ui_position[1],
    ];
    sibling.ui_size = first_clip.ui_size;
    let sibling_id = sibling.id;
    project.add_clip(sibling);
    assert!(project.attach_clip_to_track(track_id, sibling_id).is_ok());
    let sibling_rect = project
        .get_clip(sibling_id)
        .map(|clip| container_rect(clip.ui_position, clip.ui_size));
    assert!(sibling_rect.is_some());
    let Some(sibling_rect) = sibling_rect else {
        return;
    };
    ensure_container_hierarchy_contains(&mut project, NodeContainer::Track(track_id), sibling_rect);
    assert!(!layout_needs_reflow(&project, composition_id));

    let origin_position = project.get_node(node_id).map(|node| node.ui_position);
    assert!(origin_position.is_some());
    let Some(origin_position) = origin_position else {
        return;
    };
    let gesture = NodeEditorReparentGesture {
        origins: HashMap::from([(
            node_id,
            NodeEditorNodeDragOrigin {
                container: NodeContainer::Clip(clip_id),
                position: origin_position,
            },
        )]),
        primary_node_id: Some(node_id),
        hovered_target: None,
        hovered_node_id: None,
        hovered_score: None,
    };
    let graph_delta = (NODE_REPARENT_DRAG_THRESHOLD - 1.0) / NODE_EDITOR_MIN_SCALE;
    let final_min = egui::pos2(origin_position[0] + graph_delta, origin_position[1]);
    let exact_size = estimated_node_size(&project, node_id) * 0.4;
    let final_rect = egui::Rect::from_min_size(final_min, exact_size);
    let rendered_rect = egui::Rect::from_min_size(
        egui::pos2(origin_position[0], origin_position[1]),
        exact_size,
    );
    let intents = node_drop_intents(
        &project,
        composition_id,
        &gesture,
        &HashMap::from([(node_id, rendered_rect)]),
        &HashMap::from([(node_id, [final_min.x, final_min.y])]),
        final_rect.center(),
        NODE_EDITOR_MIN_SCALE,
    );
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].final_rect, final_rect);
    if let Some(node) = project.get_node_mut(node_id) {
        node.ui_position = [final_min.x, final_min.y];
    }
    let initial_clip_geometry = project
        .get_clip(clip_id)
        .map(|clip| (clip.ui_position, clip.ui_size));
    assert_eq!(
        finish_node_reparent(&mut project, &intents, Some(&gesture)),
        ReparentReleaseOutcome::Rejected
    );
    assert_eq!(
        project.get_node(node_id).map(|node| node.ui_position),
        Some(origin_position)
    );
    assert_eq!(
        project
            .get_clip(clip_id)
            .map(|clip| (clip.ui_position, clip.ui_size)),
        initial_clip_geometry
    );
    assert_eq!(
        project.find_node_container(node_id),
        Some(NodeContainer::Clip(clip_id))
    );
    assert!(!layout_needs_reflow(&project, composition_id));
}

#[test]
fn reparent_applies_when_only_node_overlap_requires_optional_auto_layout() {
    let (mut project, composition_id, track_id, clip_id, _, node_id) = fixture();
    let overlap_node = Node::new_merge("Intentional overlap");
    let overlap_node_id = overlap_node.id;
    project.add_node(overlap_node);
    assert!(project
        .attach_node_to_container(NodeContainer::Track(track_id), overlap_node_id)
        .is_ok());
    let plan = compute_auto_layout(&project, composition_id, AutoLayoutScope::All);
    assert!(plan.is_some());
    let Some(plan) = plan else {
        return;
    };
    assert!(apply_auto_layout(&mut project, composition_id, &plan));
    assert!(!container_hierarchy_needs_reflow(&project, composition_id));

    let origin_position = project.get_node(node_id).map(|node| node.ui_position);
    let final_position = project
        .get_node(overlap_node_id)
        .map(|node| node.ui_position);
    assert!(origin_position.is_some());
    assert!(final_position.is_some());
    let (Some(origin_position), Some(final_position)) = (origin_position, final_position) else {
        return;
    };
    let gesture = NodeEditorReparentGesture {
        origins: HashMap::from([(
            node_id,
            NodeEditorNodeDragOrigin {
                container: NodeContainer::Clip(clip_id),
                position: origin_position,
            },
        )]),
        primary_node_id: Some(node_id),
        hovered_target: None,
        hovered_node_id: None,
        hovered_score: None,
    };
    let final_rect = egui::Rect::from_min_size(
        egui::pos2(final_position[0], final_position[1]),
        estimated_node_size(&project, node_id),
    );
    let rendered_rect = egui::Rect::from_min_size(
        egui::pos2(origin_position[0], origin_position[1]),
        final_rect.size(),
    );
    let intents = node_drop_intents(
        &project,
        composition_id,
        &gesture,
        &HashMap::from([(node_id, rendered_rect)]),
        &HashMap::from([(node_id, final_position)]),
        final_rect.center(),
        1.0,
    );
    assert!(intents
        .first()
        .is_some_and(|intent| { intent.target.container == NodeContainer::Track(track_id) }));
    assert_eq!(intents[0].final_rect, final_rect);
    if let Some(node) = project.get_node_mut(node_id) {
        node.ui_position = final_position;
    }

    assert_eq!(
        finish_node_reparent(&mut project, &intents, Some(&gesture)),
        ReparentReleaseOutcome::Applied
    );
    assert_eq!(
        project.find_node_container(node_id),
        Some(NodeContainer::Track(track_id))
    );
    assert_eq!(
        project.get_node(node_id).map(|node| node.ui_position),
        Some(final_position)
    );
    assert!(layout_needs_reflow(&project, composition_id));
    assert!(!container_hierarchy_needs_reflow(&project, composition_id));
}
