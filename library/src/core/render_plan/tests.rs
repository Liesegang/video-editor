use std::collections::HashMap;

use ordered_float::OrderedFloat;

use crate::model::authoring::*;
use crate::model::frame::color::Color;
use crate::model::project::property::PropertyMap;

use super::*;

fn timeline(id: TimelineId, track_id: TimelineTrackId) -> Timeline {
    Timeline {
        id,
        name: "Main".to_string(),
        width: 1920,
        height: 1080,
        fps: OrderedFloat(30.0),
        duration: OrderedFloat(10.0),
        background_color: Color::black(),
        track_order: vec![track_id],
        authored_properties: PropertyMap::new(),
    }
}

fn project_with_items(
    timeline_id: TimelineId,
    track_id: TimelineTrackId,
    items: HashMap<TimelineItemId, TimelineItem>,
) -> AuthoringProject {
    AuthoringProject {
        name: "RenderPlan".to_string(),
        root_timeline_id: timeline_id,
        timelines: HashMap::from([(timeline_id, timeline(timeline_id, track_id))]),
        tracks: HashMap::from([(
            track_id,
            TimelineTrack {
                id: track_id,
                timeline_id,
                name: "Video".to_string(),
                kind: TimelineTrackKind::Visual,
                authored_properties: PropertyMap::new(),
            },
        )]),
        items,
        module_definitions: HashMap::new(),
        module_instances: HashMap::new(),
        attachments: HashMap::new(),
        signal_bindings: HashMap::new(),
        event_bindings: HashMap::new(),
        data_sources: HashMap::new(),
        generated_items: HashMap::new(),
        overrides: HashMap::new(),
        assets: Vec::new(),
    }
}

#[test]
fn compiler_orders_schedule_without_creating_nodes() {
    let timeline_id = TimelineId::new();
    let track_id = TimelineTrackId::new();
    let first = TimelineItemId::new();
    let second = TimelineItemId::new();
    let make_item = |id, start, layer| TimelineItem {
        id,
        track_id,
        name: "Text".to_string(),
        source: SourceRef::Text {
            text: "Hello".to_string(),
        },
        interval: TimelineInterval::new(start, 1.0).expect("valid interval"),
        layer,
        parent: None,
        authored_properties: PropertyMap::new(),
    };
    let project = project_with_items(
        timeline_id,
        track_id,
        HashMap::from([
            (first, make_item(first, 2.0, 1)),
            (second, make_item(second, 1.0, 0)),
        ]),
    );
    let plan = RenderPlanCompiler::compile(&project).expect("plan must compile");
    let schedule = &plan.timelines[&timeline_id].schedule;
    assert_eq!(schedule.len(), 2);
    assert_eq!(schedule[0].item_id, second);
    assert!(plan.module_definitions.is_empty());
    assert!(plan.module_invocations.is_empty());
}

#[test]
fn nested_timeline_cycles_are_rejected() {
    let first_timeline = TimelineId::new();
    let second_timeline = TimelineId::new();
    let first_track = TimelineTrackId::new();
    let second_track = TimelineTrackId::new();
    let first_item = TimelineItemId::new();
    let second_item = TimelineItemId::new();
    let composition_item = |id, track, nested| TimelineItem {
        id,
        track_id: track,
        name: "Nested".to_string(),
        source: SourceRef::Composition(CompositionInstance {
            timeline_id: nested,
            time_map: TimeMap::default(),
            duration_policy: DurationPolicy::Fixed,
            parameter_overrides: HashMap::new(),
        }),
        interval: TimelineInterval::new(0.0, 1.0).expect("valid interval"),
        layer: 0,
        parent: None,
        authored_properties: PropertyMap::new(),
    };
    let mut project = project_with_items(
        first_timeline,
        first_track,
        HashMap::from([(
            first_item,
            composition_item(first_item, first_track, second_timeline),
        )]),
    );
    project
        .timelines
        .insert(second_timeline, timeline(second_timeline, second_track));
    project.tracks.insert(
        second_track,
        TimelineTrack {
            id: second_track,
            timeline_id: second_timeline,
            name: "Nested".to_string(),
            kind: TimelineTrackKind::Visual,
            authored_properties: PropertyMap::new(),
        },
    );
    project.items.insert(
        second_item,
        composition_item(second_item, second_track, first_timeline),
    );
    assert!(RenderPlanCompiler::compile(&project).is_err());
}

