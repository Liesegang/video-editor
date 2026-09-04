use std::collections::HashMap;

use super::*;
use crate::core::render_plan::RenderPlanCompiler;
use crate::editor::TimelineEditorService;
use crate::model::authoring::{
    CompositionParameterTarget, ModuleDefinition, ModuleDefinitionSharing, ModuleInstance,
    ModuleInstanceId, ModuleInvocation, PublishedMediaInput, PublishedMediaInputId, RationalRate,
    TimeMap, TimelineInterval, TimelineTrack, TimelineTrackId,
};
use crate::model::frame::color::Color;
use crate::model::frame::entity::{FrameContent, FrameItem};
use crate::model::project::connection::PortDataType;
use crate::model::project::property::{Property, PropertyMap, PropertyValue, Vec2};

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).expect("whole seconds")
}

fn object_count(items: &[FrameItem]) -> usize {
    items
        .iter()
        .map(|item| match item {
            FrameItem::Object(_) => 1,
            FrameItem::Group(group) => object_count(&group.items),
        })
        .sum()
}

struct RepeatedNestedFixture {
    project: AuthoringProject,
    child_timeline_id: TimelineId,
    first_path: InstancePath,
    second_path: InstancePath,
}

fn repeated_nested_fixture() -> RepeatedNestedFixture {
    let mut project = AuthoringProject::new(
        "repeated nested instance",
        320,
        180,
        RationalRate::new(30, 1).unwrap(),
        seconds(20),
    )
    .unwrap();
    let root_timeline_id = project.root_timeline_id;
    let root_track_id = project.timelines[&root_timeline_id].track_order[0];

    let child_timeline_id = TimelineId::new();
    let child_track_id = TimelineTrackId::new();
    project.timelines.insert(
        child_timeline_id,
        Timeline {
            id: child_timeline_id,
            name: "Lower Third".to_string(),
            width: 320,
            height: 180,
            fps: RationalRate::new(30, 1).unwrap(),
            duration: seconds(4),
            background_color: Color::black(),
            color_profile: "sRGB".to_string(),
            track_order: vec![child_track_id],
            authored_properties: PropertyMap::new(),
            published_parameters: Vec::new(),
        },
    );
    project.tracks.insert(
        child_track_id,
        TimelineTrack {
            id: child_track_id,
            timeline_id: child_timeline_id,
            name: "Child video".to_string(),
            kind: TimelineTrackKind::Visual,
            authored_properties: PropertyMap::new(),
        },
    );

    // This source exists only while the second Composition placement is active.
    let root_source_id = TimelineItemId::new();
    project.items.insert(
        root_source_id,
        TimelineItem {
            id: root_source_id,
            track_id: root_track_id,
            name: "Root source".to_string(),
            source: SourceRef::Solid {
                color: Color::white(),
            },
            interval: TimelineInterval::new(seconds(10), seconds(2)).unwrap(),
            time_map: TimeMap::default(),
            layer: 0,
            parent: None,
            blend_mode: BlendMode::Normal,
            authored_properties: PropertyMap::new(),
        },
    );

    let (mut definition, output_id) =
        ModuleDefinition::new_image("Root input adapter", ModuleDefinitionSharing::Private);
    let definition_id = definition.id;
    let instance_id = ModuleInstanceId::new();
    let input_id = PublishedMediaInputId::new();
    let output_target = definition
        .output(output_id)
        .unwrap()
        .target(PortDataType::Image)
        .unwrap();
    definition.interface.media_inputs.push(PublishedMediaInput {
        id: input_id,
        name: "Root image".to_string(),
        data_type: PortDataType::Image,
        target: output_target,
        required: true,
        primary: false,
    });
    project.module_definitions.insert(definition_id, definition);
    project.module_instances.insert(
        instance_id,
        ModuleInstance {
            id: instance_id,
            definition_id,
            parameter_overrides: HashMap::new(),
        },
    );
    let node_clip_id = TimelineItemId::new();
    project.items.insert(
        node_clip_id,
        TimelineItem {
            id: node_clip_id,
            track_id: child_track_id,
            name: "Bound Node Clip".to_string(),
            source: SourceRef::Module(ModuleInvocation {
                instance_id,
                output_id,
                input_bindings: HashMap::from([(
                    input_id,
                    MediaInputBinding::TimelineItemOutput {
                        locator: InstanceLocator::Exact(InstancePath::root(root_timeline_id)),
                        item_id: root_source_id,
                        output: MediaOutputKind::Image,
                        stage: ItemOutputStage::PostTransform,
                    },
                )]),
                automation_tracks: HashMap::new(),
            }),
            interval: TimelineInterval::new(seconds(0), seconds(4)).unwrap(),
            time_map: TimeMap::default(),
            layer: 0,
            parent: None,
            blend_mode: BlendMode::Normal,
            authored_properties: PropertyMap::new(),
        },
    );

    let mut placement_ids = Vec::new();
    for (layer, start) in [(1, 2), (2, 10)] {
        let item_id = TimelineItemId::new();
        project.items.insert(
            item_id,
            TimelineItem {
                id: item_id,
                track_id: root_track_id,
                name: format!("Placement {layer}"),
                source: SourceRef::Composition(crate::model::authoring::CompositionInstance {
                    timeline_id: child_timeline_id,
                    duration_policy: DurationPolicy::Fixed,
                    parameter_overrides: HashMap::new(),
                }),
                interval: TimelineInterval::new(seconds(start), seconds(4)).unwrap(),
                time_map: TimeMap::default(),
                layer,
                parent: None,
                blend_mode: BlendMode::Normal,
                authored_properties: PropertyMap::new(),
            },
        );
        placement_ids.push(item_id);
    }
    project.validate().unwrap();

    RepeatedNestedFixture {
        project,
        child_timeline_id,
        first_path: InstancePath::root(root_timeline_id).nested(placement_ids[0]),
        second_path: InstancePath::root(root_timeline_id).nested(placement_ids[1]),
    }
}

