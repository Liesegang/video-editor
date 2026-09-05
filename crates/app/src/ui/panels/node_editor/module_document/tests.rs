use std::collections::{HashMap, HashSet};

use egui_snarl::ui::SnarlViewer;
use egui_snarl::{InPinId, OutPinId};
use library::editor::ModuleNodeRequest;
use library::model::authoring::{
    ModuleConnection, ModuleDefinitionSharing, ProjectPalette, TransitionMediaType,
};
use library::model::frame::color::Color;
use library::model::project::{
    PortDataType, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, SOUND_INPUT_PORT,
};
use std::sync::{Arc, Mutex};

use super::viewer::{ModuleNodeViewer, ModuleSurfaceCapture};
use super::*;

const fn property_context() -> ModulePropertyContext {
    ModulePropertyContext {
        time: 0.0,
        fps: 30.0,
        resolution: (1920, 1080),
    }
}

fn fixture(plugins: &PluginManager) -> (ModuleDefinition, Uuid, Uuid) {
    let (mut definition, output_id) =
        ModuleDefinition::new_image("Production surface", ModuleDefinitionSharing::Private);
    let output = definition.output(output_id).expect("image Output terminal");
    let output_node_id = output.node_id;
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
    source.ui_position = [40.0, 80.0];
    let source_id = source.id;
    let connection = ModuleConnection {
        id: ModuleConnectionId::new(),
        from: ModulePortAddress {
            node_id: source_id,
            port: IMAGE_OUTPUT_PORT.to_string(),
        },
        to: ModulePortAddress {
            node_id: output_node_id,
            port: IMAGE_INPUT_PORT.to_string(),
        },
        order: 0,
        blend_mode: library::model::BlendMode::Normal,
    };
    definition.graph.nodes.insert(source_id, source);
    definition.graph.connections.push(connection);
    (definition, source_id, output_node_id)
}

#[test]
fn module_definition_builds_the_production_snarl_without_container_nodes() {
    let plugins = PluginManager::default();
    let (definition, source_id, target_id) = fixture(&plugins);
    let snarl = surface::build_module_snarl(&definition, &HashMap::new());

    let mut values = snarl.nodes().copied().collect::<Vec<_>>();
    values.sort_unstable();
    let mut expected = vec![source_id, target_id];
    expected.sort_unstable();
    assert_eq!(values, expected);
    assert_eq!(snarl.wires().count(), 1);
    assert_eq!(snarl.nodes().count(), definition.graph.nodes.len());
}

#[test]
fn module_surface_keeps_timeline_graph_expansion_out_of_the_document() {
    let plugins = PluginManager::default();
    let (definition, _, _) = fixture(&plugins);
    let context = egui::Context::default();
    let actions = std::cell::RefCell::new(Vec::new());
    let mut state = NodeEditorState::default();
    let palette = ProjectPalette::default();
    drop(context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 700.0),
            )),
            focused: true,
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                *actions.borrow_mut() = show_module_document(
                    ui,
                    &definition,
                    &palette,
                    &mut state,
                    &plugins,
                    property_context(),
                );
            });
        },
    ));

    assert!(actions.into_inner().is_empty());
    assert_eq!(
        state.canvas,
        pan_zoom_ui::CanvasState::uniform(egui::Vec2::ZERO, 1.0)
    );
}

