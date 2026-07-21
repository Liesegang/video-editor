use super::*;

#[test]
fn scc_ranking_is_deterministic_and_keeps_cycles_in_one_rank() {
    let a = Uuid::from_u128(1);
    let b = Uuid::from_u128(2);
    let c = Uuid::from_u128(3);
    let nodes = vec![c, b, a];
    let edges = vec![(a, b), (b, a), (b, c)];
    let ranks = rank_nodes_by_scc(&nodes, &edges);
    assert_eq!(ranks[&a], ranks[&b]);
    assert!(ranks[&b] < ranks[&c]);
}

#[test]
fn full_layout_preserves_hierarchy_non_overlap_and_global_ltr_in_rendered_rects() {
    let (mut project, ids) = adversarial_hierarchy_fixture();
    assert!(layout_needs_reflow(&project, ids.composition));

    let full = compute_full_composition_layout(&project, ids.composition).unwrap();
    for scope in [
        AutoLayoutScope::All,
        AutoLayoutScope::Container(PortOwner::Composition(ids.composition)),
    ] {
        assert_eq!(
            compute_auto_layout(&project, ids.composition, scope).unwrap(),
            full
        );
    }

    assert!(apply_auto_layout(&mut project, ids.composition, &full));
    assert!(!layout_needs_reflow(&project, ids.composition));

    let rects = render_test_graph(&project, ids.composition);
    let composition = project.get_composition(ids.composition).unwrap();
    let composition_rect = rects
        .get(&format!(
            "node_editor.container.{}",
            qa_container_key(PortOwner::Composition(ids.composition))
        ))
        .copied()
        .unwrap();
    let scale = composition_rect.width() / composition.ui_size[0];
    let screen_padding = AUTO_LAYOUT_NODE_PADDING * scale;

    let node_ids = composition_graph_node_ids(&project, ids.composition);
    let rendered_nodes = node_ids
        .iter()
        .filter_map(|node_id| {
            let rect = rects.get(&format!("node_editor.node:{node_id}")).copied()?;
            let estimated = estimated_node_size(&project, *node_id);
            assert!(
                rect.width() <= estimated.x * scale + 1.0,
                "{} ({node_id}): {rect:?}, estimated={estimated:?}, scale={scale}",
                node_title(&project, *node_id),
            );
            assert!(
                rect.height() <= estimated.y * scale + 1.0,
                "{} ({node_id}): {rect:?}, estimated={estimated:?}, scale={scale}",
                node_title(&project, *node_id),
            );
            Some((*node_id, rect))
        })
        .collect::<HashMap<_, _>>();
    let mut node_ids = rendered_nodes.keys().copied().collect::<Vec<_>>();
    node_ids.sort_unstable();

    for (index, left_id) in node_ids.iter().enumerate() {
        for right_id in &node_ids[index + 1..] {
            let left = rendered_nodes[left_id].expand(screen_padding * 0.5);
            let right = rendered_nodes[right_id].expand(screen_padding * 0.5);
            assert!(
                !left.intersects(right),
                "rendered Nodes overlap: {left_id} {left:?}, {right_id} {right:?}"
            );
        }
    }

    let immediate_owners = [
        (ids.solid, PortOwner::Clip(ids.clip)),
        (ids.merge, PortOwner::Clip(ids.clip)),
        (ids.text, PortOwner::Clip(ids.sibling_clip)),
        (ids.text_fill, PortOwner::Clip(ids.sibling_clip)),
        (ids.shape, PortOwner::Track(ids.track)),
        (ids.shape_fill, PortOwner::Track(ids.track)),
        (
            ids.composition_node,
            PortOwner::Composition(ids.composition),
        ),
    ];
    for (node_id, owner) in immediate_owners {
        let owner_key = qa_container_key(owner);
        let owner_rect = rects
            .get(&format!("node_editor.container.{owner_key}"))
            .copied()
            .unwrap();
        let node_rect = rendered_nodes[&node_id];
        assert!(
            rect_contains_rect(owner_rect.expand(0.5), node_rect),
            "{node_id} escaped immediate owner {owner_key}: {node_rect:?} not in {owner_rect:?}"
        );
        assert!(
            node_rect.top() >= owner_rect.top() + CONTAINER_HEADER_HEIGHT * scale - 1.0,
            "{node_id} overlaps integrated header of {owner_key}"
        );
    }

    let sibling_clips = [ids.clip, ids.sibling_clip, ids.empty_clip];
    for (index, left_id) in sibling_clips.iter().enumerate() {
        let left = rects
            .get(&format!(
                "node_editor.container.{}",
                qa_container_key(PortOwner::Clip(*left_id))
            ))
            .copied()
            .unwrap()
            .expand(screen_padding * 0.5);
        for right_id in &sibling_clips[index + 1..] {
            let right = rects
                .get(&format!(
                    "node_editor.container.{}",
                    qa_container_key(PortOwner::Clip(*right_id))
                ))
                .copied()
                .unwrap()
                .expand(screen_padding * 0.5);
            assert!(
                !left.intersects(right),
                "Clip overlap: {left:?} / {right:?}"
            );
        }
    }

    let track = rects
        .get(&format!(
            "node_editor.container.{}",
            qa_container_key(PortOwner::Track(ids.track))
        ))
        .copied()
        .unwrap();
    let empty_track = rects
        .get(&format!(
            "node_editor.container.{}",
            qa_container_key(PortOwner::Track(ids.empty_track))
        ))
        .copied()
        .unwrap();
    assert!(!track.intersects(empty_track));
    assert!(rect_contains_rect(composition_rect.expand(0.5), track));
    assert!(rect_contains_rect(
        composition_rect.expand(0.5),
        empty_track
    ));

    let all_nodes = composition_graph_node_ids(&project, ids.composition);
    let edges = canonical_edges(&project, &all_nodes);
    let ranks = rank_nodes_by_scc(&all_nodes, &edges);
    for (from, to) in edges {
        if ranks[&from] == ranks[&to] {
            continue;
        }
        let Some((from_rect, to_rect)) = rendered_nodes.get(&from).zip(rendered_nodes.get(&to))
        else {
            continue;
        };
        assert!(
            from_rect.right() + screen_padding <= to_rect.left() + 1.0,
            "canonical edge is not LTR: {from} {from_rect:?} -> {to} {to_rect:?}"
        );
    }
}

