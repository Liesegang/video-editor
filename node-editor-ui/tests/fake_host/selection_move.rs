use super::support::*;

#[test]
fn fake_host_emits_select_move_and_nested_reparent_intents() {
    let context = egui::Context::default();
    let graph = FakeGraph::new();
    let mut state = State::default();
    let selected = [ItemId::Node(1)];
    let start = graph.nodes[0].header_rect.center();
    let nested = pos2(500.0, 260.0);

    let (pressed, _, _) = run_frame(
        &context,
        &graph,
        &mut state,
        &selected,
        Some(selected[0]),
        vec![
            Event::PointerMoved(start),
            pointer_button(start, true, Modifiers::NONE),
        ],
        Modifiers::NONE,
    );
    assert!(pressed.iter().any(|output| matches!(
        output,
        EditorOutput::Select {
            primary: Some(ItemId::Node(1)),
            ..
        }
    )));

    let (dragged, _, _) = run_frame(
        &context,
        &graph,
        &mut state,
        &selected,
        Some(selected[0]),
        vec![Event::PointerMoved(nested)],
        Modifiers::NONE,
    );
    assert!(dragged.iter().any(|output| matches!(
        output,
        EditorOutput::Move { items, delta, .. }
            if items == &[ItemId::Node(1)] && delta.x > 300.0 && delta.y > 100.0
    )));

    let (released, _, _) = run_frame(
        &context,
        &graph,
        &mut state,
        &selected,
        Some(selected[0]),
        vec![pointer_button(nested, false, Modifiers::NONE)],
        Modifiers::NONE,
    );
    assert!(released.contains(&EditorOutput::Reparent {
        nodes: vec![1],
        parent: Some(11),
    }));
}

#[test]
fn move_freezes_multi_selection_tracks_grabbed_item_and_emits_final_delta() {
    let context = egui::Context::default();
    let graph = FakeGraph::new();
    let mut state = State::default();
    let selected = [ItemId::Node(1), ItemId::Node(2), ItemId::Group(11)];
    let start = graph.nodes[0].header_rect.center();

    let pressed = run_interaction_frame_with(
        &context,
        &graph,
        &mut state,
        InteractionOptions::SELECTION_AND_MOVE,
        &selected,
        Some(ItemId::Group(11)),
        vec![
            Event::PointerMoved(start),
            pointer_button(start, true, Modifiers::NONE),
        ],
        Modifiers::NONE,
        false,
    );
    assert!(state.is_move_active());
    assert!(pressed
        .iter()
        .all(|output| !matches!(output, EditorOutput::Move { .. })));

    let below_threshold = run_interaction_frame_with(
        &context,
        &graph,
        &mut state,
        InteractionOptions::SELECTION_AND_MOVE,
        &[ItemId::Node(1)],
        Some(ItemId::Node(1)),
        vec![Event::PointerMoved(start + vec2(2.0, 1.0))],
        Modifiers::NONE,
        false,
    );
    assert!(below_threshold
        .iter()
        .all(|output| !matches!(output, EditorOutput::Move { .. })));

    let moved = run_interaction_frame_with(
        &context,
        &graph,
        &mut state,
        InteractionOptions::SELECTION_AND_MOVE,
        &[ItemId::Node(1)],
        Some(ItemId::Node(1)),
        vec![Event::PointerMoved(start + vec2(12.0, 6.0))],
        Modifiers::NONE,
        false,
    );
    assert!(moved.iter().any(|output| matches!(
        output,
        EditorOutput::Move {
            items,
            grabbed: ItemId::Node(1),
            delta,
        } if items == &selected && *delta == vec2(12.0, 6.0)
    )));

    let released = run_interaction_frame_with(
        &context,
        &graph,
        &mut state,
        InteractionOptions::SELECTION_AND_MOVE,
        &[ItemId::Node(1)],
        Some(ItemId::Node(1)),
        vec![pointer_button(
            start + vec2(17.0, 9.0),
            false,
            Modifiers::NONE,
        )],
        Modifiers::NONE,
        false,
    );
    assert!(released.iter().any(|output| matches!(
        output,
        EditorOutput::Move {
            items,
            grabbed: ItemId::Node(1),
            delta,
        } if items == &selected && *delta == vec2(5.0, 3.0)
    )));
    assert!(!state.is_active());
}

#[test]
fn batched_press_and_motion_uses_press_origin_for_the_full_move_delta() {
    let context = egui::Context::default();
    let graph = FakeGraph::new();
    let mut state = State::default();
    let start = graph.nodes[0].header_rect.center();
    let end = start + vec2(24.0, 12.0);
    let selected = [ItemId::Node(1), ItemId::Node(2)];

    let outputs = run_interaction_frame_with(
        &context,
        &graph,
        &mut state,
        InteractionOptions::SELECTION_AND_MOVE,
        &selected,
        Some(ItemId::Node(2)),
        vec![
            Event::PointerMoved(start),
            pointer_button(start, true, Modifiers::NONE),
            Event::PointerMoved(end),
        ],
        Modifiers::NONE,
        false,
    );

    assert!(outputs.iter().any(|output| matches!(
        output,
        EditorOutput::Move {
            items,
            grabbed: ItemId::Node(1),
            delta,
        } if items == &selected && *delta == vec2(24.0, 12.0)
    )));
}

