use std::collections::HashMap;
use std::sync::Arc;

use crate::cache::CacheManager;
use crate::editor::project_service::{GeneratorNodeRequest, test_generator_node};
use crate::editor::{RenderDestination, RenderService};
use crate::model::animation::EasingFunction;
use crate::model::authoring::{
    AuthoringProject, AutomationKeyframe, AutomationTrack, InstanceLocator, ItemOutputStage,
    MediaInputBinding, MediaOutputKind, MediaTime, ModuleDefinition, ModuleDefinitionId,
    ModuleDefinitionSharing, ModuleInstance, ModuleInstanceId, ModuleInvocation, ModulePortAddress,
    ModuleTemplateOrigin, PublishedMediaInput, PublishedMediaInputId, PublishedParameter,
    PublishedParameterId, RationalRate, SourceRef, TimeMap, TimelineInterval, TimelineItem,
    TimelineItemId,
};
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::DrawStyle;
use crate::model::frame::entity::{FrameContent, FrameGroupKind, FrameItem};
use crate::model::node::Node;
use crate::model::project::connection::{IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, PortDataType};
use crate::model::project::property::{PropertyMap, PropertyValue};
use crate::plugin::PluginManager;
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_renderer::SkiaRenderer;

use super::{
    PlannedSource, RenderPlanCache, RenderPlanCompiler, evaluate_render_plan_frame,
    evaluate_timeline_render_plan_frame,
};

struct NodeClipFixture {
    project: AuthoringProject,
    definition_id: ModuleDefinitionId,
    parameter_id: PublishedParameterId,
    item_ids: Vec<TimelineItemId>,
    instance_ids: Vec<ModuleInstanceId>,
}

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).expect("whole seconds")
}

fn node_clip_fixture(count: usize) -> NodeClipFixture {
    let mut project = AuthoringProject::new(
        "Node Clip",
        320,
        180,
        RationalRate::new(30, 1).unwrap(),
        seconds(20),
    )
    .unwrap();
    let track_id = *project.tracks.keys().next().unwrap();
    let node = test_generator_node(
        "Shared Solid",
        GeneratorNodeRequest::Solid {
            color: Color::white(),
        },
    );
    let node_id = node.id;
    let (mut definition, output_id) = ModuleDefinition::new_image(
        "Shared generator",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
    );
    let definition_id = definition.id;
    let output_target = definition
        .output(output_id)
        .unwrap()
        .target(PortDataType::Image)
        .unwrap();
    let parameter_id = PublishedParameterId::new();
    definition.graph.nodes.insert(node_id, node);
    definition
        .graph
        .connections
        .push(crate::model::authoring::ModuleConnection {
            id: crate::model::authoring::ModuleConnectionId::new(),
            from: ModulePortAddress {
                node_id,
                port: IMAGE_OUTPUT_PORT.to_string(),
            },
            to: output_target,
            order: 0,
            blend_mode: crate::model::BlendMode::Normal,
        });
    definition.interface.parameters.push(PublishedParameter {
        id: parameter_id,
        name: "Color".to_string(),
        data_type: PortDataType::Color,
        default_value: PropertyValue::Color(Color::white()),
        target: ModulePortAddress {
            node_id,
            port: "color".to_string(),
        },
    });
    definition.topology_revision = 2;
    project.module_definitions.insert(definition_id, definition);
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
                name: format!("Node Clip {index}"),
                source: SourceRef::Module(ModuleInvocation {
                    instance_id,
                    output_id,
                    input_bindings: HashMap::new(),
                    automation_tracks: HashMap::new(),
                }),
                interval: TimelineInterval::new(seconds(index as i64), seconds(5)).unwrap(),
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
    NodeClipFixture {
        project,
        definition_id,
        parameter_id,
        item_ids,
        instance_ids,
    }
}

fn shape_fill_colors(items: &[FrameItem]) -> Vec<Color> {
    let mut colors = Vec::new();
    for item in items {
        match item {
            FrameItem::Group(group) => colors.extend(shape_fill_colors(&group.items)),
            FrameItem::Object(object) => {
                let FrameContent::Shape { styles, .. } = &object.content else {
                    continue;
                };
                for style in styles {
                    if let DrawStyle::Fill { color, .. } = &style.style {
                        colors.push(color.clone());
                    }
                }
            }
            FrameItem::Transition(transition) => {
                colors.extend(shape_fill_colors(std::slice::from_ref(
                    &transition.from.item,
                )));
                colors.extend(shape_fill_colors(std::slice::from_ref(&transition.to.item)));
            }
        }
    }
    colors
}

