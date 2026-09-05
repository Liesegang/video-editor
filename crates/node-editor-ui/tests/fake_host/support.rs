use std::cell::RefCell;

pub(super) use egui::{pos2, vec2, Event, Modifiers, Pos2, RawInput, Rect};
pub(super) use node_editor_ui::{
    AuthoritativeSelection, CubicBezier, Editor, EditorConfig, EditorOutput, GraphFrame,
    GroupChrome, GroupDescriptor, HeaderGlyph, InteractionOptions, InteractionState, ItemId,
    MoveEndOutcome, NodeBodyRenderer, NodeBodyResponse, NodeDescriptor, NodeHeader, NodePalette,
    PortDescriptor, PortDirection, PortLabel, PortOwner, TypeKey, WireDescriptor,
};

pub(super) type Output = EditorOutput<u8, u8, u8, u8>;
pub(super) type State = InteractionState<u8, u8, u8, u8>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DataKind {
    Image,
    Number,
    Integer,
}

fn ports_compatible(source: &DataKind, target: &DataKind) -> bool {
    source == target || (*source == DataKind::Integer && *target == DataKind::Number)
}

pub(super) struct FakeGraph {
    pub(super) nodes: Vec<NodeDescriptor<'static, u8, u8>>,
    pub(super) ports: Vec<PortDescriptor<'static, u8, u8, u8, DataKind>>,
    pub(super) wires: Vec<WireDescriptor<u8, u8>>,
    pub(super) groups: Vec<GroupDescriptor<'static, u8>>,
    pub(super) selection_order: Vec<ItemId<u8, u8, u8>>,
}

impl FakeGraph {
    pub(super) fn new() -> Self {
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
            color: egui::Color32::from_rgb(145, 151, 170),
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

    pub(super) fn frame<'a>(
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
            ports_compatible,
            selection_order: &self.selection_order,
            selection: AuthoritativeSelection {
                items: selected,
                primary,
            },
        }
    }
}

#[derive(Default)]
pub(super) struct FakeBodyRenderer {
    pub(super) rendered: RefCell<Vec<u8>>,
}

impl NodeBodyRenderer<u8> for FakeBodyRenderer {
    fn show(&mut self, node: &u8, ui: &mut egui::Ui) -> NodeBodyResponse {
        self.rendered.borrow_mut().push(*node);
        ui.label(format!("fake property for {node}"));
        NodeBodyResponse::NONE
    }
}

pub(super) struct DragValueBodyRenderer<'a> {
    pub(super) value: &'a mut f64,
    pub(super) response_rect: &'a RefCell<Rect>,
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

pub(super) fn pointer_button(position: Pos2, pressed: bool, modifiers: Modifiers) -> Event {
    pointer_button_with(position, egui::PointerButton::Primary, pressed, modifiers)
}

pub(super) fn pointer_button_with(
    position: Pos2,
    button: egui::PointerButton,
    pressed: bool,
    modifiers: Modifiers,
) -> Event {
    Event::PointerButton {
        pos: position,
        button,
        pressed,
        modifiers,
    }
}

pub(super) fn key(key: egui::Key) -> Event {
    Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

pub(super) fn run_frame(
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

pub(super) fn run_interaction_frame(
    context: &egui::Context,
    graph: &FakeGraph,
    state: &mut State,
    options: InteractionOptions,
    events: Vec<Event>,
) -> Vec<Output> {
    run_interaction_frame_with(
        context,
        graph,
        state,
        options,
        &[],
        None,
        events,
        Modifiers::NONE,
        false,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "the selection and input snapshot are the fake host's complete frame contract"
)]
pub(super) fn run_interaction_frame_with(
    context: &egui::Context,
    graph: &FakeGraph,
    state: &mut State,
    options: InteractionOptions,
    selected: &[ItemId<u8, u8, u8>],
    primary: Option<ItemId<u8, u8, u8>>,
    events: Vec<Event>,
    modifiers: Modifiers,
    pointer_blocked: bool,
) -> Vec<Output> {
    let outputs = RefCell::new(Vec::new());
    drop(context.run(
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
                    let overlay_painter = ui.painter().clone();
                    outputs.borrow_mut().extend(Editor::interact(
                        ui,
                        &overlay_painter,
                        &graph.frame(selected, primary),
                        state,
                        options,
                        pointer_blocked,
                    ));
                });
        },
    ));
    outputs.into_inner()
}
