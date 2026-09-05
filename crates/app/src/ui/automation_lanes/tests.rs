use std::collections::HashMap;
use std::path::Path;

use library::editor::{
    AuthoringPropertyOwner, ParticleNodeClipPlacement, TextEnsembleOperationKind,
    TimelineEditorService,
};
use library::model::authoring::{
    InstancePath, MediaTime, ModuleDefinition, ModuleDefinitionSharing, ModuleInstance,
    ModuleInstanceId, ModuleInvocation, PublishedParameterId, SourceRef, TimelineInterval,
    TimelineItem, TimelineItemId, Transition, TransitionAlignment, TransitionId,
    TransitionProcessor,
};
use library::model::frame::color::Color;
use library::model::property::PropertyValue;
use library::model::Node;
use library::plugin::PluginManager;

use super::*;

#[test]
fn authored_and_empty_published_lanes_share_one_discovery_contract() {
    let service = TimelineEditorService::create_default("lanes").expect("service");
    let project = service.snapshot().expect("project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let (item_id, _) = service
        .add_item(
            track_id,
            "Solid".to_string(),
            SourceRef::Solid {
                color: Color::black(),
            },
            TimelineInterval::new(MediaTime::zero(), MediaTime::new(5, 1).unwrap()).unwrap(),
            0,
        )
        .unwrap();
    service
        .set_authored_property_constant(
            AuthoringPropertyOwner::Item(item_id),
            "position".to_string(),
            PropertyValue::from(2.0),
        )
        .unwrap();
    let project = service.snapshot().unwrap();
    let authored = collect_item_lanes(&project, item_id);
    assert_eq!(authored.len(), 1);
    assert_eq!(authored[0].label, "Position");
    assert!(authored[0].points.is_empty());
    assert!(collect_item_keyframed_lanes(&project, item_id).is_empty());

    let (mut definition, output_id) =
        ModuleDefinition::new_image("Module", ModuleDefinitionSharing::Private);
    let numeric = Node::new_add("Amount");
    let numeric_id = numeric.id;
    definition.graph.nodes.insert(numeric_id, numeric);
    let parameter_id = PublishedParameterId::new();
    definition
        .interface
        .parameters
        .push(library::model::authoring::PublishedParameter {
            id: parameter_id,
            name: "Amount".to_string(),
            data_type: library::model::project::PortDataType::Number,
            default_value: PropertyValue::from(1.0),
            target: library::model::authoring::ModulePortAddress {
                node_id: numeric_id,
                port: library::model::project::NUMERIC_A_INPUT_PORT.to_string(),
            },
        });
    let definition_id = definition.id;
    let instance_id = ModuleInstanceId::new();
    let module_item = TimelineItemId::new();
    let mut project = (*service.snapshot().unwrap()).clone();
    project.module_definitions.insert(definition_id, definition);
    project.module_instances.insert(
        instance_id,
        ModuleInstance {
            id: instance_id,
            definition_id,
            parameter_overrides: HashMap::new(),
        },
    );
    project.items.insert(
        module_item,
        TimelineItem {
            id: module_item,
            track_id,
            name: "Module".to_string(),
            source: SourceRef::Module(ModuleInvocation {
                instance_id,
                output_id,
                input_bindings: HashMap::new(),
                automation_tracks: HashMap::new(),
            }),
            interval: TimelineInterval::new(MediaTime::zero(), MediaTime::new(5, 1).unwrap())
                .unwrap(),
            time_map: Default::default(),
            layer: 1,
            parent: None,
            blend_mode: library::model::BlendMode::Normal,
            authored_properties: Default::default(),
        },
    );
    let module = collect_item_lanes(&project, module_item);
    assert_eq!(module.len(), 1);
    assert_eq!(
        module[0].id.target,
        AutomationTarget::ModuleParameter(parameter_id)
    );
    assert!(module[0].points.is_empty());
    assert!(collect_item_keyframed_lanes(&project, module_item).is_empty());
}

#[test]
fn constant_only_particle_parameters_are_not_advertised_as_automation_lanes() {
    let service = TimelineEditorService::create_default("particle lanes").expect("service");
    let project = service.snapshot().expect("project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let created = service
        .create_particle_node_clip(ParticleNodeClipPlacement {
            track_id,
            name: "Particle".to_string(),
            interval: TimelineInterval::new(MediaTime::zero(), MediaTime::new(5, 1).unwrap())
                .unwrap(),
            layer: 0,
        })
        .expect("Particle Node Clip");

    let project = service.snapshot().expect("Particle project");
    let lanes = collect_item_lanes(&project, created.item_id);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].label, "Color");
    assert_eq!(
        lanes[0].id.target,
        AutomationTarget::ModuleParameter(created.parameters.color)
    );
}

