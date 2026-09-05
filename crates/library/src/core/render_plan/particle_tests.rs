use std::collections::HashMap;
use std::sync::Arc;

use ordered_float::OrderedFloat;

use super::{RenderCapability, RenderPlanCompiler, evaluate_render_plan_frame};
use crate::editor::project_service::{GeneratorNodeRequest, test_generator_node};
use crate::editor::{ParticleNodeClipFactory, ParticleNodeClipPlacement, TimelineEditorService};
use crate::model::BlendMode;
use crate::model::animation::EasingFunction;
use crate::model::authoring::{
    AuthoringProject, AutomationKeyframe, AutomationTrack, MediaTime, ModuleDefinitionId,
    ModuleDefinitionSharing, ModuleInstance, ModuleInstanceId, ModuleInvocation, ModuleOutputId,
    ModulePortAddress, RationalRate, SourceRef, TimeMap, TimelineInterval, TimelineItem,
    TimelineItemId,
};
use crate::model::authoring::{ModuleConnection, ModuleConnectionId};
use crate::model::frame::entity::{FrameContent, FrameGroupKind, FrameItem};
use crate::model::frame::particle::ParticleSceneFrame;
use crate::model::node::{Node, NodeContent};
use crate::model::project::property::{PropertyMap, PropertyValue};
use crate::model::project::{IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT};
use crate::plugin::PluginManager;

struct ParticleFixture {
    project: AuthoringProject,
    definition_id: ModuleDefinitionId,
    output_id: ModuleOutputId,
    item_ids: Vec<TimelineItemId>,
    instance_ids: Vec<ModuleInstanceId>,
    emission_rate: crate::model::authoring::PublishedParameterId,
}

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).expect("whole seconds")
}

fn zero_vec3() -> crate::model::property::Vec3 {
    crate::model::property::Vec3 {
        x: OrderedFloat(0.0),
        y: OrderedFloat(0.0),
        z: OrderedFloat(0.0),
    }
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
        output_id: factory.output_id,
        item_ids,
        instance_ids,
        emission_rate,
    }
}

fn connection(
    from_node: uuid::Uuid,
    from_port: &str,
    to_node: uuid::Uuid,
    to_port: &str,
    order: i64,
) -> ModuleConnection {
    ModuleConnection {
        id: ModuleConnectionId::new(),
        from: ModulePortAddress {
            node_id: from_node,
            port: from_port.to_string(),
        },
        to: ModulePortAddress {
            node_id: to_node,
            port: to_port.to_string(),
        },
        order,
        blend_mode: BlendMode::Normal,
    }
}

fn has_group(items: &[FrameItem], source_id: uuid::Uuid, kind: FrameGroupKind) -> bool {
    items.iter().any(|item| match item {
        FrameItem::Group(group) => {
            (group.source_id == source_id && group.kind == kind)
                || has_group(&group.items, source_id, kind)
        }
        FrameItem::Transition(transition) => {
            has_group(std::slice::from_ref(&transition.from.item), source_id, kind)
                || has_group(std::slice::from_ref(&transition.to.item), source_id, kind)
        }
        FrameItem::Object(_) => false,
    })
}

fn group_by_source(
    items: &[FrameItem],
    source_id: uuid::Uuid,
) -> Option<&crate::model::frame::entity::FrameGroup> {
    items.iter().find_map(|item| match item {
        FrameItem::Group(group) => (group.source_id == source_id)
            .then_some(group)
            .or_else(|| group_by_source(&group.items, source_id)),
        FrameItem::Transition(transition) => {
            group_by_source(std::slice::from_ref(&transition.from.item), source_id)
                .or_else(|| group_by_source(std::slice::from_ref(&transition.to.item), source_id))
        }
        FrameItem::Object(_) => None,
    })
}

fn particle_renderer_and_output(fixture: &ParticleFixture) -> (uuid::Uuid, uuid::Uuid) {
    let definition = &fixture.project.module_definitions[&fixture.definition_id];
    let renderer_id = definition
        .graph
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::NativeOperation(operation)
                    if operation.catalog_id == "native.particle.sprite-renderer"
            )
        })
        .expect("Sprite Renderer")
        .id;
    let output_node_id = definition
        .output(fixture.output_id)
        .expect("Image Output")
        .node_id;
    (renderer_id, output_node_id)
}

