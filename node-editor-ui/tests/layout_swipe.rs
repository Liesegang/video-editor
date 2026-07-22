use std::cell::RefCell;

use egui::{pos2, vec2, Event, Modifiers, Pos2, RawInput, Rect};
use node_editor_ui::{
    AuthoritativeSelection, CubicBezier, Editor, EditorOutput, GraphFrame, GroupDescriptor,
    InteractionOptions, InteractionState, ItemId, LayoutSwipeAxis, LayoutSwipeIntent,
    LayoutSwipePhase, NodeDescriptor, PortDescriptor, PortDirection, PortOwner, TypeKey,
    WireDescriptor,
};

type Output = EditorOutput<u8, u8, u8, u8>;
type State = InteractionState<u8, u8, u8, u8>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DataKind {
    Image,
}

#[derive(Clone)]
struct FakeGraph {
    nodes: Vec<NodeDescriptor<'static, u8, u8>>,
    ports: Vec<PortDescriptor<'static, u8, u8, u8, DataKind>>,
    wires: Vec<WireDescriptor<u8, u8>>,
    groups: Vec<GroupDescriptor<'static, u8>>,
    order: Vec<ItemId<u8, u8, u8>>,
}

impl FakeGraph {
    fn new() -> Self {
        Self {
            nodes: vec![
                NodeDescriptor {
                    id: 1,
                    title: "Anchor",
                    rect: Rect::from_min_size(pos2(100.0, 100.0), vec2(120.0, 100.0)),
                    header_rect: Rect::from_min_size(pos2(100.0, 100.0), vec2(120.0, 24.0)),
                    parent: None,
                    enabled: true,
                },
                NodeDescriptor {
                    id: 2,
                    title: "Target",
                    rect: Rect::from_min_size(pos2(360.0, 100.0), vec2(120.0, 100.0)),
                    header_rect: Rect::from_min_size(pos2(360.0, 100.0), vec2(120.0, 24.0)),
                    parent: None,
                    enabled: true,
                },
            ],
            ports: vec![
                PortDescriptor {
                    id: 10,
                    owner: PortOwner::Node(1),
                    label: "Image",
                    center: pos2(220.0, 160.0),
                    direction: PortDirection::Output,
                    type_key: TypeKey::new(DataKind::Image),
                    connectable: true,
                },
                PortDescriptor {
                    id: 11,
                    owner: PortOwner::Node(2),
                    label: "Image",
                    center: pos2(360.0, 160.0),
                    direction: PortDirection::Input,
                    type_key: TypeKey::new(DataKind::Image),
                    connectable: true,
                },
            ],
            wires: vec![WireDescriptor {
                id: 20,
                from: 10,
                to: 11,
                curve: CubicBezier::new(
                    pos2(220.0, 160.0),
                    pos2(270.0, 160.0),
                    pos2(310.0, 160.0),
                    pos2(360.0, 160.0),
                ),
                editable: true,
            }],
            groups: vec![GroupDescriptor {
                id: 30,
                title: "Group",
                rect: Rect::from_min_size(pos2(40.0, 40.0), vec2(500.0, 240.0)),
                header_rect: Rect::from_min_size(pos2(40.0, 40.0), vec2(500.0, 24.0)),
                parent: None,
                resizable: true,
            }],
            order: vec![ItemId::Group(30), ItemId::Node(1), ItemId::Node(2)],
        }
    }

    fn frame(
        &self,
        transform: egui::emath::TSTransform,
    ) -> GraphFrame<'_, u8, u8, u8, u8, DataKind> {
        GraphFrame {
            viewport: Rect::from_min_size(Pos2::ZERO, vec2(900.0, 700.0)),
            transform,
            nodes: &self.nodes,
            ports: &self.ports,
            wires: &self.wires,
            groups: &self.groups,
            selection_order: &self.order,
            selection: AuthoritativeSelection::default(),
        }
    }
}

struct FrameInput {
    events: Vec<Event>,
    modifiers: Modifiers,
    focused: bool,
    pointer_blocked: bool,
    keyboard_focus: bool,
}

impl FrameInput {
    fn events(events: Vec<Event>, modifiers: Modifiers) -> Self {
        Self {
            events,
            modifiers,
            focused: true,
            pointer_blocked: false,
            keyboard_focus: false,
        }
    }
}

