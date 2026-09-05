use super::support::*;

#[test]
fn fake_host_emits_connect_disconnect_delete_and_deselect_wire_intents() {
    let graph = FakeGraph::new();

    let connect_context = egui::Context::default();
    let mut connect_state = State::default();
    let source = graph.ports[0].center;
    let target = graph.ports[1].center;
    let _ = run_frame(
        &connect_context,
        &graph,
        &mut connect_state,
        &[],
        None,
        vec![
            Event::PointerMoved(source),
            pointer_button(source, true, Modifiers::NONE),
        ],
        Modifiers::NONE,
    );
    let (connected, _, _) = run_frame(
        &connect_context,
        &graph,
        &mut connect_state,
        &[],
        None,
        vec![
            Event::PointerMoved(target),
            pointer_button(target, false, Modifiers::NONE),
        ],
        Modifiers::NONE,
    );
    assert!(connected.contains(&EditorOutput::Connect { from: 20, to: 21 }));

    let disconnect_context = egui::Context::default();
    let mut disconnect_state = State::default();
    let wire_midpoint = pos2(330.0, 170.0);
    let alt = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };
    let (disconnected, _, _) = run_frame(
        &disconnect_context,
        &graph,
        &mut disconnect_state,
        &[],
        None,
        vec![
            Event::PointerMoved(wire_midpoint),
            pointer_button(wire_midpoint, true, alt),
        ],
        alt,
    );
    assert!(disconnected.contains(&EditorOutput::Disconnect { wire: 30 }));

    let keyboard_context = egui::Context::default();
    let selected_items = [ItemId::Node(1), ItemId::Wire(30)];
    let mut keyboard_state = State::default();
    let (deleted, _, _) = run_frame(
        &keyboard_context,
        &graph,
        &mut keyboard_state,
        &selected_items,
        Some(ItemId::Wire(30)),
        vec![key(egui::Key::Delete)],
        Modifiers::NONE,
    );
    assert!(deleted.contains(&EditorOutput::Disconnect { wire: 30 }));
    assert!(deleted.contains(&EditorOutput::Delete {
        items: vec![ItemId::Node(1)],
    }));

    let (deselected, _, _) = run_frame(
        &keyboard_context,
        &graph,
        &mut keyboard_state,
        &[ItemId::Wire(30)],
        Some(ItemId::Wire(30)),
        vec![key(egui::Key::Escape)],
        Modifiers::NONE,
    );
    assert!(deselected.contains(&EditorOutput::DeselectWire { wire: 30 }));
}

#[test]
fn right_click_requests_a_context_menu_for_the_wire_under_the_pointer() {
    let graph = FakeGraph::new();
    let context = egui::Context::default();
    let mut state = State::default();
    let target = Editor::wire_selection_target(&graph.frame(&[], None), &30).unwrap();
    let secondary = egui::PointerButton::Secondary;

    let _ = run_interaction_frame(
        &context,
        &graph,
        &mut state,
        InteractionOptions::ALL,
        vec![
            Event::PointerMoved(target),
            pointer_button_with(target, secondary, true, Modifiers::NONE),
        ],
    );
    let outputs = run_interaction_frame(
        &context,
        &graph,
        &mut state,
        InteractionOptions::ALL,
        vec![
            Event::PointerMoved(target),
            pointer_button_with(target, secondary, false, Modifiers::NONE),
        ],
    );

    assert_eq!(
        outputs,
        vec![EditorOutput::WireContextMenu {
            wire: 30,
            screen_position: target,
        }]
    );
}

#[test]
fn node_occlusion_wins_over_a_wire_right_click() {
    let mut graph = FakeGraph::new();
    let occluder = Rect::from_min_size(pos2(300.0, 145.0), vec2(60.0, 50.0));
    graph.nodes.push(NodeDescriptor {
        id: 3,
        title: "Occluder",
        rect: occluder,
        header_rect: occluder,
        parent: Some(10),
        enabled: true,
    });
    graph.selection_order.push(ItemId::Node(3));
    let context = egui::Context::default();
    let mut state = State::default();
    let target = pos2(330.0, 170.0);
    let secondary = egui::PointerButton::Secondary;

    let _ = run_interaction_frame(
        &context,
        &graph,
        &mut state,
        InteractionOptions::ALL,
        vec![
            Event::PointerMoved(target),
            pointer_button_with(target, secondary, true, Modifiers::NONE),
        ],
    );
    let outputs = run_interaction_frame(
        &context,
        &graph,
        &mut state,
        InteractionOptions::ALL,
        vec![pointer_button_with(
            target,
            secondary,
            false,
            Modifiers::NONE,
        )],
    );

    assert!(outputs.is_empty());
}

