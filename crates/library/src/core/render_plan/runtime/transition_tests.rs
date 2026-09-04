use std::collections::HashMap;
use std::sync::Arc;

use super::*;
use crate::animation::EasingFunction;
use crate::cache::CacheManager;
use crate::core::render_plan::RenderPlanCompiler;
use crate::editor::{RenderDestination, RenderService, TimelineEditorService};
use crate::model::Asset;
use crate::model::authoring::{
    AutomationKeyframe, AutomationTrack, CompositionInstance, DurationPolicy, InstanceLocator,
    InstancePath, ItemOutputStage, MediaInputBinding, MediaOutputKind, ModuleConnection,
    ModuleConnectionId, ModuleDefinition, ModuleDefinitionId, ModuleDefinitionSharing,
    ModuleInstance, ModuleInstanceId, ModulePortAddress, ProjectInvalidation, PublishedMediaInput,
    PublishedMediaInputId, PublishedParameter, PublishedParameterId, RationalRate, TimeMap,
    Timeline, TimelineInterval, TimelineTrack, TimelineTrackId, TimelineTrackKind, Transition,
    TransitionAlignment, TransitionId, TransitionMediaType, TransitionProcessor,
};
use crate::model::frame::color::Color;
use crate::model::frame::entity::{FrameGroupKind, FrameTransitionKind};
use crate::model::node::Node;
use crate::model::project::property::{Property, PropertyMap, PropertyValue};
use crate::model::project::{
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, PortDataType,
    TRANSITION_PROGRESS_INPUT_PORT, TRANSITION_PROGRESS_PROPERTY,
};
use crate::plugin::PluginManager;
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_renderer::SkiaRenderer;

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).unwrap()
}

fn adjacent_solid_project() -> (
    AuthoringProject,
    TransitionId,
    TimelineItemId,
    TimelineItemId,
) {
    let mut project = AuthoringProject::new(
        "Cross Dissolve runtime",
        2,
        2,
        RationalRate::new(30, 1).unwrap(),
        seconds(10),
    )
    .unwrap();
    let timeline_id = project.root_timeline_id;
    let track_id = project.timelines[&timeline_id].track_order[0];
    let from = insert_solid(
        &mut project,
        track_id,
        0,
        seconds(0),
        seconds(5),
        Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        },
    );
    let to = insert_solid(
        &mut project,
        track_id,
        1,
        seconds(5),
        seconds(5),
        Color {
            r: 0,
            g: 0,
            b: 255,
            a: 255,
        },
    );
    let transition_id = TransitionId::new();
    project.transitions.insert(
        transition_id,
        Transition {
            id: transition_id,
            timeline_id,
            from_item_id: from,
            to_item_id: to,
            edit_point: seconds(5),
            duration: seconds(4),
            alignment: TransitionAlignment::CenteredOnEdit,
            processor: TransitionProcessor::cross_dissolve(),
            parameters: HashMap::new(),
        },
    );
    project.validate().unwrap();
    (project, transition_id, from, to)
}

fn insert_solid(
    project: &mut AuthoringProject,
    track_id: crate::model::authoring::TimelineTrackId,
    layer: i64,
    start: MediaTime,
    duration: MediaTime,
    color: Color,
) -> TimelineItemId {
    let id = TimelineItemId::new();
    project.items.insert(
        id,
        TimelineItem {
            id,
            track_id,
            name: format!("Solid {layer}"),
            source: SourceRef::Solid { color },
            interval: TimelineInterval::new(start, duration).unwrap(),
            time_map: TimeMap::default(),
            layer,
            parent: None,
            blend_mode: BlendMode::Normal,
            authored_properties: PropertyMap::new(),
        },
    );
    id
}

fn insert_visual_track(project: &mut AuthoringProject, name: &str) -> TimelineTrackId {
    let timeline_id = project.root_timeline_id;
    let track_id = TimelineTrackId::new();
    project.tracks.insert(
        track_id,
        TimelineTrack {
            id: track_id,
            timeline_id,
            name: name.to_string(),
            kind: TimelineTrackKind::Visual,
            authored_properties: PropertyMap::new(),
        },
    );
    project
        .timelines
        .get_mut(&timeline_id)
        .expect("root Timeline")
        .track_order
        .push(track_id);
    track_id
}

