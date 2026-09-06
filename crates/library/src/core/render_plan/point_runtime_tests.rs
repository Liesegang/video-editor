use ordered_float::OrderedFloat;

use super::particle_tests::{ParticleFixture, particle_fixture, particle_scenes, point_scenes};
use super::point_tests::{PointNodes, point_fixture, replace_fixture_source_with_grid};
use super::point_typed_tests::append_uniform_store;
use super::{
    CompiledPointInstruction, RenderPlanCache, RenderPlanCompiler, evaluate_render_plan_frame,
};
use crate::model::animation::EasingFunction;
use crate::model::authoring::{
    AuthoringProject, AutomationKeyframe, AutomationTrack, MediaTime, ModulePortAddress,
    PublishedParameter, PublishedParameterId, SourceRef,
};
use crate::model::frame::particle::ParticleSceneParameters;
use crate::model::frame::point::{PointSceneFrame, PointSceneSource};
use crate::model::point::{PointAttributeElementType, PointInstruction, PointRenderProgram};
use crate::model::project::{NUMERIC_B_INPUT_PORT, PortDataType};
use crate::model::property::{PropertyValue, Vec3};
use crate::plugin::PluginManager;

fn number(value: f64) -> PropertyValue {
    PropertyValue::Number(OrderedFloat(value))
}

fn vec3(value: f64) -> Vec3 {
    Vec3 {
        x: OrderedFloat(value),
        y: OrderedFloat(value),
        z: OrderedFloat(value),
    }
}

fn scenes(fixture: &ParticleFixture, frame: u64) -> Vec<PointSceneFrame> {
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
    particle_scenes(&frame.items).into_iter().cloned().collect()
}

fn particle_parameters(scene: &PointSceneFrame) -> &ParticleSceneParameters {
    let PointSceneSource::Particle { parameters, .. } = &scene.source else {
        panic!("expected Particle source");
    };
    parameters
}

fn factor_register(fixture: &ParticleFixture, nodes: &PointNodes) -> usize {
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
    plan.module_definitions[&fixture.definition_id].point_renderers[&nodes.renderer]
        .point_program.as_ref().unwrap().instructions.iter().position(|instruction| {
            matches!(instruction, CompiledPointInstruction::Uniform {node_id, port, ..} if *node_id == nodes.math && port == NUMERIC_B_INPUT_PORT)
        }).unwrap()
}

fn factor(program: &PointRenderProgram, register: usize) -> &PropertyValue {
    let PointInstruction::Constant { value } = &program.instructions[register] else {
        panic!("uniform input must be sampled once, never per point");
    };
    value
}

