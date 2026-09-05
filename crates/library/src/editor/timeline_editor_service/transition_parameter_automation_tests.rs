use super::*;

use ordered_float::OrderedFloat;

use crate::animation::EasingFunction;
use crate::model::authoring::{
    CompositionInstance, DurationPolicy, ModulePortAddress, ProjectInvalidation,
};
use crate::model::node::{Node, TRANSITION_IMAGE_MIX_NODE_ID, ValueContent};
use crate::model::project::connection::{NUMERIC_A_INPUT_PORT, TRANSITION_PROGRESS_INPUT_PORT};

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).expect("whole seconds")
}

fn number(value: f64) -> PropertyValue {
    PropertyValue::Number(OrderedFloat(value))
}

pub(super) fn transition_project() -> (AuthoringProject, TransitionId, PublishedParameterId) {
    let setup = TimelineEditorService::create_default("Transition parameter automation")
        .expect("default project");
    let project = setup.snapshot().expect("project");
    let timeline_id = project.root_timeline_id;
    let track_id = project.timelines[&timeline_id].track_order[0];
    drop(project);

    let solid = |red| SourceRef::Solid {
        color: Color {
            r: red,
            g: 0,
            b: 0,
            a: 255,
        },
    };
    let (from_item_id, _) = setup
        .add_item(
            track_id,
            "From".to_string(),
            solid(32),
            TimelineInterval::new(seconds(0), seconds(7)).expect("from interval"),
            0,
        )
        .expect("from item");
    let (to_item_id, _) = setup
        .add_item(
            track_id,
            "To".to_string(),
            solid(224),
            TimelineInterval::new(seconds(3), seconds(7)).expect("to interval"),
            1,
        )
        .expect("to item");
    let (transition_id, _) = setup
        .add_transition(TransitionPlacement {
            from_item_id,
            to_item_id,
            edit_point: seconds(5),
            duration: seconds(4),
            alignment: crate::model::authoring::TransitionAlignment::CenteredOnEdit,
            processor: crate::model::authoring::TransitionProcessor::cross_dissolve(),
            parameters: HashMap::new(),
        })
        .expect("Transition");
    let (_, instance_id, _) = setup
        .promote_transition_to_module(transition_id, "Automatable Transition")
        .expect("promote Transition");

    let value_node = Node::new_value("Amount", ValueContent::Add);
    let value_node_id = value_node.id;
    setup
        .add_instance_module_node(instance_id, value_node)
        .expect("add value Node");
    let (published, _, _) = setup
        .edit_instance_module_interface(
            instance_id,
            ModuleInterfaceCommand::PublishParameter {
                name: "Amount".to_string(),
                default_value: number(0.0),
                target: ModulePortAddress {
                    node_id: value_node_id,
                    port: NUMERIC_A_INPUT_PORT.to_string(),
                },
            },
        )
        .expect("publish parameter");
    let ModuleInterfaceEditResult::PublishedParameter(parameter_id) = published else {
        panic!("PublishParameter must return its stable ID");
    };

    (
        setup
            .snapshot()
            .expect("configured project")
            .as_ref()
            .clone(),
        transition_id,
        parameter_id,
    )
}

fn transition_track(
    project: &AuthoringProject,
    transition_id: TransitionId,
    parameter_id: PublishedParameterId,
) -> Option<&AutomationTrack> {
    project
        .transitions
        .get(&transition_id)?
        .processor
        .module_processor()?
        .automation_tracks
        .get(&parameter_id)
}

fn bounded_transition_project() -> (AuthoringProject, TransitionId, PublishedParameterId) {
    let (mut project, transition_id, parameter_id) = transition_project();
    let instance_id = project.transitions[&transition_id]
        .processor
        .module_processor()
        .expect("Module Transition")
        .instance_id;
    let definition_id = project.module_instances[&instance_id].definition_id;
    let definition = project
        .module_definitions
        .get_mut(&definition_id)
        .expect("Transition definition");
    let bounded = Node::new_catalog_node(TRANSITION_IMAGE_MIX_NODE_ID)
        .expect("bounded native Progress property");
    let bounded_id = bounded.id;
    definition.graph.nodes.insert(bounded_id, bounded);
    let parameter = definition
        .interface
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == parameter_id)
        .expect("ordinary parameter");
    parameter.name = "Bounded Progress".to_string();
    parameter.default_value = number(0.5);
    parameter.target = ModulePortAddress {
        node_id: bounded_id,
        port: TRANSITION_PROGRESS_INPUT_PORT.to_string(),
    };
    project.validate().expect("bounded Transition project");
    (project, transition_id, parameter_id)
}