#[test]
fn movement_modifiers_and_node_body_do_not_start_header_movement() {
    let graph = FakeGraph::new();
    let start = graph.nodes[0].header_rect.center();
    for modifiers in [
        Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
        Modifiers {
            command: true,
            ctrl: true,
            ..Modifiers::NONE
        },
    ] {
        let context = egui::Context::default();
        let mut state = State::default();
        let outputs = run_interaction_frame_with(
            &context,
            &graph,
            &mut state,
            InteractionOptions::SELECTION_AND_MOVE,
            &[ItemId::Node(1)],
            Some(ItemId::Node(1)),
            vec![
                Event::PointerMoved(start),
                pointer_button(start, true, modifiers),
            ],
            modifiers,
            false,
        );
        assert!(!state.is_active());
        assert!(outputs
            .iter()
            .all(|output| !matches!(output, EditorOutput::Move { .. })));
        assert!(outputs.contains(&EditorOutput::Select {
            items: Vec::new(),
            primary: None,
        }));
    }

    let context = egui::Context::default();
    let mut state = State::default();
    let body = pos2(
        graph.nodes[0].rect.center().x,
        graph.nodes[0].rect.bottom() - 16.0,
    );
    let _ = run_interaction_frame_with(
        &context,
        &graph,
        &mut state,
        InteractionOptions::SELECTION_AND_MOVE,
        &[ItemId::Node(1)],
        Some(ItemId::Node(1)),
        vec![
            Event::PointerMoved(body),
            pointer_button(body, true, Modifiers::NONE),
        ],
        Modifiers::NONE,
        false,
    );
    assert!(state.is_active());
    assert!(!state.is_move_active());
    let dragged = run_interaction_frame_with(
        &context,
        &graph,
        &mut state,
        InteractionOptions::SELECTION_AND_MOVE,
        &[ItemId::Node(1)],
        Some(ItemId::Node(1)),
        vec![Event::PointerMoved(body + vec2(40.0, 20.0))],
        Modifiers::NONE,
        false,
    );
    assert!(dragged
        .iter()
        .all(|output| !matches!(output, EditorOutput::Move { .. })));
}

#[test]
fn cross_kind_overlap_and_marquee_follow_one_host_z_order() {
    let mut graph = FakeGraph::new();
    graph.groups[0].header_rect = graph.nodes[0].rect;
    graph.selection_order = vec![ItemId::Node(1), ItemId::Group(10)];
    let context = egui::Context::default();
    let mut state = State::default();
    let overlap = graph.nodes[0].rect.center();

    let clicked = run_interaction_frame(
        &context,
        &graph,
        &mut state,
        InteractionOptions::SELECTION,
        vec![
            Event::PointerMoved(overlap),
            pointer_button(overlap, true, Modifiers::NONE),
        ],
    );
    assert!(clicked.contains(&EditorOutput::Select {
        items: vec![ItemId::Group(10)],
        primary: Some(ItemId::Group(10)),
    }));

    let context = egui::Context::default();
    let mut state = State::default();
    let start = pos2(70.0, 80.0);
    let end = pos2(240.0, 230.0);
    let _ = run_interaction_frame(
        &context,
        &graph,
        &mut state,
        InteractionOptions::SELECTION,
        vec![
            Event::PointerMoved(start),
            pointer_button(start, true, Modifiers::NONE),
        ],
    );
    let marquee = run_interaction_frame(
        &context,
        &graph,
        &mut state,
        InteractionOptions::SELECTION,
        vec![
            Event::PointerMoved(end),
            pointer_button(end, false, Modifiers::NONE),
        ],
    );
    assert!(marquee.contains(&EditorOutput::Select {
        items: vec![ItemId::Node(1), ItemId::Group(10)],
        primary: Some(ItemId::Group(10)),
    }));
}

#[test]
fn select_false_never_emits_select_for_nodes_groups_wires_or_blank_canvas() {
    let graph = FakeGraph::new();
    let options = InteractionOptions {
        select: false,
        select_wires: true,
        marquee: true,
        move_items: false,
        connect: false,
        disconnect: false,
        delete: false,
        reparent: false,
        resize_groups: false,
        layout_swipe: node_editor_ui::LayoutSwipeHitArea::Disabled,
    };
    for point in [
        graph.nodes[0].rect.center(),
        graph.groups[0].header_rect.center(),
        graph.wires[0].curve.point(0.5),
        pos2(780.0, 480.0),
    ] {
        let context = egui::Context::default();
        let mut state = State::default();
        let outputs = run_interaction_frame(
            &context,
            &graph,
            &mut state,
            options,
            vec![
                Event::PointerMoved(point),
                pointer_button(point, true, Modifiers::NONE),
            ],
        );
        assert!(outputs
            .iter()
            .all(|output| !matches!(output, EditorOutput::Select { .. })));
    }
}
