use std::cell::RefCell;

use egui::{pos2, vec2, Event, Modifiers, Pos2, RawInput, Rect};
use node_editor_ui::{
    AuthoritativeSelection, CubicBezier, Editor, EditorConfig, EditorOutput, GraphFrame,
    GroupDescriptor, InteractionOptions, InteractionState, ItemId, NodeBodyRenderer,
    NodeBodyResponse, NodeDescriptor, PortDescriptor, PortDirection, PortOwner, TypeKey,
    WireDescriptor,
};

type Output = EditorOutput<u8, u8, u8, u8>;
type State = InteractionState<u8, u8, u8, u8>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DataKind {
    Image,
}

struct FakeGraph {
    nodes: Vec<NodeDescriptor<'static, u8, u8>>,
    ports: Vec<PortDescriptor<'static, u8, u8, u8, DataKind>>,
    wires: Vec<WireDescriptor<u8, u8>>,
    groups: Vec<GroupDescriptor<'static, u8>>,
    selection_order: Vec<ItemId<u8, u8, u8>>,
}

impl FakeGraph {
    fn new() -> Self {
        let groups = vec![
            GroupDescriptor {
                id: 10,
                title: "Root group",
                rect: Rect::from_min_size(pos2(20.0, 20.0), vec2(740.0, 430.0)),
                header_rect: Rect::from_min_size(pos2(20.0, 20.0), vec2(740.0, 32.0)),
                parent: None,
                resizable: true,
            },
            GroupDescriptor {
                id: 11,
                title: "Nested group",
                rect: Rect::from_min_size(pos2(380.0, 60.0), vec2(340.0, 300.0)),
                header_rect: Rect::from_min_size(pos2(380.0, 60.0), vec2(340.0, 28.0)),
                parent: Some(10),
                resizable: true,
            },
        ];
        let nodes = vec![
            NodeDescriptor {
                id: 1,
                title: "Source",
                rect: Rect::from_min_size(pos2(80.0, 90.0), vec2(150.0, 130.0)),
                header_rect: Rect::from_min_size(pos2(80.0, 90.0), vec2(150.0, 28.0)),
                parent: Some(10),
                enabled: true,
            },
            NodeDescriptor {
                id: 2,
                title: "Result",
                rect: Rect::from_min_size(pos2(430.0, 100.0), vec2(150.0, 130.0)),
                header_rect: Rect::from_min_size(pos2(430.0, 100.0), vec2(150.0, 28.0)),
                parent: Some(11),
                enabled: true,
            },
        ];
        let ports = vec![
            PortDescriptor {
                id: 20,
                owner: PortOwner::Node(1),
                label: "Image",
                center: pos2(230.0, 170.0),
                direction: PortDirection::Output,
                type_key: TypeKey::new(DataKind::Image),
                connectable: true,
            },
            PortDescriptor {
                id: 21,
                owner: PortOwner::Node(2),
                label: "Image",
                center: pos2(430.0, 170.0),
                direction: PortDirection::Input,
                type_key: TypeKey::new(DataKind::Image),
                connectable: true,
            },
        ];
        let wires = vec![WireDescriptor {
            id: 30,
            from: 20,
            to: 21,
            curve: CubicBezier::new(
                pos2(230.0, 170.0),
                pos2(290.0, 170.0),
                pos2(370.0, 170.0),
                pos2(430.0, 170.0),
            ),
            editable: true,
        }];
        Self {
            nodes,
            ports,
            wires,
            groups,
            selection_order: vec![
                ItemId::Group(10),
                ItemId::Group(11),
                ItemId::Node(1),
                ItemId::Node(2),
            ],
        }
    }

    fn frame<'a>(
        &'a self,
        selected: &'a [ItemId<u8, u8, u8>],
        primary: Option<ItemId<u8, u8, u8>>,
    ) -> GraphFrame<'a, u8, u8, u8, u8, DataKind> {
        GraphFrame {
            viewport: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 500.0)),
            transform: egui::emath::TSTransform::IDENTITY,
            nodes: &self.nodes,
            ports: &self.ports,
            wires: &self.wires,
            groups: &self.groups,
            selection_order: &self.selection_order,
            selection: AuthoritativeSelection {
                items: selected,
                primary,
            },
        }
    }
}

#[derive(Default)]
struct FakeBodyRenderer {
    rendered: RefCell<Vec<u8>>,
}

impl NodeBodyRenderer<u8> for FakeBodyRenderer {
    fn show(&mut self, node: &u8, ui: &mut egui::Ui) -> NodeBodyResponse {
        self.rendered.borrow_mut().push(*node);
        ui.label(format!("fake property for {node}"));
        NodeBodyResponse::NONE
    }
}

struct DragValueBodyRenderer<'a> {
    value: &'a mut f64,
    response_rect: &'a RefCell<Rect>,
}