fn particle_node_id(fixture: &ParticleFixture, catalog_id: &str) -> uuid::Uuid {
    fixture.project.module_definitions[&fixture.definition_id]
        .graph
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::NativeOperation(operation) if operation.catalog_id == catalog_id
            )
        })
        .unwrap_or_else(|| panic!("missing Particle node {catalog_id}"))
        .id
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
            FrameItem::Transition(transition) => {
                scenes.extend(particle_scenes(std::slice::from_ref(&transition.from.item)));
                scenes.extend(particle_scenes(std::slice::from_ref(&transition.to.item)));
            }
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
    assert_eq!(compiled.particle_renderers.len(), 1);
    assert!(compiled.outputs[&fixture.output_id].requires(RenderCapability::Gpu));

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
    let error = RenderPlanCompiler::compile(&fixture.project).unwrap_err();
    assert!(error.contains("constant-only"));
    assert!(error.contains("fixed-step parameter schedule"));
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
    let expected_gravity = crate::model::property::Vec3 {
        x: OrderedFloat(12.0),
        y: OrderedFloat(240.0),
        z: OrderedFloat(-30.0),
    };
    let gravity = PropertyValue::Vec3(expected_gravity);
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
        .set_module_parameter(created.instance_id, created.parameters.gravity, gravity)
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
    assert_eq!(scene.parameters.gravity, expected_gravity);
    assert_eq!(scene.parameters.color, color);
}

#[test]
fn particle_sprite_renderer_preserves_its_authored_blend_mode() {
    let mut fixture = particle_fixture(1);
    let (renderer_id, _) = particle_renderer_and_output(&fixture);
    fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap()
        .graph
        .nodes
        .get_mut(&renderer_id)
        .unwrap()
        .blend_mode = BlendMode::Screen;

    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
    let frame = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        30,
        1.0,
        None,
    )
    .unwrap();
    let renderer_group = group_by_source(&frame.items, renderer_id).expect("Sprite Renderer group");
    assert_eq!(renderer_group.kind, FrameGroupKind::Node);
    assert_eq!(renderer_group.blend_mode, BlendMode::Screen);
    assert_eq!(particle_scenes(&renderer_group.items).len(), 1);
}

#[test]
fn implemented_particle_modifiers_are_optional_in_canonical_order() {
    let mut fixture = particle_fixture(1);
    let (renderer_id, _) = particle_renderer_and_output(&fixture);
    let gravity_id = particle_node_id(&fixture, "native.particle.gravity-force");
    let drag_id = particle_node_id(&fixture, "native.particle.drag-force");
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    definition.graph.connections.retain(|connection| {
        connection.from.node_id != drag_id && connection.to.node_id != drag_id
    });
    definition.graph.connections.push(connection(
        gravity_id,
        "particles",
        renderer_id,
        "particles",
        0,
    ));
    definition.topology_revision += 1;

    fixture
        .project
        .validate()
        .expect("model-valid reduced chain");
    let plan = RenderPlanCompiler::compile(&fixture.project).expect("compiled reduced chain");
    let particle =
        &plan.module_definitions[&fixture.definition_id].particle_renderers[&renderer_id];
    assert_eq!(particle.drag_node_id, None);
    let frame = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        30,
        1.0,
        None,
    )
    .unwrap();
    assert_eq!(
        particle_scenes(&frame.items)[0].parameters.drag,
        OrderedFloat(0.0)
    );
}

#[test]
fn particle_renderer_branches_own_distinct_runtime_state_slots() {
    let mut fixture = particle_fixture(1);
    let (first_renderer_id, output_node_id) = particle_renderer_and_output(&fixture);
    let drag_id = particle_node_id(&fixture, "native.particle.drag-force");
    let second_renderer = Node::new_catalog_node("native.particle.sprite-renderer").unwrap();
    let second_renderer_id = second_renderer.id;
    let merge = Node::new_merge("Particle branches");
    let merge_id = merge.id;
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    definition.graph.connections.retain(|connection| {
        !(connection.from.node_id == first_renderer_id && connection.to.node_id == output_node_id)
    });
    definition
        .graph
        .nodes
        .insert(second_renderer_id, second_renderer);
    definition.graph.nodes.insert(merge_id, merge);
    definition.graph.connections.extend([
        connection(drag_id, "particles", second_renderer_id, "particles", 0),
        connection(
            first_renderer_id,
            IMAGE_OUTPUT_PORT,
            merge_id,
            MERGE_IMAGES_PORT,
            0,
        ),
        connection(
            second_renderer_id,
            IMAGE_OUTPUT_PORT,
            merge_id,
            MERGE_IMAGES_PORT,
            1,
        ),
        connection(
            merge_id,
            IMAGE_OUTPUT_PORT,
            output_node_id,
            IMAGE_INPUT_PORT,
            0,
        ),
    ]);
    definition.topology_revision += 1;

    let plan = RenderPlanCompiler::compile(&fixture.project).expect("compiled Particle branches");
    let particles = &plan.module_definitions[&fixture.definition_id].particle_renderers;
    assert_eq!(particles.len(), 2);
    assert_eq!(
        particles[&first_renderer_id].state_slot_id,
        first_renderer_id
    );
    assert_eq!(
        particles[&second_renderer_id].state_slot_id,
        second_renderer_id
    );
    assert_ne!(
        particles[&first_renderer_id].state_slot_id,
        particles[&second_renderer_id].state_slot_id
    );
}

