use std::collections::HashMap;
use std::sync::Arc;

use super::*;
use crate::cache::CacheManager;
use crate::core::render_plan::{RenderPlanCache, RenderPlanCompiler};
use crate::editor::{RenderDestination, RenderService};
use crate::model::authoring::{
    ModuleDefinition, ModuleDefinitionSharing, ModuleInstance, ModuleInstanceId, RationalRate,
    TimeMap, TimelineInterval, Transition, TransitionAlignment, TransitionId, TransitionMediaType,
    TransitionProcessor,
};
use crate::model::frame::color::Color;
use crate::model::frame::entity::FrameItem;
use crate::model::project::property::PropertyMap;
use crate::plugin::PluginManager;
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_renderer::SkiaRenderer;

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).expect("whole seconds")
}

fn insert_solid(
    project: &mut AuthoringProject,
    track_id: crate::model::authoring::TimelineTrackId,
    layer: i64,
    start: MediaTime,
    duration: MediaTime,
    color: Color,
    blend_mode: BlendMode,
) -> TimelineItemId {
    let id = TimelineItemId::new();
    project.items.insert(
        id,
        TimelineItem {
            id,
            track_id,
            name: format!("Solid {layer}"),
            source: SourceRef::Solid { color },
            interval: TimelineInterval::new(start, duration).expect("valid interval"),
            time_map: TimeMap::default(),
            layer,
            parent: None,
            blend_mode,
            authored_properties: PropertyMap::new(),
        },
    );
    id
}

fn transition_over_green(
    from_blend_mode: BlendMode,
    to_blend_mode: BlendMode,
) -> (AuthoringProject, TransitionId) {
    let mut project = AuthoringProject::new(
        "Transition output blend",
        2,
        2,
        RationalRate::new(30, 1).expect("valid frame rate"),
        seconds(10),
    )
    .expect("valid Project");
    let timeline_id = project.root_timeline_id;
    let track_id = project.timelines[&timeline_id].track_order[0];
    insert_solid(
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
        BlendMode::Normal,
    );
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
        from_blend_mode,
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
        to_blend_mode,
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
    project.validate().expect("valid Transition Project");
    (project, transition_id)
}

fn promote_to_image_module(project: &mut AuthoringProject, transition_id: TransitionId) {
    let (definition, _) = ModuleDefinition::new_transition(
        "Blend-aware Transition",
        ModuleDefinitionSharing::Private,
        TransitionMediaType::Image,
    )
    .expect("create starter Transition Module");
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
        .expect("Transition")
        .processor = TransitionProcessor::module(instance_id, TransitionMediaType::Image);
    project.validate().expect("valid promoted Transition");
}

fn midpoint_frame(project: &AuthoringProject) -> FrameInfo {
    let plan = RenderPlanCompiler::compile(project).expect("compile RenderPlan");
    evaluate_render_plan_frame(project, &plan, &PluginManager::default(), 150, 1.0, None)
        .expect("evaluate midpoint")
}

fn first_transition(items: &[FrameItem]) -> Option<&crate::model::frame::entity::FrameTransition> {
    items.iter().find_map(|item| match item {
        FrameItem::Transition(transition) => Some(transition.as_ref()),
        FrameItem::Group(group) => first_transition(&group.items),
        FrameItem::Object(_) => None,
    })
}

fn render_midpoint(project: &AuthoringProject) -> [u8; 4] {
    let frame = midpoint_frame(project);
    let plugins = Arc::new(PluginManager::default());
    let cache = Arc::new(CacheManager::new());
    let renderer = SkiaRenderer::new(2, 2, Color::black(), false, None, Some(cache.clone()))
        .expect("create renderer");
    let RenderOutput::Image(image) = RenderService::new(renderer, plugins, cache)
        .render_authoring_frame(project, &frame, RenderDestination::Preview)
        .expect("render midpoint")
    else {
        panic!("Preview must terminate to encoded pixels");
    };
    image.data[..4].try_into().expect("one RGBA pixel")
}

fn assert_pixel_near(actual: [u8; 4], expected: [u8; 4]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            actual.abs_diff(expected) <= 2,
            "pixel {actual:?} differs from {expected:?}"
        );
    }
}

#[test]
fn transition_inputs_are_neutral_and_to_placement_owns_output_blend() {
    let (project, transition_id) = transition_over_green(BlendMode::Screen, BlendMode::Multiply);
    let plan = RenderPlanCompiler::compile(&project).expect("compile RenderPlan");
    let compiled = plan.timelines[&project.root_timeline_id]
        .transitions
        .iter()
        .find(|transition| transition.id == transition_id)
        .expect("compiled Transition");
    assert_eq!(compiled.output_blend_mode, BlendMode::Multiply);

    let frame = midpoint_frame(&project);
    let transition = first_transition(&frame.items).expect("evaluated Transition");
    assert_eq!(transition.blend_mode, BlendMode::Multiply);
    for source in [&transition.from, &transition.to] {
        assert!(matches!(
            &source.item,
            FrameItem::Group(group) if group.blend_mode == BlendMode::Normal
        ));
    }
}

#[test]
fn transition_output_blend_matches_normal_multiply_and_screen_goldens() {
    for (mode, expected) in [
        (BlendMode::Normal, [188, 0, 188, 255]),
        (BlendMode::Multiply, [0, 0, 0, 255]),
        (BlendMode::Screen, [188, 255, 188, 255]),
    ] {
        // Deliberately disagree with A: only B owns the output schedule slot.
        let (project, _) = transition_over_green(BlendMode::Screen, mode);
        assert_pixel_near(render_midpoint(&project), expected);
    }
}

#[test]
fn transition_module_applies_placement_blend_outside_its_node_topology() {
    let (mut project, transition_id) =
        transition_over_green(BlendMode::Multiply, BlendMode::Screen);
    promote_to_image_module(&mut project, transition_id);

    let frame = midpoint_frame(&project);
    let transition = first_transition(&frame.items).expect("internal Mix operation");
    assert_eq!(transition.blend_mode, BlendMode::Normal);
    assert!(matches!(
        &frame.items[0],
        FrameItem::Group(composition)
            if matches!(
                &composition.items[0],
                FrameItem::Group(track)
                    if matches!(
                        &track.items[1],
                        FrameItem::Group(output)
                            if output.kind == crate::model::frame::entity::FrameGroupKind::TransitionOutput
                                && output.blend_mode == BlendMode::Screen
                    )
            )
    ));
    assert_pixel_near(render_midpoint(&project), [188, 255, 188, 255]);
}

#[test]
fn changing_to_blend_recompiles_only_the_owning_timeline_plan() {
    let (mut project, transition_id) =
        transition_over_green(BlendMode::Normal, BlendMode::Multiply);
    let mut cache = RenderPlanCache::default();
    let (initial, initial_stats) = cache.compile(&project).expect("initial compile");
    assert_eq!(initial_stats.compiled_timelines, 1);
    assert_eq!(
        initial.timelines[&project.root_timeline_id].transitions[0].output_blend_mode,
        BlendMode::Multiply
    );

    let to_item_id = project.transitions[&transition_id].to_item_id;
    project
        .items
        .get_mut(&to_item_id)
        .expect("to placement")
        .blend_mode = BlendMode::Screen;
    let (changed, changed_stats) = cache.compile(&project).expect("changed compile");
    assert_eq!(changed_stats.compiled_timelines, 1);
    assert_eq!(changed_stats.reused_timelines, 0);
    assert_eq!(
        changed.timelines[&project.root_timeline_id].transitions[0].output_blend_mode,
        BlendMode::Screen
    );
}