fn render_pixel(project: &AuthoringProject, frame_number: u64) -> [u8; 4] {
    let plan = RenderPlanCompiler::compile(project).unwrap();
    let plugins = Arc::new(PluginManager::default());
    let frame =
        evaluate_render_plan_frame(project, &plan, plugins.as_ref(), frame_number, 1.0, None)
            .unwrap();
    let cache = Arc::new(CacheManager::new());
    let renderer =
        SkiaRenderer::new(2, 2, Color::black(), false, None, Some(cache.clone())).unwrap();
    let mut service = RenderService::new(renderer, plugins, cache);
    let RenderOutput::Image(image) = service
        .render_authoring_frame(project, &frame, RenderDestination::Preview)
        .unwrap()
    else {
        panic!("authoring Preview must terminate to encoded pixels");
    };
    image.data[0..4].try_into().unwrap()
}

fn render_instance_pixel(
    project: &AuthoringProject,
    timeline_id: TimelineId,
    instance_path: &InstancePath,
    frame_number: i64,
) -> [u8; 4] {
    let plan = RenderPlanCompiler::compile(project).unwrap();
    let plugins = Arc::new(PluginManager::default());
    let frame = evaluate_timeline_render_plan_frame_at_instance(
        project,
        &plan,
        plugins.as_ref(),
        timeline_id,
        frame_number,
        1.0,
        None,
        Some(instance_path),
    )
    .unwrap();
    let cache = Arc::new(CacheManager::new());
    let renderer =
        SkiaRenderer::new(2, 2, Color::black(), false, None, Some(cache.clone())).unwrap();
    let mut service = RenderService::new(renderer, plugins, cache);
    let RenderOutput::Image(image) = service
        .render_authoring_frame(project, &frame, RenderDestination::Preview)
        .unwrap()
    else {
        panic!("authoring Preview must terminate to encoded pixels");
    };
    image.data[0..4].try_into().unwrap()
}

fn wrap_root_with_two_instances(
    project: &mut AuthoringProject,
    nested_timeline_id: TimelineId,
) -> (TimelineId, TimelineItemId, TimelineItemId) {
    let root_timeline_id = TimelineId::new();
    let root_track_id = TimelineTrackId::new();
    project.root_timeline_id = root_timeline_id;
    project.timelines.insert(
        root_timeline_id,
        Timeline {
            id: root_timeline_id,
            name: "Root".to_string(),
            width: 2,
            height: 2,
            fps: RationalRate::new(30, 1).unwrap(),
            duration: seconds(10),
            background_color: Color::black(),
            color_profile: "sRGB".to_string(),
            track_order: vec![root_track_id],
            authored_properties: PropertyMap::new(),
            published_parameters: Vec::new(),
        },
    );
    project.tracks.insert(
        root_track_id,
        TimelineTrack {
            id: root_track_id,
            timeline_id: root_timeline_id,
            name: "Instances".to_string(),
            kind: TimelineTrackKind::Visual,
            authored_properties: PropertyMap::new(),
        },
    );
    let placement = |id, layer| TimelineItem {
        id,
        track_id: root_track_id,
        name: format!("Instance {layer}"),
        source: SourceRef::Composition(CompositionInstance {
            timeline_id: nested_timeline_id,
            duration_policy: DurationPolicy::Fixed,
            parameter_overrides: HashMap::new(),
            transition_module_overrides: Vec::new(),
        }),
        interval: TimelineInterval::new(seconds(0), seconds(10)).unwrap(),
        time_map: TimeMap::default(),
        layer,
        parent: None,
        blend_mode: BlendMode::Normal,
        authored_properties: PropertyMap::new(),
    };
    let first_item_id = TimelineItemId::new();
    let second_item_id = TimelineItemId::new();
    project
        .items
        .insert(first_item_id, placement(first_item_id, 0));
    project
        .items
        .insert(second_item_id, placement(second_item_id, 1));
    (root_timeline_id, first_item_id, second_item_id)
}

