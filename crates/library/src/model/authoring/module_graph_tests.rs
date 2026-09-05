use super::*;
use crate::model::BlendMode;
use crate::model::node::Node;
use crate::model::project::{
    IMAGE_INPUT_PORT, NUMBER_RESULT_OUTPUT_PORT, PortDataType, SOUND_INPUT_PORT,
};

#[test]
fn media_module_constructor_has_one_stable_terminal_with_image_and_sound_inputs() {
    let (definition, output_id) =
        ModuleDefinition::new_image("Image Module", ModuleDefinitionSharing::Private);

    definition.validate().expect("valid image Module");
    let outputs = definition.outputs().collect::<Vec<_>>();
    assert_eq!(definition.graph.nodes.len(), 1);
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].id, output_id);
    assert_eq!(
        outputs[0].target(PortDataType::Image).unwrap().port,
        IMAGE_INPUT_PORT
    );
    assert_eq!(
        outputs[0].target(PortDataType::Audio).unwrap().port,
        SOUND_INPUT_PORT
    );
    assert!(definition.interface.signals.is_empty());
}

#[test]
fn native_constant_only_inputs_reject_dynamic_graph_authoring() {
    let (mut definition, _) =
        ModuleDefinition::new_image("Particle input contract", ModuleDefinitionSharing::Private);
    let emitter = Node::new_catalog_node("native.particle.emitter").expect("Particle Emitter");
    let sprite =
        Node::new_catalog_node("native.particle.sprite-renderer").expect("Sprite Renderer");
    let value = Node::new_add("Number source");
    let (emitter_id, sprite_id, value_id) = (emitter.id, sprite.id, value.id);
    definition.graph.nodes.extend([
        (emitter_id, emitter),
        (sprite_id, sprite),
        (value_id, value),
    ]);

    let rate = ModulePortAddress {
        node_id: emitter_id,
        port: "rate".to_string(),
    };
    let color = ModulePortAddress {
        node_id: sprite_id,
        port: "color".to_string(),
    };
    assert!(!definition.input_port_accepts_connection(&rate));
    assert!(definition.input_port_accepts_connection(&color));
    assert!(
        !definition.input_port_accepts_connection(&ModulePortAddress {
            node_id: uuid::Uuid::new_v4(),
            port: "missing".to_string(),
        })
    );

    definition.graph.connections.push(ModuleConnection {
        id: ModuleConnectionId::new(),
        from: ModulePortAddress {
            node_id: value_id,
            port: NUMBER_RESULT_OUTPUT_PORT.to_string(),
        },
        to: rate,
        order: 0,
        blend_mode: BlendMode::Normal,
    });
    let error = definition
        .graph
        .validate()
        .expect_err("constant-only Particle input must reject graph wiring");
    assert!(error.contains("constant-only input"));
    assert!(error.contains("fixed-step parameter schedule"));
}