#[test]
fn repeated_module_instances_share_one_compiled_definition() {
    let timeline_id = TimelineId::new();
    let track_id = TimelineTrackId::new();
    let definition_id = ModuleDefinitionId::new();
    let mut items = HashMap::new();
    let mut instances = HashMap::new();
    for index in 0..100 {
        let item_id = TimelineItemId::new();
        let instance_id = ModuleInstanceId::new();
        items.insert(
            item_id,
            TimelineItem {
                id: item_id,
                track_id,
                name: format!("Module {index}"),
                source: SourceRef::Module {
                    module_instance_id: instance_id,
                },
                interval: TimelineInterval::new(index as f64, 1.0).expect("valid interval"),
                layer: 0,
                parent: None,
                authored_properties: PropertyMap::new(),
            },
        );
        instances.insert(
            instance_id,
            ModuleInstance {
                id: instance_id,
                definition_id,
                parameter_overrides: HashMap::new(),
            },
        );
    }
    let mut project = project_with_items(timeline_id, track_id, items);
    project.module_definitions.insert(
        definition_id,
        ModuleDefinition {
            id: definition_id,
            name: "Lower Third".to_string(),
            graph: ModuleGraph::default(),
            published_parameters: Vec::new(),
            published_signals: Vec::new(),
            published_actions: Vec::new(),
            version: 1,
        },
    );
    project.module_instances = instances;

    let plan = RenderPlanCompiler::compile(&project).expect("plan must compile");
    assert_eq!(plan.module_definitions.len(), 1);
    assert_eq!(plan.module_invocations.len(), 100);
    assert_eq!(
        plan.dependencies.definition_invocations[&definition_id].len(),
        100
    );
}

#[test]
fn instance_parameter_change_reuses_compiled_definition() {
    let timeline_id = TimelineId::new();
    let track_id = TimelineTrackId::new();
    let definition_id = ModuleDefinitionId::new();
    let instance_id = ModuleInstanceId::new();
    let item_id = TimelineItemId::new();
    let mut project = project_with_items(
        timeline_id,
        track_id,
        HashMap::from([(
            item_id,
            TimelineItem {
                id: item_id,
                track_id,
                name: "Generator".to_string(),
                source: SourceRef::Module {
                    module_instance_id: instance_id,
                },
                interval: TimelineInterval::new(0.0, 1.0).expect("valid interval"),
                layer: 0,
                parent: None,
                authored_properties: PropertyMap::new(),
            },
        )]),
    );
    project.module_definitions.insert(
        definition_id,
        ModuleDefinition {
            id: definition_id,
            name: "Generator".to_string(),
            graph: ModuleGraph::default(),
            published_parameters: Vec::new(),
            published_signals: Vec::new(),
            published_actions: Vec::new(),
            version: 1,
        },
    );
    project.module_instances.insert(
        instance_id,
        ModuleInstance {
            id: instance_id,
            definition_id,
            parameter_overrides: HashMap::new(),
        },
    );
    let mut cache = RenderPlanCache::default();
    let (_, first) = cache.compile(&project).expect("first compile");
    assert_eq!(first.compiled_definitions, 1);

    project
        .module_instances
        .get_mut(&instance_id)
        .expect("instance")
        .parameter_overrides
        .insert(
            PublishedParameterId::new(),
            crate::model::project::property::PropertyValue::Integer(42),
        );
    let (_, second) = cache.compile(&project).expect("incremental compile");
    assert_eq!(second.compiled_definitions, 0);
    assert_eq!(second.reused_definitions, 1);
}
