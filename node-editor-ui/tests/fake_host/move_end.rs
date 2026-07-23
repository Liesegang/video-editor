use super::support::*;

fn begin_changed_move(
    context: &egui::Context,
    graph: &FakeGraph,
    state: &mut State,
    options: InteractionOptions,
) -> (Pos2, Pos2) {
    let start = graph.nodes[0].header_rect.center();
    let end = pos2(500.0, 260.0);
    let _ = run_interaction_frame(
        context,
        graph,
        state,
        options,
        vec![
            Event::PointerMoved(start),
            pointer_button(start, true, Modifiers::NONE),
        ],
    );
    let moved = run_interaction_frame(
        context,
        graph,
        state,
        options,
        vec![Event::PointerMoved(end)],
    );
    assert!(moved
        .iter()
        .any(|output| matches!(output, EditorOutput::Move { .. })));
    (start, end)
}

fn assert_one_end(outputs: &[Output], expected: MoveEndOutcome) {
    assert_eq!(
        outputs
            .iter()
            .filter(|output| matches!(output, EditorOutput::MoveEnd { .. }))
            .count(),
        1
    );
    assert!(outputs.contains(&EditorOutput::MoveEnd { outcome: expected }));
}

#[test]
fn changed_move_reports_released_once_and_release_alone_can_reparent() {
    let context = egui::Context::default();
    let graph = FakeGraph::new();
    let mut state = State::default();
    let (_, end) = begin_changed_move(&context, &graph, &mut state, InteractionOptions::ALL);

    let outputs = run_interaction_frame(
        &context,
        &graph,
        &mut state,
        InteractionOptions::ALL,
        vec![pointer_button(end, false, Modifiers::NONE)],
    );

    assert_one_end(&outputs, MoveEndOutcome::Released);
    assert!(outputs.contains(&EditorOutput::Reparent {
        nodes: vec![1],
        parent: Some(11),
    }));
    assert!(!state.is_active());
}

#[test]
fn escape_after_delta_reports_cancelled_once_without_release_semantics() {
    let context = egui::Context::default();
    let graph = FakeGraph::new();
    let mut state = State::default();
    let (_, end) = begin_changed_move(&context, &graph, &mut state, InteractionOptions::ALL);

    let cancelled = run_interaction_frame(
        &context,
        &graph,
        &mut state,
        InteractionOptions::ALL,
        vec![key(egui::Key::Escape)],
    );
    assert_one_end(&cancelled, MoveEndOutcome::Cancelled);
    assert!(cancelled
        .iter()
        .all(|output| !matches!(output, EditorOutput::Reparent { .. })));
    assert!(!state.is_active());

    let released = run_interaction_frame(
        &context,
        &graph,
        &mut state,
        InteractionOptions::ALL,
        vec![pointer_button(end, false, Modifiers::NONE)],
    );
    assert!(released.iter().all(|output| !matches!(
        output,
        EditorOutput::MoveEnd { .. } | EditorOutput::Reparent { .. }
    )));
}

#[test]
fn pointer_loss_and_option_disable_cancel_changed_moves() {
    let graph = FakeGraph::new();

    let pointer_context = egui::Context::default();
    let mut pointer_state = State::default();
    begin_changed_move(
        &pointer_context,
        &graph,
        &mut pointer_state,
        InteractionOptions::ALL,
    );
    let pointer_lost = run_interaction_frame(
        &pointer_context,
        &graph,
        &mut pointer_state,
        InteractionOptions::ALL,
        vec![Event::PointerGone],
    );
    assert_one_end(&pointer_lost, MoveEndOutcome::Cancelled);
    assert!(pointer_lost
        .iter()
        .all(|output| !matches!(output, EditorOutput::Reparent { .. })));

    let option_context = egui::Context::default();
    let mut option_state = State::default();
    begin_changed_move(
        &option_context,
        &graph,
        &mut option_state,
        InteractionOptions::ALL,
    );
    let disabled = run_interaction_frame(
        &option_context,
        &graph,
        &mut option_state,
        InteractionOptions::SELECTION,
        Vec::new(),
    );
    assert_one_end(&disabled, MoveEndOutcome::Cancelled);
    assert!(disabled
        .iter()
        .all(|output| !matches!(output, EditorOutput::Reparent { .. })));
}

#[test]
fn move_end_is_not_emitted_before_threshold_or_without_a_move() {
    for finish in [
        vec![pointer_button(pos2(157.0, 105.0), false, Modifiers::NONE)],
        vec![key(egui::Key::Escape)],
        vec![Event::PointerGone],
    ] {
        let context = egui::Context::default();
        let graph = FakeGraph::new();
        let mut state = State::default();
        let start = graph.nodes[0].header_rect.center();
        let _ = run_interaction_frame(
            &context,
            &graph,
            &mut state,
            InteractionOptions::ALL,
            vec![
                Event::PointerMoved(start),
                pointer_button(start, true, Modifiers::NONE),
                Event::PointerMoved(start + vec2(2.0, 1.0)),
            ],
        );
        let outputs = run_interaction_frame(
            &context,
            &graph,
            &mut state,
            InteractionOptions::ALL,
            finish,
        );
        assert!(outputs
            .iter()
            .all(|output| !matches!(output, EditorOutput::MoveEnd { .. })));
    }

    let context = egui::Context::default();
    let graph = FakeGraph::new();
    let mut state = State::default();
    let start = graph.nodes[0].header_rect.center();
    let _ = run_interaction_frame(
        &context,
        &graph,
        &mut state,
        InteractionOptions::ALL,
        vec![
            Event::PointerMoved(start),
            pointer_button(start, true, Modifiers::NONE),
        ],
    );
    let disabled = run_interaction_frame(
        &context,
        &graph,
        &mut state,
        InteractionOptions::SELECTION,
        Vec::new(),
    );
    assert!(disabled
        .iter()
        .all(|output| !matches!(output, EditorOutput::MoveEnd { .. })));
}