fn promote_cross_dissolve_to_module(
    project: &mut AuthoringProject,
    transition_id: TransitionId,
) -> ModuleDefinitionId {
    let (mut definition, contract) = ModuleDefinition::new_transition(
        "Editable Cross Dissolve",
        ModuleDefinitionSharing::Private,
        TransitionMediaType::Image,
    )
    .unwrap();
    let progress_node_id = definition
        .interface
        .parameters
        .iter()
        .find(|parameter| parameter.id == contract.progress_parameter_id)
        .unwrap()
        .target
        .node_id;
    definition
        .graph
        .nodes
        .get_mut(&progress_node_id)
        .unwrap()
        .set_property(
            TRANSITION_PROGRESS_PROPERTY.to_string(),
            Property::constant(crate::model::project::property::PropertyValue::Number(
                OrderedFloat(1.0),
            )),
        )
        .unwrap();
    let definition_id = definition.id;
    let instance_id = ModuleInstanceId::new();
    project.module_definitions.insert(definition_id, definition);
    project.module_instances.insert(
        instance_id,
        ModuleInstance {
            id: instance_id,
            definition_id,
            parameter_overrides: HashMap::new(),
        },
    );
    project
        .transitions
        .get_mut(&transition_id)
        .unwrap()
        .processor = TransitionProcessor::module(instance_id, TransitionMediaType::Image);
    project.validate().unwrap();
    definition_id
}

fn append_blur_to_transition_module(
    project: &mut AuthoringProject,
    definition_id: ModuleDefinitionId,
) -> uuid::Uuid {
    let definition = project.module_definitions.get_mut(&definition_id).unwrap();
    let output = definition
        .outputs()
        .find(|output| definition.host_contract.transition().unwrap().output_id == output.id)
        .unwrap();
    let output_node_id = output.node_id;
    let output_connection = definition
        .graph
        .connections
        .iter()
        .position(|connection| {
            connection.to.node_id == output_node_id && connection.to.port == IMAGE_INPUT_PORT
        })
        .unwrap();
    let output_connection = definition.graph.connections.remove(output_connection);
    let blur = PluginManager::default()
        .create_effect_operation_node("blur")
        .unwrap();
    let blur_id = blur.id;
    definition.graph.nodes.insert(blur_id, blur);
    let address = |node_id, port: &str| ModulePortAddress {
        node_id,
        port: port.to_string(),
    };
    definition.graph.connections.extend([
        ModuleConnection {
            id: ModuleConnectionId::new(),
            from: output_connection.from,
            to: address(blur_id, IMAGE_INPUT_PORT),
            order: 0,
            blend_mode: BlendMode::Normal,
        },
        ModuleConnection {
            id: ModuleConnectionId::new(),
            from: address(blur_id, IMAGE_OUTPUT_PORT),
            to: output_connection.to,
            order: 0,
            blend_mode: BlendMode::Normal,
        },
    ]);
    definition.topology_revision += 1;
    definition.validate().unwrap();
    project.validate().unwrap();
    blur_id
}

fn route_transition_output_from_public_input(
    project: &mut AuthoringProject,
    definition_id: ModuleDefinitionId,
) -> PublishedMediaInputId {
    let definition = project.module_definitions.get_mut(&definition_id).unwrap();
    let contract = definition
        .host_contract
        .transition()
        .expect("Transition contract")
        .clone();
    let output_node_id = definition
        .output(contract.output_id)
        .expect("protected Output")
        .node_id;
    let output_connection = definition
        .graph
        .connections
        .iter()
        .position(|connection| {
            connection.to.node_id == output_node_id && connection.to.port == IMAGE_INPUT_PORT
        })
        .map(|index| definition.graph.connections.remove(index))
        .expect("starter output connection");
    let merge = Node::new_merge("External Image");
    let merge_id = merge.id;
    definition.graph.nodes.insert(merge_id, merge);
    definition.graph.connections.push(ModuleConnection {
        id: ModuleConnectionId::new(),
        from: ModulePortAddress {
            node_id: merge_id,
            port: IMAGE_OUTPUT_PORT.to_string(),
        },
        to: output_connection.to,
        order: 0,
        blend_mode: BlendMode::Normal,
    });
    let input_id = PublishedMediaInputId::new();
    definition.interface.media_inputs.push(PublishedMediaInput {
        id: input_id,
        name: "External Image".to_string(),
        data_type: PortDataType::Image,
        target: ModulePortAddress {
            node_id: merge_id,
            port: MERGE_IMAGES_PORT.to_string(),
        },
        required: true,
        primary: false,
    });
    definition.topology_revision += 1;
    definition.interface_version += 1;
    definition.validate().unwrap();
    input_id
}