#[test]
fn ctrl_right_drag_cuts_each_crossed_wire() {
    let graph = FakeGraph::new();
    let context = egui::Context::default();
    let mut state = State::default();
    let modifiers = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    let secondary = egui::PointerButton::Secondary;
    let start = pos2(330.0, 130.0);
    let end = pos2(330.0, 210.0);

    let _ = run_interaction_frame_with(
        &context,
        &graph,
        &mut state,
        InteractionOptions::ALL,
        &[],
        None,
        vec![
            Event::PointerMoved(start),
            pointer_button_with(start, secondary, true, modifiers),
        ],
        modifiers,
        false,
    );
    let outputs = run_interaction_frame_with(
        &context,
        &graph,
        &mut state,
        InteractionOptions::ALL,
        &[],
        None,
        vec![
            Event::PointerMoved(end),
            pointer_button_with(end, secondary, false, modifiers),
        ],
        modifiers,
        false,
    );

    assert_eq!(outputs, vec![EditorOutput::Disconnect { wire: 30 }]);
}

#[test]
fn ctrl_right_click_without_a_drag_does_not_cut() {
    let graph = FakeGraph::new();
    let context = egui::Context::default();
    let mut state = State::default();
    let modifiers = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    let secondary = egui::PointerButton::Secondary;
    let target = Editor::wire_selection_target(&graph.frame(&[], None), &30).unwrap();

    let _ = run_interaction_frame_with(
        &context,
        &graph,
        &mut state,
        InteractionOptions::ALL,
        &[],
        None,
        vec![
            Event::PointerMoved(target),
            pointer_button_with(target, secondary, true, modifiers),
        ],
        modifiers,
        false,
    );
    let outputs = run_interaction_frame_with(
        &context,
        &graph,
        &mut state,
        InteractionOptions::ALL,
        &[],
        None,
        vec![pointer_button_with(target, secondary, false, modifiers)],
        modifiers,
        false,
    );

    assert!(outputs.is_empty());
}

#[test]
fn alt_right_drag_lazy_connects_compatible_nodes() {
    let mut graph = FakeGraph::new();
    graph.wires.clear();
    let context = egui::Context::default();
    let mut state = State::default();
    let modifiers = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };
    let secondary = egui::PointerButton::Secondary;
    let source = graph.nodes[0].rect.center();
    let target = graph.nodes[1].rect.center();

    let _ = run_interaction_frame_with(
        &context,
        &graph,
        &mut state,
        InteractionOptions::ALL,
        &[],
        None,
        vec![
            Event::PointerMoved(source),
            pointer_button_with(source, secondary, true, modifiers),
        ],
        modifiers,
        false,
    );
    let outputs = run_interaction_frame_with(
        &context,
        &graph,
        &mut state,
        InteractionOptions::ALL,
        &[],
        None,
        vec![
            Event::PointerMoved(target),
            pointer_button_with(target, secondary, false, modifiers),
        ],
        modifiers,
        false,
    );

    assert_eq!(outputs, vec![EditorOutput::Connect { from: 20, to: 21 }]);
}