#[test]
fn opening_a_module_fits_every_node_inside_the_visible_canvas() {
    let plugins = PluginManager::default();
    let (mut definition, source_id, output_node_id) = fixture(&plugins);
    definition
        .graph
        .nodes
        .get_mut(&source_id)
        .expect("source node")
        .ui_position = [-180.0, 120.0];
    definition
        .graph
        .nodes
        .get_mut(&output_node_id)
        .expect("output node")
        .ui_position = [980.0, 420.0];
    let viewport = egui::Rect::from_min_size(egui::pos2(140.0, 90.0), egui::vec2(760.0, 460.0));

    let canvas = surface::fit_module_document_canvas(&definition, viewport)
        .expect("finite module layout fits the viewport");
    let transform = pan_zoom_ui::CanvasTransform::new(viewport.min, canvas);

    for node in definition.graph.nodes.values() {
        let size = layout::sanitized_size(node.ui_size);
        let world_rect = egui::Rect::from_min_size(
            egui::pos2(node.ui_position[0], node.ui_position[1]),
            egui::vec2(size[0], size[1]),
        );
        let screen_rect = transform
            .world_rect_to_screen(world_rect)
            .expect("valid fitted node rectangle");
        assert!(
            viewport.contains_rect(screen_rect),
            "node {node_id} escaped the fitted viewport: {screen_rect:?}",
            node_id = node.id,
        );
    }
}

#[test]
fn snarl_is_layout_and_paint_only_for_connection_gestures() {
    let plugins = PluginManager::default();
    let (mut definition, source_id, target_id) = fixture(&plugins);
    definition.graph.connections.clear();
    let mut snarl = surface::build_module_snarl(&definition, &HashMap::new());
    let source = snarl
        .nodes_ids_data()
        .find_map(|(id, value)| (value.value == source_id).then_some(id))
        .expect("source Snarl node");
    let target = snarl
        .nodes_ids_data()
        .find_map(|(id, value)| (value.value == target_id).then_some(id))
        .expect("target Snarl node");
    let from = snarl.out_pin(OutPinId {
        node: source,
        output: surface::port_index(
            &definition,
            source_id,
            PortDirection::Output,
            IMAGE_OUTPUT_PORT,
        )
        .expect("source image output"),
    });
    let to = snarl.in_pin(InPinId {
        node: target,
        input: surface::port_index(
            &definition,
            target_id,
            PortDirection::Input,
            IMAGE_INPUT_PORT,
        )
        .expect("target image input"),
    });
    let selected = HashSet::new();
    let mut actions = Vec::new();
    let mut transform = egui::emath::TSTransform::IDENTITY;
    let mut clip = egui::Rect::EVERYTHING;
    let palette = ProjectPalette::default();
    {
        let mut viewer = ModuleNodeViewer {
            definition: &definition,
            palette: &palette,
            plugins: &plugins,
            property_context: property_context(),
            selected_nodes: &selected,
            actions: &mut actions,
            canvas_transform: egui::emath::TSTransform::IDENTITY,
            to_global: &mut transform,
            canvas_clip: &mut clip,
            capture: Arc::new(Mutex::new(ModuleSurfaceCapture::default())),
        };
        SnarlViewer::connect(&mut viewer, &from, &to, &mut snarl);
    }

    assert_eq!(snarl.wires().count(), 0);
    assert!(actions.is_empty());
}

#[test]
fn production_snarl_consumes_the_authoritative_application_transform() {
    let plugins = PluginManager::default();
    let (definition, _, _) = fixture(&plugins);
    let mut snarl = surface::build_module_snarl(&definition, &HashMap::new());
    let selected = HashSet::new();
    let mut actions = Vec::new();
    let authoritative = egui::emath::TSTransform::new(egui::vec2(240.0, -80.0), 0.375);
    let mut captured = egui::emath::TSTransform::IDENTITY;
    let mut clip = egui::Rect::EVERYTHING;
    let palette = ProjectPalette::default();
    let mut viewer = ModuleNodeViewer {
        definition: &definition,
        palette: &palette,
        plugins: &plugins,
        property_context: property_context(),
        selected_nodes: &selected,
        actions: &mut actions,
        canvas_transform: authoritative,
        to_global: &mut captured,
        canvas_clip: &mut clip,
        capture: Arc::new(Mutex::new(ModuleSurfaceCapture::default())),
    };
    let mut snarl_proposal = egui::emath::TSTransform::new(egui::vec2(-900.0, 700.0), 1.25);

    SnarlViewer::current_transform(&mut viewer, &mut snarl_proposal, &mut snarl);

    assert_eq!(snarl_proposal, authoritative);
    assert_eq!(captured, authoritative);
}