#[test]
fn point_field_uniform_keyframes_are_instance_scoped_and_leave_simulation_parameters_unchanged() {
    let (mut fixture, nodes) = point_fixture(2);
    let register = factor_register(&fixture, &nodes);
    let SourceRef::Module(invocation) = &mut fixture
        .project
        .items
        .get_mut(&fixture.item_ids[0])
        .unwrap()
        .source
    else {
        panic!("module item")
    };
    invocation.automation_tracks.insert(
        nodes.factor_parameter,
        AutomationTrack {
            keyframes: vec![
                AutomationKeyframe::new(MediaTime::zero(), number(2.0), EasingFunction::Linear),
                AutomationKeyframe::new(
                    MediaTime::new(1, 1).unwrap(),
                    number(4.0),
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
        .insert(nodes.factor_parameter, number(7.0));
    let first = scenes(&fixture, 0);
    for (frame, expected) in [(0, 2.0), (15, 3.0), (30, 4.0)] {
        let sampled = scenes(&fixture, frame);
        assert_eq!(sampled.len(), 2);
        for scene in &sampled {
            let program = scene.point_program.as_ref().expect("GPU Point program");
            program.validate().unwrap();
            let expected = if scene.invocation.module_instance_id == fixture.instance_ids[0] {
                expected
            } else {
                7.0
            };
            assert_eq!(factor(program, register), &number(expected));
            assert_eq!(
                particle_parameters(scene),
                particle_parameters(
                    first
                        .iter()
                        .find(|first| first.invocation == scene.invocation)
                        .unwrap()
                )
            );
        }
        assert_ne!(sampled[0].invocation, sampled[1].invocation);
    }
}

#[test]
fn point_instance_uniform_changes_reuse_compiled_definition() {
    let (mut fixture, nodes) = point_fixture(2);
    let mut cache = RenderPlanCache::default();
    let (before, first_stats) = cache.compile(&fixture.project).unwrap();
    assert_eq!(first_stats.compiled_definitions, 1);
    fixture
        .project
        .module_instances
        .get_mut(&fixture.instance_ids[0])
        .unwrap()
        .parameter_overrides
        .insert(nodes.factor_parameter, number(3.0));
    let (after, stats) = cache.compile(&fixture.project).unwrap();
    assert_eq!(stats.compiled_definitions, 0);
    assert_eq!(stats.reused_definitions, 1);
    assert!(std::sync::Arc::ptr_eq(
        &before.module_definitions[&fixture.definition_id],
        &after.module_definitions[&fixture.definition_id]
    ));
}

#[test]
fn grid_source_samples_the_same_point_program_without_particle_state() {
    let (mut fixture, nodes) = point_fixture(1);
    let grid = replace_fixture_source_with_grid(&mut fixture, &nodes, "random");
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
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
    assert_eq!(scenes.len(), 1);
    let scene = scenes[0];
    assert_eq!(scene.source_node_id, grid);
    let PointSceneSource::Grid(parameters) = &scene.source else {
        panic!("expected procedural Grid source");
    };
    assert_eq!(parameters.counts, [8, 8, 1]);
    assert_eq!(parameters.capacity(), 64);
    assert!(scene.point_program.as_ref().is_some_and(|program| {
        program
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, PointInstruction::Random { .. }))
    }));
}

#[test]
fn grid_published_spacing_samples_local_time_and_keeps_sibling_instances_independent() {
    let (mut fixture, nodes) = point_fixture(2);
    let grid = replace_fixture_source_with_grid(&mut fixture, &nodes, "random");
    let spacing_parameter = PublishedParameterId::new();
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let spacing_default = definition.graph.nodes[&grid]
        .properties()
        .get("spacing")
        .and_then(|property| property.value())
        .cloned()
        .expect("Grid spacing default");
    definition.interface.parameters.push(PublishedParameter {
        id: spacing_parameter,
        name: "Grid Spacing".to_string(),
        data_type: PortDataType::Vec3,
        default_value: spacing_default,
        target: ModulePortAddress {
            node_id: grid,
            port: "spacing".to_string(),
        },
    });
    definition.interface_version += 1;
    let mut cache = RenderPlanCache::default();
    let (_, initial) = cache.compile(&fixture.project).unwrap();
    assert_eq!(initial.compiled_definitions, 1);

    let SourceRef::Module(first) = &mut fixture
        .project
        .items
        .get_mut(&fixture.item_ids[0])
        .unwrap()
        .source
    else {
        panic!("module item");
    };
    first.automation_tracks.insert(
        spacing_parameter,
        AutomationTrack {
            keyframes: vec![
                AutomationKeyframe::new(
                    MediaTime::zero(),
                    PropertyValue::Vec3(vec3(10.0)),
                    EasingFunction::Linear,
                ),
                AutomationKeyframe::new(
                    MediaTime::new(1, 1).unwrap(),
                    PropertyValue::Vec3(vec3(30.0)),
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
        .insert(spacing_parameter, PropertyValue::Vec3(vec3(70.0)));
    let (_, reused) = cache.compile(&fixture.project).unwrap();
    assert_eq!(reused.compiled_definitions, 0);
    assert_eq!(reused.reused_definitions, 1);

    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
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
    assert_eq!(scenes.len(), 2);
    for scene in scenes {
        let PointSceneSource::Grid(parameters) = &scene.source else {
            panic!("expected Grid source");
        };
        let expected = if scene.invocation.module_instance_id == fixture.instance_ids[0] {
            vec3(20.0)
        } else {
            vec3(70.0)
        };
        assert_eq!(parameters.spacing, expected);
    }
}

#[test]
fn typed_store_uniform_keyframes_sample_locally_and_keep_siblings_independent() {
    let mut fixture = particle_fixture(2);
    let (renderer, store) = append_uniform_store(&mut fixture, PointAttributeElementType::Vec3);
    let parameter_id = PublishedParameterId::new();
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let default_value = definition.graph.nodes[&store]
        .properties()
        .get("value")
        .and_then(|property| property.value())
        .cloned()
        .unwrap();
    definition.interface.parameters.push(PublishedParameter {
        id: parameter_id,
        name: "Point Vector".to_string(),
        data_type: PortDataType::Vec3,
        default_value,
        target: ModulePortAddress {
            node_id: store,
            port: "value".to_string(),
        },
    });
    definition.interface_version += 1;
    let mut cache = RenderPlanCache::default();
    cache.compile(&fixture.project).unwrap();

    let SourceRef::Module(first) = &mut fixture
        .project
        .items
        .get_mut(&fixture.item_ids[0])
        .unwrap()
        .source
    else {
        panic!("module item");
    };
    first.automation_tracks.insert(
        parameter_id,
        AutomationTrack {
            keyframes: vec![
                AutomationKeyframe::new(
                    MediaTime::zero(),
                    PropertyValue::Vec3(vec3(10.0)),
                    EasingFunction::Linear,
                ),
                AutomationKeyframe::new(
                    MediaTime::new(1, 1).unwrap(),
                    PropertyValue::Vec3(vec3(30.0)),
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
        .insert(parameter_id, PropertyValue::Vec3(vec3(70.0)));
    let (plan, stats) = cache.compile(&fixture.project).unwrap();
    assert_eq!(stats.compiled_definitions, 0);
    assert_eq!(stats.reused_definitions, 1);
    let uniform_register = plan.module_definitions[&fixture.definition_id].point_renderers
        [&renderer]
        .point_program
        .as_ref()
        .unwrap()
        .instructions
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                CompiledPointInstruction::Uniform { node_id, port, value_type }
                    if *node_id == store
                        && port == "value"
                        && *value_type == super::CompiledPointValueType::Exact(PointAttributeElementType::Vec3)
            )
        })
        .unwrap();
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
    assert_eq!(scenes.len(), 2);
    for scene in scenes {
        let expected = if scene.invocation.module_instance_id == fixture.instance_ids[0] {
            20.0
        } else {
            70.0
        };
        let PointInstruction::Constant { value } =
            &scene.point_program.as_ref().unwrap().instructions[uniform_register]
        else {
            panic!("typed Store uniform must be sampled once per invocation");
        };
        assert_eq!(value, &PropertyValue::Vec3(vec3(expected)));
    }
}

#[test]
fn point_frame_commands_roundtrip_from_project_without_persisting_gpu_state_or_compiled_program() {
    let (mut fixture, nodes) = point_fixture(1);
    fixture
        .project
        .module_instances
        .get_mut(&fixture.instance_ids[0])
        .unwrap()
        .parameter_overrides
        .insert(nodes.factor_parameter, number(1.5));
    let before = scenes(&fixture, 45);
    let json = serde_json::to_string(&fixture.project).unwrap();
    for forbidden in [
        "point_program",
        "color_register",
        "serial_offset_bytes",
        "PointInstruction",
    ] {
        assert!(
            !json.contains(forbidden),
            "Project persisted derived {forbidden}"
        );
    }
    fixture.project = serde_json::from_str::<AuthoringProject>(&json).unwrap();
    assert_eq!(scenes(&fixture, 45), before);
    // Seek away and back changes only a derived evaluation request.
    scenes(&fixture, 10);
    assert_eq!(scenes(&fixture, 45), before);
}