#[test]
fn particle_on_a_dead_output_branch_does_not_require_gpu() {
    let mut fixture = particle_fixture(1);
    let (renderer_id, output_node_id) = particle_renderer_and_output(&fixture);
    let solid = test_generator_node(
        "Ordinary output",
        GeneratorNodeRequest::Solid {
            color: crate::model::frame::color::Color::white(),
        },
    );
    let solid_id = solid.id;
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    definition.graph.connections.retain(|candidate| {
        !(candidate.from.node_id == renderer_id && candidate.to.node_id == output_node_id)
    });
    definition.graph.nodes.insert(solid_id, solid);
    definition.graph.connections.push(connection(
        solid_id,
        IMAGE_OUTPUT_PORT,
        output_node_id,
        IMAGE_INPUT_PORT,
        0,
    ));
    definition.topology_revision += 1;

    let plan = RenderPlanCompiler::compile(&fixture.project).expect("dead Particle branch");
    let compiled = &plan.module_definitions[&fixture.definition_id];
    assert!(compiled.particle_renderers.is_empty());
    assert!(!compiled.outputs[&fixture.output_id].requires(RenderCapability::Gpu));
    let frame = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        30,
        1.0,
        None,
    )
    .expect("ordinary output must evaluate without Particle");
    assert!(particle_scenes(&frame.items).is_empty());
}

#[test]
fn disabled_downstream_node_hides_an_incomplete_particle_branch() {
    let mut fixture = particle_fixture(1);
    let plugins = PluginManager::default();
    let (renderer_id, output_node_id) = particle_renderer_and_output(&fixture);
    let mut blur = plugins
        .create_effect_operation_node("blur")
        .expect("Blur operation");
    blur.enabled = false;
    let blur_id = blur.id;
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .expect("Particle definition");
    definition.graph.connections.retain(|candidate| {
        !(candidate.from.node_id == renderer_id && candidate.to.node_id == output_node_id)
            && candidate.to.node_id != renderer_id
    });
    definition.graph.nodes.insert(blur_id, blur);
    definition.graph.connections.extend([
        connection(renderer_id, IMAGE_OUTPUT_PORT, blur_id, IMAGE_INPUT_PORT, 0),
        connection(
            blur_id,
            IMAGE_OUTPUT_PORT,
            output_node_id,
            IMAGE_INPUT_PORT,
            0,
        ),
    ]);
    definition.topology_revision += 1;

    let plan = RenderPlanCompiler::compile(&fixture.project)
        .expect("disabled consumer must hide incomplete Particle topology");
    let compiled = &plan.module_definitions[&fixture.definition_id];
    assert_eq!(compiled.nodes.len(), 1);
    assert!(compiled.nodes.contains_key(&blur_id));
    assert!(compiled.particle_renderers.is_empty());
    assert!(!compiled.outputs[&fixture.output_id].requires(RenderCapability::Gpu));
    let frame = evaluate_render_plan_frame(&fixture.project, &plan, &plugins, 30, 1.0, None)
        .expect("disabled consumer evaluates to no image");
    assert!(particle_scenes(&frame.items).is_empty());
}