#[test]
fn repeated_composition_paths_map_the_same_local_time_to_distinct_root_times() {
    let fixture = repeated_nested_fixture();

    assert_eq!(
        root_time_for_instance_local_time(&fixture.project, &fixture.first_path, seconds(1))
            .unwrap(),
        (fixture.child_timeline_id, seconds(3))
    );
    assert_eq!(
        root_time_for_instance_local_time(&fixture.project, &fixture.second_path, seconds(1))
            .unwrap(),
        (fixture.child_timeline_id, seconds(11))
    );
}

#[test]
fn exact_binding_resolves_against_the_selected_composition_instance_path() {
    let fixture = repeated_nested_fixture();
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();

    let first = evaluate_timeline_render_plan_frame_at_instance(
        &fixture.project,
        &plan,
        &crate::plugin::PluginManager::default(),
        fixture.child_timeline_id,
        30,
        1.0,
        None,
        Some(&fixture.first_path),
    )
    .unwrap();
    let second = evaluate_timeline_render_plan_frame_at_instance(
        &fixture.project,
        &plan,
        &crate::plugin::PluginManager::default(),
        fixture.child_timeline_id,
        30,
        1.0,
        None,
        Some(&fixture.second_path),
    )
    .unwrap();

    assert_eq!(object_count(&first.items), 0);
    assert_eq!(object_count(&second.items), 1);
}

fn first_text(items: &[FrameItem]) -> Option<&str> {
    for item in items {
        match item {
            FrameItem::Object(object) => {
                if let FrameContent::Text { text, .. } = &object.content {
                    return Some(text);
                }
            }
            FrameItem::Group(group) => {
                if let Some(text) = first_text(&group.items) {
                    return Some(text);
                }
            }
        }
    }
    None
}

fn item_position(items: &[FrameItem], item_id: TimelineItemId) -> Option<(f64, f64)> {
    for item in items {
        if let FrameItem::Group(group) = item {
            if group.kind == FrameGroupKind::Clip && group.source_id == item_id.as_uuid() {
                return Some((group.transform.position.x, group.transform.position.y));
            }
            if let Some(position) = item_position(&group.items, item_id) {
                return Some(position);
            }
        }
    }
    None
}

