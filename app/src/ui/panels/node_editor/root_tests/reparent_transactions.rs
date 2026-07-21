use super::*;

#[test]
fn multi_node_drag_uses_primary_target_without_splitting_the_group() {
    let (mut project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
    if let Some(track) = project.get_track_mut(track_id) {
        track.ui_size = [2_100.0, 1_000.0];
    }
    if let Some(composition) = project.get_composition_mut(composition_id) {
        composition.ui_size = [2_500.0, 1_400.0];
    }
    let Some(solid) = project.get_node(solid_id) else {
        return;
    };
    let Some(merge) = project.get_node(merge_id) else {
        return;
    };
    let gesture = NodeEditorReparentGesture {
        origins: HashMap::from([
            (
                solid_id,
                NodeEditorNodeDragOrigin {
                    container: NodeContainer::Clip(clip_id),
                    position: solid.ui_position,
                },
            ),
            (
                merge_id,
                NodeEditorNodeDragOrigin {
                    container: NodeContainer::Clip(clip_id),
                    position: merge.ui_position,
                },
            ),
        ]),
        primary_node_id: Some(solid_id),
        hovered_target: None,
        hovered_node_id: None,
        hovered_score: None,
    };
    let solid_min = egui::pos2(1_120.0, 470.0);
    let merge_min = egui::pos2(1_440.0, 470.0);
    let solid_rect = egui::Rect::from_min_size(solid_min, estimated_node_size(&project, solid_id));
    let merge_rect = egui::Rect::from_min_size(merge_min, estimated_node_size(&project, merge_id));
    let final_positions = HashMap::from([
        (solid_id, [solid_min.x, solid_min.y]),
        (merge_id, [merge_min.x, merge_min.y]),
    ]);
    let final_rects = HashMap::from([(solid_id, solid_rect), (merge_id, merge_rect)]);
    let intents = node_drop_intents(
        &project,
        composition_id,
        &gesture,
        &final_rects,
        &final_positions,
        solid_rect.center(),
        1.0,
    );
    assert_eq!(intents.len(), 2);
    assert!(intents
        .iter()
        .all(|intent| intent.target.container == NodeContainer::Track(track_id)));

    if let Some(node) = project.get_node_mut(solid_id) {
        node.ui_position = [solid_min.x, solid_min.y];
    }
    if let Some(node) = project.get_node_mut(merge_id) {
        node.ui_position = [merge_min.x, merge_min.y];
    }
    let connection_ids = project
        .connections
        .iter()
        .map(|connection| connection.id)
        .collect::<Vec<_>>();
    assert!(reparent_nodes_from_intents(&mut project, &intents));
    assert_eq!(
        project.find_node_container(solid_id),
        Some(NodeContainer::Track(track_id))
    );
    assert_eq!(
        project.find_node_container(merge_id),
        Some(NodeContainer::Track(track_id))
    );
    assert_eq!(
        project
            .connections
            .iter()
            .map(|connection| connection.id)
            .collect::<Vec<_>>(),
        connection_ids
    );
    assert_eq!(
        project
            .get_node(merge_id)
            .map(|node| node.ui_position[0])
            .zip(project.get_node(solid_id).map(|node| node.ui_position[0]))
            .map(|(merge_x, solid_x)| merge_x - solid_x),
        Some(320.0)
    );
}

#[test]
fn reparent_min_edge_growth_keeps_node_drop_position_and_contains_every_ancestor() {
    let (mut project, composition_id, track_id, clip_id, node_id, _) = fixture();
    let other_node_ids = project
        .get_clip(clip_id)
        .map(|clip| {
            clip.node_ids
                .iter()
                .copied()
                .filter(|candidate| *candidate != node_id)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for other_node_id in other_node_ids {
        assert!(project.remove_node(other_node_id).is_ok());
    }
    let Some(composition) = project.get_composition_mut(composition_id) else {
        assert!(project.get_composition(composition_id).is_some());
        return;
    };
    composition.ui_position = [100.0, 100.0];
    composition.ui_size = [1_700.0, 1_300.0];
    let Some(track) = project.get_track_mut(track_id) else {
        assert!(project.get_track(track_id).is_some());
        return;
    };
    track.ui_position = [300.0, 240.0];
    track.ui_size = [1_100.0, 900.0];
    let Some(clip) = project.get_clip_mut(clip_id) else {
        assert!(project.get_clip(clip_id).is_some());
        return;
    };
    clip.ui_position = [490.0, 380.0];
    clip.ui_size = [800.0, 600.0];
    assert!(project
        .attach_node_to_container(NodeContainer::Composition(composition_id), node_id)
        .is_ok());

    let clip_before = project.get_clip(clip_id).map(|clip| clip.ui_position);
    let track_before = project.get_track(track_id).map(|track| track.ui_position);
    let composition_before = project
        .get_composition(composition_id)
        .map(|composition| composition.ui_position);
    assert!(clip_before.is_some() && track_before.is_some() && composition_before.is_some());
    let Some(clip) = project.get_clip(clip_id) else {
        return;
    };
    let clip_content = nested_content_rect(
        container_rect(clip.ui_position, clip.ui_size),
        AUTO_LAYOUT_CLIP_TOP,
    );
    let node_size = estimated_node_size(&project, node_id);
    let exact_snarl_size = node_size * 0.45;
    let final_min = clip_content.min - exact_snarl_size * 0.4;
    let final_rect = egui::Rect::from_min_size(final_min, exact_snarl_size);
    let Some(node) = project.get_node_mut(node_id) else {
        return;
    };
    node.ui_position = [final_min.x, final_min.y];
    let target = deepest_legal_reparent_target(
        &project,
        composition_id,
        final_rect,
        clip_content.min + egui::vec2(2.0, 2.0),
    );
    assert!(
        target.is_some(),
        "partially overlapping final Node rect had no legal target"
    );
    let Some(target) = target else {
        return;
    };
    assert_eq!(target.container, NodeContainer::Clip(clip_id));
    assert!(reparent_nodes_from_intents(
        &mut project,
        &[NodeDropIntent {
            node_id,
            final_rect,
            target,
        }],
    ));

    assert_eq!(
        project.get_node(node_id).map(|node| node.ui_position),
        Some([final_min.x, final_min.y])
    );
    assert!(project
        .get_clip(clip_id)
        .zip(clip_before)
        .is_some_and(|(clip, before)| {
            clip.ui_position[0] < before[0] && clip.ui_position[1] < before[1]
        }));
    assert_eq!(
        project.get_track(track_id).map(|track| track.ui_position),
        track_before,
        "the reduced port rail leaves enough track content space to contain the grown clip"
    );
    assert_eq!(
        project
            .get_composition(composition_id)
            .map(|composition| composition.ui_position),
        composition_before,
        "the composition must not drift when its content already contains the track"
    );

    let Some(clip) = project.get_clip(clip_id) else {
        return;
    };
    let clip_rect = container_rect(clip.ui_position, clip.ui_size);
    let persisted_rect = egui::Rect::from_min_size(final_min, node_size);
    assert!(rect_contains_rect(
        nested_content_rect(clip_rect, AUTO_LAYOUT_CLIP_TOP),
        final_rect,
    ));
    assert!(rect_contains_rect(
        nested_content_rect(clip_rect, AUTO_LAYOUT_CLIP_TOP),
        persisted_rect,
    ));
    let Some(track) = project.get_track(track_id) else {
        return;
    };
    let track_rect = container_rect(track.ui_position, track.ui_size);
    assert!(rect_contains_rect(
        nested_content_rect(track_rect, AUTO_LAYOUT_TRACK_TOP),
        clip_rect,
    ));
    let Some(composition) = project.get_composition(composition_id) else {
        return;
    };
    assert!(rect_contains_rect(
        composition_content_rect(container_rect(composition.ui_position, composition.ui_size,)),
        track_rect,
    ));
    assert!(!container_hierarchy_needs_reflow(&project, composition_id));
}

#[test]
fn composition_root_fallback_expands_same_owner_on_left_and_top() {
    let (mut project, composition_id, _, _, node_id, _) = fixture();
    assert!(project
        .attach_node_to_container(NodeContainer::Composition(composition_id), node_id)
        .is_ok());
    let plan = compute_auto_layout(&project, composition_id, AutoLayoutScope::All);
    assert!(plan.is_some());
    let Some(plan) = plan else {
        return;
    };
    assert!(apply_auto_layout(&mut project, composition_id, &plan));
    assert!(!container_hierarchy_needs_reflow(&project, composition_id));
    let Some(origin_node) = project.get_node(node_id) else {
        return;
    };
    let gesture = NodeEditorReparentGesture {
        origins: HashMap::from([(
            node_id,
            NodeEditorNodeDragOrigin {
                container: NodeContainer::Composition(composition_id),
                position: origin_node.ui_position,
            },
        )]),
        primary_node_id: Some(node_id),
        hovered_target: None,
        hovered_node_id: None,
        hovered_score: None,
    };
    let final_min = egui::pos2(-800.0, -260.0);
    let estimated_size = estimated_node_size(&project, node_id);
    let final_rect = egui::Rect::from_min_size(final_min, estimated_size * 0.4);
    let final_positions = HashMap::from([(node_id, [final_min.x, final_min.y])]);
    let final_rects = HashMap::from([(node_id, final_rect)]);
    let intents = node_drop_intents(
        &project,
        composition_id,
        &gesture,
        &final_rects,
        &final_positions,
        final_rect.center(),
        1.0,
    );
    assert_eq!(intents.len(), 1);
    assert!(intents.first().is_some_and(|intent| {
        intent.target.container == NodeContainer::Composition(composition_id)
            && intent.target.root_fallback
            && intent.target.kind == ReparentTargetKind::RootFallback
    }));
    let reported = intents
        .first()
        .copied()
        .and_then(|active| reported_reparent_target(None, true, active));
    assert!(reported.is_some_and(|target| {
        target.kind == ReparentTargetKind::RootFallback
            && (target.score + 1.0).abs() <= f32::EPSILON
    }));
    if let Some(node) = project.get_node_mut(node_id) {
        node.ui_position = [final_min.x, final_min.y];
    }
    let before = project
        .get_composition(composition_id)
        .map(|composition| composition.ui_position);
    assert!(reparent_nodes_from_intents(&mut project, &intents));
    assert!(project
        .get_composition(composition_id)
        .zip(before)
        .is_some_and(|(composition, before)| {
            composition.ui_position[0] < before[0]
                && composition.ui_position[1] < before[1]
                && rect_contains_rect(
                    composition_content_rect(container_rect(
                        composition.ui_position,
                        composition.ui_size,
                    )),
                    final_rect,
                )
        }));
    assert_eq!(
        project.get_node(node_id).map(|node| node.ui_position),
        Some([final_min.x, final_min.y])
    );
    let root_content = project.get_composition(composition_id).map(|composition| {
        composition_content_rect(container_rect(composition.ui_position, composition.ui_size))
    });
    assert!(root_content.is_some_and(|content| {
        rect_contains_rect(
            content,
            egui::Rect::from_min_size(final_min, estimated_size),
        )
    }));
    assert!(!container_hierarchy_needs_reflow(&project, composition_id));
}

#[test]
fn reparent_clears_only_old_output_binding_and_preserves_typed_wire_identity_and_history() {
    let (mut project, composition_id, track_id, clip_id, solid_id, merge_id) = fixture();
    let mut track_output = Node::new_merge("Track Output");
    track_output.ui_position = [1_350.0, 420.0];
    let track_output_id = track_output.id;
    project.add_node(track_output);
    assert!(project
        .attach_node_to_container(NodeContainer::Track(track_id), track_output_id)
        .is_ok());
    let structural_merge_id = project
        .get_track(track_id)
        .unwrap()
        .structural_merge_node_id;
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(structural_merge_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(track_output_id), MERGE_IMAGES_PORT),
        )
        .unwrap();
    assert!(project
        .set_output_node(NodeContainer::Track(track_id), Some(track_output_id))
        .is_ok());
    assert!(project
        .set_output_node(NodeContainer::Clip(clip_id), Some(solid_id))
        .is_ok());
    if let Some(track) = project.get_track_mut(track_id) {
        track.ui_size = [1_800.0, 1_000.0];
    }
    if let Some(composition) = project.get_composition_mut(composition_id) {
        composition.ui_size = [2_200.0, 1_400.0];
    }
    let time_before = project
        .connections
        .iter()
        .find(|connection| {
            connection.from == PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT)
                && connection.to.owner == PortOwner::Node(solid_id)
        })
        .cloned();
    let image_before = project
        .connections
        .iter()
        .find(|connection| {
            connection.from.owner == PortOwner::Node(solid_id)
                && connection.to.owner == PortOwner::Node(merge_id)
        })
        .cloned();
    assert!(time_before.is_some() && image_before.is_some());
    let initial = project.clone();
    let mut history = HistoryManager::new();
    history.push_project_state(initial.clone());

    let final_min = egui::pos2(1_120.0, 470.0);
    let final_rect = egui::Rect::from_min_size(final_min, estimated_node_size(&project, solid_id));
    if let Some(node) = project.get_node_mut(solid_id) {
        node.ui_position = [final_min.x, final_min.y];
    }
    let target =
        deepest_legal_reparent_target(&project, composition_id, final_rect, final_rect.center());
    assert!(
        target.is_some(),
        "Track content did not resolve as a drop target"
    );
    let Some(target) = target else {
        return;
    };
    assert_eq!(target.container, NodeContainer::Track(track_id));
    assert!(reparent_nodes_from_intents(
        &mut project,
        &[NodeDropIntent {
            node_id: solid_id,
            final_rect,
            target,
        }],
    ));
    history.push_project_state(project.clone());

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
        project
            .get_track(track_id)
            .and_then(|track| track.output_node_id),
        Some(track_output_id)
    );
    assert_eq!(
        image_before.as_ref().and_then(|connection| {
            project
                .connections
                .iter()
                .find(|candidate| candidate.id == connection.id)
        }),
        image_before.as_ref(),
    );
    let time_after = time_before.as_ref().and_then(|connection| {
        project
            .connections
            .iter()
            .find(|candidate| candidate.id == connection.id)
    });
    assert!(time_after.is_some_and(|connection| {
        connection.id == time_before.as_ref().map_or(Uuid::nil(), |before| before.id)
            && connection.from == PortAddress::new(PortOwner::Track(track_id), TIME_PORT)
            && connection.to == PortAddress::new(PortOwner::Node(solid_id), TIME_PORT)
    }));
    assert_eq!(project.connections.len(), initial.connections.len());
    assert!(project.validate_containment().is_empty());
    assert!(project.validate_connections().is_empty());
    let edited = project.clone();
    assert_single_gesture_undo_redo(&mut history, &initial, &edited);
}

