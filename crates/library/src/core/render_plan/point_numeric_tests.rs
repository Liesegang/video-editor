//! Point arithmetic uses ordinary Numeric Nodes and late-bound uniform shapes.

use super::particle_tests::{ParticleFixture, connection, point_scenes};
use super::point_tests::{PointNodes, point_fixture, replace_fixture_source_with_grid};
use super::{
    CompiledPointInstruction, CompiledPointValueType, RenderPlanCache, RenderPlanCompiler,
    evaluate_render_plan_frame,
};
use crate::model::animation::EasingFunction;
use crate::model::authoring::{AutomationKeyframe, AutomationTrack, MediaTime, SourceRef};
use crate::model::node::{
    COLOR_RAMP_FACTOR_PORT, NUMERIC_LENGTH_CATALOG_ID, NUMERIC_LENGTH_INPUT_PORT, Node,
    POINT_ATTRIBUTE_OUTPUT_PORT, PointNodeRole,
};
use crate::model::point::{PointAttributeElementType, PointInstruction, PointRenderProgram};
use crate::model::project::{NUMBER_RESULT_OUTPUT_PORT, NUMERIC_B_INPUT_PORT, PortDataType};
use crate::model::property::{PropertyValue, Vec2, Vec3};
use crate::plugin::PluginManager;

fn number(value: f64) -> PropertyValue {
    PropertyValue::Number(value.into())
}

fn vector(x: f64, y: f64, z: f64) -> PropertyValue {
    PropertyValue::Vec3(Vec3 {
        x: x.into(),
        y: y.into(),
        z: z.into(),
    })
}

fn vector_fixture(count: usize, grid: bool) -> (ParticleFixture, PointNodes, uuid::Uuid) {
    let (mut fixture, nodes) = point_fixture(count);
    if grid {
        replace_fixture_source_with_grid(&mut fixture, &nodes, "position");
    }
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let mut store = Node::new_catalog_node(
        PointNodeRole::StoreAttribute(PointAttributeElementType::Vec3).catalog_id(),
    )
    .unwrap();
    store.id = nodes.store;
    store.name = "scaled position".into();
    definition.graph.nodes.insert(store.id, store);
    for edge in &mut definition.graph.connections {
        if edge.from.node_id == nodes.info && edge.to.node_id == nodes.math {
            edge.from.port = "position".into();
        }
    }
    definition
        .graph
        .connections
        .retain(|edge| !(edge.from.node_id == nodes.store && edge.to.node_id == nodes.ramp));
    let length = Node::new_catalog_node(NUMERIC_LENGTH_CATALOG_ID).unwrap();
    let length_id = length.id;
    definition.graph.nodes.insert(length_id, length);
    definition.graph.connections.extend([
        connection(
            nodes.store,
            POINT_ATTRIBUTE_OUTPUT_PORT,
            length_id,
            NUMERIC_LENGTH_INPUT_PORT,
            0,
        ),
        connection(
            length_id,
            NUMBER_RESULT_OUTPUT_PORT,
            nodes.ramp,
            COLOR_RAMP_FACTOR_PORT,
            0,
        ),
    ]);
    definition
        .interface
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == nodes.factor_parameter)
        .unwrap()
        .data_type = PortDataType::Numeric;
    definition.topology_revision += 1;
    definition.interface_version += 1;
    fixture.project.validate().unwrap();
    (fixture, nodes, length_id)
}

fn programs(fixture: &ParticleFixture, frame: u64) -> Vec<PointRenderProgram> {
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
    let frame = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        frame,
        1.0,
        None,
    )
    .unwrap();
    point_scenes(&frame.items)
        .iter()
        .map(|scene| scene.point_program.clone().unwrap())
        .collect()
}

fn numeric_uniform_register(fixture: &ParticleFixture, nodes: &PointNodes) -> usize {
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
    plan.module_definitions[&fixture.definition_id].point_renderers[&nodes.renderer]
        .point_program.as_ref().unwrap().instructions.iter().position(|instruction| {
            matches!(instruction,
                CompiledPointInstruction::Uniform { node_id, port, value_type: CompiledPointValueType::Numeric }
                if *node_id == nodes.math && port == NUMERIC_B_INPUT_PORT)
        }).unwrap()
}

#[test]
fn vector_fields_and_length_execute_in_particle_and_grid_sampled_programs() {
    for grid in [false, true] {
        let (fixture, _, _) = vector_fixture(1, grid);
        let programs = programs(&fixture, 15);
        let program = &programs[0];
        let types = program.register_types().unwrap();
        assert_eq!(
            program.schema.attributes()[0].element_type(),
            PointAttributeElementType::Vec3
        );
        let binary = program
            .instructions
            .iter()
            .position(|op| matches!(op, PointInstruction::Binary { .. }))
            .unwrap();
        let length = program
            .instructions
            .iter()
            .position(|op| matches!(op, PointInstruction::Length { .. }))
            .unwrap();
        assert_eq!(types[binary], PointAttributeElementType::Vec3);
        assert_eq!(types[length], PointAttributeElementType::Number);
        let serialized = serde_json::to_string(&fixture.project).unwrap();
        let restored = serde_json::from_str(&serialized).unwrap();
        assert_eq!(fixture.project, restored);
    }
}