#[test]
fn local_and_timeline_time_round_trip_through_item_time_map() {
    let service = TimelineEditorService::create_default("time").unwrap();
    let project = service.snapshot().unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let (item_id, _) = service
        .add_item(
            track_id,
            "Solid".to_string(),
            SourceRef::Solid {
                color: Color::black(),
            },
            TimelineInterval::new(MediaTime::new(3, 1).unwrap(), MediaTime::new(5, 1).unwrap())
                .unwrap(),
            0,
        )
        .unwrap();
    let project = service.snapshot().unwrap();
    let local = MediaTime::new(2, 1).unwrap();
    let owner = AutomationOwner::Item(item_id);
    let timeline = timeline_time_for_local(&project, &owner, local).unwrap();
    assert_eq!(timeline, MediaTime::new(5, 1).unwrap());
    assert_eq!(
        local_time_for_timeline(&project, &owner, timeline),
        Some(local)
    );
}

#[test]
fn transition_time_is_interval_local_and_concrete_paths_are_distinct_owners() {
    let service = TimelineEditorService::create_default("transition time").unwrap();
    let mut project = service.snapshot().unwrap().as_ref().clone();
    let transition_id = TransitionId::new();
    project.transitions.insert(
        transition_id,
        Transition {
            id: transition_id,
            timeline_id: project.root_timeline_id,
            from_item_id: TimelineItemId::new(),
            to_item_id: TimelineItemId::new(),
            edit_point: MediaTime::new(5, 1).unwrap(),
            duration: MediaTime::new(4, 1).unwrap(),
            alignment: TransitionAlignment::CenteredOnEdit,
            processor: TransitionProcessor::cross_dissolve(),
            parameters: HashMap::new(),
        },
    );
    let definition_owner = AutomationOwner::TransitionDefinition(transition_id);
    let local = MediaTime::new(1, 1).unwrap();
    let timeline = timeline_time_for_local(&project, &definition_owner, local).unwrap();
    assert_eq!(timeline, MediaTime::new(4, 1).unwrap());
    assert_eq!(
        local_time_for_timeline(&project, &definition_owner, timeline),
        Some(local)
    );

    let first = transition_owner(
        transition_id,
        Some(&InstancePath {
            root_timeline_id: project.root_timeline_id,
            composition_items: vec![TimelineItemId::new()],
        }),
    );
    let second = transition_owner(
        transition_id,
        Some(&InstancePath {
            root_timeline_id: project.root_timeline_id,
            composition_items: vec![TimelineItemId::new()],
        }),
    );
    assert_ne!(first, second);
}

#[test]
fn builtin_effect_keyframes_keep_one_id_across_inspector_timeline_and_curve() {
    let media = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("test_data")
        .join("e2e_media");
    let fixture = library::editor::build_authoring_e2e_fixture(&media, &PluginManager::default())
        .expect("fixture");
    let attachment_id = fixture.info.effect_attachment_ids[0];
    let local_time = MediaTime::new(1, 1).unwrap();
    let (keyframe_id, _) = fixture
        .service
        .upsert_builtin_effect_parameter_keyframe(
            attachment_id,
            "sigma_x",
            local_time,
            PropertyValue::from(4.0),
            None,
        )
        .expect("Inspector keyframe");
    let project = fixture.service.snapshot().unwrap();
    let lanes = collect_item_lanes(&project, fixture.info.text_item_id);
    let target = AutomationTarget::AttachmentParameter {
        attachment_id,
        key: "sigma_x".to_string(),
    };
    let lane = lanes
        .iter()
        .find(|lane| lane.id.target == target)
        .expect("Timeline effect lane");
    assert_eq!(lane.points[0].id, keyframe_id);
    let curve = numeric_channels(&lanes)
        .into_iter()
        .find(|channel| channel.id.target == target)
        .expect("Curve effect channel");
    assert_eq!(curve.points[0].id, keyframe_id);

    update_keyframe(
        &fixture.service,
        &AutomationLaneId {
            owner: AutomationOwner::Item(fixture.info.text_item_id),
            target: target.clone(),
        },
        keyframe_id,
        AuthoringKeyframeUpdate {
            time: Some(MediaTime::new(3, 2).unwrap()),
            value: None,
            easing: None,
        },
    )
    .expect("shared update");
    let project = fixture.service.snapshot().unwrap();
    let lane = collect_item_lanes(&project, fixture.info.text_item_id)
        .into_iter()
        .find(|lane| lane.id.target == target)
        .expect("updated lane");
    assert_eq!(lane.points[0].id, keyframe_id);
    assert_eq!(lane.points[0].time, MediaTime::new(3, 2).unwrap());
}