fn assert_definition_invalidation(
    project: &AuthoringProject,
    transition_id: TransitionId,
    changes: &ChangeSet,
) {
    let transition = &project.transitions[&transition_id];
    let interval = transition.interval().expect("Transition interval");
    assert_eq!(
        changes.invalidations,
        vec![ProjectInvalidation::TimelineRange {
            timeline_id: transition.timeline_id,
            start: interval.start,
            duration: interval.duration,
        }]
    );
}

#[test]
fn definition_keyframe_commands_are_exact_and_each_is_one_undo_step() {
    let (project, transition_id, parameter_id) = transition_project();
    let service = TimelineEditorService::new(project).expect("clean service");
    let owner = TransitionAutomationOwner::Definition(transition_id);
    let baseline = service.snapshot().expect("baseline");

    let (first_id, first_change) = service
        .upsert_transition_parameter_keyframe(
            &owner,
            parameter_id,
            seconds(0),
            number(1.0),
            Some(EasingFunction::Linear),
        )
        .expect("first Keyframe");
    assert_definition_invalidation(&baseline, transition_id, &first_change);
    let after_first = service.snapshot().expect("first state");

    let (second_id, second_change) = service
        .upsert_transition_parameter_keyframe(
            &owner,
            parameter_id,
            seconds(2),
            number(2.0),
            Some(EasingFunction::EaseInOutQuad),
        )
        .expect("second Keyframe");
    assert_definition_invalidation(&after_first, transition_id, &second_change);
    let after_second = service.snapshot().expect("second state");
    assert_eq!(
        transition_track(&after_second, transition_id, parameter_id)
            .expect("automation")
            .keyframes
            .iter()
            .map(|keyframe| keyframe.id)
            .collect::<Vec<_>>(),
        vec![first_id, second_id]
    );

    let update_change = service
        .update_keyframe(
            &AuthoringKeyframeTarget::TransitionParameter {
                owner: owner.clone(),
                parameter_id,
            },
            second_id,
            AuthoringKeyframeUpdate {
                time: Some(seconds(3)),
                value: Some(number(3.0)),
                easing: Some(EasingFunction::Constant),
            },
        )
        .expect("update Keyframe");
    assert_definition_invalidation(&after_second, transition_id, &update_change);
    let after_update = service.snapshot().expect("updated state");
    let updated = transition_track(&after_update, transition_id, parameter_id)
        .expect("automation")
        .keyframes
        .iter()
        .find(|keyframe| keyframe.id == second_id)
        .expect("updated Keyframe");
    assert_eq!(updated.time, seconds(3));
    assert_eq!(updated.value, number(3.0));
    assert!(matches!(updated.easing, EasingFunction::Constant));

    let remove_change = service
        .remove_transition_parameter_keyframe(&owner, parameter_id, first_id)
        .expect("remove Keyframe");
    assert_definition_invalidation(&after_update, transition_id, &remove_change);
    let after_remove = service.snapshot().expect("removed state");
    assert_eq!(
        transition_track(&after_remove, transition_id, parameter_id)
            .expect("remaining automation")
            .keyframes
            .iter()
            .map(|keyframe| keyframe.id)
            .collect::<Vec<_>>(),
        vec![second_id]
    );

    let constant_change = service
        .set_transition_parameter_constant(&owner, parameter_id, number(9.0))
        .expect("switch to constant");
    assert_definition_invalidation(&after_remove, transition_id, &constant_change);
    let constant = service.snapshot().expect("constant state");
    assert!(transition_track(&constant, transition_id, parameter_id).is_none());
    let instance_id = constant.transitions[&transition_id]
        .processor
        .module_processor()
        .expect("Transition Module")
        .instance_id;
    assert_eq!(
        constant.module_instances[&instance_id].parameter_overrides[&parameter_id],
        number(9.0)
    );

    service.undo().expect("undo constant").expect("change");
    assert_eq!(
        service.snapshot().expect("state").as_ref(),
        after_remove.as_ref()
    );
    service.undo().expect("undo remove").expect("change");
    assert_eq!(
        service.snapshot().expect("state").as_ref(),
        after_update.as_ref()
    );
    service.undo().expect("undo update").expect("change");
    assert_eq!(
        service.snapshot().expect("state").as_ref(),
        after_second.as_ref()
    );
    service.undo().expect("undo second").expect("change");
    assert_eq!(
        service.snapshot().expect("state").as_ref(),
        after_first.as_ref()
    );
    service.undo().expect("undo first").expect("change");
    assert_eq!(
        service.snapshot().expect("state").as_ref(),
        baseline.as_ref()
    );
    assert!(!service.can_undo().expect("clean Undo state"));
}