#[test]
fn rejected_reparent_restores_all_live_positions_without_history_or_wire_mutation() {
    let (mut project, composition_id, _, clip_id, node_id, _) = fixture();
    let initial = project.clone();
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
    let final_min = egui::pos2(origin_position[0] + 700.0, origin_position[1] + 120.0);
    let final_rect =
        egui::Rect::from_min_size(final_min, estimated_node_size(&project, node_id) * 0.5);
    if let Some(node) = project.get_node_mut(node_id) {
        node.ui_position = [final_min.x, final_min.y];
    }
    let target =
        deepest_legal_reparent_target(&project, composition_id, final_rect, final_rect.center());
    assert!(target.is_some());
    let Some(mut target) = target else {
        return;
    };
    target.container = NodeContainer::Clip(Uuid::from_u128(0xdead_beef));
    let outcome = finish_node_reparent(
        &mut project,
        &[NodeDropIntent {
            node_id,
            final_rect,
            target,
        }],
        Some(&gesture),
    );
    assert_eq!(outcome, ReparentReleaseOutcome::Rejected);
    let moved_node_ids = HashSet::from([node_id]);
    assert_eq!(
        splice_node_for_release(outcome, &moved_node_ids, Some(node_id)),
        None
    );
    assert_eq!(
        splice_node_for_release(ReparentReleaseOutcome::NoIntent, &moved_node_ids, None,),
        None
    );
    assert_eq!(
        splice_node_for_release(
            ReparentReleaseOutcome::NoIntent,
            &moved_node_ids,
            Some(node_id),
        ),
        Some(node_id)
    );
    assert_eq!(project, initial);
    let mut history = HistoryManager::new();
    history.push_project_state(initial);
    history.push_project_state(project);
    assert_eq!(history.undo_depth(), 1);
}