#[test]
fn lazy_connect_also_resolves_a_reverse_node_drag() {
    let mut graph = FakeGraph::new();
    graph.wires.clear();
    let context = egui::Context::default();
    let mut state = State::default();
    let modifiers = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };
    let secondary = egui::PointerButton::Secondary;
    let target = graph.nodes[1].rect.center();
    let source = graph.nodes[0].rect.center();

    let _ = run_interaction_frame_with(
        &context,
        &graph,
        &mut state,
        InteractionOptions::ALL,
        &[],
        None,
        vec![
            Event::PointerMoved(target),
            pointer_button_with(target, secondary, true, modifiers),
        ],
        modifiers,
        false,
    );
    let outputs = run_interaction_frame_with(
        &context,
        &graph,
        &mut state,
        InteractionOptions::ALL,
        &[],
        None,
        vec![
            Event::PointerMoved(source),
            pointer_button_with(source, secondary, false, modifiers),
        ],
        modifiers,
        false,
    );

    assert_eq!(outputs, vec![EditorOutput::Connect { from: 20, to: 21 }]);
}

#[test]
fn authoritative_wire_target_avoids_node_occlusion_and_selects_the_wire() {
    let mut graph = FakeGraph::new();
    let occluder = Rect::from_min_size(pos2(300.0, 145.0), vec2(60.0, 50.0));
    graph.nodes.push(NodeDescriptor {
        id: 3,
        title: "Occluder",
        rect: occluder,
        header_rect: occluder,
        parent: Some(10),
        enabled: true,
    });
    graph.selection_order.push(ItemId::Node(3));
    let target = Editor::wire_selection_target(&graph.frame(&[], None), &30)
        .expect("the visible remainder of the curve should remain selectable");
    assert!(!occluder.contains(target));

    let context = egui::Context::default();
    let mut state = State::default();
    let (outputs, _, _) = run_frame(
        &context,
        &graph,
        &mut state,
        &[],
        None,
        vec![
            Event::PointerMoved(target),
            pointer_button(target, true, Modifiers::NONE),
        ],
        Modifiers::NONE,
    );
    assert!(outputs.contains(&EditorOutput::Select {
        items: vec![ItemId::Wire(30)],
        primary: Some(ItemId::Wire(30)),
    }));
}

#[test]
fn selected_wire_handle_reconnects_one_endpoint_without_disconnect() {
    let mut graph = FakeGraph::new();
    graph.ports.push(PortDescriptor {
        id: 22,
        owner: PortOwner::Node(2),
        label: "Alternate image",
        center: pos2(620.0, 280.0),
        direction: PortDirection::Output,
        type_key: TypeKey::new(DataKind::Image),
        connectable: true,
    });
    let context = egui::Context::default();
    let mut state = State::default();
    let selected = [ItemId::Wire(30)];
    // The production reconnect handle is 11 screen points inside the source
    // endpoint, leaving the socket itself available for an ordinary fan-out.
    let handle = pos2(241.0, 170.0);
    let _ = run_frame(
        &context,
        &graph,
        &mut state,
        &selected,
        Some(ItemId::Wire(30)),
        vec![
            Event::PointerMoved(handle),
            pointer_button(handle, true, Modifiers::NONE),
        ],
        Modifiers::NONE,
    );
    let (outputs, _, _) = run_frame(
        &context,
        &graph,
        &mut state,
        &selected,
        Some(ItemId::Wire(30)),
        vec![
            Event::PointerMoved(pos2(620.0, 280.0)),
            pointer_button(pos2(620.0, 280.0), false, Modifiers::NONE),
        ],
        Modifiers::NONE,
    );
    assert!(outputs.contains(&EditorOutput::Reconnect {
        wire: 30,
        from: 22,
        to: 21,
    }));
    assert!(!outputs
        .iter()
        .any(|output| matches!(output, EditorOutput::Disconnect { .. })));
}

#[test]
fn selected_wire_target_handle_reconnects_the_input_endpoint() {
    let mut graph = FakeGraph::new();
    graph.ports.push(PortDescriptor {
        id: 23,
        owner: PortOwner::Node(1),
        label: "Alternate input",
        center: pos2(80.0, 280.0),
        direction: PortDirection::Input,
        type_key: TypeKey::new(DataKind::Image),
        connectable: true,
    });
    let context = egui::Context::default();
    let mut state = State::default();
    let selected = [ItemId::Wire(30)];
    let handle = pos2(419.0, 170.0);
    let _ = run_frame(
        &context,
        &graph,
        &mut state,
        &selected,
        Some(ItemId::Wire(30)),
        vec![
            Event::PointerMoved(handle),
            pointer_button(handle, true, Modifiers::NONE),
        ],
        Modifiers::NONE,
    );
    let (outputs, _, _) = run_frame(
        &context,
        &graph,
        &mut state,
        &selected,
        Some(ItemId::Wire(30)),
        vec![
            Event::PointerMoved(pos2(80.0, 280.0)),
            pointer_button(pos2(80.0, 280.0), false, Modifiers::NONE),
        ],
        Modifiers::NONE,
    );
    assert!(outputs.contains(&EditorOutput::Reconnect {
        wire: 30,
        from: 20,
        to: 23,
    }));
}

