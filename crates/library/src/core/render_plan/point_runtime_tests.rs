use ordered_float::OrderedFloat;

use super::particle_tests::{ParticleFixture, particle_scenes};
use super::point_tests::{PointNodes, point_fixture};
use super::{
    CompiledPointInstruction, RenderPlanCache, RenderPlanCompiler, evaluate_render_plan_frame,
};
use crate::model::animation::EasingFunction;
use crate::model::authoring::{
    AuthoringProject, AutomationKeyframe, AutomationTrack, MediaTime, SourceRef,
};
use crate::model::frame::particle::ParticleSceneFrame;
use crate::model::point::{PointInstruction, PointRenderProgram};
use crate::model::project::NUMERIC_B_INPUT_PORT;
use crate::model::property::PropertyValue;
use crate::plugin::PluginManager;

fn number(value: f64) -> PropertyValue {
    PropertyValue::Number(OrderedFloat(value))
}

fn scenes(fixture: &ParticleFixture, frame: u64) -> Vec<ParticleSceneFrame> {
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

fn factor_register(fixture: &ParticleFixture, nodes: &PointNodes) -> usize {
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
    plan.module_definitions[&fixture.definition_id].particle_renderers[&nodes.renderer]
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
                scene.parameters,
                first
                    .iter()
                    .find(|first| first.invocation == scene.invocation)
                    .unwrap()
                    .parameters
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
