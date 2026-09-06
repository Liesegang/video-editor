use ordered_float::OrderedFloat;

use super::compiler::compile_module;
use super::particle_tests::{
    ParticleFixture, connection, particle_fixture, particle_node_id, particle_renderer_and_output,
};
use crate::editor::TimelineEditorService;
use crate::editor::project_service::{GeneratorNodeRequest, test_generator_node};
use crate::model::authoring::{ModulePortAddress, PublishedParameter, PublishedParameterId};
use crate::model::node::{
    COLOR_RAMP_FACTOR_PORT, COLOR_VALUE_PORT, ColorContent, Node, PARTICLE_SYSTEM_PORT,
    POINT_ATTRIBUTE_OUTPUT_PORT, POINT_ATTRIBUTE_VALUE_PORT, POINT_SOURCE_PORT, PointNodeRole,
};
use crate::model::point::PointAttributeId;
use crate::model::project::{
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NUMBER_RESULT_OUTPUT_PORT,
    NUMERIC_A_INPUT_PORT, NUMERIC_B_INPUT_PORT, PortDataType,
};
use crate::model::property::PropertyValue;

pub(super) struct PointNodes {
    pub(super) info: uuid::Uuid,
    pub(super) store: uuid::Uuid,
    pub(super) ramp: uuid::Uuid,
    pub(super) math: uuid::Uuid,
    pub(super) renderer: uuid::Uuid,
    pub(super) factor_parameter: PublishedParameterId,
}

/// Shared positive fixture for compiler and sampled-runtime tests.
pub(super) fn point_fixture(count: usize) -> (ParticleFixture, PointNodes) {
    let mut fixture = particle_fixture(count);
    let renderer = particle_node_id(
        &fixture,
        crate::model::node::ParticleNodeRole::SpriteRenderer.catalog_id(),
    );
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .expect("Particle definition");
    let old_input = definition
        .graph
        .connections
        .iter()
        .find(|connection| {
            connection.to.node_id == renderer && connection.to.port == PARTICLE_SYSTEM_PORT
        })
        .expect("Sprite Point input")
        .from
        .clone();
    definition.graph.connections.retain(|connection| {
        !(connection.to.node_id == renderer && connection.to.port == PARTICLE_SYSTEM_PORT)
    });

    let info_node = Node::new_catalog_node(PointNodeRole::Info.catalog_id()).unwrap();
    let mut store_node = Node::new_catalog_node(
        PointNodeRole::StoreAttribute(crate::model::point::PointAttributeElementType::Number)
            .catalog_id(),
    )
    .unwrap();
    store_node.name = "heat".to_string();
    let math_node = Node::new_multiply("Scale Heat");
    let ramp_node = Node::new_color("Heat Ramp", ColorContent::ColorRamp);
    let info = info_node.id;
    let store = store_node.id;
    let math = math_node.id;
    let ramp = ramp_node.id;
    definition.graph.nodes.extend([
        (info, info_node),
        (store, store_node),
        (math, math_node),
        (ramp, ramp_node),
    ]);
    definition.graph.connections.extend([
        connection(
            old_input.node_id,
            &old_input.port,
            info,
            POINT_SOURCE_PORT,
            0,
        ),
        connection(
            old_input.node_id,
            &old_input.port,
            store,
            POINT_SOURCE_PORT,
            0,
        ),
        connection(info, "normalized_age", math, NUMERIC_A_INPUT_PORT, 0),
        connection(
            math,
            NUMBER_RESULT_OUTPUT_PORT,
            store,
            POINT_ATTRIBUTE_VALUE_PORT,
            0,
        ),
        connection(store, POINT_SOURCE_PORT, renderer, PARTICLE_SYSTEM_PORT, 0),
        connection(
            store,
            POINT_ATTRIBUTE_OUTPUT_PORT,
            ramp,
            COLOR_RAMP_FACTOR_PORT,
            0,
        ),
        connection(ramp, COLOR_VALUE_PORT, renderer, "color", 0),
    ]);

    // A connected input cannot simultaneously be a Published target. The
    // varying Color branch replaces only that one interface projection.
    definition.interface.parameters.retain(|parameter| {
        !(parameter.target.node_id == renderer && parameter.target.port == "color")
    });
    let factor_default = definition.graph.nodes[&math]
        .properties()
        .get(NUMERIC_B_INPUT_PORT)
        .and_then(|property| property.value())
        .cloned()
        .expect("Multiply factor default");
    let factor_parameter = PublishedParameterId::new();
    definition.interface.parameters.push(PublishedParameter {
        id: factor_parameter,
        name: "Heat Scale".to_string(),
        data_type: PortDataType::Number,
        default_value: factor_default,
        target: ModulePortAddress {
            node_id: math,
            port: NUMERIC_B_INPUT_PORT.to_string(),
        },
    });
    definition.topology_revision += 1;
    definition.interface_version += 1;
    (
        fixture,
        PointNodes {
            info,
            store,
            ramp,
            math,
            renderer,
            factor_parameter,
        },
    )
}

