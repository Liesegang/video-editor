use super::compiler::compile_module;
use super::particle_tests::{ParticleFixture, connection, particle_fixture, particle_node_id};
use crate::model::authoring::ModulePortAddress;
use crate::model::node::{
    COLOR_RAMP_FACTOR_PORT, COLOR_VALUE_PORT, ColorContent, Node, PARTICLE_SYSTEM_PORT,
    POINT_ATTRIBUTE_OUTPUT_PORT, POINT_ATTRIBUTE_VALUE_PORT, POINT_SOURCE_PORT, PointNodeRole,
};
use crate::model::point::PointAttributeElementType;

fn renderer(fixture: &ParticleFixture) -> uuid::Uuid {
    particle_node_id(
        fixture,
        crate::model::node::ParticleNodeRole::SpriteRenderer.catalog_id(),
    )
}

fn detach_renderer_stream(
    fixture: &mut ParticleFixture,
    renderer: uuid::Uuid,
) -> ModulePortAddress {
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let source = definition
        .graph
        .connections
        .iter()
        .find(|connection| {
            connection.to.node_id == renderer && connection.to.port == PARTICLE_SYSTEM_PORT
        })
        .unwrap()
        .from
        .clone();
    definition.graph.connections.retain(|connection| {
        !(connection.to.node_id == renderer && connection.to.port == PARTICLE_SYSTEM_PORT)
    });
    source
}

fn store(element_type: PointAttributeElementType, name: &str) -> Node {
    let mut node = Node::new_catalog_node(PointNodeRole::StoreAttribute(element_type).catalog_id())
        .expect("typed Point Store");
    node.name = name.to_string();
    node
}

pub(super) fn append_uniform_store(
    fixture: &mut ParticleFixture,
    element_type: PointAttributeElementType,
) -> (uuid::Uuid, uuid::Uuid) {
    let renderer = renderer(fixture);
    let source = detach_renderer_stream(fixture, renderer);
    let node = store(element_type, &format!("{element_type:?} value"));
    let store_id = node.id;
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    definition.graph.nodes.insert(store_id, node);
    definition.graph.connections.extend([
        connection(source.node_id, &source.port, store_id, POINT_SOURCE_PORT, 0),
        connection(
            store_id,
            POINT_SOURCE_PORT,
            renderer,
            PARTICLE_SYSTEM_PORT,
            0,
        ),
    ]);
    definition.topology_revision += 1;
    (renderer, store_id)
}

fn remove_renderer_color_parameter(fixture: &mut ParticleFixture, renderer: uuid::Uuid) {
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    definition.interface.parameters.retain(|parameter| {
        !(parameter.target.node_id == renderer && parameter.target.port == "color")
    });
    definition.interface_version += 1;
}

#[test]
fn every_typed_store_uses_its_catalog_type_and_authoritative_default() {
    for element_type in [
        PointAttributeElementType::Number,
        PointAttributeElementType::Integer,
        PointAttributeElementType::Vec2,
        PointAttributeElementType::Vec3,
        PointAttributeElementType::Vec4,
        PointAttributeElementType::Color,
    ] {
        let mut fixture = particle_fixture(1);
        let (renderer, store) = append_uniform_store(&mut fixture, element_type);
        let compiled = compile_module(&fixture.project.module_definitions[&fixture.definition_id])
            .expect("typed Store program");
        let program = compiled.point_renderers[&renderer]
            .point_program
            .as_ref()
            .expect("Store requires a Point program");
        let attribute = &program.schema.attributes()[0];
        assert_eq!(attribute.element_type(), element_type);
        assert_eq!(attribute.default_value(), &element_type.default_value());
        assert!(program.instructions.iter().any(|instruction| matches!(
            instruction,
            super::CompiledPointInstruction::Uniform {
                node_id,
                port,
                element_type: actual,
            } if *node_id == store && port == POINT_ATTRIBUTE_VALUE_PORT && *actual == element_type
        )));
        assert!(program.instructions.iter().any(|instruction| matches!(
            instruction,
            super::CompiledPointInstruction::StoreAttribute { attribute: 0, .. }
        )));
    }
}

#[test]
fn color_ramp_can_store_load_and_chain_color_into_the_shared_sprite() {
    let mut fixture = particle_fixture(1);
    let renderer = renderer(&fixture);
    let source = detach_renderer_stream(&mut fixture, renderer);
    remove_renderer_color_parameter(&mut fixture, renderer);
    let info = Node::new_catalog_node(PointNodeRole::Info.catalog_id()).unwrap();
    let first = store(PointAttributeElementType::Color, "surface color");
    let second = store(PointAttributeElementType::Color, "final color");
    let ramp = Node::new_color("Point Color Ramp", ColorContent::ColorRamp);
    let (info_id, first_id, second_id, ramp_id) = (info.id, first.id, second.id, ramp.id);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    definition.graph.nodes.extend([
        (info_id, info),
        (first_id, first),
        (second_id, second),
        (ramp_id, ramp),
    ]);
    definition.graph.connections.extend([
        connection(source.node_id, &source.port, info_id, POINT_SOURCE_PORT, 0),
        connection(source.node_id, &source.port, first_id, POINT_SOURCE_PORT, 0),
        connection(
            info_id,
            "normalized_age",
            ramp_id,
            COLOR_RAMP_FACTOR_PORT,
            0,
        ),
        connection(
            ramp_id,
            COLOR_VALUE_PORT,
            first_id,
            POINT_ATTRIBUTE_VALUE_PORT,
            0,
        ),
        connection(first_id, POINT_SOURCE_PORT, second_id, POINT_SOURCE_PORT, 0),
        connection(
            first_id,
            POINT_ATTRIBUTE_OUTPUT_PORT,
            second_id,
            POINT_ATTRIBUTE_VALUE_PORT,
            0,
        ),
        connection(
            second_id,
            POINT_SOURCE_PORT,
            renderer,
            PARTICLE_SYSTEM_PORT,
            0,
        ),
        connection(second_id, POINT_ATTRIBUTE_OUTPUT_PORT, renderer, "color", 0),
    ]);
    definition.topology_revision += 1;

    let compiled = compile_module(definition).expect("chained Point Color attributes");
    let program = compiled.point_renderers[&renderer]
        .point_program
        .as_ref()
        .unwrap();
    assert_eq!(
        program
            .schema
            .attributes()
            .iter()
            .map(|attribute| attribute.element_type())
            .collect::<Vec<_>>(),
        vec![PointAttributeElementType::Color; 2]
    );
    assert!(matches!(
        program.instructions[usize::from(program.color_register)],
        super::CompiledPointInstruction::LoadAttribute { attribute: 1 }
    ));
}