impl NodeBodyRenderer<u8> for DragValueBodyRenderer<'_> {
    fn show(&mut self, node: &u8, ui: &mut egui::Ui) -> NodeBodyResponse {
        if *node != 1 {
            return NodeBodyResponse::NONE;
        }
        let response = ui.add(egui::DragValue::new(self.value).speed(1.0));
        self.response_rect.replace(response.rect);
        NodeBodyResponse::from_response(&response)
    }
}

fn pointer_button(position: Pos2, pressed: bool, modifiers: Modifiers) -> Event {
    Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers,
    }
}

fn key(key: egui::Key) -> Event {
    Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

fn run_frame(
    context: &egui::Context,
    graph: &FakeGraph,
    state: &mut State,
    selected: &[ItemId<u8, u8, u8>],
    primary: Option<ItemId<u8, u8, u8>>,
    events: Vec<Event>,
    modifiers: Modifiers,
) -> (Vec<Output>, egui::FullOutput, Vec<u8>) {
    let outputs = RefCell::new(Vec::new());
    let mut renderer = FakeBodyRenderer::default();
    let full = context.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 500.0))),
            events,
            modifiers,
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(context, |ui| {
                    outputs.borrow_mut().extend(Editor::show(
                        ui,
                        &graph.frame(selected, primary),
                        state,
                        &mut renderer,
                        EditorConfig::default(),
                    ));
                });
        },
    );
    let rendered = renderer.rendered.into_inner();
    (outputs.into_inner(), full, rendered)
}

fn run_interaction_frame(
    context: &egui::Context,
    graph: &FakeGraph,
    state: &mut State,
    options: InteractionOptions,
    events: Vec<Event>,
) -> Vec<Output> {
    let outputs = RefCell::new(Vec::new());
    drop(context.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 500.0))),
            events,
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(context, |ui| {
                    outputs.borrow_mut().extend(Editor::interact(
                        ui,
                        &graph.frame(&[], None),
                        state,
                        options,
                        false,
                    ));
                });
        },
    ));
    outputs.into_inner()
}

#[test]
fn fake_host_renders_nested_groups_nodes_wires_ports_and_host_bodies() {
    let context = egui::Context::default();
    let graph = FakeGraph::new();
    let mut state = State::default();

    let (outputs, full, rendered) = run_frame(
        &context,
        &graph,
        &mut state,
        &[],
        None,
        Vec::new(),
        Modifiers::NONE,
    );

    assert!(outputs.is_empty());
    assert_eq!(rendered, [1, 2]);
    assert!(!full.shapes.is_empty());
    assert_eq!(graph.groups[1].parent, Some(graph.groups[0].id));
}

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
        EditorOutput::Move { items, delta }
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

#[test]
fn body_drag_value_owns_pointer_while_header_still_moves_node() {
    let context = egui::Context::default();
    let graph = FakeGraph::new();
    let mut state = State::default();
    let response_rect = RefCell::new(Rect::NOTHING);
    let mut value = 0.0;

    // First layout publishes the real DragValue rectangle.
    {
        let mut render = |events: Vec<Event>| {
            let outputs = RefCell::new(Vec::new());
            let mut renderer = DragValueBodyRenderer {
                value: &mut value,
                response_rect: &response_rect,
            };
            drop(context.run(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 500.0))),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(context, |ui| {
                            outputs.borrow_mut().extend(Editor::show(
                                ui,
                                &graph.frame(&[ItemId::Node(1)], Some(ItemId::Node(1))),
                                &mut state,
                                &mut renderer,
                                EditorConfig::default(),
                            ));
                        });
                },
            ));
            outputs.into_inner()
        };
        assert!(render(Vec::new()).is_empty());
        let control = response_rect.borrow().center();
        assert!(graph.nodes[0].rect.contains(control));
        assert!(!graph.nodes[0].header_rect.contains(control));

        let pressed = render(vec![
            Event::PointerMoved(control),
            pointer_button(control, true, Modifiers::NONE),
        ]);
        let dragged = render(vec![Event::PointerMoved(control + vec2(35.0, 0.0))]);
        let released = render(vec![pointer_button(
            control + vec2(35.0, 0.0),
            false,
            Modifiers::NONE,
        )]);
        assert!(pressed
            .iter()
            .all(|output| !matches!(output, EditorOutput::Move { .. })));
        assert!(dragged
            .iter()
            .all(|output| !matches!(output, EditorOutput::Move { .. })));
        assert!(released
            .iter()
            .all(|output| !matches!(output, EditorOutput::Move { .. })));
        let header = graph.nodes[0].header_rect.center();
        let _ = render(vec![
            Event::PointerMoved(header),
            pointer_button(header, true, Modifiers::NONE),
        ]);
        let moved = render(vec![Event::PointerMoved(header + vec2(24.0, 12.0))]);
        assert!(moved.iter().any(|output| matches!(
            output,
            EditorOutput::Move { items, delta }
                if items == &[ItemId::Node(1)] && *delta == vec2(24.0, 12.0)
        )));
    }
    assert_ne!(value, 0.0, "the real DragValue must receive the drag");
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