fn append_store(fixture: &mut ParticleFixture, nodes: &PointNodes, name: &str) -> uuid::Uuid {
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    definition.graph.connections.retain(|connection| {
        !(connection.from.node_id == nodes.store
            && connection.from.port == POINT_SOURCE_PORT
            && connection.to.node_id == nodes.renderer
            && connection.to.port == PARTICLE_SYSTEM_PORT)
    });
    let mut store = Node::new_catalog_node(
        PointNodeRole::StoreAttribute(crate::model::point::PointAttributeElementType::Number)
            .catalog_id(),
    )
    .unwrap();
    store.name = name.to_string();
    let store_id = store.id;
    definition.graph.nodes.insert(store_id, store);
    definition.graph.connections.extend([
        connection(
            nodes.store,
            POINT_SOURCE_PORT,
            store_id,
            POINT_SOURCE_PORT,
            0,
        ),
        connection(
            store_id,
            POINT_SOURCE_PORT,
            nodes.renderer,
            PARTICLE_SYSTEM_PORT,
            0,
        ),
    ]);
    definition.topology_revision += 1;
    store_id
}

pub(super) fn replace_fixture_source_with_grid(
    fixture: &mut ParticleFixture,
    nodes: &PointNodes,
    info_output: &str,
) -> uuid::Uuid {
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let grid = Node::new_catalog_node(PointNodeRole::Grid.catalog_id()).unwrap();
    let grid_id = grid.id;
    definition.graph.nodes.insert(grid_id, grid);
    for connection in &mut definition.graph.connections {
        if (connection.to.node_id == nodes.info || connection.to.node_id == nodes.store)
            && connection.to.port == POINT_SOURCE_PORT
        {
            connection.from = ModulePortAddress {
                node_id: grid_id,
                port: POINT_SOURCE_PORT.to_string(),
            };
        }
        if connection.from.node_id == nodes.info
            && matches!(
                connection.from.port.as_str(),
                "age" | "normalized_age" | "random"
            )
        {
            connection.from.port = info_output.to_string();
        }
    }
    definition.topology_revision += 1;
    grid_id
}

#[test]
fn heat_field_compiles_store_before_load_and_color_ramp() {
    let (fixture, nodes) = point_fixture(1);
    let definition = &fixture.project.module_definitions[&fixture.definition_id];
    let compiled = compile_module(definition).expect("Point heat program");
    assert!(compiled.outputs.contains_key(&fixture.output_id));
    let program = compiled.point_renderers[&nodes.renderer]
        .point_program
        .as_ref()
        .expect("varying Point program");
    assert_eq!(program.schema.attributes().len(), 1);
    assert_eq!(
        program.schema.attributes()[0].id(),
        PointAttributeId::from_uuid(nodes.store)
    );
    assert_eq!(program.schema.attributes()[0].display_name(), "heat");
    let store = program
        .instructions
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                super::CompiledPointInstruction::StoreAttribute { .. }
            )
        })
        .expect("Store instruction");
    let load = program
        .instructions
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                super::CompiledPointInstruction::LoadAttribute { .. }
            )
        })
        .expect("Load instruction");
    let ramp = program
        .instructions
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                super::CompiledPointInstruction::ColorRamp { .. }
            )
        })
        .expect("Color Ramp instruction");
    assert!(store < load && load < ramp);
}

#[test]
fn grid_random_uses_the_same_store_field_and_sprite_consumer() {
    let (mut fixture, nodes) = point_fixture(1);
    let grid = replace_fixture_source_with_grid(&mut fixture, &nodes, "random");
    let definition = &fixture.project.module_definitions[&fixture.definition_id];
    let compiled = compile_module(definition).expect("Grid Point field");
    let renderer = &compiled.point_renderers[&nodes.renderer];
    assert!(compiled.outputs[&fixture.output_id].requires(super::RenderCapability::Gpu));
    assert_eq!(
        renderer.source,
        super::CompiledPointSource::Grid { node_id: grid }
    );
    let program = renderer.point_program.as_ref().expect("Grid Point program");
    assert!(
        program.instructions.iter().any(|instruction| matches!(
            instruction,
            super::CompiledPointInstruction::Random { .. }
        ))
    );
    assert!(program.instructions.iter().any(|instruction| matches!(
        instruction,
        super::CompiledPointInstruction::StoreAttribute { .. }
    )));
}