fn publish_transition_mix_progress(
    project: &mut AuthoringProject,
    definition_id: ModuleDefinitionId,
) -> PublishedParameterId {
    let definition = project.module_definitions.get_mut(&definition_id).unwrap();
    let progress_connection = definition
        .graph
        .connections
        .iter()
        .position(|connection| connection.to.port == TRANSITION_PROGRESS_INPUT_PORT)
        .map(|index| definition.graph.connections.remove(index))
        .expect("starter Progress connection");
    let parameter_id = PublishedParameterId::new();
    definition.interface.parameters.push(PublishedParameter {
        id: parameter_id,
        name: "Mix".to_string(),
        data_type: PortDataType::Number,
        default_value: PropertyValue::Number(OrderedFloat(0.0)),
        target: progress_connection.to,
    });
    definition.topology_revision += 1;
    definition.interface_version += 1;
    definition.validate().unwrap();
    parameter_id
}

fn has_group(item: &FrameItem, source_id: uuid::Uuid, kind: FrameGroupKind) -> bool {
    match item {
        FrameItem::Object(_) => false,
        FrameItem::Group(group) => {
            (group.source_id == source_id && group.kind == kind)
                || group
                    .items
                    .iter()
                    .any(|item| has_group(item, source_id, kind))
        }
        FrameItem::Transition(transition) => {
            has_group(&transition.from.item, source_id, kind)
                || has_group(&transition.to.item, source_id, kind)
        }
    }
}

#[test]
fn adjacent_cross_dissolve_renders_a_deterministic_linear_light_golden() {
    let (project, _, _, _) = adjacent_solid_project();
    assert_eq!(render_pixel(&project, 90), [255, 0, 0, 255]);
    let midpoint = render_pixel(&project, 150);
    assert!(
        (187..=189).contains(&midpoint[0]) && (187..=189).contains(&midpoint[2]),
        "linear-light red/blue midpoint must terminal-transform near 188: {midpoint:?}"
    );
    assert!(midpoint[1] <= 1);
    assert_eq!(midpoint[3], 255);
    assert_eq!(render_pixel(&project, 210), [0, 0, 255, 255]);
}

#[test]
fn transition_module_uses_hidden_sources_and_host_owned_progress() {
    let (mut project, transition_id, _, _) = adjacent_solid_project();
    promote_cross_dissolve_to_module(&mut project, transition_id);

    assert_eq!(render_pixel(&project, 90), [255, 0, 0, 255]);
    let midpoint = render_pixel(&project, 150);
    assert!(
        (187..=189).contains(&midpoint[0]) && (187..=189).contains(&midpoint[2]),
        "host Progress must override the instance's authored 1.0: {midpoint:?}"
    );
    assert!(midpoint[1] <= 1);
    assert_eq!(midpoint[3], 255);
    assert_eq!(render_pixel(&project, 210), [0, 0, 255, 255]);
}

#[test]
fn transition_module_runs_a_custom_blur_after_the_mix() {
    let (mut project, transition_id, _, _) = adjacent_solid_project();
    let definition_id = promote_cross_dissolve_to_module(&mut project, transition_id);
    let blur_id = append_blur_to_transition_module(&mut project, definition_id);
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let plugins = PluginManager::default();
    let frame = evaluate_render_plan_frame(&project, &plan, &plugins, 150, 1.0, None).unwrap();

    assert!(
        frame
            .items
            .iter()
            .any(|item| has_group(item, blur_id, FrameGroupKind::Effect))
    );
    let midpoint = render_pixel(&project, 150);
    assert!((187..=189).contains(&midpoint[0]));
    assert!((187..=189).contains(&midpoint[2]));
}