#[test]
fn delete_intent_removes_only_explicit_module_entities() {
    let node_id = Uuid::new_v4();
    let connection_id = ModuleConnectionId::new();
    let mut state = NodeEditorState::default();
    let (definition, _, output_node_id) = fixture(&PluginManager::default());
    let actions = translate_surface_outputs(
        &definition,
        vec![EditorOutput::Delete {
            items: vec![
                ItemId::Node(node_id),
                ItemId::Node(output_node_id),
                ItemId::Wire(connection_id),
            ],
        }],
        &mut state,
    );
    assert!(matches!(
        actions.as_slice(),
        [
            ModuleEditorAction::DeleteConnections(connections),
            ModuleEditorAction::DeleteNodes(nodes)
        ] if connections == &[connection_id] && nodes == &[node_id]
    ));
}

#[test]
fn shared_wire_delete_routes_to_the_authoritative_module_connection() {
    let (definition, _, _) = fixture(&PluginManager::default());
    let connection_id = definition.graph.connections[0].id;
    let mut state = NodeEditorState::default();
    let actions = translate_surface_outputs(
        &definition,
        vec![EditorOutput::Disconnect {
            wire: connection_id,
        }],
        &mut state,
    );

    assert_eq!(actions, vec![ModuleEditorAction::Disconnect(connection_id)]);
}

#[test]
fn shared_wire_reconnect_routes_both_complete_module_addresses_atomically() {
    let (definition, source_id, output_node_id) = fixture(&PluginManager::default());
    let connection_id = definition.graph.connections[0].id;
    let from = ModuleEditorPortId {
        address: ModulePortAddress {
            node_id: source_id,
            port: IMAGE_OUTPUT_PORT.to_string(),
        },
        direction: PortDirection::Output,
    };
    let to = ModuleEditorPortId {
        address: ModulePortAddress {
            node_id: output_node_id,
            port: IMAGE_INPUT_PORT.to_string(),
        },
        direction: PortDirection::Input,
    };
    let mut state = NodeEditorState::default();
    let actions = translate_surface_outputs(
        &definition,
        vec![EditorOutput::Reconnect {
            wire: connection_id,
            from: from.clone(),
            to: to.clone(),
        }],
        &mut state,
    );

    assert_eq!(
        actions,
        vec![ModuleEditorAction::Reconnect {
            connection_id,
            from: from.address,
            to: to.address,
        }]
    );
}

#[test]
fn module_output_is_an_input_only_terminal_outside_the_creation_catalog() {
    let plugins = PluginManager::default();
    let (definition, _, output_node_id) = fixture(&plugins);
    let output = definition
        .graph
        .nodes
        .get(&output_node_id)
        .expect("Output node");
    let contract = library::model::authoring::ModuleNodePortContract::resolve(output)
        .expect("Output port contract");

    assert!(matches!(output.content(), NodeContent::ModuleOutput(_)));
    let inputs = contract
        .ports
        .iter()
        .filter(|port| port.direction == PortDirection::Input)
        .map(|port| (port.key.as_str(), port.label.as_str(), port.data_type))
        .collect::<Vec<_>>();
    assert_eq!(
        inputs,
        vec![
            (IMAGE_INPUT_PORT, "Image", PortDataType::Image),
            (SOUND_INPUT_PORT, "Audio", PortDataType::Audio),
        ]
    );
    assert!(contract
        .ports
        .iter()
        .all(|port| port.direction != PortDirection::Output));
    assert!(
        menu::module_node_menu_items(&plugins, &definition.host_contract)
            .iter()
            .all(|item| {
                matches!(
                    item.value,
                    menu::ModuleNodeCreateRequest::Native(_)
                        | menu::ModuleNodeCreateRequest::PluginOperation { .. }
                )
            })
    );
}

