use super::*;

use library::model::authoring::{
    ModuleDefinitionId, ModuleDefinitionSharing, ModuleGraph, ModuleInterface,
};
use library::model::project::{IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT};

const SCREEN_SIZE: egui::Vec2 = egui::vec2(800.0, 500.0);
const SOURCE_POSITION: egui::Pos2 = egui::pos2(40.0, 80.0);
const TARGET_POSITION: egui::Pos2 = egui::pos2(420.0, 80.0);

fn fixture(plugins: &PluginManager) -> (ModuleDefinition, Uuid, Uuid) {
    let service = TimelineEditorService::create_default("Factory").expect("authoring service");
    let mut source = service
        .create_module_node(
            plugins,
            ModuleNodeRequest::Solid {
                color: Color::white(),
            },
            1920,
            1080,
        )
        .expect("Solid Node");
    source.ui_position = [SOURCE_POSITION.x, SOURCE_POSITION.y];
    let source_id = source.id;
    let mut target = plugins
        .create_image_opacity_style_operation_node()
        .expect("Image Opacity Node");
    target.ui_position = [TARGET_POSITION.x, TARGET_POSITION.y];
    let target_id = target.id;
    (
        ModuleDefinition {
            id: ModuleDefinitionId::new(),
            name: "Adapter interaction".to_string(),
            sharing: ModuleDefinitionSharing::Private,
            graph: ModuleGraph {
                nodes: HashMap::from([(source_id, source), (target_id, target)]),
                connections: Vec::new(),
            },
            interface: ModuleInterface::default(),
            topology_revision: 1,
            interface_version: 1,
        },
        source_id,
        target_id,
    )
}

fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn run_frame(
    context: &egui::Context,
    definition: &ModuleDefinition,
    state: &mut ModuleNodeEditorState,
    plugins: &PluginManager,
    events: Vec<egui::Event>,
    focused: bool,
) -> (Vec<ModuleEditorAction>, egui::Rect) {
    let actions = std::cell::RefCell::new(Vec::new());
    let viewport = std::cell::Cell::new(egui::Rect::NOTHING);
    drop(context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN_SIZE)),
            events,
            focused,
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(context, |ui| {
                    viewport.set(ui.available_rect_before_wrap());
                    *actions.borrow_mut() =
                        show_module_document(ui, definition, None, state, plugins, 0.0);
                });
        },
    ));
    (actions.into_inner(), viewport.get())
}

fn port_position(
    viewport: egui::Rect,
    definition: &ModuleDefinition,
    node_id: Uuid,
    port_key: &str,
    direction: PortDirection,
) -> egui::Pos2 {
    let node = &definition.graph.nodes[&node_id];
    let contract = ModuleNodePortContract::resolve(node).expect("port contract");
    let row = contract
        .ports
        .iter()
        .filter(|port| port.direction == direction)
        .position(|port| port.key == port_key)
        .expect("fixture port");
    let width = node.ui_size[0].max(MIN_NODE_WIDTH).max(260.0);
    let x = node.ui_position[0]
        + if direction == PortDirection::Output {
            width
        } else {
            0.0
        };
    viewport.min
        + egui::vec2(
            x,
            node.ui_position[1] + HEADER_HEIGHT + 12.0 + row as f32 * PORT_ROW_HEIGHT,
        )
}

#[test]
fn real_module_adapter_emits_connect_after_port_drag_at_scale_one() {
    let context = egui::Context::default();
    let plugins = PluginManager::default();
    let (definition, source_id, target_id) = fixture(&plugins);
    let mut state = ModuleNodeEditorState::default();
    let (_, viewport) = run_frame(
        &context,
        &definition,
        &mut state,
        &plugins,
        Vec::new(),
        true,
    );
    let source = port_position(
        viewport,
        &definition,
        source_id,
        IMAGE_OUTPUT_PORT,
        PortDirection::Output,
    );
    let target = port_position(
        viewport,
        &definition,
        target_id,
        IMAGE_INPUT_PORT,
        PortDirection::Input,
    );

    let _ = run_frame(
        &context,
        &definition,
        &mut state,
        &plugins,
        vec![egui::Event::PointerMoved(source)],
        true,
    );
    let _ = run_frame(
        &context,
        &definition,
        &mut state,
        &plugins,
        vec![pointer_button(source, true)],
        true,
    );
    let _ = run_frame(
        &context,
        &definition,
        &mut state,
        &plugins,
        vec![egui::Event::PointerMoved(source.lerp(target, 0.5))],
        true,
    );
    let _ = run_frame(
        &context,
        &definition,
        &mut state,
        &plugins,
        vec![egui::Event::PointerMoved(target)],
        true,
    );
    let (actions, _) = run_frame(
        &context,
        &definition,
        &mut state,
        &plugins,
        vec![pointer_button(target, false)],
        true,
    );

    assert!(actions.iter().any(|action| matches!(
        action,
        ModuleEditorAction::Connect { from, to }
            if from.node_id == source_id
                && from.port == IMAGE_OUTPUT_PORT
                && to.node_id == target_id
                && to.port == IMAGE_INPUT_PORT
    )));
}

#[test]
fn real_module_adapter_emits_move_and_finish_after_header_drag_at_scale_one() {
    let context = egui::Context::default();
    let plugins = PluginManager::default();
    let (definition, source_id, _) = fixture(&plugins);
    let mut state = ModuleNodeEditorState::default();
    let (_, viewport) = run_frame(
        &context,
        &definition,
        &mut state,
        &plugins,
        Vec::new(),
        true,
    );
    let start = viewport.min + SOURCE_POSITION.to_vec2() + egui::vec2(80.0, 15.0);
    let end = start + egui::vec2(48.0, 28.0);

    let _ = run_frame(
        &context,
        &definition,
        &mut state,
        &plugins,
        vec![egui::Event::PointerMoved(start)],
        true,
    );
    let _ = run_frame(
        &context,
        &definition,
        &mut state,
        &plugins,
        vec![pointer_button(start, true)],
        true,
    );
    let (moved, _) = run_frame(
        &context,
        &definition,
        &mut state,
        &plugins,
        vec![egui::Event::PointerMoved(end)],
        true,
    );
    let (finished, _) = run_frame(
        &context,
        &definition,
        &mut state,
        &plugins,
        vec![pointer_button(end, false)],
        true,
    );

    assert!(moved.iter().any(|action| matches!(
        action,
        ModuleEditorAction::MoveNodes { node_ids, delta }
            if node_ids == &[source_id] && *delta == end - start
    )));
    assert!(finished.iter().any(|action| matches!(
        action,
        ModuleEditorAction::FinishMove {
            outcome: MoveEndOutcome::Released
        }
    )));
}

#[test]
fn real_module_adapter_rejects_pointer_press_while_native_window_is_unfocused() {
    let context = egui::Context::default();
    let plugins = PluginManager::default();
    let (definition, source_id, _) = fixture(&plugins);
    let mut state = ModuleNodeEditorState::default();
    let (_, viewport) = run_frame(
        &context,
        &definition,
        &mut state,
        &plugins,
        Vec::new(),
        true,
    );
    let source = port_position(
        viewport,
        &definition,
        source_id,
        IMAGE_OUTPUT_PORT,
        PortDirection::Output,
    );

    let (actions, _) = run_frame(
        &context,
        &definition,
        &mut state,
        &plugins,
        vec![
            egui::Event::PointerMoved(source),
            pointer_button(source, true),
        ],
        false,
    );

    assert!(actions.is_empty());
    assert!(!state.surface_interaction.is_active());
}