fn item_blend_mode(
    items: &[FrameItem],
    item_id: TimelineItemId,
) -> Option<crate::model::BlendMode> {
    for item in items {
        if let FrameItem::Group(group) = item {
            if group.kind == FrameGroupKind::Clip && group.source_id == item_id.as_uuid() {
                return Some(group.blend_mode);
            }
            if let Some(mode) = item_blend_mode(&group.items, item_id) {
                return Some(mode);
            }
        }
    }
    None
}

fn group_blend_mode(
    items: &[FrameItem],
    source_id: uuid::Uuid,
    kind: FrameGroupKind,
) -> Option<crate::model::BlendMode> {
    for item in items {
        if let FrameItem::Group(group) = item {
            if group.kind == kind && group.source_id == source_id {
                return Some(group.blend_mode);
            }
            if let Some(mode) = group_blend_mode(&group.items, source_id, kind) {
                return Some(mode);
            }
        }
    }
    None
}

#[test]
fn ordinary_items_never_become_module_invocations() {
    let mut fixture = node_clip_fixture(1);
    let track_id = *fixture.project.tracks.keys().next().unwrap();
    let ordinary_id = TimelineItemId::new();
    fixture.project.items.insert(
        ordinary_id,
        TimelineItem {
            id: ordinary_id,
            track_id,
            name: "Ordinary Timeline solid".to_string(),
            source: SourceRef::Solid {
                color: Color::black(),
            },
            interval: TimelineInterval::new(MediaTime::zero(), seconds(5)).unwrap(),
            time_map: TimeMap::default(),
            layer: -1,
            parent: None,
            blend_mode: crate::model::BlendMode::Normal,
            authored_properties: PropertyMap::new(),
        },
    );

    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
    let timeline = &plan.timelines[&fixture.project.root_timeline_id];

    assert_eq!(plan.module_invocations.len(), 1);
    assert_eq!(plan.module_definitions.len(), 1);
    assert_eq!(timeline.schedule.len(), 2);
    assert_eq!(
        timeline
            .schedule
            .iter()
            .filter(|item| item.source == PlannedSource::Module)
            .count(),
        1
    );
}

#[test]
fn repeated_node_clips_share_one_compiled_definition() {
    let fixture = node_clip_fixture(100);
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();

    assert_eq!(plan.module_definitions.len(), 1);
    assert_eq!(plan.module_invocations.len(), 100);
    assert_eq!(
        plan.module_definitions[&fixture.definition_id].nodes.len(),
        1
    );
}

#[test]
fn instance_parameter_edits_reuse_definition_and_timeline_schedule() {
    let mut fixture = node_clip_fixture(1);
    let mut cache = RenderPlanCache::default();
    let (first, first_stats) = cache.compile(&fixture.project).unwrap();
    assert_eq!(first_stats.compiled_definitions, 1);
    assert_eq!(first_stats.compiled_timelines, 1);

    fixture
        .project
        .module_instances
        .get_mut(&fixture.instance_ids[0])
        .unwrap()
        .parameter_overrides
        .insert(fixture.parameter_id, PropertyValue::Color(Color::black()));
    let (second, second_stats) = cache.compile(&fixture.project).unwrap();

    assert_eq!(second_stats.reused_definitions, 1);
    assert_eq!(second_stats.reused_timelines, 1);
    assert!(Arc::ptr_eq(
        &first.module_definitions[&fixture.definition_id],
        &second.module_definitions[&fixture.definition_id]
    ));
    assert!(Arc::ptr_eq(
        &first.timelines[&fixture.project.root_timeline_id],
        &second.timelines[&fixture.project.root_timeline_id]
    ));
}

