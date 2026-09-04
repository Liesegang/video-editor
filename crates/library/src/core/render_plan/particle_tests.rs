use std::collections::HashMap;
use std::sync::Arc;

use ordered_float::OrderedFloat;

use super::{RenderPlanCompiler, evaluate_render_plan_frame};
use crate::editor::{ParticleNodeClipFactory, ParticleNodeClipPlacement, TimelineEditorService};
use crate::model::animation::EasingFunction;
use crate::model::authoring::{
    AuthoringProject, AutomationKeyframe, AutomationTrack, MediaTime, ModuleDefinitionId,
    ModuleDefinitionSharing, ModuleInstance, ModuleInstanceId, ModuleInvocation, RationalRate,
    SourceRef, TimeMap, TimelineInterval, TimelineItem, TimelineItemId,
};
use crate::model::frame::entity::{FrameContent, FrameItem};
use crate::model::frame::particle::ParticleSceneFrame;
use crate::model::project::property::{PropertyMap, PropertyValue};
use crate::plugin::PluginManager;

struct ParticleFixture {
    project: AuthoringProject,
    definition_id: ModuleDefinitionId,
    item_ids: Vec<TimelineItemId>,
    instance_ids: Vec<ModuleInstanceId>,
    emission_rate: crate::model::authoring::PublishedParameterId,
}

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).expect("whole seconds")
}

fn particle_fixture(count: usize) -> ParticleFixture {
    let mut project = AuthoringProject::new(
        "Particle runtime",
        320,
        180,
        RationalRate::new(30, 1).unwrap(),
        seconds(20),
    )
    .unwrap();
    let track_id = *project.tracks.keys().next().unwrap();
    let mut factory = ParticleNodeClipFactory::create("Shared Particles").unwrap();
    factory.definition.sharing = ModuleDefinitionSharing::SharedLocal;
    let definition_id = factory.definition.id;
    let emission_rate = factory.parameters.emission_rate;
    project
        .module_definitions
        .insert(definition_id, factory.definition);

    let mut item_ids = Vec::new();
    let mut instance_ids = Vec::new();
    for index in 0..count {
        let instance_id = ModuleInstanceId::new();
        project.module_instances.insert(
            instance_id,
            ModuleInstance {
                id: instance_id,
                definition_id,
                parameter_overrides: HashMap::new(),
            },
        );
        let item_id = TimelineItemId::new();
        project.items.insert(
            item_id,
            TimelineItem {
                id: item_id,
                track_id,
                name: format!("Particle {index}"),
                source: SourceRef::Module(ModuleInvocation {
                    instance_id,
                    output_id: factory.output_id,
                    input_bindings: HashMap::new(),
                    automation_tracks: HashMap::new(),
                }),
                interval: TimelineInterval::new(MediaTime::zero(), seconds(10)).unwrap(),
                time_map: TimeMap::default(),
                layer: index as i64,
                parent: None,
                blend_mode: crate::model::BlendMode::Normal,
                authored_properties: PropertyMap::new(),
            },
        );
        item_ids.push(item_id);
        instance_ids.push(instance_id);
    }
    ParticleFixture {
        project,
        definition_id,
        item_ids,
        instance_ids,
        emission_rate,
    }
}

fn particle_scenes(items: &[FrameItem]) -> Vec<&ParticleSceneFrame> {
    let mut scenes = Vec::new();
    for item in items {
        match item {
            FrameItem::Object(object) => {
                if let FrameContent::ParticleScene { scene, .. } = &object.content {
                    scenes.push(scene);
                }
            }
            FrameItem::Group(group) => scenes.extend(particle_scenes(&group.items)),
        }
    }
    scenes
}

#[test]
fn repeated_particle_items_share_compiled_definition_but_not_invocation_state_keys() {
    let fixture = particle_fixture(2);
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
    assert_eq!(plan.module_definitions.len(), 1);
    assert_eq!(plan.module_invocations.len(), 2);
    assert!(
        plan.module_invocations
            .iter()
            .all(|invocation| invocation.definition_id == fixture.definition_id)
    );
    let compiled = Arc::clone(&plan.module_definitions[&fixture.definition_id]);
    assert_eq!(compiled.particle_outputs.len(), 1);

    let frame = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        30,
        1.0,
        None,
    )
    .unwrap();
    let scenes = particle_scenes(&frame.items);
    assert_eq!(scenes.len(), 2);
    assert_eq!(scenes[0].executable_hash, scenes[1].executable_hash);
    assert_ne!(scenes[0].invocation, scenes[1].invocation);
    assert_ne!(
        scenes[0].invocation.module_instance_id,
        scenes[1].invocation.module_instance_id
    );
}