#[test]
fn transition_module_resolves_extra_image_input_by_published_id() {
    let (mut project, transition_id, _, _) = adjacent_solid_project();
    let definition_id = promote_cross_dissolve_to_module(&mut project, transition_id);
    let input_id = route_transition_output_from_public_input(&mut project, definition_id);
    let source_track = insert_visual_track(&mut project, "External");
    let source_id = insert_solid(
        &mut project,
        source_track,
        0,
        seconds(0),
        seconds(10),
        Color {
            r: 0,
            g: 255,
            b: 0,
            a: 255,
        },
    );
    project
        .transitions
        .get_mut(&transition_id)
        .unwrap()
        .processor
        .module_processor_mut()
        .unwrap()
        .input_bindings
        .insert(
            input_id,
            MediaInputBinding::TimelineItemOutput {
                locator: InstanceLocator::SameTimeline,
                item_id: source_id,
                output: MediaOutputKind::Image,
                stage: ItemOutputStage::PostTransform,
            },
        );
    project.validate().unwrap();

    assert_eq!(render_pixel(&project, 150), [0, 255, 0, 255]);
}

#[test]
fn transition_module_evaluates_parameter_automation_in_transition_local_time() {
    let (mut project, transition_id, _, _) = adjacent_solid_project();
    let definition_id = promote_cross_dissolve_to_module(&mut project, transition_id);
    let parameter_id = publish_transition_mix_progress(&mut project, definition_id);
    project.validate().unwrap();
    assert_eq!(render_pixel(&project, 150), [255, 0, 0, 255]);

    project
        .transitions
        .get_mut(&transition_id)
        .unwrap()
        .processor
        .module_processor_mut()
        .unwrap()
        .automation_tracks
        .insert(
            parameter_id,
            AutomationTrack {
                keyframes: vec![AutomationKeyframe::new(
                    MediaTime::zero(),
                    PropertyValue::Number(OrderedFloat(1.0)),
                    EasingFunction::Linear,
                )],
            },
        );
    project.validate().unwrap();

    assert_eq!(render_pixel(&project, 150), [0, 0, 255, 255]);
}