#[test]
fn scoped_layouts_leave_every_out_of_scope_entity_byte_identical() {
    let (mut base, ids) = adversarial_hierarchy_fixture();
    let full = compute_full_composition_layout(&base, ids.composition).unwrap();
    apply_auto_layout(&mut base, ids.composition, &full);
    assert!(!layout_needs_reflow(&base, ids.composition));

    let mut track_project = base.clone();
    let unrelated_track_before = track_project.get_track(ids.empty_track).unwrap().clone();
    let root_node_before = track_project
        .get_node(ids.composition_node)
        .unwrap()
        .clone();
    let track_plan = compute_auto_layout(
        &track_project,
        ids.composition,
        AutoLayoutScope::Container(PortOwner::Track(ids.track)),
    )
    .unwrap();
    assert!(!track_plan.track_layouts.contains_key(&ids.empty_track));
    assert!(!track_plan
        .node_positions
        .contains_key(&ids.composition_node));
    apply_auto_layout(&mut track_project, ids.composition, &track_plan);
    assert_eq!(
        track_project.get_track(ids.empty_track).unwrap(),
        &unrelated_track_before
    );
    assert_eq!(
        track_project.get_node(ids.composition_node).unwrap(),
        &root_node_before
    );
    assert!(!layout_needs_reflow(&track_project, ids.composition));

    let mut clip_project = base.clone();
    let sibling_before = clip_project.get_clip(ids.sibling_clip).unwrap().clone();
    let text_before = clip_project.get_node(ids.text).unwrap().clone();
    let shape_before = clip_project.get_node(ids.shape).unwrap().clone();
    let clip_plan = compute_auto_layout(
        &clip_project,
        ids.composition,
        AutoLayoutScope::Container(PortOwner::Clip(ids.clip)),
    )
    .unwrap();
    assert!(!clip_plan.clip_layouts.contains_key(&ids.sibling_clip));
    assert!(!clip_plan.node_positions.contains_key(&ids.text));
    assert!(!clip_plan.node_positions.contains_key(&ids.shape));
    apply_auto_layout(&mut clip_project, ids.composition, &clip_plan);
    assert_eq!(
        clip_project.get_clip(ids.sibling_clip).unwrap(),
        &sibling_before
    );
    assert_eq!(clip_project.get_node(ids.text).unwrap(), &text_before);
    assert_eq!(clip_project.get_node(ids.shape).unwrap(), &shape_before);
    assert!(
        !layout_needs_reflow(&clip_project, ids.composition),
        "Clip-scoped plan violated layout invariants: {clip_plan:#?}"
    );

    let mut selection_project = base.clone();
    let merge_position = selection_project.get_node(ids.merge).unwrap().ui_position;
    selection_project
        .get_node_mut(ids.solid)
        .unwrap()
        .ui_position = merge_position;
    let merge_before = selection_project.get_node(ids.merge).unwrap().clone();
    let text_before = selection_project.get_node(ids.text).unwrap().clone();
    let selection_plan = compute_auto_layout(
        &selection_project,
        ids.composition,
        AutoLayoutScope::Selection(vec![ids.solid]),
    )
    .unwrap();
    assert_eq!(
        selection_plan
            .node_positions
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![ids.solid]
    );
    apply_auto_layout(&mut selection_project, ids.composition, &selection_plan);
    assert_eq!(
        selection_project.get_node(ids.merge).unwrap(),
        &merge_before
    );
    assert_eq!(selection_project.get_node(ids.text).unwrap(), &text_before);
    assert!(!layout_needs_reflow(&selection_project, ids.composition));
}

#[test]
fn new_nodes_use_a_dependency_near_free_slot_inside_clip() {
    let (mut project, _, _, clip_id, existing_id, _) = fixture();
    let node = Node::new_merge("New");
    let node_id = node.id;
    project.add_node(node);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
        .unwrap();
    let placed = place_node_in_free_slot(
        &mut project,
        node_id,
        NodeContainer::Clip(clip_id),
        egui::Pos2::ZERO,
        &[existing_id],
    )
    .unwrap();
    let existing = project.get_node(existing_id).unwrap();
    assert!(placed.x >= existing.ui_position[0] + estimated_node_width() + AUTO_LAYOUT_COLUMN_GAP);
    let placed_rect = egui::Rect::from_min_size(placed, estimated_node_size(&project, node_id));
    let clip = project.get_clip(clip_id).unwrap();
    let clip_rect = container_rect(clip.ui_position, clip.ui_size);
    assert!(clip_rect.contains(placed_rect.min));
    assert!(clip_rect.contains(placed_rect.max));
}