#[test]
fn reconnect_rejects_a_port_with_a_different_type() {
    let mut graph = FakeGraph::new();
    graph.ports.push(PortDescriptor {
        id: 24,
        owner: PortOwner::Node(2),
        label: "Number",
        center: pos2(620.0, 280.0),
        direction: PortDirection::Output,
        type_key: TypeKey::new(DataKind::Number),
        connectable: true,
    });
    let context = egui::Context::default();
    let mut state = State::default();
    let selected = [ItemId::Wire(30)];
    let handle = pos2(241.0, 170.0);
    let _ = run_frame(
        &context,
        &graph,
        &mut state,
        &selected,
        Some(ItemId::Wire(30)),
        vec![
            Event::PointerMoved(handle),
            pointer_button(handle, true, Modifiers::NONE),
        ],
        Modifiers::NONE,
    );
    let (outputs, _, _) = run_frame(
        &context,
        &graph,
        &mut state,
        &selected,
        Some(ItemId::Wire(30)),
        vec![
            Event::PointerMoved(pos2(620.0, 280.0)),
            pointer_button(pos2(620.0, 280.0), false, Modifiers::NONE),
        ],
        Modifiers::NONE,
    );
    assert!(!outputs
        .iter()
        .any(|output| matches!(output, EditorOutput::Reconnect { .. })));
}

#[test]
fn host_type_policy_allows_a_directional_numeric_widening() {
    let mut graph = FakeGraph::new();
    graph.ports.extend([
        PortDescriptor {
            id: 25,
            owner: PortOwner::Node(1),
            label: "Integer",
            center: pos2(230.0, 280.0),
            direction: PortDirection::Output,
            type_key: TypeKey::new(DataKind::Integer),
            connectable: true,
        },
        PortDescriptor {
            id: 26,
            owner: PortOwner::Node(2),
            label: "Number",
            center: pos2(430.0, 280.0),
            direction: PortDirection::Input,
            type_key: TypeKey::new(DataKind::Number),
            connectable: true,
        },
    ]);
    let context = egui::Context::default();
    let mut state = State::default();
    let source = pos2(230.0, 280.0);
    let target = pos2(430.0, 280.0);
    let _ = run_frame(
        &context,
        &graph,
        &mut state,
        &[],
        None,
        vec![
            Event::PointerMoved(source),
            pointer_button(source, true, Modifiers::NONE),
        ],
        Modifiers::NONE,
    );
    let (outputs, _, _) = run_frame(
        &context,
        &graph,
        &mut state,
        &[],
        None,
        vec![
            Event::PointerMoved(target),
            pointer_button(target, false, Modifiers::NONE),
        ],
        Modifiers::NONE,
    );

    assert!(outputs.contains(&EditorOutput::Connect { from: 25, to: 26 }));
}

#[test]
fn fake_host_emits_group_resize_intent_from_invisible_edge_hit() {
    let context = egui::Context::default();
    let graph = FakeGraph::new();
    let mut state = State::default();
    let selected = [ItemId::Group(11)];
    let start = graph.groups[1].rect.right_bottom();
    let end = start + vec2(30.0, 25.0);

    let _ = run_frame(
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
    let (resized, _, _) = run_frame(
        &context,
        &graph,
        &mut state,
        &selected,
        Some(selected[0]),
        vec![Event::PointerMoved(end)],
        Modifiers::NONE,
    );
    assert!(resized.iter().any(|output| matches!(
        output,
        EditorOutput::ResizeGroup { group: 11, rect }
            if rect.size() == graph.groups[1].rect.size() + vec2(30.0, 25.0)
    )));
}