#[test]
fn grid_with_uniform_color_compiles_the_existing_sprite_fast_path() {
    let mut fixture = particle_fixture(1);
    let renderer = particle_node_id(
        &fixture,
        crate::model::node::ParticleNodeRole::SpriteRenderer.catalog_id(),
    );
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    definition.graph.connections.retain(|connection| {
        !(connection.to.node_id == renderer && connection.to.port == PARTICLE_SYSTEM_PORT)
    });
    let grid = Node::new_catalog_node(PointNodeRole::Grid.catalog_id()).unwrap();
    let grid_id = grid.id;
    definition.graph.nodes.insert(grid_id, grid);
    definition.graph.connections.push(connection(
        grid_id,
        POINT_SOURCE_PORT,
        renderer,
        PARTICLE_SYSTEM_PORT,
        0,
    ));
    definition.topology_revision += 1;

    let compiled = compile_module(definition).expect("uniform Grid Sprite");
    let point_renderer = &compiled.point_renderers[&renderer];
    assert_eq!(
        point_renderer.source,
        super::CompiledPointSource::Grid { node_id: grid_id }
    );
    assert!(point_renderer.point_program.is_none());
}

#[test]
fn one_definition_compiles_particle_and_grid_sprite_endpoints_together() {
    let mut fixture = particle_fixture(1);
    let (particle_renderer, output) = particle_renderer_and_output(&fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    definition.graph.connections.retain(|connection| {
        !(connection.from.node_id == particle_renderer
            && connection.from.port == IMAGE_OUTPUT_PORT
            && connection.to.node_id == output)
    });
    let grid = Node::new_catalog_node(PointNodeRole::Grid.catalog_id()).unwrap();
    let grid_id = grid.id;
    let grid_renderer =
        Node::new_catalog_node(crate::model::node::ParticleNodeRole::SpriteRenderer.catalog_id())
            .unwrap();
    let grid_renderer_id = grid_renderer.id;
    let merge = Node::new_merge("Particle and Grid");
    let merge_id = merge.id;
    definition.graph.nodes.extend([
        (grid_id, grid),
        (grid_renderer_id, grid_renderer),
        (merge_id, merge),
    ]);
    definition.graph.connections.extend([
        connection(
            grid_id,
            POINT_SOURCE_PORT,
            grid_renderer_id,
            PARTICLE_SYSTEM_PORT,
            0,
        ),
        connection(
            particle_renderer,
            IMAGE_OUTPUT_PORT,
            merge_id,
            MERGE_IMAGES_PORT,
            0,
        ),
        connection(
            grid_renderer_id,
            IMAGE_OUTPUT_PORT,
            merge_id,
            MERGE_IMAGES_PORT,
            1,
        ),
        connection(merge_id, IMAGE_OUTPUT_PORT, output, IMAGE_INPUT_PORT, 0),
    ]);
    definition.topology_revision += 1;

    let compiled = compile_module(definition).expect("mixed Point sources");
    assert_eq!(compiled.point_renderers.len(), 2);
    assert!(matches!(
        &compiled.point_renderers[&particle_renderer].source,
        super::CompiledPointSource::Particle(_)
    ));
    assert_eq!(
        compiled.point_renderers[&grid_renderer_id].source,
        super::CompiledPointSource::Grid { node_id: grid_id }
    );
}

#[test]
fn grid_rejects_particle_age_fields_without_inventing_a_lifetime() {
    for output in ["age", "normalized_age"] {
        let (mut fixture, nodes) = point_fixture(1);
        replace_fixture_source_with_grid(&mut fixture, &nodes, output);
        let definition = &fixture.project.module_definitions[&fixture.definition_id];
        let error = compile_module(definition).expect_err("Grid must not expose Particle age");
        assert!(error.contains("requires a Particle source"), "{error}");
    }
}

#[test]
fn point_info_accepts_an_upstream_stage_only_on_the_same_particle_lineage() {
    let (mut fixture, nodes) = point_fixture(1);
    let emitter = particle_node_id(
        &fixture,
        crate::model::node::ParticleNodeRole::Emitter.catalog_id(),
    );
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let info_input = definition
        .graph
        .connections
        .iter_mut()
        .find(|connection| {
            connection.to.node_id == nodes.info && connection.to.port == POINT_SOURCE_PORT
        })
        .unwrap();
    info_input.from = ModulePortAddress {
        node_id: emitter,
        port: PARTICLE_SYSTEM_PORT.to_string(),
    };
    compile_module(definition).expect("same Particle lineage");

    let other =
        Node::new_catalog_node(crate::model::node::ParticleNodeRole::Emitter.catalog_id()).unwrap();
    let other_id = other.id;
    definition.graph.nodes.insert(other_id, other);
    let info_input = definition
        .graph
        .connections
        .iter_mut()
        .find(|connection| {
            connection.to.node_id == nodes.info && connection.to.port == POINT_SOURCE_PORT
        })
        .unwrap();
    info_input.from.node_id = other_id;
    let error = compile_module(definition).unwrap_err();
    assert!(error.contains("different Point domain"));
}

#[test]
fn renaming_store_changes_display_only_and_project_round_trip_retains_identity() {
    let (mut fixture, nodes) = point_fixture(1);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let before = compile_module(definition).unwrap();
    definition.graph.nodes.get_mut(&nodes.store).unwrap().name = "temperature".to_string();
    let after = compile_module(definition).unwrap();
    let before_attribute = &before.point_renderers[&nodes.renderer]
        .point_program
        .as_ref()
        .unwrap()
        .schema
        .attributes()[0];
    let after_attribute = &after.point_renderers[&nodes.renderer]
        .point_program
        .as_ref()
        .unwrap()
        .schema
        .attributes()[0];
    assert_eq!(before_attribute.id(), after_attribute.id());
    assert_eq!(after_attribute.display_name(), "temperature");

    let json = serde_json::to_string(&fixture.project).unwrap();
    let restored = serde_json::from_str(&json).unwrap();
    let compiled = super::RenderPlanCompiler::compile(&restored).unwrap();
    let definition = &compiled.module_definitions[&fixture.definition_id];
    assert_eq!(
        definition.point_renderers[&nodes.renderer]
            .point_program
            .as_ref()
            .unwrap()
            .schema
            .attributes()[0]
            .id(),
        PointAttributeId::from_uuid(nodes.store)
    );
}

#[test]
fn warm_cache_recompiles_when_the_semantic_store_name_changes() {
    let (mut fixture, nodes) = point_fixture(1);
    let mut cache = super::RenderPlanCache::default();
    let (_, initial) = cache.compile(&fixture.project).unwrap();
    assert_eq!(initial.compiled_definitions, 1);

    fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap()
        .graph
        .nodes
        .get_mut(&nodes.store)
        .unwrap()
        .name = "temperature".to_string();
    let (plan, renamed) = cache.compile(&fixture.project).unwrap();
    assert_eq!(renamed.compiled_definitions, 1);
    assert_eq!(renamed.reused_definitions, 0);
    assert_eq!(
        plan.module_definitions[&fixture.definition_id].point_renderers[&nodes.renderer]
            .point_program
            .as_ref()
            .unwrap()
            .schema
            .attributes()[0]
            .display_name(),
        "temperature"
    );
}

#[test]
fn duplicate_store_name_fails_identically_with_a_warm_or_cold_compiler() {
    let (mut fixture, nodes) = point_fixture(1);
    let second_store = append_store(&mut fixture, &nodes, "density");
    let mut cache = super::RenderPlanCache::default();
    cache.compile(&fixture.project).expect("unique Store names");

    fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap()
        .graph
        .nodes
        .get_mut(&second_store)
        .unwrap()
        .name = "heat".to_string();
    let warm_error = cache.compile(&fixture.project).unwrap_err();
    let cold_error = super::RenderPlanCompiler::compile(&fixture.project).unwrap_err();
    assert!(warm_error.contains("duplicate display name"));
    assert_eq!(warm_error, cold_error);
}

#[test]
fn instance_store_rename_recompiles_only_the_copy_on_write_definition() {
    let (fixture, nodes) = point_fixture(2);
    let before = fixture.project.clone();
    let first_instance = fixture.instance_ids[0];
    let second_instance = fixture.instance_ids[1];
    let original_definition = fixture.definition_id;
    let service = TimelineEditorService::new(fixture.project).unwrap();

    let (edited_definition, _) = service
        .set_instance_module_node_state(
            first_instance,
            nodes.store,
            "temperature".to_string(),
            true,
            false,
        )
        .unwrap();
    let project = service.snapshot().unwrap();
    assert_ne!(edited_definition, original_definition);
    assert_eq!(
        project.module_instances[&first_instance].definition_id,
        edited_definition
    );
    assert_eq!(
        project.module_instances[&second_instance].definition_id,
        original_definition
    );

    let plan = super::RenderPlanCompiler::compile(&project).unwrap();
    let attribute_name = |definition_id| {
        plan.module_definitions[&definition_id].point_renderers[&nodes.renderer]
            .point_program
            .as_ref()
            .unwrap()
            .schema
            .attributes()[0]
            .display_name()
            .to_string()
    };
    assert_eq!(attribute_name(edited_definition), "temperature");
    assert_eq!(attribute_name(original_definition), "heat");

    service
        .undo()
        .unwrap()
        .expect("one copy-on-write rename undo");
    assert_eq!(service.snapshot().unwrap().as_ref(), &before);
}

#[test]
fn store_with_uniform_published_sprite_color_still_uses_point_program() {
    let mut fixture = particle_fixture(1);
    let renderer = particle_node_id(
        &fixture,
        crate::model::node::ParticleNodeRole::SpriteRenderer.catalog_id(),
    );
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let input = definition
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
    let store = Node::new_catalog_node(
        PointNodeRole::StoreAttribute(crate::model::point::PointAttributeElementType::Number)
            .catalog_id(),
    )
    .unwrap();
    let store_id = store.id;
    definition.graph.nodes.insert(store_id, store);
    definition.graph.connections.extend([
        connection(input.node_id, &input.port, store_id, POINT_SOURCE_PORT, 0),
        connection(
            store_id,
            POINT_SOURCE_PORT,
            renderer,
            PARTICLE_SYSTEM_PORT,
            0,
        ),
    ]);
    definition.topology_revision += 1;
    let compiled = compile_module(definition).expect("published uniform Sprite color");
    let program = compiled.point_renderers[&renderer]
        .point_program
        .as_ref()
        .expect("Store requires a Point program");
    assert!(matches!(
        &program.instructions[usize::from(program.color_register)],
        super::CompiledPointInstruction::Uniform { node_id, port, .. }
            if *node_id == renderer && port == "color"
    ));
}

#[test]
fn point_value_cannot_leak_into_a_frame_uniform_consumer() {
    let (mut fixture, nodes) = point_fixture(1);
    let (_, output) = particle_renderer_and_output(&fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let solid = test_generator_node(
        "Uniform Solid",
        GeneratorNodeRequest::Solid {
            color: crate::model::frame::color::Color::white(),
        },
    );
    let solid_id = solid.id;
    let merge = Node::new_merge("Visible Images");
    let merge_id = merge.id;
    definition.graph.connections.retain(|connection| {
        !(connection.from.node_id == nodes.renderer && connection.to.node_id == output)
    });
    definition
        .graph
        .nodes
        .extend([(solid_id, solid), (merge_id, merge)]);
    definition.graph.connections.extend([
        connection(nodes.ramp, COLOR_VALUE_PORT, solid_id, "color", 0),
        connection(
            nodes.renderer,
            IMAGE_OUTPUT_PORT,
            merge_id,
            MERGE_IMAGES_PORT,
            0,
        ),
        connection(solid_id, IMAGE_OUTPUT_PORT, merge_id, MERGE_IMAGES_PORT, 1),
        connection(merge_id, IMAGE_OUTPUT_PORT, output, IMAGE_INPUT_PORT, 0),
    ]);
    let error = compile_module(definition).unwrap_err();
    assert!(error.contains("unsupported input"));
}

#[test]
fn fixture_exposes_uniform_parameter_for_runtime_sampling() {
    let (fixture, nodes) = point_fixture(2);
    let definition = &fixture.project.module_definitions[&fixture.definition_id];
    let parameter = definition
        .interface
        .parameters
        .iter()
        .find(|parameter| parameter.id == nodes.factor_parameter)
        .unwrap();
    assert_eq!(parameter.target.node_id, nodes.math);
    assert_eq!(
        parameter.default_value,
        PropertyValue::Number(OrderedFloat(1.0))
    );
    assert_ne!(nodes.info, nodes.ramp);
}