fn run_frame(
    context: &egui::Context,
    graph: &FakeGraph,
    state: &mut State,
    transform: egui::emath::TSTransform,
    options: InteractionOptions,
    input: FrameInput,
) -> Vec<Output> {
    let outputs = RefCell::new(Vec::new());
    drop(context.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 700.0))),
            events: input.events,
            modifiers: input.modifiers,
            focused: input.focused,
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(context, |ui| {
                    if input.keyboard_focus {
                        ui.ctx()
                            .memory_mut(|memory| memory.request_focus(egui::Id::new("search")));
                    }
                    outputs.borrow_mut().extend(Editor::interact(
                        ui,
                        &graph.frame(transform),
                        state,
                        options,
                        input.pointer_blocked,
                    ));
                });
        },
    ));
    outputs.into_inner()
}

fn pointer(position: Pos2, pressed: bool, modifiers: Modifiers) -> Event {
    Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers,
    }
}

fn middle(position: Pos2, pressed: bool, modifiers: Modifiers) -> Event {
    Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Middle,
        pressed,
        modifiers,
    }
}

fn key(key: egui::Key, pressed: bool, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat: false,
        modifiers,
    }
}

fn arm(
    context: &egui::Context,
    graph: &FakeGraph,
    state: &mut State,
    transform: egui::emath::TSTransform,
    modifiers: Modifiers,
) -> (Pos2, Vec<Output>) {
    let start = transform * graph.nodes[0].header_rect.center();
    let outputs = run_frame(
        context,
        graph,
        state,
        transform,
        InteractionOptions::ALL,
        FrameInput::events(
            vec![
                key(egui::Key::A, true, modifiers),
                Event::PointerMoved(start),
                pointer(start, true, modifiers),
            ],
            modifiers,
        ),
    );
    (start, outputs)
}

fn swipe(output: &Output) -> Option<&LayoutSwipeIntent<u8>> {
    let Output::LayoutSwipe(intent) = output else {
        return None;
    };
    Some(intent)
}