#[test]
fn placement_edit_invalidates_only_the_timeline_schedule() {
    let mut fixture = node_clip_fixture(1);
    let mut cache = RenderPlanCache::default();
    cache.compile(&fixture.project).unwrap();
    fixture
        .project
        .items
        .get_mut(&fixture.item_ids[0])
        .unwrap()
        .interval = TimelineInterval::new(seconds(2), seconds(5)).unwrap();

    let (_, stats) = cache.compile(&fixture.project).unwrap();

    assert_eq!(stats.reused_definitions, 1);
    assert_eq!(stats.compiled_timelines, 1);
    assert_eq!(stats.compiled_definitions, 0);
}

#[test]
fn schedule_maps_exact_timeline_time_to_node_clip_local_time() {
    let mut fixture = node_clip_fixture(1);
    let item = fixture.project.items.get_mut(&fixture.item_ids[0]).unwrap();
    item.interval = TimelineInterval::new(seconds(2), seconds(8)).unwrap();
    item.time_map = TimeMap {
        source_start: MediaTime::new(1, 2).unwrap(),
        playback_rate: RationalRate::new(2, 1).unwrap(),
    };
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
    let scheduled = plan.timelines[&fixture.project.root_timeline_id]
        .schedule
        .iter()
        .find(|scheduled| scheduled.item_id == fixture.item_ids[0])
        .unwrap();

    assert_eq!(
        scheduled.local_time(seconds(3)).unwrap(),
        MediaTime::new(5, 2).unwrap()
    );
}

#[test]
fn dead_module_branches_do_not_enter_the_compiled_output() {
    let mut fixture = node_clip_fixture(1);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let dead = test_generator_node(
        "Dead",
        GeneratorNodeRequest::Solid {
            color: Color::black(),
        },
    );
    definition.graph.nodes.insert(dead.id, dead);
    definition.topology_revision += 1;

    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
    let compiled = &plan.module_definitions[&fixture.definition_id];

    assert_eq!(compiled.nodes.len(), 1);
    assert_eq!(
        compiled
            .outputs
            .values()
            .next()
            .unwrap()
            .evaluation_order
            .len(),
        1
    );
}

#[test]
fn definition_invalidation_is_range_scoped_to_its_invocations() {
    let fixture = node_clip_fixture(2);
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
    let affected = plan
        .dependencies
        .affected_by_definition(fixture.definition_id);

    assert_eq!(affected.invocations.len(), 2);
    assert_eq!(affected.ranges.len(), 2);
    assert_eq!(affected.timelines.len(), 1);
}

#[test]
fn node_clip_runtime_applies_instance_parameters_without_mutating_definition() {
    let mut fixture = node_clip_fixture(1);
    let authored = Color {
        r: 24,
        g: 96,
        b: 192,
        a: 255,
    };
    fixture
        .project
        .module_instances
        .get_mut(&fixture.instance_ids[0])
        .unwrap()
        .parameter_overrides
        .insert(fixture.parameter_id, PropertyValue::Color(authored.clone()));
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();

    let frame = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        0,
        1.0,
        None,
    )
    .unwrap();

    assert!(shape_fill_colors(&frame.items).contains(&authored));
    assert_eq!(
        fixture.project.module_definitions[&fixture.definition_id]
            .interface
            .parameters[0]
            .default_value,
        PropertyValue::Color(Color::white())
    );
}

#[test]
fn node_clip_automation_uses_exact_clip_local_time() {
    let mut fixture = node_clip_fixture(1);
    fixture
        .project
        .items
        .get_mut(&fixture.item_ids[0])
        .unwrap()
        .interval = TimelineInterval::new(seconds(2), seconds(5)).unwrap();
    let SourceRef::Module(invocation) = &mut fixture
        .project
        .items
        .get_mut(&fixture.item_ids[0])
        .unwrap()
        .source
    else {
        panic!("fixture must contain a Node Clip");
    };
    invocation.automation_tracks.insert(
        fixture.parameter_id,
        AutomationTrack {
            keyframes: vec![
                AutomationKeyframe::new(
                    MediaTime::zero(),
                    PropertyValue::Color(Color {
                        r: 255,
                        g: 0,
                        b: 0,
                        a: 255,
                    }),
                    EasingFunction::Linear,
                ),
                AutomationKeyframe::new(
                    seconds(2),
                    PropertyValue::Color(Color {
                        r: 0,
                        g: 0,
                        b: 255,
                        a: 255,
                    }),
                    EasingFunction::Linear,
                ),
            ],
        },
    );
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();

    // Timeline t=3s is Node Clip local t=1s: exactly halfway between keys.
    let frame = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        90,
        1.0,
        None,
    )
    .unwrap();

    assert!(shape_fill_colors(&frame.items).contains(&Color {
        r: 128,
        g: 0,
        b: 128,
        a: 255,
    }));
}