#[test]
fn published_composition_values_are_owned_by_each_concrete_instance_path() {
    let service = TimelineEditorService::create_default("Composition parameters").unwrap();
    let (child_timeline_id, child_track_id, _) = service
        .add_timeline(
            "Lower Third".to_string(),
            320,
            180,
            RationalRate::new(30, 1).unwrap(),
            seconds(4),
        )
        .unwrap();
    let base_position = PropertyValue::Vec2(Vec2 {
        x: OrderedFloat(0.0),
        y: OrderedFloat(0.0),
    });
    let (text_item_id, _) = service
        .add_item(
            child_track_id,
            "Title".to_string(),
            SourceRef::Text {
                text: "Definition title".to_string(),
                ensemble_operations: Vec::new(),
            },
            TimelineInterval::new(seconds(0), seconds(4)).unwrap(),
            0,
        )
        .unwrap();
    service
        .set_authored_property(
            crate::editor::AuthoringPropertyOwner::Item(text_item_id),
            "position".to_string(),
            Property::constant(base_position.clone()),
        )
        .unwrap();
    let (text_parameter_id, _) = service
        .publish_composition_parameter(
            child_timeline_id,
            "Title".to_string(),
            CompositionParameterTarget::TextContent {
                item_id: text_item_id,
            },
            PropertyValue::String("Definition title".to_string()),
        )
        .unwrap();
    let (position_parameter_id, _) = service
        .publish_composition_parameter(
            child_timeline_id,
            "Position".to_string(),
            CompositionParameterTarget::ItemProperty {
                item_id: text_item_id,
                property_key: "position".to_string(),
            },
            base_position.clone(),
        )
        .unwrap();

    let root = service.snapshot().unwrap();
    let root_timeline_id = root.root_timeline_id;
    let root_track_id = root.timelines[&root_timeline_id].track_order[0];
    drop(root);
    let mut placements = Vec::new();
    for (name, start, text, x) in [
        ("First", 0, "First title", 20.0),
        ("Second", 5, "Second title", -30.0),
    ] {
        let (placement_id, _) = service
            .add_item(
                root_track_id,
                name.to_string(),
                SourceRef::Composition(crate::model::authoring::CompositionInstance {
                    timeline_id: child_timeline_id,
                    duration_policy: DurationPolicy::Fixed,
                    parameter_overrides: HashMap::new(),
                }),
                TimelineInterval::new(seconds(start), seconds(4)).unwrap(),
                i64::try_from(placements.len()).unwrap(),
            )
            .unwrap();
        service
            .set_composition_parameter_override(
                placement_id,
                text_parameter_id,
                PropertyValue::String(text.to_string()),
            )
            .unwrap();
        service
            .set_composition_parameter_override(
                placement_id,
                position_parameter_id,
                PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(x),
                    y: OrderedFloat(12.0),
                }),
            )
            .unwrap();
        placements.push(placement_id);
    }

    let project = service.snapshot().unwrap();
    let SourceRef::Text { text, .. } = &project.items[&text_item_id].source else {
        panic!("definition target must remain Text");
    };
    assert_eq!(text, "Definition title");
    assert_eq!(
        project.items[&text_item_id]
            .authored_properties
            .get("position")
            .and_then(|property| property.value()),
        Some(&base_position)
    );
    let first_instance = match &project.items[&placements[0]].source {
        SourceRef::Composition(instance) => instance,
        _ => panic!("first placement must remain a Composition"),
    };
    let second_instance = match &project.items[&placements[1]].source {
        SourceRef::Composition(instance) => instance,
        _ => panic!("second placement must remain a Composition"),
    };
    assert_eq!(
        first_instance.parameter_overrides[&text_parameter_id],
        PropertyValue::String("First title".to_string())
    );
    assert_eq!(
        second_instance.parameter_overrides[&text_parameter_id],
        PropertyValue::String("Second title".to_string())
    );

    let plan = RenderPlanCompiler::compile(&project).unwrap();
    for (placement_id, expected_text, expected_x) in [
        (placements[0], "First title", 20.0),
        (placements[1], "Second title", -30.0),
    ] {
        let path = InstancePath::root(root_timeline_id).nested(placement_id);
        let frame = evaluate_timeline_render_plan_frame_at_instance(
            &project,
            &plan,
            &crate::plugin::PluginManager::default(),
            child_timeline_id,
            0,
            1.0,
            None,
            Some(&path),
        )
        .unwrap();
        assert_eq!(first_text(&frame.items), Some(expected_text));
        assert_eq!(
            item_position(&frame.items, text_item_id),
            Some((expected_x, 12.0))
        );
    }

    let updated_definition_position = PropertyValue::Vec2(Vec2 {
        x: OrderedFloat(44.0),
        y: OrderedFloat(-8.0),
    });
    service
        .set_text(text_item_id, "Updated definition".to_string())
        .unwrap();
    service
        .set_authored_property_constant(
            crate::editor::AuthoringPropertyOwner::Item(text_item_id),
            "position".to_string(),
            updated_definition_position,
        )
        .unwrap();
    service
        .clear_composition_parameter_override(placements[0], text_parameter_id)
        .unwrap();
    service
        .clear_composition_parameter_override(placements[0], position_parameter_id)
        .unwrap();

    let updated_project = service.snapshot().unwrap();
    let updated_plan = RenderPlanCompiler::compile(&updated_project).unwrap();
    let first = evaluate_timeline_render_plan_frame_at_instance(
        &updated_project,
        &updated_plan,
        &crate::plugin::PluginManager::default(),
        child_timeline_id,
        0,
        1.0,
        None,
        Some(&InstancePath::root(root_timeline_id).nested(placements[0])),
    )
    .unwrap();
    let second = evaluate_timeline_render_plan_frame_at_instance(
        &updated_project,
        &updated_plan,
        &crate::plugin::PluginManager::default(),
        child_timeline_id,
        0,
        1.0,
        None,
        Some(&InstancePath::root(root_timeline_id).nested(placements[1])),
    )
    .unwrap();

    assert_eq!(first_text(&first.items), Some("Updated definition"));
    assert_eq!(
        item_position(&first.items, text_item_id),
        Some((44.0, -8.0))
    );
    assert_eq!(first_text(&second.items), Some("Second title"));
    assert_eq!(
        item_position(&second.items, text_item_id),
        Some((-30.0, 12.0))
    );
}