#[test]
fn numeric_uniform_shape_changes_reuse_definition_and_validate_each_sample() {
    let (mut fixture, nodes, _) = vector_fixture(1, true);
    let mut cache = RenderPlanCache::default();
    cache.compile(&fixture.project).unwrap();
    let register = numeric_uniform_register(&fixture, &nodes);
    for value in [
        number(2.0),
        vector(2.0, 3.0, 4.0),
        PropertyValue::Integer(7),
    ] {
        fixture
            .project
            .module_instances
            .get_mut(&fixture.instance_ids[0])
            .unwrap()
            .parameter_overrides
            .insert(nodes.factor_parameter, value.clone());
        let (plan, stats) = cache.compile(&fixture.project).unwrap();
        assert_eq!(stats.compiled_definitions, 0);
        assert_eq!(stats.reused_definitions, 1);
        let frame = evaluate_render_plan_frame(
            &fixture.project,
            &plan,
            &PluginManager::default(),
            15,
            1.0,
            None,
        )
        .unwrap();
        let scenes = point_scenes(&frame.items);
        let program = scenes[0].point_program.as_ref().unwrap();
        let expected = if let PropertyValue::Integer(value) = value {
            number(value as f64)
        } else {
            value
        };
        assert_eq!(
            program.instructions[register],
            PointInstruction::Constant { value: expected }
        );
        program.validate().unwrap();
    }
    fixture
        .project
        .module_instances
        .get_mut(&fixture.instance_ids[0])
        .unwrap()
        .parameter_overrides
        .insert(
            nodes.factor_parameter,
            PropertyValue::Vec2(Vec2 {
                x: 1.0.into(),
                y: 2.0.into(),
            }),
        );
    let (plan, stats) = cache.compile(&fixture.project).unwrap();
    assert_eq!(stats.compiled_definitions, 0);
    let error = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        15,
        1.0,
        None,
    )
    .unwrap_err()
    .to_string();
    assert!(error.to_ascii_lowercase().contains("dimension"), "{error}");
}

#[test]
fn vector_uniform_keyframes_and_sibling_overrides_stay_instance_local() {
    let (mut fixture, nodes, _) = vector_fixture(2, true);
    let register = numeric_uniform_register(&fixture, &nodes);
    let SourceRef::Module(invocation) = &mut fixture
        .project
        .items
        .get_mut(&fixture.item_ids[0])
        .unwrap()
        .source
    else {
        panic!("Node Clip")
    };
    invocation.automation_tracks.insert(
        nodes.factor_parameter,
        AutomationTrack {
            keyframes: vec![
                AutomationKeyframe::new(
                    MediaTime::zero(),
                    vector(1.0, 2.0, 3.0),
                    EasingFunction::Linear,
                ),
                AutomationKeyframe::new(
                    MediaTime::new(1, 1).unwrap(),
                    vector(3.0, 4.0, 5.0),
                    EasingFunction::Linear,
                ),
            ],
        },
    );
    fixture
        .project
        .module_instances
        .get_mut(&fixture.instance_ids[1])
        .unwrap()
        .parameter_overrides
        .insert(nodes.factor_parameter, number(9.0));
    let sampled = programs(&fixture, 15);
    assert_eq!(sampled.len(), 2);
    assert_eq!(
        sampled[0].instructions[register],
        PointInstruction::Constant {
            value: vector(2.0, 3.0, 4.0)
        }
    );
    assert_eq!(
        sampled[1].instructions[register],
        PointInstruction::Constant { value: number(9.0) }
    );
}

#[test]
fn length_rejects_varying_color_instead_of_reinterpreting_its_four_lanes() {
    let (mut fixture, nodes, length) = vector_fixture(1, true);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let mut color_store = Node::new_catalog_node(
        PointNodeRole::StoreAttribute(PointAttributeElementType::Color).catalog_id(),
    )
    .unwrap();
    color_store.id = nodes.store;
    definition.graph.nodes.insert(nodes.store, color_store);
    definition
        .graph
        .connections
        .retain(|edge| !(edge.to.node_id == nodes.store && edge.to.port == "value"));
    assert!(
        definition
            .graph
            .connections
            .iter()
            .any(|edge| edge.to.node_id == length)
    );
    let error = RenderPlanCompiler::compile(&fixture.project).unwrap_err();
    assert!(error.contains("cannot connect Color to Numeric"), "{error}");
}

#[test]
fn length_rejects_varying_integer_even_though_numeric_ports_accept_uniform_integers() {
    let (mut fixture, nodes, _) = vector_fixture(1, true);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let mut integer_store = Node::new_catalog_node(
        PointNodeRole::StoreAttribute(PointAttributeElementType::Integer).catalog_id(),
    )
    .unwrap();
    integer_store.id = nodes.store;
    definition.graph.nodes.insert(nodes.store, integer_store);
    definition
        .graph
        .connections
        .retain(|edge| !(edge.to.node_id == nodes.store && edge.to.port == "value"));
    let error = RenderPlanCompiler::compile(&fixture.project).unwrap_err();
    assert!(error.contains("implicit varying conversions"), "{error}");
}