#[allow(
    clippy::expect_used,
    reason = "the preceding assertion makes a missing typed test output the failure being reported"
)]
fn only_swipe(outputs: &[Output]) -> &LayoutSwipeIntent<u8> {
    assert_eq!(outputs.len(), 1, "unexpected outputs: {outputs:?}");
    swipe(&outputs[0]).expect("layout swipe output")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CancelCase {
    Threshold,
    ARelease,
    Escape,
    Focus,
}

#[test]
fn raw_gesture_uses_a_screen_threshold_and_locks_the_first_dominant_axis() {
    let context = egui::Context::default();
    let graph = FakeGraph::new();
    let mut state = State::default();
    let transform = egui::emath::TSTransform::IDENTITY;
    let (start, started) = arm(&context, &graph, &mut state, transform, Modifiers::NONE);
    let started = only_swipe(&started);
    assert_eq!(started.phase, LayoutSwipePhase::Start);
    assert_eq!(started.anchor, 1);
    assert_eq!(started.start, start);
    assert_eq!(started.current, start);
    assert_eq!(started.axis, None);
    assert_eq!(started.transform, transform);
    assert!(state.is_layout_swipe_active());

    let below = run_frame(
        &context,
        &graph,
        &mut state,
        transform,
        InteractionOptions::ALL,
        FrameInput::events(
            vec![Event::PointerMoved(start + vec2(11.99, 0.0))],
            Modifiers::NONE,
        ),
    );
    assert!(below.is_empty());

    let activated = run_frame(
        &context,
        &graph,
        &mut state,
        transform,
        InteractionOptions::ALL,
        FrameInput::events(
            vec![Event::PointerMoved(start + vec2(12.0, 0.0))],
            Modifiers::NONE,
        ),
    );
    assert_eq!(
        only_swipe(&activated).axis,
        Some(LayoutSwipeAxis::Horizontal)
    );

    let crossed = run_frame(
        &context,
        &graph,
        &mut state,
        transform,
        InteractionOptions::ALL,
        FrameInput::events(
            vec![Event::PointerMoved(start + vec2(2.0, 40.0))],
            Modifiers::NONE,
        ),
    );
    assert_eq!(only_swipe(&crossed).axis, Some(LayoutSwipeAxis::Horizontal));

    let committed = run_frame(
        &context,
        &graph,
        &mut state,
        transform,
        InteractionOptions::ALL,
        FrameInput::events(
            vec![pointer(start + vec2(2.0, 40.0), false, Modifiers::NONE)],
            Modifiers::NONE,
        ),
    );
    assert_eq!(only_swipe(&committed).phase, LayoutSwipePhase::Commit);
    assert!(!state.is_active());
}

#[test]
fn modifiers_are_frozen_exactly_for_plain_shift_alt_and_shift_alt() {
    let combinations = [
        Modifiers::NONE,
        Modifiers::SHIFT,
        Modifiers::ALT,
        Modifiers {
            shift: true,
            alt: true,
            ..Modifiers::NONE
        },
    ];
    for modifiers in combinations {
        let context = egui::Context::default();
        let graph = FakeGraph::new();
        let mut state = State::default();
        let transform = egui::emath::TSTransform::IDENTITY;
        let (start, started) = arm(&context, &graph, &mut state, transform, modifiers);
        assert_eq!(only_swipe(&started).modifiers, modifiers);
        assert!(started
            .iter()
            .all(|output| !matches!(output, Output::Select { .. } | Output::Move { .. })));

        let updated = run_frame(
            &context,
            &graph,
            &mut state,
            transform,
            InteractionOptions::ALL,
            FrameInput::events(
                vec![Event::PointerMoved(start + vec2(0.0, 20.0))],
                modifiers,
            ),
        );
        let intent = only_swipe(&updated);
        assert_eq!(intent.modifiers, modifiers);
        assert_eq!(intent.axis, Some(LayoutSwipeAxis::Vertical));
    }

    let context = egui::Context::default();
    let graph = FakeGraph::new();
    let mut state = State::default();
    let start = graph.nodes[0].header_rect.center();
    let event_modifiers = Modifiers {
        shift: true,
        alt: true,
        ..Modifiers::NONE
    };
    let started = run_frame(
        &context,
        &graph,
        &mut state,
        egui::emath::TSTransform::IDENTITY,
        InteractionOptions::ALL,
        FrameInput::events(
            vec![
                key(egui::Key::A, true, event_modifiers),
                Event::PointerMoved(start),
                pointer(start, true, event_modifiers),
            ],
            Modifiers::NONE,
        ),
    );
    assert_eq!(only_swipe(&started).modifiers, event_modifiers);
}

#[test]
fn batched_press_motion_and_release_keeps_the_complete_phase_protocol() {
    let context = egui::Context::default();
    let graph = FakeGraph::new();
    let mut state = State::default();
    let start = graph.nodes[0].header_rect.center();
    let end = start + vec2(24.0, 0.0);
    let outputs = run_frame(
        &context,
        &graph,
        &mut state,
        egui::emath::TSTransform::IDENTITY,
        InteractionOptions::ALL,
        FrameInput::events(
            vec![
                key(egui::Key::A, true, Modifiers::NONE),
                Event::PointerMoved(start),
                pointer(start, true, Modifiers::NONE),
                Event::PointerMoved(end),
                pointer(end, false, Modifiers::NONE),
            ],
            Modifiers::NONE,
        ),
    );
    let phases = outputs
        .iter()
        .filter_map(swipe)
        .map(|intent| intent.phase)
        .collect::<Vec<_>>();
    assert_eq!(
        phases,
        [
            LayoutSwipePhase::Start,
            LayoutSwipePhase::Update,
            LayoutSwipePhase::Commit,
        ]
    );
    assert!(!state.is_active());
}

#[test]
fn threshold_is_zoom_independent_and_transform_stays_frozen() {
    for scale in [0.25, 4.0] {
        let context = egui::Context::default();
        let graph = FakeGraph::new();
        let mut state = State::default();
        let frozen = egui::emath::TSTransform::from_scaling(scale);
        let (start, _) = arm(&context, &graph, &mut state, frozen, Modifiers::NONE);
        let changed = egui::emath::TSTransform::from_scaling(1.0);
        assert_eq!(state.locked_transform(), Some(frozen));

        let below = run_frame(
            &context,
            &graph,
            &mut state,
            changed,
            InteractionOptions::ALL,
            FrameInput::events(
                vec![Event::PointerMoved(start + vec2(0.0, 11.99))],
                Modifiers::NONE,
            ),
        );
        assert!(below.is_empty());
        let active = run_frame(
            &context,
            &graph,
            &mut state,
            changed,
            InteractionOptions::ALL,
            FrameInput::events(
                vec![Event::PointerMoved(start + vec2(0.0, 12.0))],
                Modifiers::NONE,
            ),
        );
        let intent = only_swipe(&active);
        assert_eq!(intent.axis, Some(LayoutSwipeAxis::Vertical));
        assert_eq!(intent.transform, frozen);
        assert_eq!(state.locked_transform(), Some(frozen));
    }
}

#[test]
fn release_before_threshold_a_release_escape_and_context_loss_cancel() {
    let cases = [
        CancelCase::Threshold,
        CancelCase::ARelease,
        CancelCase::Escape,
        CancelCase::Focus,
    ];
    for case in cases {
        let context = egui::Context::default();
        let graph = FakeGraph::new();
        let mut state = State::default();
        let transform = egui::emath::TSTransform::IDENTITY;
        let (start, _) = arm(&context, &graph, &mut state, transform, Modifiers::NONE);
        if case != CancelCase::Threshold {
            let _ = run_frame(
                &context,
                &graph,
                &mut state,
                transform,
                InteractionOptions::ALL,
                FrameInput::events(
                    vec![Event::PointerMoved(start + vec2(20.0, 0.0))],
                    Modifiers::NONE,
                ),
            );
        }
        let input = match case {
            CancelCase::Threshold => FrameInput::events(
                vec![pointer(start + vec2(4.0, 0.0), false, Modifiers::NONE)],
                Modifiers::NONE,
            ),
            CancelCase::ARelease => FrameInput::events(
                vec![key(egui::Key::A, false, Modifiers::NONE)],
                Modifiers::NONE,
            ),
            CancelCase::Escape => FrameInput::events(
                vec![key(egui::Key::Escape, true, Modifiers::NONE)],
                Modifiers::NONE,
            ),
            CancelCase::Focus => FrameInput {
                focused: false,
                ..FrameInput::events(Vec::new(), Modifiers::NONE)
            },
        };
        let cancelled = run_frame(
            &context,
            &graph,
            &mut state,
            transform,
            InteractionOptions::ALL,
            input,
        );
        assert_eq!(
            only_swipe(&cancelled).phase,
            LayoutSwipePhase::Cancel,
            "{case:?}"
        );
        assert!(!state.is_active(), "{case:?}");
    }
}

#[test]
fn same_frame_release_order_distinguishes_commit_from_a_first_cancel() {
    for pointer_first in [true, false] {
        let context = egui::Context::default();
        let graph = FakeGraph::new();
        let mut state = State::default();
        let transform = egui::emath::TSTransform::IDENTITY;
        let (start, _) = arm(&context, &graph, &mut state, transform, Modifiers::NONE);
        let end = start + vec2(20.0, 0.0);
        let _ = run_frame(
            &context,
            &graph,
            &mut state,
            transform,
            InteractionOptions::ALL,
            FrameInput::events(vec![Event::PointerMoved(end)], Modifiers::NONE),
        );
        let release_pointer = pointer(end, false, Modifiers::NONE);
        let release_a = key(egui::Key::A, false, Modifiers::NONE);
        let events = if pointer_first {
            vec![release_pointer, release_a]
        } else {
            vec![release_a, release_pointer]
        };
        let ended = run_frame(
            &context,
            &graph,
            &mut state,
            transform,
            InteractionOptions::ALL,
            FrameInput::events(events, Modifiers::NONE),
        );
        assert_eq!(
            only_swipe(&ended).phase,
            if pointer_first {
                LayoutSwipePhase::Commit
            } else {
                LayoutSwipePhase::Cancel
            }
        );
    }
}

#[test]
fn specialized_gestures_navigation_and_keyboard_focus_take_priority() {
    let graph = FakeGraph::new();
    let transform = egui::emath::TSTransform::IDENTITY;
    let start = graph.nodes[0].header_rect.center();

    for input in [
        FrameInput {
            pointer_blocked: true,
            ..FrameInput::events(
                vec![
                    key(egui::Key::A, true, Modifiers::NONE),
                    Event::PointerMoved(start),
                    pointer(start, true, Modifiers::NONE),
                ],
                Modifiers::NONE,
            )
        },
        FrameInput {
            keyboard_focus: true,
            ..FrameInput::events(
                vec![
                    key(egui::Key::A, true, Modifiers::NONE),
                    Event::PointerMoved(start),
                    pointer(start, true, Modifiers::NONE),
                ],
                Modifiers::NONE,
            )
        },
        FrameInput::events(
            vec![
                key(egui::Key::Space, true, Modifiers::NONE),
                key(egui::Key::A, true, Modifiers::NONE),
                Event::PointerMoved(start),
                pointer(start, true, Modifiers::NONE),
            ],
            Modifiers::NONE,
        ),
        FrameInput::events(
            vec![
                key(egui::Key::A, true, Modifiers::NONE),
                Event::PointerMoved(start),
                middle(start, true, Modifiers::NONE),
                pointer(start, true, Modifiers::NONE),
            ],
            Modifiers::NONE,
        ),
    ] {
        let context = egui::Context::default();
        let mut state = State::default();
        let outputs = run_frame(
            &context,
            &graph,
            &mut state,
            transform,
            InteractionOptions::ALL,
            input,
        );
        assert!(outputs.iter().all(|output| swipe(output).is_none()));
        assert!(!state.is_layout_swipe_active());
    }

    let mut port_graph = graph.clone();
    port_graph.ports[0].center = start;
    let context = egui::Context::default();
    let mut state = State::default();
    let outputs = arm(
        &context,
        &port_graph,
        &mut state,
        transform,
        Modifiers::NONE,
    )
    .1;
    assert!(outputs.iter().all(|output| swipe(output).is_none()));
    assert!(state.is_active());
    assert!(!state.is_layout_swipe_active());

    let mut resize_graph = graph;
    resize_graph.groups[0].rect = Rect::from_min_max(pos2(40.0, 40.0), start);
    let context = egui::Context::default();
    let mut state = State::default();
    let outputs = arm(
        &context,
        &resize_graph,
        &mut state,
        transform,
        Modifiers::NONE,
    )
    .1;
    assert!(outputs.iter().all(|output| swipe(output).is_none()));
    assert!(state.is_active());
    assert!(!state.is_layout_swipe_active());
}

#[test]
fn header_hit_suppresses_ordinary_edits_and_overview_can_use_the_whole_node() {
    let graph = FakeGraph::new();
    let transform = egui::emath::TSTransform::IDENTITY;
    let context = egui::Context::default();
    let mut state = State::default();
    let (start, started) = arm(&context, &graph, &mut state, transform, Modifiers::SHIFT);
    assert!(started
        .iter()
        .all(|output| !matches!(output, Output::Select { .. } | Output::Move { .. })));
    let moved = run_frame(
        &context,
        &graph,
        &mut state,
        transform,
        InteractionOptions::ALL,
        FrameInput::events(
            vec![Event::PointerMoved(start + vec2(24.0, 8.0))],
            Modifiers::SHIFT,
        ),
    );
    assert!(moved
        .iter()
        .all(|output| !matches!(output, Output::Select { .. } | Output::Move { .. })));

    let body = graph.nodes[0].rect.center_bottom() - vec2(0.0, 12.0);
    let context = egui::Context::default();
    let mut state = State::default();
    let detailed = run_frame(
        &context,
        &graph,
        &mut state,
        transform,
        InteractionOptions::SELECTION,
        FrameInput::events(
            vec![
                key(egui::Key::A, true, Modifiers::NONE),
                Event::PointerMoved(body),
                pointer(body, true, Modifiers::NONE),
            ],
            Modifiers::NONE,
        ),
    );
    assert!(detailed.iter().all(|output| swipe(output).is_none()));

    let context = egui::Context::default();
    let mut state = State::default();
    let overview = run_frame(
        &context,
        &graph,
        &mut state,
        transform,
        InteractionOptions::OVERVIEW_SELECTION,
        FrameInput::events(
            vec![
                key(egui::Key::A, true, Modifiers::NONE),
                Event::PointerMoved(body),
                pointer(body, true, Modifiers::NONE),
            ],
            Modifiers::NONE,
        ),
    );
    assert_eq!(only_swipe(&overview).phase, LayoutSwipePhase::Start);
}

#[test]
fn ordinary_header_move_still_works_without_a() {
    let context = egui::Context::default();
    let graph = FakeGraph::new();
    let mut state = State::default();
    let transform = egui::emath::TSTransform::IDENTITY;
    let start = graph.nodes[0].header_rect.center();
    let pressed = run_frame(
        &context,
        &graph,
        &mut state,
        transform,
        InteractionOptions::ALL,
        FrameInput::events(
            vec![
                Event::PointerMoved(start),
                pointer(start, true, Modifiers::NONE),
            ],
            Modifiers::NONE,
        ),
    );
    assert!(pressed
        .iter()
        .any(|output| matches!(output, Output::Select { .. })));
    assert!(pressed.iter().all(|output| swipe(output).is_none()));
    let dragged = run_frame(
        &context,
        &graph,
        &mut state,
        transform,
        InteractionOptions::ALL,
        FrameInput::events(
            vec![Event::PointerMoved(start + vec2(20.0, 4.0))],
            Modifiers::NONE,
        ),
    );
    assert!(dragged.iter().any(|output| matches!(
        output,
        Output::Move { items, delta }
            if items == &[ItemId::Node(1)] && *delta == vec2(20.0, 4.0)
    )));
}