#[test]
fn disabling_any_particle_stage_compiles_to_no_image() {
    for catalog_id in [
        "native.particle.emitter",
        "native.particle.initialize",
        "native.particle.gravity-force",
        "native.particle.drag-force",
        "native.particle.sprite-renderer",
    ] {
        let mut fixture = particle_fixture(1);
        let node_id = particle_node_id(&fixture, catalog_id);
        let definition = fixture
            .project
            .module_definitions
            .get_mut(&fixture.definition_id)
            .unwrap();
        definition.graph.nodes.get_mut(&node_id).unwrap().enabled = false;
        definition.topology_revision += 1;

        let plan = RenderPlanCompiler::compile(&fixture.project)
            .unwrap_or_else(|error| panic!("disabled {catalog_id} must compile: {error}"));
        let compiled = &plan.module_definitions[&fixture.definition_id];
        assert!(compiled.particle_renderers.is_empty(), "{catalog_id}");
        assert!(
            !compiled.outputs[&fixture.output_id].requires(RenderCapability::Gpu),
            "{catalog_id}"
        );
        let frame = evaluate_render_plan_frame(
            &fixture.project,
            &plan,
            &PluginManager::default(),
            30,
            1.0,
            None,
        )
        .unwrap_or_else(|error| panic!("disabled {catalog_id} must evaluate: {error}"));
        assert!(particle_scenes(&frame.items).is_empty(), "{catalog_id}");
    }
}

#[test]
fn bypassing_particle_endpoints_compiles_to_no_image() {
    for catalog_id in ["native.particle.emitter", "native.particle.sprite-renderer"] {
        let mut fixture = particle_fixture(1);
        let node_id = particle_node_id(&fixture, catalog_id);
        let definition = fixture
            .project
            .module_definitions
            .get_mut(&fixture.definition_id)
            .unwrap();
        definition.graph.nodes.get_mut(&node_id).unwrap().bypassed = true;
        definition.topology_revision += 1;

        let plan = RenderPlanCompiler::compile(&fixture.project)
            .unwrap_or_else(|error| panic!("bypassed {catalog_id} must compile: {error}"));
        assert!(
            plan.module_definitions[&fixture.definition_id]
                .particle_renderers
                .is_empty(),
            "{catalog_id}"
        );
        let frame = evaluate_render_plan_frame(
            &fixture.project,
            &plan,
            &PluginManager::default(),
            30,
            1.0,
            None,
        )
        .unwrap_or_else(|error| panic!("bypassed {catalog_id} must evaluate: {error}"));
        assert!(particle_scenes(&frame.items).is_empty(), "{catalog_id}");
    }
}

#[test]
fn bypassed_particle_modifiers_use_neutral_stage_values() {
    enum Stage {
        Initialize,
        Gravity,
        Drag,
    }
    let expectations = [
        ("native.particle.initialize", Stage::Initialize),
        ("native.particle.gravity-force", Stage::Gravity),
        ("native.particle.drag-force", Stage::Drag),
    ];
    for (catalog_id, stage) in expectations {
        let mut fixture = particle_fixture(1);
        let node_id = particle_node_id(&fixture, catalog_id);
        let definition = fixture
            .project
            .module_definitions
            .get_mut(&fixture.definition_id)
            .unwrap();
        definition.graph.nodes.get_mut(&node_id).unwrap().bypassed = true;
        definition.topology_revision += 1;

        let plan = RenderPlanCompiler::compile(&fixture.project)
            .unwrap_or_else(|error| panic!("bypassed {catalog_id} must compile: {error}"));
        let output = &plan.module_definitions[&fixture.definition_id].outputs[&fixture.output_id];
        assert!(output.requires(RenderCapability::Gpu), "{catalog_id}");
        let frame = evaluate_render_plan_frame(
            &fixture.project,
            &plan,
            &PluginManager::default(),
            30,
            1.0,
            None,
        )
        .unwrap_or_else(|error| panic!("bypassed {catalog_id} must evaluate: {error}"));
        let scene = particle_scenes(&frame.items)[0];
        match stage {
            Stage::Initialize => {
                assert_eq!(scene.parameters.velocity_min, zero_vec3());
                assert_eq!(scene.parameters.velocity_max, zero_vec3());
                assert_eq!(scene.parameters.size_min, OrderedFloat(1.0));
                assert_eq!(scene.parameters.size_max, OrderedFloat(1.0));
            }
            Stage::Gravity => {
                assert_eq!(scene.parameters.gravity, zero_vec3());
            }
            Stage::Drag => assert_eq!(scene.parameters.drag, OrderedFloat(0.0)),
        }
    }
}