#[test]
fn nested_transition_controls_are_isolated_by_concrete_instance_path() {
    let (mut project, transition_id, _, _) = adjacent_solid_project();
    let nested_timeline_id = project.root_timeline_id;
    let definition_id = promote_cross_dissolve_to_module(&mut project, transition_id);
    let parameter_id = publish_transition_mix_progress(&mut project, definition_id);
    let module_instance_id = project.transitions[&transition_id]
        .processor
        .module_processor()
        .unwrap()
        .instance_id;

    let (root_timeline_id, first_item_id, second_item_id) =
        wrap_root_with_two_instances(&mut project, nested_timeline_id);
    project.validate().unwrap();
    let service = TimelineEditorService::new(project).unwrap();
    let first_path = InstancePath::root(root_timeline_id).nested(first_item_id);
    let second_path = InstancePath::root(root_timeline_id).nested(second_item_id);

    let changes = service
        .set_transition_module_instance_parameter(
            &first_path,
            transition_id,
            parameter_id,
            PropertyValue::Number(OrderedFloat(1.0)),
        )
        .unwrap();
    assert_eq!(
        changes.invalidations,
        vec![ProjectInvalidation::TimelineInstanceRange {
            instance_path: first_path.clone(),
            timeline_id: nested_timeline_id,
            transition_id,
            start: seconds(3),
            duration: seconds(4),
        }]
    );

    let project = service.snapshot().unwrap();
    assert!(
        project.module_instances[&module_instance_id]
            .parameter_overrides
            .is_empty(),
        "concrete edit must not mutate the shared Module instance"
    );
    let first_target = project
        .resolve_transition_module_instance_target(&first_path, transition_id)
        .unwrap();
    let second_target = project
        .resolve_transition_module_instance_target(&second_path, transition_id)
        .unwrap();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    assert_eq!(plan.module_definitions.len(), 1);
    assert_eq!(plan.module_invocations.len(), 1);
    assert_eq!(plan.transition_instance_controls.len(), 1);
    assert_eq!(
        plan.effective_transition_invocation(
            ModuleHost::Transition {
                timeline_id: nested_timeline_id,
                transition_id,
            },
            &first_path,
        )
        .unwrap()
        .parameter_overrides[&parameter_id],
        PropertyValue::Number(OrderedFloat(1.0))
    );
    assert!(
        !plan
            .effective_transition_invocation(
                ModuleHost::Transition {
                    timeline_id: nested_timeline_id,
                    transition_id,
                },
                &second_path,
            )
            .unwrap()
            .parameter_overrides
            .contains_key(&parameter_id)
    );
    let affected = plan
        .dependencies
        .affected_by_transition_instance(&first_target);
    assert!(affected.transition_instances.contains(&first_target));
    assert!(!affected.transition_instances.contains(&second_target));
    assert_eq!(affected.instance_ranges.len(), 1);

    assert_eq!(
        render_instance_pixel(&project, nested_timeline_id, &first_path, 150),
        [0, 0, 255, 255]
    );
    assert_eq!(
        render_instance_pixel(&project, nested_timeline_id, &second_path, 150),
        [255, 0, 0, 255]
    );

    drop(project);
    service
        .clear_transition_module_instance_parameter(&first_path, transition_id, parameter_id)
        .unwrap();
    service
        .set_transition_module_parameter_automation(
            transition_id,
            parameter_id,
            AutomationTrack::new(AutomationKeyframe::new(
                MediaTime::zero(),
                PropertyValue::Number(OrderedFloat(1.0)),
                EasingFunction::Linear,
            ))
            .unwrap(),
        )
        .unwrap();
    service
        .clear_transition_module_instance_parameter_automation(
            &first_path,
            transition_id,
            parameter_id,
        )
        .unwrap();
    let project = service.snapshot().unwrap();
    assert_eq!(
        render_instance_pixel(&project, nested_timeline_id, &first_path, 150),
        [255, 0, 0, 255],
        "explicit clear must suppress inherited automation"
    );
    assert_eq!(
        render_instance_pixel(&project, nested_timeline_id, &second_path, 150),
        [0, 0, 255, 255]
    );
    drop(project);
    service
        .inherit_transition_module_instance_parameter_automation(
            &first_path,
            transition_id,
            parameter_id,
        )
        .unwrap();
    let project = service.snapshot().unwrap();
    assert_eq!(
        render_instance_pixel(&project, nested_timeline_id, &first_path, 150),
        [0, 0, 255, 255],
        "inherit must remove the explicit clear mask"
    );
}

