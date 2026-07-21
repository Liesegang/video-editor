use std::cell::RefCell;

use egui::{pos2, vec2, Event, Modifiers, Pos2, RawInput, Rect};
use node_editor_ui::{
    AuthoritativeSelection, CubicBezier, Editor, EditorConfig, EditorOutput, GraphFrame,
    GroupDescriptor, InteractionState, ItemId, NodeBodyRenderer, NodeDescriptor, PortDescriptor,
    PortDirection, PortOwner, TypeKey, WireDescriptor,
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
                parent: Some(10),
                enabled: true,
            },
            NodeDescriptor {
                id: 2,
                title: "Result",
                rect: Rect::from_min_size(pos2(430.0, 100.0), vec2(150.0, 130.0)),
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
    fn show(&mut self, node: &u8, ui: &mut egui::Ui) {
        self.rendered.borrow_mut().push(*node);
        ui.label(format!("fake property for {node}"));
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
    let start = pos2(140.0, 135.0);
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