#[test]
fn particle_sprite_flows_through_an_effect_before_output() {
    let mut fixture = particle_fixture(1);
    let plugins = PluginManager::default();
    let (renderer_id, output_node_id) = particle_renderer_and_output(&fixture);
    let blur = plugins
        .create_effect_operation_node("blur")
        .expect("Blur operation");
    let blur_id = blur.id;
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .expect("Particle definition");
    definition.graph.connections.retain(|candidate| {
        !(candidate.from.node_id == renderer_id && candidate.to.node_id == output_node_id)
    });
    definition.graph.nodes.insert(blur_id, blur);
    definition.graph.connections.extend([
        connection(renderer_id, IMAGE_OUTPUT_PORT, blur_id, IMAGE_INPUT_PORT, 0),
        connection(
            blur_id,
            IMAGE_OUTPUT_PORT,
            output_node_id,
            IMAGE_INPUT_PORT,
            0,
        ),
    ]);
    definition.topology_revision += 1;

    let plan = RenderPlanCompiler::compile(&fixture.project).expect("compiled Particle effect");
    assert_eq!(
        plan.module_definitions[&fixture.definition_id]
            .particle_renderers
            .len(),
        1
    );
    let frame = evaluate_render_plan_frame(&fixture.project, &plan, &plugins, 30, 1.0, None)
        .expect("evaluated Particle effect");
    assert_eq!(particle_scenes(&frame.items).len(), 1);
    assert!(has_group(&frame.items, blur_id, FrameGroupKind::Effect));
}

#[test]
fn bypassed_downstream_effect_preserves_particle_reachability() {
    let mut fixture = particle_fixture(1);
    let plugins = PluginManager::default();
    let (renderer_id, output_node_id) = particle_renderer_and_output(&fixture);
    let mut blur = plugins
        .create_effect_operation_node("blur")
        .expect("Blur operation");
    blur.bypassed = true;
    let blur_id = blur.id;
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .expect("Particle definition");
    definition.graph.connections.retain(|candidate| {
        !(candidate.from.node_id == renderer_id && candidate.to.node_id == output_node_id)
    });
    definition.graph.nodes.insert(blur_id, blur);
    definition.graph.connections.extend([
        connection(renderer_id, IMAGE_OUTPUT_PORT, blur_id, IMAGE_INPUT_PORT, 0),
        connection(
            blur_id,
            IMAGE_OUTPUT_PORT,
            output_node_id,
            IMAGE_INPUT_PORT,
            0,
        ),
    ]);
    definition.topology_revision += 1;

    let plan = RenderPlanCompiler::compile(&fixture.project).expect("bypassed Particle effect");
    let compiled = &plan.module_definitions[&fixture.definition_id];
    assert_eq!(compiled.particle_renderers.len(), 1);
    assert!(compiled.outputs[&fixture.output_id].requires(RenderCapability::Gpu));
    let frame = evaluate_render_plan_frame(&fixture.project, &plan, &plugins, 30, 1.0, None)
        .expect("bypassed effect must pass through its Image input");
    assert_eq!(particle_scenes(&frame.items).len(), 1);
    assert!(!has_group(&frame.items, blur_id, FrameGroupKind::Effect));
}

#[test]
fn particle_sprite_flows_through_a_merge_before_output() {
    let mut fixture = particle_fixture(1);
    let (renderer_id, output_node_id) = particle_renderer_and_output(&fixture);
    let merge = Node::new_merge("Particle Merge");
    let merge_id = merge.id;
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .expect("Particle definition");
    definition.graph.connections.retain(|candidate| {
        !(candidate.from.node_id == renderer_id && candidate.to.node_id == output_node_id)
    });
    definition.graph.nodes.insert(merge_id, merge);
    definition.graph.connections.extend([
        connection(
            renderer_id,
            IMAGE_OUTPUT_PORT,
            merge_id,
            MERGE_IMAGES_PORT,
            0,
        ),
        connection(
            merge_id,
            IMAGE_OUTPUT_PORT,
            output_node_id,
            IMAGE_INPUT_PORT,
            0,
        ),
    ]);
    definition.topology_revision += 1;

    let plan = RenderPlanCompiler::compile(&fixture.project).expect("compiled Particle Merge");
    let frame = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        30,
        1.0,
        None,
    )
    .expect("evaluated Particle Merge");
    assert_eq!(particle_scenes(&frame.items).len(), 1);
    assert!(has_group(&frame.items, merge_id, FrameGroupKind::Merge));
}