#[test]
fn nested_transition_media_binding_is_public_id_scoped_and_path_isolated() {
    let (mut project, transition_id, from_item_id, to_item_id) = adjacent_solid_project();
    let nested_timeline_id = project.root_timeline_id;
    project.items.get_mut(&from_item_id).unwrap().interval =
        TimelineInterval::new(seconds(0), seconds(7)).unwrap();
    project.items.get_mut(&to_item_id).unwrap().interval =
        TimelineInterval::new(seconds(3), seconds(7)).unwrap();
    let definition_id = promote_cross_dissolve_to_module(&mut project, transition_id);
    let input_id = route_transition_output_from_public_input(&mut project, definition_id);
    let contract = project.module_definitions[&definition_id]
        .host_contract
        .transition()
        .unwrap()
        .clone();
    project
        .transitions
        .get_mut(&transition_id)
        .unwrap()
        .processor
        .module_processor_mut()
        .unwrap()
        .input_bindings
        .insert(
            input_id,
            MediaInputBinding::TimelineItemOutput {
                locator: InstanceLocator::SameTimeline,
                item_id: from_item_id,
                output: MediaOutputKind::Image,
                stage: ItemOutputStage::PostTransform,
            },
        );
    let (root_timeline_id, first_item_id, second_item_id) =
        wrap_root_with_two_instances(&mut project, nested_timeline_id);
    project.validate().unwrap();
    let service = TimelineEditorService::new(project).unwrap();
    let first_path = InstancePath::root(root_timeline_id).nested(first_item_id);
    let second_path = InstancePath::root(root_timeline_id).nested(second_item_id);

    service
        .bind_transition_module_input_at_instance(
            &first_path,
            transition_id,
            input_id,
            MediaInputBinding::TimelineItemOutput {
                locator: InstanceLocator::SameTimeline,
                item_id: to_item_id,
                output: MediaOutputKind::Image,
                stage: ItemOutputStage::PostTransform,
            },
        )
        .unwrap();
    assert!(
        service
            .bind_transition_module_input_at_instance(
                &first_path,
                transition_id,
                contract.from_input_id,
                MediaInputBinding::TimelineItemOutput {
                    locator: InstanceLocator::SameTimeline,
                    item_id: to_item_id,
                    output: MediaOutputKind::Image,
                    stage: ItemOutputStage::PostTransform,
                },
            )
            .is_err(),
        "host-owned A/B inputs must not become placement bindings"
    );
    let project = service.snapshot().unwrap();
    let first_target = project
        .resolve_transition_module_instance_target(&first_path, transition_id)
        .unwrap();
    let second_target = project
        .resolve_transition_module_instance_target(&second_path, transition_id)
        .unwrap();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let affected = plan.dependencies.affected_by_item(to_item_id);
    assert!(affected.transition_instances.contains(&first_target));
    assert!(!affected.transition_instances.contains(&second_target));
    assert_eq!(
        render_instance_pixel(&project, nested_timeline_id, &first_path, 150),
        [0, 0, 255, 255]
    );
    assert_eq!(
        render_instance_pixel(&project, nested_timeline_id, &second_path, 150),
        [255, 0, 0, 255]
    );
    drop(project);

    service
        .inherit_transition_module_input_at_instance(&first_path, transition_id, input_id)
        .unwrap();
    let project = service.snapshot().unwrap();
    assert_eq!(
        render_instance_pixel(&project, nested_timeline_id, &first_path, 150),
        [255, 0, 0, 255],
        "inherit must restore the Timeline-definition binding"
    );
}

#[test]
fn cross_dissolve_respects_a_region_render_target() {
    let (project, _, _, _) = adjacent_solid_project();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let plugins = Arc::new(PluginManager::default());
    let frame = evaluate_render_plan_frame(
        &project,
        &plan,
        plugins.as_ref(),
        150,
        1.0,
        Some(crate::model::frame::frame::Region {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }),
    )
    .unwrap();
    let cache = Arc::new(CacheManager::new());
    let renderer =
        SkiaRenderer::new(1, 1, Color::black(), false, None, Some(cache.clone())).unwrap();
    let RenderOutput::Image(image) = RenderService::new(renderer, plugins, cache)
        .render_authoring_frame(&project, &frame, RenderDestination::Preview)
        .unwrap()
    else {
        panic!("region Preview must terminate to encoded pixels");
    };
    assert_eq!((image.width, image.height), (1, 1));
    assert!((187..=189).contains(&image.data[0]));
    assert!((187..=189).contains(&image.data[2]));
}

