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