pub(super) fn wrap_with_two_composition_instances(
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
            width: 1920,
            height: 1080,
            fps: RationalRate::new(30, 1).expect("fps"),
            duration: seconds(20),
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
            name: "Composition instances".to_string(),
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
        interval: TimelineInterval::new(seconds(0), seconds(10)).expect("placement interval"),
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

#[test]
fn transition_defaults_overrides_and_keyframes_obey_native_hard_bounds() {
    let (project, transition_id, parameter_id) = bounded_transition_project();
    let instance_id = project.transitions[&transition_id]
        .processor
        .module_processor()
        .expect("Module Transition")
        .instance_id;
    let definition_id = project.module_instances[&instance_id].definition_id;

    let mut invalid_default = project.clone();
    invalid_default
        .module_definitions
        .get_mut(&definition_id)
        .unwrap()
        .interface
        .parameters
        .iter_mut()
        .find(|parameter| parameter.id == parameter_id)
        .unwrap()
        .default_value = number(-0.1);
    let error = invalid_default.validate().unwrap_err();
    assert!(error.contains("cannot be less than 0"), "{error}");

    let mut invalid_override = project.clone();
    invalid_override
        .module_instances
        .get_mut(&instance_id)
        .unwrap()
        .parameter_overrides
        .insert(parameter_id, number(1.1));
    let error = invalid_override.validate().unwrap_err();
    assert!(error.contains("cannot be greater than 1"), "{error}");

    let service = TimelineEditorService::new(project.clone()).expect("service");
    let owner = TransitionAutomationOwner::Definition(transition_id);
    let error = service
        .upsert_transition_parameter_keyframe(
            &owner,
            parameter_id,
            seconds(0),
            number(-0.1),
            Some(EasingFunction::Linear),
        )
        .expect_err("keyframe must obey the target PropertyDefinition");
    assert!(error.to_string().contains("cannot be less than 0"));

    let mut nested = project;
    let nested_timeline_id = nested.root_timeline_id;
    let (root_timeline_id, first_item_id, _) =
        wrap_with_two_composition_instances(&mut nested, nested_timeline_id);
    nested.validate().expect("nested project");
    let service = TimelineEditorService::new(nested).expect("nested service");
    let before = service.snapshot().expect("before invalid override");
    let path = InstancePath::root(root_timeline_id).nested(first_item_id);
    let error = service
        .set_transition_module_instance_parameter(&path, transition_id, parameter_id, number(1.1))
        .expect_err("concrete Transition override must obey hard bounds");
    assert!(error.to_string().contains("cannot be greater than 1"));
    assert_eq!(service.snapshot().expect("unchanged project"), before);
}

#[test]
fn nested_instance_keyframes_copy_inherited_track_and_isolate_siblings() {
    let (project, transition_id, parameter_id) = transition_project();
    let definition_service = TimelineEditorService::new(project).expect("definition service");
    let definition_owner = TransitionAutomationOwner::Definition(transition_id);
    let (inherited_id, _) = definition_service
        .upsert_transition_parameter_keyframe(
            &definition_owner,
            parameter_id,
            seconds(0),
            number(1.0),
            Some(EasingFunction::Linear),
        )
        .expect("definition Keyframe");
    let mut nested_project = definition_service
        .snapshot()
        .expect("definition project")
        .as_ref()
        .clone();
    let nested_timeline_id = nested_project.root_timeline_id;
    let (root_timeline_id, first_item_id, second_item_id) =
        wrap_with_two_composition_instances(&mut nested_project, nested_timeline_id);
    nested_project.validate().expect("nested project");

    let service = TimelineEditorService::new(nested_project).expect("clean nested service");
    let baseline = service.snapshot().expect("baseline");
    let first_path = InstancePath::root(root_timeline_id).nested(first_item_id);
    let second_path = InstancePath::root(root_timeline_id).nested(second_item_id);
    let first_owner = TransitionAutomationOwner::Instance {
        transition_id,
        instance_path: first_path.clone(),
    };

    let (placement_id, change) = service
        .upsert_transition_parameter_keyframe(
            &first_owner,
            parameter_id,
            seconds(1),
            number(2.0),
            Some(EasingFunction::EaseOutQuad),
        )
        .expect("placement Keyframe");
    let interval = baseline.transitions[&transition_id]
        .interval()
        .expect("Transition interval");
    assert_eq!(
        change.invalidations,
        vec![ProjectInvalidation::TimelineInstanceRange {
            instance_path: first_path.clone(),
            timeline_id: nested_timeline_id,
            transition_id,
            start: interval.start,
            duration: interval.duration,
        }]
    );

    let after_upsert = service.snapshot().expect("placement state");
    let first_target = after_upsert
        .resolve_transition_module_instance_target(&first_path, transition_id)
        .expect("first target");
    let second_target = after_upsert
        .resolve_transition_module_instance_target(&second_path, transition_id)
        .expect("second target");
    let definition_ids = transition_track(&after_upsert, transition_id, parameter_id)
        .expect("definition automation")
        .keyframes
        .iter()
        .map(|keyframe| keyframe.id)
        .collect::<Vec<_>>();
    assert_eq!(definition_ids, vec![inherited_id]);

    let first_effective = after_upsert
        .effective_transition_module_controls(&first_target)
        .expect("first controls");
    assert_eq!(
        first_effective.automation_tracks[&parameter_id]
            .keyframes
            .iter()
            .map(|keyframe| keyframe.id)
            .collect::<Vec<_>>(),
        vec![inherited_id, placement_id],
        "the concrete edit must copy the complete inherited track"
    );
    let second_effective = after_upsert
        .effective_transition_module_controls(&second_target)
        .expect("second controls");
    assert_eq!(
        second_effective.automation_tracks[&parameter_id]
            .keyframes
            .iter()
            .map(|keyframe| keyframe.id)
            .collect::<Vec<_>>(),
        vec![inherited_id],
        "the sibling must continue to inherit the definition"
    );
    let persisted = after_upsert
        .transition_module_instance_overrides(&first_target)
        .expect("first override lookup")
        .expect("first override");
    assert!(matches!(
        persisted.automation_tracks.get(&parameter_id),
        Some(Some(track)) if track.keyframes.len() == 2
    ));
    assert!(
        after_upsert
            .transition_module_instance_overrides(&second_target)
            .expect("second override lookup")
            .is_none(),
        "editing the first placement must not materialize a sibling override"
    );

    let update_change = service
        .update_keyframe(
            &AuthoringKeyframeTarget::TransitionParameter {
                owner: first_owner,
                parameter_id,
            },
            placement_id,
            AuthoringKeyframeUpdate {
                time: Some(seconds(2)),
                value: Some(number(3.0)),
                easing: Some(EasingFunction::Constant),
            },
        )
        .expect("update placement Keyframe");
    assert_eq!(update_change.invalidations, change.invalidations);
    let after_update = service.snapshot().expect("updated placement");
    let first_effective = after_update
        .effective_transition_module_controls(&first_target)
        .expect("first controls");
    let placement = first_effective.automation_tracks[&parameter_id]
        .keyframes
        .iter()
        .find(|keyframe| keyframe.id == placement_id)
        .expect("placement Keyframe");
    assert_eq!(placement.time, seconds(2));
    assert_eq!(placement.value, number(3.0));
    assert!(matches!(placement.easing, EasingFunction::Constant));
    assert_eq!(
        after_update
            .effective_transition_module_controls(&second_target)
            .expect("second controls")
            .automation_tracks[&parameter_id]
            .keyframes
            .iter()
            .map(|keyframe| keyframe.id)
            .collect::<Vec<_>>(),
        vec![inherited_id]
    );

    service.undo().expect("undo update").expect("change");
    assert_eq!(
        service.snapshot().expect("state").as_ref(),
        after_upsert.as_ref()
    );
    service.undo().expect("undo COW").expect("change");
    assert_eq!(
        service.snapshot().expect("state").as_ref(),
        baseline.as_ref()
    );
    assert!(!service.can_undo().expect("clean Undo state"));
}