#[test]
fn exact_overlap_is_continuous_and_uses_the_adjacent_to_item_slot() {
    let (mut project, transition_id, from, to) = adjacent_solid_project();
    let timeline_id = project.root_timeline_id;
    let track_id = project.items[&from].track_id;
    project.items.get_mut(&from).unwrap().interval =
        TimelineInterval::new(seconds(0), seconds(7)).unwrap();
    project.items.get_mut(&to).unwrap().interval =
        TimelineInterval::new(seconds(3), seconds(7)).unwrap();
    let middle = insert_solid(
        &mut project,
        track_id,
        -1,
        seconds(0),
        seconds(10),
        Color {
            r: 0,
            g: 255,
            b: 0,
            a: 255,
        },
    );
    project.items.get_mut(&to).unwrap().layer = 1;
    project.validate().unwrap();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let compiled = &plan.timelines[&timeline_id];
    let transition = compiled
        .transitions
        .iter()
        .find(|candidate| candidate.id == transition_id)
        .unwrap();
    assert_eq!(
        transition.output_schedule_index,
        transition.to.schedule_index
    );
    assert!(transition.from.required_hidden_handle.is_empty());
    assert!(transition.to.required_hidden_handle.is_empty());

    let frame =
        evaluate_render_plan_frame(&project, &plan, &PluginManager::default(), 150, 1.0, None)
            .unwrap();
    let FrameItem::Group(composition) = &frame.items[0] else {
        panic!("root Timeline must remain hierarchical");
    };
    let FrameItem::Group(track) = &composition.items[0] else {
        panic!("Track must remain hierarchical");
    };
    assert_eq!(track.items.len(), 2);
    assert!(matches!(
        &track.items[0],
        FrameItem::Group(group) if group.source_id == middle.as_uuid()
    ));
    assert!(matches!(
        &track.items[1],
        FrameItem::Transition(transition)
            if transition.transition_id == transition_id.as_uuid()
                && transition.kind == FrameTransitionKind::CrossDissolve
                && (transition.progress.as_f32() - 0.5).abs()
                    <= 0.5 / f32::from(u16::MAX)
    ));
    assert_eq!(render_pixel(&project, 89), [255, 0, 0, 255]);
    assert_eq!(render_pixel(&project, 90), [255, 0, 0, 255]);
    assert_eq!(render_pixel(&project, 210), [0, 0, 255, 255]);
}

#[test]
fn missing_video_head_handle_is_a_typed_runtime_diagnostic() {
    let (mut project, transition_id, _, to) = adjacent_solid_project();
    let mut asset = Asset::new("missing head", "not-opened.mp4", AssetKind::Video);
    asset.duration = Some(10.0);
    asset.fps = Some(30.0);
    asset.frame_count = Some(300);
    let asset_id = asset.id;
    project.assets.push(asset);
    project.items.get_mut(&to).unwrap().source = SourceRef::Asset { asset_id };
    project.validate().unwrap();
    let plan = RenderPlanCompiler::compile(&project).unwrap();

    let error =
        evaluate_render_plan_frame(&project, &plan, &PluginManager::default(), 90, 1.0, None)
            .unwrap_err();
    assert!(matches!(
        error,
        LibraryError::TransitionSourceHandleUnavailable(ref detail)
            if detail.transition_id == transition_id.as_uuid()
                && detail.item_id == to.as_uuid()
                && (detail.source_time + 2.0).abs() < f64::EPSILON
    ));
}

#[test]
fn decoder_eof_during_cross_dissolve_keeps_transition_source_context() {
    let (mut project, transition_id, from, _) = adjacent_solid_project();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data/e2e_media/h264_24.mp4")
        .to_string_lossy()
        .into_owned();
    let asset = Asset::new("short video", &path, AssetKind::Video);
    let asset_id = asset.id;
    project.assets.push(asset);
    project.items.get_mut(&from).unwrap().source = SourceRef::Asset { asset_id };
    project.validate().unwrap();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let plugins = Arc::new(PluginManager::default());
    let frame =
        evaluate_render_plan_frame(&project, &plan, plugins.as_ref(), 180, 1.0, None).unwrap();
    let cache = Arc::new(CacheManager::new());
    let renderer =
        SkiaRenderer::new(2, 2, Color::black(), false, None, Some(cache.clone())).unwrap();
    let error = RenderService::new(renderer, plugins, cache)
        .render_authoring_frame(&project, &frame, RenderDestination::Preview)
        .unwrap_err();

    assert!(matches!(
        error,
        LibraryError::TransitionSourceHandleUnavailable(ref detail)
            if detail.transition_id == transition_id.as_uuid()
                && detail.item_id == from.as_uuid()
                && (detail.timeline_time - 6.0).abs() < f64::EPSILON
                && (detail.source_time - 6.0).abs() < f64::EPSILON
    ));
}