#[test]
fn same_timeline_media_input_uses_timeline_time_not_node_clip_local_time() {
    let mut fixture = node_clip_fixture(1);
    let track_id = fixture.project.items[&fixture.item_ids[0]].track_id;
    let source_id = TimelineItemId::new();
    let source_color = Color {
        r: 0,
        g: 255,
        b: 64,
        a: 255,
    };
    fixture.project.items.insert(
        source_id,
        TimelineItem {
            id: source_id,
            track_id,
            name: "Aux image at timeline t=3s".to_string(),
            source: SourceRef::Solid {
                color: source_color.clone(),
            },
            interval: TimelineInterval::new(seconds(3), seconds(1)).unwrap(),
            time_map: TimeMap::default(),
            layer: 0,
            parent: None,
            blend_mode: crate::model::BlendMode::Normal,
            authored_properties: PropertyMap::new(),
        },
    );
    fixture
        .project
        .items
        .get_mut(&fixture.item_ids[0])
        .unwrap()
        .interval = TimelineInterval::new(seconds(2), seconds(5)).unwrap();

    let input_id = PublishedMediaInputId::new();
    let output_id = match &fixture.project.items[&fixture.item_ids[0]].source {
        SourceRef::Module(invocation) => invocation.output_id,
        _ => panic!("fixture must contain a Node Clip"),
    };
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let output = definition.output(output_id).unwrap();
    definition.graph.nodes.retain(|_, node| {
        matches!(
            node.content(),
            crate::model::node::NodeContent::ModuleOutput(_)
        )
    });
    definition.graph.connections.clear();
    definition.interface.parameters.clear();
    definition.interface.media_inputs = vec![PublishedMediaInput {
        id: input_id,
        name: "Image".to_string(),
        data_type: PortDataType::Image,
        target: output.target(PortDataType::Image).unwrap(),
        required: true,
        primary: false,
    }];
    definition.topology_revision += 1;
    let SourceRef::Module(invocation) = &mut fixture
        .project
        .items
        .get_mut(&fixture.item_ids[0])
        .unwrap()
        .source
    else {
        panic!("fixture must contain a Node Clip");
    };
    invocation.input_bindings.insert(
        input_id,
        MediaInputBinding::TimelineItemOutput {
            locator: InstanceLocator::SameTimeline,
            item_id: source_id,
            output: MediaOutputKind::Image,
            stage: ItemOutputStage::PostTransform,
        },
    );
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();

    let frame = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        90,
        1.0,
        None,
    )
    .unwrap();

    // One shape is the authored source and the other is the Node Clip's
    // invocation of that published input. Querying at clip-local t=1s would
    // make the source inactive and leave only one.
    assert!(
        shape_fill_colors(&frame.items)
            .iter()
            .filter(|color| **color == source_color)
            .count()
            >= 2
    );
}

#[test]
fn authoring_node_clip_reaches_the_existing_rasterizer() {
    let fixture = node_clip_fixture(1);
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
    let cache = Arc::new(CacheManager::new());
    let renderer = SkiaRenderer::new(
        320,
        180,
        Color::black(),
        false,
        None,
        Some(Arc::clone(&cache)),
    )
    .unwrap();
    let plugins = Arc::new(PluginManager::default());
    let mut service = RenderService::new(renderer, Arc::clone(&plugins), cache);

    let frame = evaluate_render_plan_frame(&fixture.project, &plan, plugins.as_ref(), 0, 1.0, None)
        .unwrap();
    let output = service
        .render_authoring_frame(&fixture.project, &frame, RenderDestination::Preview)
        .unwrap();

    let RenderOutput::Image(image) = output else {
        panic!("managed authoring Preview must terminate to an Image");
    };
    assert_eq!((image.width, image.height), (320, 180));
    assert!(
        image
            .data
            .chunks_exact(4)
            .any(|pixel| pixel == [255, 255, 255, 255])
    );
}