#[test]
fn producer_position_can_chain_through_vec3_attributes_for_particle_and_grid() {
    for use_grid in [false, true] {
        let mut fixture = particle_fixture(1);
        let renderer = renderer(&fixture);
        let mut source = detach_renderer_stream(&mut fixture, renderer);
        let definition = fixture
            .project
            .module_definitions
            .get_mut(&fixture.definition_id)
            .unwrap();
        if use_grid {
            let grid = Node::new_catalog_node(PointNodeRole::Grid.catalog_id()).unwrap();
            source = ModulePortAddress {
                node_id: grid.id,
                port: POINT_SOURCE_PORT.to_string(),
            };
            definition.graph.nodes.insert(grid.id, grid);
        }
        let info = Node::new_catalog_node(PointNodeRole::Info.catalog_id()).unwrap();
        let first = store(PointAttributeElementType::Vec3, "position copy");
        let second = store(PointAttributeElementType::Vec3, "position final");
        let (info_id, first_id, second_id) = (info.id, first.id, second.id);
        definition
            .graph
            .nodes
            .extend([(info_id, info), (first_id, first), (second_id, second)]);
        definition.graph.connections.extend([
            connection(source.node_id, &source.port, info_id, POINT_SOURCE_PORT, 0),
            connection(source.node_id, &source.port, first_id, POINT_SOURCE_PORT, 0),
            connection(info_id, "position", first_id, POINT_ATTRIBUTE_VALUE_PORT, 0),
            connection(first_id, POINT_SOURCE_PORT, second_id, POINT_SOURCE_PORT, 0),
            connection(
                first_id,
                POINT_ATTRIBUTE_OUTPUT_PORT,
                second_id,
                POINT_ATTRIBUTE_VALUE_PORT,
                0,
            ),
            connection(
                second_id,
                POINT_SOURCE_PORT,
                renderer,
                PARTICLE_SYSTEM_PORT,
                0,
            ),
        ]);
        definition.topology_revision += 1;
        let compiled = compile_module(definition).expect("Position Vec3 stores");
        let program = compiled.point_renderers[&renderer]
            .point_program
            .as_ref()
            .unwrap();
        assert!(
            program.instructions.iter().any(|instruction| matches!(
                instruction,
                super::CompiledPointInstruction::Position
            ))
        );
        assert_eq!(
            program
                .schema
                .attributes()
                .iter()
                .map(|attribute| attribute.element_type())
                .collect::<Vec<_>>(),
            vec![PointAttributeElementType::Vec3; 2]
        );
    }
}

#[test]
fn varying_integer_does_not_implicitly_convert_to_number() {
    let mut fixture = particle_fixture(1);
    let renderer = renderer(&fixture);
    let source = detach_renderer_stream(&mut fixture, renderer);
    let integer = store(PointAttributeElementType::Integer, "integer");
    let number = store(PointAttributeElementType::Number, "number");
    let (integer_id, number_id) = (integer.id, number.id);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    definition
        .graph
        .nodes
        .extend([(integer_id, integer), (number_id, number)]);
    definition.graph.connections.extend([
        connection(
            source.node_id,
            &source.port,
            integer_id,
            POINT_SOURCE_PORT,
            0,
        ),
        connection(
            integer_id,
            POINT_SOURCE_PORT,
            number_id,
            POINT_SOURCE_PORT,
            0,
        ),
        connection(
            integer_id,
            POINT_ATTRIBUTE_OUTPUT_PORT,
            number_id,
            POINT_ATTRIBUTE_VALUE_PORT,
            0,
        ),
        connection(
            number_id,
            POINT_SOURCE_PORT,
            renderer,
            PARTICLE_SYSTEM_PORT,
            0,
        ),
    ]);
    definition.topology_revision += 1;
    let error = compile_module(definition).expect_err("varying Integer needs an explicit convert");
    assert!(
        error.contains("implicit varying conversions are not supported"),
        "{error}"
    );
}

#[test]
fn typed_store_rename_invalidates_the_warm_definition_fingerprint() {
    let mut fixture = particle_fixture(1);
    let (_, store) = append_uniform_store(&mut fixture, PointAttributeElementType::Color);
    let mut cache = super::RenderPlanCache::default();
    cache.compile(&fixture.project).unwrap();
    fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap()
        .graph
        .nodes
        .get_mut(&store)
        .unwrap()
        .name = "renamed color".to_string();
    let (plan, stats) = cache.compile(&fixture.project).unwrap();
    assert_eq!(stats.compiled_definitions, 1);
    let definition = &plan.module_definitions[&fixture.definition_id];
    let renderer = renderer(&fixture);
    assert_eq!(
        definition.point_renderers[&renderer]
            .point_program
            .as_ref()
            .unwrap()
            .schema
            .attributes()[0]
            .display_name(),
        "renamed color"
    );
}