#[test]
fn direct_text_operation_properties_share_lane_identity_and_mutation() {
    let plugins = PluginManager::default();
    let service = TimelineEditorService::create_default("direct operation lanes").unwrap();
    let project = service.snapshot().unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let (item_id, _) = service
        .add_item(
            track_id,
            "Text".to_string(),
            SourceRef::Text {
                text: "Tracked".to_string(),
                appearance_operations: Vec::new(),
                ensemble_operations: Vec::new(),
            },
            TimelineInterval::new(MediaTime::zero(), MediaTime::new(5, 1).unwrap()).unwrap(),
            0,
        )
        .unwrap();
    let (fill_id, _) = service
        .add_appearance_operation(&plugins, item_id, "fill", 0)
        .unwrap();
    let (tracking_id, _) = service
        .add_text_ensemble_operation_by_id(
            &plugins,
            item_id,
            TextEnsembleOperationKind::Effector,
            "tracking",
        )
        .unwrap();
    let tracking_owner = AuthoringPropertyOwner::TextEnsemble {
        item_id,
        operation_id: tracking_id,
    };
    let (tracking_key_id, _) = service
        .set_authored_property_keyframe_mode(
            tracking_owner,
            "amount".to_string(),
            MediaTime::new(1, 1).unwrap(),
            PropertyValue::from(12.0),
        )
        .unwrap();
    let fill_owner = AuthoringPropertyOwner::Appearance {
        item_id,
        operation_id: fill_id,
    };
    let (fill_key_id, _) = service
        .set_authored_property_keyframe_mode(
            fill_owner,
            "offset".to_string(),
            MediaTime::new(1, 1).unwrap(),
            PropertyValue::from(2.0),
        )
        .unwrap();

    let project = service.snapshot().unwrap();
    let lanes = collect_item_lanes(&project, item_id);
    let tracking_target = AutomationTarget::AuthoredProperty {
        owner: tracking_owner,
        key: "amount".to_string(),
    };
    let tracking = lanes
        .iter()
        .find(|lane| lane.id.target == tracking_target)
        .expect("Tracking lane");
    assert_eq!(tracking.label, "Tracking · Amount");
    assert_eq!(tracking.points[0].id, tracking_key_id);
    let dope_tracking = collect_item_keyframed_lanes(&project, item_id)
        .into_iter()
        .find(|lane| lane.id.target == tracking_target)
        .expect("Tracking Dope Sheet lane");
    assert_eq!(dope_tracking.points[0].id, tracking_key_id);
    assert!(numeric_channels(&lanes).iter().any(|channel| {
        channel.id.target == tracking_target
            && channel.component == CurveValueComponent::Scalar
            && channel.points[0].id == tracking_key_id
    }));
    assert_eq!(
        target_metadata(&tracking_target),
        serde_json::json!({
            "kind": "authored_property",
            "owner": {
                "kind": "text_ensemble",
                "item_id": item_id,
                "operation_id": tracking_id,
            },
            "key": "amount",
        })
    );

    let fill_target = AutomationTarget::AuthoredProperty {
        owner: fill_owner,
        key: "offset".to_string(),
    };
    let fill = lanes
        .iter()
        .find(|lane| lane.id.target == fill_target)
        .expect("Appearance lane");
    assert_eq!(fill.label, "Fill · Offset");
    assert_eq!(fill.points[0].id, fill_key_id);

    update_keyframe(
        &service,
        &tracking.id,
        tracking_key_id,
        AuthoringKeyframeUpdate {
            time: Some(MediaTime::new(3, 2).unwrap()),
            value: Some(PropertyValue::from(24.0)),
            easing: Some(EasingFunction::EaseInOutQuad),
        },
    )
    .unwrap();
    let project = service.snapshot().unwrap();
    let updated = collect_item_lanes(&project, item_id)
        .into_iter()
        .find(|lane| lane.id.target == tracking_target)
        .expect("updated Tracking lane");
    assert_eq!(updated.points[0].id, tracking_key_id);
    assert_eq!(updated.points[0].time, MediaTime::new(3, 2).unwrap());
    assert_eq!(updated.points[0].value, PropertyValue::from(24.0));
    assert_eq!(updated.points[0].easing, EasingFunction::EaseInOutQuad);

    let mismatched_lane = AutomationLaneId {
        owner: AutomationOwner::Item(TimelineItemId::new()),
        target: tracking_target,
    };
    let error = update_keyframe(
        &service,
        &mismatched_lane,
        tracking_key_id,
        AuthoringKeyframeUpdate {
            time: None,
            value: Some(PropertyValue::from(-100.0)),
            easing: None,
        },
    )
    .expect_err("a lane cannot mutate another Item's authored operation");
    assert!(matches!(error, library::LibraryError::Validation(_)));
}