#[test]
fn timeline_item_blend_mode_reaches_its_clip_compositing_boundary() {
    let mut fixture = node_clip_fixture(1);
    let item_id = fixture.item_ids[0];
    fixture.project.items.get_mut(&item_id).unwrap().blend_mode = crate::model::BlendMode::Screen;
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();

    let frame = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        0,
        1.0,
        None,
    )
    .unwrap();

    assert_eq!(
        item_blend_mode(&frame.items, item_id),
        Some(crate::model::BlendMode::Screen)
    );
}

#[test]
fn module_merge_keeps_independent_blend_per_connection() {
    let mut project = AuthoringProject::new(
        "Module Blend",
        320,
        180,
        RationalRate::new(30, 1).unwrap(),
        seconds(4),
    )
    .unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let back = test_generator_node(
        "Back",
        GeneratorNodeRequest::Solid {
            color: Color::black(),
        },
    );
    let front = test_generator_node(
        "Front",
        GeneratorNodeRequest::Solid {
            color: Color::white(),
        },
    );
    let merge = Node::new_merge("Merge");
    let (back_id, front_id, merge_id) = (back.id, front.id, merge.id);
    let normal_connection_id = crate::model::authoring::ModuleConnectionId::new();
    let screen_connection_id = crate::model::authoring::ModuleConnectionId::new();
    let (mut definition, output_id) =
        ModuleDefinition::new_image("Layer Blend", ModuleDefinitionSharing::Private);
    let definition_id = definition.id;
    let output_target = definition
        .output(output_id)
        .unwrap()
        .target(PortDataType::Image)
        .unwrap();
    definition
        .graph
        .nodes
        .extend([(back_id, back), (front_id, front), (merge_id, merge)]);
    definition.graph.connections = vec![
        crate::model::authoring::ModuleConnection {
            id: normal_connection_id,
            from: ModulePortAddress {
                node_id: back_id,
                port: IMAGE_OUTPUT_PORT.to_string(),
            },
            to: ModulePortAddress {
                node_id: merge_id,
                port: MERGE_IMAGES_PORT.to_string(),
            },
            order: 0,
            blend_mode: crate::model::BlendMode::Normal,
        },
        crate::model::authoring::ModuleConnection {
            id: screen_connection_id,
            from: ModulePortAddress {
                node_id: front_id,
                port: IMAGE_OUTPUT_PORT.to_string(),
            },
            to: ModulePortAddress {
                node_id: merge_id,
                port: MERGE_IMAGES_PORT.to_string(),
            },
            order: 1,
            blend_mode: crate::model::BlendMode::Screen,
        },
        crate::model::authoring::ModuleConnection {
            id: crate::model::authoring::ModuleConnectionId::new(),
            from: ModulePortAddress {
                node_id: merge_id,
                port: IMAGE_OUTPUT_PORT.to_string(),
            },
            to: output_target,
            order: 0,
            blend_mode: crate::model::BlendMode::Normal,
        },
    ];
    definition.topology_revision = 2;
    project.module_definitions.insert(definition_id, definition);
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
            name: "Blended Node Clip".to_string(),
            source: SourceRef::Module(ModuleInvocation {
                instance_id,
                output_id,
                input_bindings: HashMap::new(),
                automation_tracks: HashMap::new(),
            }),
            interval: TimelineInterval::new(seconds(0), seconds(4)).unwrap(),
            time_map: TimeMap::default(),
            layer: 0,
            parent: None,
            blend_mode: crate::model::BlendMode::Normal,
            authored_properties: PropertyMap::new(),
        },
    );

    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let frame =
        evaluate_render_plan_frame(&project, &plan, &PluginManager::default(), 0, 1.0, None)
            .unwrap();
    assert_eq!(
        group_blend_mode(
            &frame.items,
            screen_connection_id.as_uuid(),
            FrameGroupKind::ConnectedImage,
        ),
        Some(crate::model::BlendMode::Screen)
    );
}

#[test]
fn timeline_frame_evaluation_rejects_negative_exact_frame_indices() {
    let fixture = node_clip_fixture(1);
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();

    let result = evaluate_timeline_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        fixture.project.root_timeline_id,
        -1,
        1.0,
        None,
    );

    assert!(matches!(result, Err(crate::error::LibraryError::Render(_))));
}