#[test]
fn transition_document_shows_only_its_typed_output_boundary() {
    for (media_type, visible_port, hidden_port) in [
        (
            TransitionMediaType::Image,
            IMAGE_INPUT_PORT,
            SOUND_INPUT_PORT,
        ),
        (
            TransitionMediaType::Audio,
            SOUND_INPUT_PORT,
            IMAGE_INPUT_PORT,
        ),
    ] {
        let (definition, contract) = ModuleDefinition::new_transition(
            "Typed Transition",
            ModuleDefinitionSharing::Private,
            media_type,
        )
        .expect("Transition Module");
        let output = definition
            .output(contract.output_id)
            .expect("protected Output");
        let node = definition
            .graph
            .nodes
            .get(&output.node_id)
            .expect("Output node");
        let presented = document_port_contract(&definition, node).expect("document contract");

        assert_eq!(presented.ports.len(), 1);
        assert_eq!(presented.ports[0].key, visible_port);
        assert_eq!(
            surface::port_index(
                &definition,
                output.node_id,
                PortDirection::Input,
                visible_port,
            ),
            Some(0)
        );
        assert_eq!(
            surface::port_index(
                &definition,
                output.node_id,
                PortDirection::Input,
                hidden_port,
            ),
            None
        );
    }
}

#[test]
fn transition_document_never_offers_a_competing_primary_media_input() {
    let (definition, contract) = ModuleDefinition::new_transition(
        "Transition",
        ModuleDefinitionSharing::Private,
        TransitionMediaType::Image,
    )
    .expect("Transition Module");
    let input = definition
        .interface
        .media_inputs
        .iter()
        .find(|input| input.id == contract.from_input_id)
        .expect("protected A input");
    let port = PortVisual {
        id: ModuleEditorPortId {
            address: input.target.clone(),
            direction: PortDirection::Input,
        },
        label: input.name.clone(),
        center: egui::Pos2::ZERO,
        data_type: input.data_type,
    };

    let error = interface::primary_media_input_action(&definition, &port)
        .expect_err("Transition A/B must remain the only host media inputs");

    assert!(error.contains("Timeline-owned A/B"));
}

#[test]
fn transition_host_inputs_are_locked_while_the_output_terminal_stays_connectable() {
    let (definition, contract) = ModuleDefinition::new_transition(
        "Transition",
        ModuleDefinitionSharing::Private,
        TransitionMediaType::Image,
    )
    .expect("Transition Module");
    let from = definition
        .interface
        .media_inputs
        .iter()
        .find(|input| input.id == contract.from_input_id)
        .expect("protected A");
    let from_port = PortVisual {
        id: ModuleEditorPortId {
            address: from.target.clone(),
            direction: PortDirection::Input,
        },
        label: from.name.clone(),
        center: egui::Pos2::ZERO,
        data_type: from.data_type,
    };
    let ownership = definition.input_port_ownership(&from.target);
    assert_eq!(ownership, ModuleInputPortOwnership::HostProtected);
    assert!(!surface::module_port_is_connectable(
        &definition,
        &from_port
    ));
    assert!(!viewer::input_allows_inline_authoring(ownership));

    let output = definition
        .output(contract.output_id)
        .expect("protected Output");
    let output_target = output
        .target(PortDataType::Image)
        .expect("Image Output target");
    let output_port = PortVisual {
        id: ModuleEditorPortId {
            address: output_target.clone(),
            direction: PortDirection::Input,
        },
        label: "Image".to_string(),
        center: egui::Pos2::ZERO,
        data_type: PortDataType::Image,
    };
    assert_eq!(
        definition.input_port_ownership(&output_target),
        ModuleInputPortOwnership::Internal
    );
    assert!(surface::module_port_is_connectable(
        &definition,
        &output_port
    ));
    assert!(viewer::input_allows_inline_authoring(
        ModuleInputPortOwnership::Internal
    ));
}