#[test]
fn evaluating_the_same_exact_time_produces_the_same_preview_export_command() {
    let fixture = particle_fixture(1);
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
    let plugins = PluginManager::default();
    let preview =
        evaluate_render_plan_frame(&fixture.project, &plan, &plugins, 73, 1.0, None).unwrap();
    let export =
        evaluate_render_plan_frame(&fixture.project, &plan, &plugins, 73, 1.0, None).unwrap();
    let preview_scene = particle_scenes(&preview.items)[0];
    let export_scene = particle_scenes(&export.items)[0];
    assert_eq!(preview_scene, export_scene);
    assert_eq!(preview_scene.target_step, 292);
}

#[test]
fn simulation_automation_is_rejected_until_step_sampled_schedules_exist() {
    let mut fixture = particle_fixture(1);
    let SourceRef::Module(invocation) = &mut fixture
        .project
        .items
        .get_mut(&fixture.item_ids[0])
        .unwrap()
        .source
    else {
        panic!("fixture must be a Module item");
    };
    invocation.automation_tracks.insert(
        fixture.emission_rate,
        AutomationTrack {
            keyframes: vec![AutomationKeyframe::new(
                MediaTime::zero(),
                PropertyValue::Number(OrderedFloat(240.0)),
                EasingFunction::Linear,
            )],
        },
    );
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
    let error = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        1,
        1.0,
        None,
    )
    .unwrap_err();
    assert!(error.to_string().contains("fixed-step parameter schedule"));
}

#[test]
fn factory_instances_remain_independent_authoring_owners() {
    let mut fixture = particle_fixture(2);
    fixture
        .project
        .module_instances
        .get_mut(&fixture.instance_ids[0])
        .unwrap()
        .parameter_overrides
        .insert(
            fixture.emission_rate,
            PropertyValue::Number(OrderedFloat(360.0)),
        );
    assert!(
        !fixture.project.module_instances[&fixture.instance_ids[1]]
            .parameter_overrides
            .contains_key(&fixture.emission_rate)
    );
    assert_eq!(
        fixture.project.module_definitions[&fixture.definition_id]
            .interface
            .parameters
            .iter()
            .find(|parameter| parameter.id == fixture.emission_rate)
            .unwrap()
            .default_value,
        PropertyValue::Number(OrderedFloat(120.0))
    );
}

#[test]
fn authored_particle_clip_parameters_reach_the_scene_command() {
    let service = TimelineEditorService::create_default("Particle vertical slice").unwrap();
    let project = service.snapshot().unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let created = service
        .create_particle_node_clip(ParticleNodeClipPlacement {
            track_id,
            name: "GPU Particles".to_string(),
            interval: TimelineInterval::new(MediaTime::zero(), seconds(5)).unwrap(),
            layer: 0,
        })
        .unwrap();
    let rate = PropertyValue::Number(OrderedFloat(360.0));
    let gravity = PropertyValue::Vec3(crate::model::property::Vec3 {
        x: OrderedFloat(12.0),
        y: OrderedFloat(240.0),
        z: OrderedFloat(-30.0),
    });
    let color = crate::model::frame::color::Color {
        r: 240,
        g: 100,
        b: 40,
        a: 200,
    };
    service
        .set_module_parameter(created.instance_id, created.parameters.emission_rate, rate)
        .unwrap();
    service
        .set_module_parameter(
            created.instance_id,
            created.parameters.gravity,
            gravity.clone(),
        )
        .unwrap();
    service
        .set_module_parameter(
            created.instance_id,
            created.parameters.color,
            PropertyValue::Color(color.clone()),
        )
        .unwrap();

    let project = service.snapshot().unwrap();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    assert_eq!(plan.module_definitions.len(), 1);
    assert_eq!(plan.module_invocations.len(), 1);
    let frame =
        evaluate_render_plan_frame(&project, &plan, &PluginManager::default(), 30, 1.0, None)
            .unwrap();
    let scenes = particle_scenes(&frame.items);
    assert_eq!(scenes.len(), 1);
    let scene = scenes[0];
    assert_eq!(scene.target_step, 120);
    assert_eq!(scene.parameters.emission_rate, OrderedFloat(360.0));
    assert_eq!(
        scene.parameters.gravity,
        match gravity {
            PropertyValue::Vec3(value) => value,
            _ => unreachable!(),
        }
    );
    assert_eq!(scene.parameters.color, color);
}
