use super::*;

use ordered_float::OrderedFloat;

use crate::model::authoring::{
    AutomatableParameter, AutomationKeyframe, AutomationTrack, OperationRef,
    ProcessorParameterContract, ProjectDocument, TRANSITION_APPLY_OPERATION, TRANSITION_CATEGORY,
    TransitionAlignment, TransitionContractSnapshot, TransitionMediaType, TransitionProcessor,
};
use crate::model::project::PortDataType;
use crate::model::project::asset::{Asset, AssetKind};

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).expect("whole seconds")
}

fn overlapping_items() -> (
    TimelineEditorService,
    TimelineTrackId,
    TimelineItemId,
    TimelineItemId,
) {
    let service = TimelineEditorService::create_default("transitions").unwrap();
    let project = service.snapshot().unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let solid = |red| SourceRef::Solid {
        color: Color {
            r: red,
            g: 0,
            b: 0,
            a: 255,
        },
    };
    let (from, _) = service
        .add_item(
            track_id,
            "From".to_string(),
            solid(32),
            TimelineInterval::new(seconds(0), seconds(7)).unwrap(),
            0,
        )
        .unwrap();
    let (to, _) = service
        .add_item(
            track_id,
            "To".to_string(),
            solid(224),
            TimelineInterval::new(seconds(3), seconds(7)).unwrap(),
            1,
        )
        .unwrap();
    (service, track_id, from, to)
}

fn cross_dissolve(from: TimelineItemId, to: TimelineItemId) -> TransitionPlacement {
    TransitionPlacement {
        from_item_id: from,
        to_item_id: to,
        edit_point: seconds(5),
        duration: seconds(4),
        alignment: TransitionAlignment::CenteredOnEdit,
        processor: TransitionProcessor::cross_dissolve(),
        parameters: HashMap::new(),
    }
}

#[test]
fn transition_creation_and_removal_are_atomic_undoable_edits() {
    let (service, _, from, to) = overlapping_items();
    let before = service.snapshot().unwrap();
    let (transition_id, _) = service.add_transition(cross_dissolve(from, to)).unwrap();
    assert!(
        service
            .snapshot()
            .unwrap()
            .transitions
            .contains_key(&transition_id)
    );
    let saved = service.document().unwrap().to_json().unwrap();
    assert!(
        ProjectDocument::from_json(&saved)
            .unwrap()
            .project
            .transitions
            .contains_key(&transition_id)
    );

    service.undo().unwrap().expect("transition creation undo");
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
    service.redo().unwrap().expect("transition creation redo");
    service.remove_transition(transition_id).unwrap();
    assert!(service.snapshot().unwrap().transitions.is_empty());
}

#[test]
fn transition_timing_edits_invalidate_the_exact_old_and_new_span() {
    let (service, _, from, to) = overlapping_items();
    service
        .trim_item(from, TimelineInterval::new(seconds(0), seconds(5)).unwrap())
        .unwrap();
    service
        .trim_item(to, TimelineInterval::new(seconds(5), seconds(5)).unwrap())
        .unwrap();
    let (transition_id, _) = service.add_transition(cross_dissolve(from, to)).unwrap();
    let original_processor = service.snapshot().unwrap().transitions[&transition_id]
        .processor
        .clone();

    let duration_changes = service
        .set_transition_duration(transition_id, seconds(2))
        .unwrap();
    let timeline_id = service.snapshot().unwrap().root_timeline_id;
    assert_eq!(
        duration_changes.invalidations,
        vec![ProjectInvalidation::TimelineRange {
            timeline_id,
            start: seconds(3),
            duration: seconds(4),
        }]
    );

    let alignment_changes = service
        .set_transition_alignment(transition_id, TransitionAlignment::StartAtEdit)
        .unwrap();
    assert_eq!(
        alignment_changes.invalidations,
        vec![ProjectInvalidation::TimelineRange {
            timeline_id,
            start: seconds(4),
            duration: seconds(3),
        }]
    );

    let project = service.snapshot().unwrap();
    let transition = &project.transitions[&transition_id];
    assert_eq!(transition.duration, seconds(2));
    assert_eq!(transition.alignment, TransitionAlignment::StartAtEdit);
    assert_eq!(transition.processor, original_processor);
}

#[test]
fn invalid_transition_duration_is_rejected_without_mutating_timeline_state() {
    let (service, _, from, to) = overlapping_items();
    let (transition_id, _) = service.add_transition(cross_dissolve(from, to)).unwrap();
    let before = service.snapshot().unwrap();

    assert!(
        service
            .set_transition_duration(transition_id, MediaTime::zero())
            .is_err()
    );
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}

#[test]
fn item_edits_cannot_leave_a_dangling_or_uncovered_transition() {
    let (service, track_id, from, to) = overlapping_items();
    let (transition_id, _) = service.add_transition(cross_dissolve(from, to)).unwrap();
    let before = service.snapshot().unwrap();

    let deletion_error = service.delete_item(from).unwrap_err();
    assert!(
        deletion_error
            .to_string()
            .contains(&format!("Transition {transition_id} participant")),
        "{deletion_error}"
    );
    assert!(
        service
            .trim_item(from, TimelineInterval::new(seconds(0), seconds(4)).unwrap())
            .is_err()
    );
    assert!(service.split_item(from, seconds(4)).is_err());

    let (_, other_track, _) = service
        .add_timeline(
            "Other".to_string(),
            320,
            180,
            RationalRate::new(30, 1).unwrap(),
            seconds(20),
        )
        .unwrap();
    assert!(service.move_item(from, other_track, seconds(0), 0).is_err());

    let after = service.snapshot().unwrap();
    assert_eq!(after.items[&from], before.items[&from]);
    assert_eq!(
        after.transitions[&transition_id],
        before.transitions[&transition_id]
    );
    assert_eq!(after.items[&from].track_id, track_id);
}

#[test]
fn cascading_a_transition_participant_removes_owned_module_and_undo_restores_exact_state() {
    let (service, _, from, to) = overlapping_items();
    let (transition_id, _) = service.add_transition(cross_dissolve(from, to)).unwrap();
    let (definition_id, instance_id, _) = service
        .promote_transition_to_module(transition_id, "Owned transition")
        .unwrap();
    let before = service.snapshot().unwrap();

    let dependencies = service.item_input_dependencies(from).unwrap();
    assert!(
        dependencies.contains(&TimelineItemDependency::TransitionParticipant { transition_id })
    );

    service.delete_item_cascade(from).unwrap();
    let deleted = service.snapshot().unwrap();
    assert!(!deleted.items.contains_key(&from));
    assert!(deleted.items.contains_key(&to));
    assert!(!deleted.transitions.contains_key(&transition_id));
    assert!(!deleted.module_instances.contains_key(&instance_id));
    assert!(!deleted.module_definitions.contains_key(&definition_id));
    deleted.validate().unwrap();
    drop(deleted);

    service.undo().unwrap().expect("participant cascade undo");
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}

#[test]
fn typed_media_contract_rejects_audio_crossfade_for_image_only_items() {
    let (service, _, from, to) = overlapping_items();
    let mut placement = cross_dissolve(from, to);
    placement.processor = TransitionProcessor::audio_crossfade();

    assert!(service.add_transition(placement).is_err());
    assert!(service.snapshot().unwrap().transitions.is_empty());
}

#[test]
fn audio_crossfade_accepts_two_audio_sources_without_node_expansion() {
    let service = TimelineEditorService::create_default("audio transition").unwrap();
    let project = service.snapshot().unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let mut item_ids = Vec::new();
    for (index, path, start) in [(0, "from.wav", seconds(0)), (1, "to.wav", seconds(3))] {
        let asset = Asset::new(path, path, AssetKind::Audio);
        let asset_id = asset.id;
        service.add_asset(asset).unwrap();
        let (item_id, _) = service
            .add_item(
                track_id,
                format!("Audio {index}"),
                SourceRef::Asset { asset_id },
                TimelineInterval::new(start, seconds(7)).unwrap(),
                index as i64,
            )
            .unwrap();
        item_ids.push(item_id);
    }
    let mut placement = cross_dissolve(item_ids[0], item_ids[1]);
    placement.processor = TransitionProcessor::audio_crossfade();
    let (transition_id, _) = service.add_transition(placement).unwrap();

    let project = service.snapshot().unwrap();
    assert!(project.transitions.contains_key(&transition_id));
    assert!(project.module_definitions.is_empty());
    assert!(project.module_instances.is_empty());
}

#[test]
fn transition_save_rejects_media_the_track_pipeline_does_not_render() {
    let (image_service, _, image_from, image_to) = overlapping_items();
    let mut image_project = image_service.snapshot().unwrap().as_ref().clone();
    let image_track_id = image_project.items[&image_from].track_id;
    image_project.tracks.get_mut(&image_track_id).unwrap().kind = TimelineTrackKind::Audio;
    let image_service = TimelineEditorService::new(image_project).unwrap();
    let image_error = image_service
        .add_transition(cross_dissolve(image_from, image_to))
        .unwrap_err();
    assert!(
        image_error.to_string().contains("does not render"),
        "{image_error}"
    );
    assert!(image_service.snapshot().unwrap().transitions.is_empty());

    let audio_service =
        TimelineEditorService::create_default("visual-only audio transition").unwrap();
    let audio_project = audio_service.snapshot().unwrap();
    let audio_track_id = audio_project.timelines[&audio_project.root_timeline_id].track_order[0];
    drop(audio_project);
    let mut audio_items = Vec::new();
    for (index, start) in [seconds(0), seconds(3)].into_iter().enumerate() {
        let path = format!("visual-{index}.wav");
        let asset = Asset::new(&path, &path, AssetKind::Audio);
        let asset_id = asset.id;
        audio_service.add_asset(asset).unwrap();
        audio_items.push(
            audio_service
                .add_item(
                    audio_track_id,
                    format!("Audio {index}"),
                    SourceRef::Asset { asset_id },
                    TimelineInterval::new(start, seconds(7)).unwrap(),
                    index as i64,
                )
                .unwrap()
                .0,
        );
    }
    let mut audio_project = audio_service.snapshot().unwrap().as_ref().clone();
    audio_project.tracks.get_mut(&audio_track_id).unwrap().kind = TimelineTrackKind::Visual;
    let audio_service = TimelineEditorService::new(audio_project).unwrap();
    let mut audio_transition = cross_dissolve(audio_items[0], audio_items[1]);
    audio_transition.processor = TransitionProcessor::audio_crossfade();
    let audio_error = audio_service.add_transition(audio_transition).unwrap_err();
    assert!(
        audio_error.to_string().contains("does not render"),
        "{audio_error}"
    );
    assert!(audio_service.snapshot().unwrap().transitions.is_empty());
}

#[test]
fn dedicated_and_combined_track_kinds_save_compatible_transitions() {
    let (visual_service, _, visual_from, visual_to) = overlapping_items();
    let mut visual_project = visual_service.snapshot().unwrap().as_ref().clone();
    let visual_track_id = visual_project.items[&visual_from].track_id;
    visual_project
        .tracks
        .get_mut(&visual_track_id)
        .unwrap()
        .kind = TimelineTrackKind::Visual;
    let visual_service = TimelineEditorService::new(visual_project).unwrap();
    visual_service
        .add_transition(cross_dissolve(visual_from, visual_to))
        .unwrap();

    let audio_service =
        TimelineEditorService::create_default("dedicated audio transition").unwrap();
    let audio_project = audio_service.snapshot().unwrap();
    let audio_track_id = audio_project.timelines[&audio_project.root_timeline_id].track_order[0];
    drop(audio_project);
    let mut audio_items = Vec::new();
    for (index, start) in [seconds(0), seconds(3)].into_iter().enumerate() {
        let path = format!("audio-{index}.wav");
        let asset = Asset::new(&path, &path, AssetKind::Audio);
        let asset_id = asset.id;
        audio_service.add_asset(asset).unwrap();
        audio_items.push(
            audio_service
                .add_item(
                    audio_track_id,
                    format!("Audio {index}"),
                    SourceRef::Asset { asset_id },
                    TimelineInterval::new(start, seconds(7)).unwrap(),
                    index as i64,
                )
                .unwrap()
                .0,
        );
    }
    let mut audio_project = audio_service.snapshot().unwrap().as_ref().clone();
    audio_project.tracks.get_mut(&audio_track_id).unwrap().kind = TimelineTrackKind::Audio;
    let audio_service = TimelineEditorService::new(audio_project).unwrap();
    let mut audio_transition = cross_dissolve(audio_items[0], audio_items[1]);
    audio_transition.processor = TransitionProcessor::audio_crossfade();
    audio_service.add_transition(audio_transition).unwrap();

    assert_eq!(visual_service.snapshot().unwrap().transitions.len(), 1);
    assert_eq!(audio_service.snapshot().unwrap().transitions.len(), 1);
}

#[test]
fn image_and_audio_transitions_can_share_one_pair_and_interval() {
    let service = TimelineEditorService::create_default("dual media transition").unwrap();
    let project = service.snapshot().unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let (definition, output_id) = ModuleDefinition::new_project_image("Dual media source");
    let definition_id = definition.id;
    service.add_module_definition(definition).unwrap();
    let mut item_ids = Vec::new();
    for (index, start) in [seconds(0), seconds(3)].into_iter().enumerate() {
        let (item_id, _, _) = service
            .place_module_item(
                definition_id,
                ModuleItemPlacement {
                    track_id,
                    name: format!("Dual media {index}"),
                    output_id,
                    interval: TimelineInterval::new(start, seconds(7)).unwrap(),
                    layer: index as i64,
                    parameter_overrides: HashMap::new(),
                    input_bindings: HashMap::new(),
                },
            )
            .unwrap();
        item_ids.push(item_id);
    }
    service
        .add_transition(cross_dissolve(item_ids[0], item_ids[1]))
        .unwrap();
    let mut audio = cross_dissolve(item_ids[0], item_ids[1]);
    audio.processor = TransitionProcessor::audio_crossfade();
    service.add_transition(audio).unwrap();

    assert_eq!(service.snapshot().unwrap().transitions.len(), 2);
}

#[test]
fn transition_parameters_and_local_automation_are_contract_checked() {
    let (service, _, from, to) = overlapping_items();
    let contract = ProcessorParameterContract {
        key: "amount".to_string(),
        data_type: PortDataType::Number,
        default_value: PropertyValue::Number(OrderedFloat(1.0)),
    };
    let mut placement = cross_dissolve(from, to);
    placement.processor = TransitionProcessor::from_operation(
        OperationRef {
            category: TRANSITION_CATEGORY.to_string(),
            component_id: "test_transition".to_string(),
            operation: TRANSITION_APPLY_OPERATION.to_string(),
            version: "1".to_string(),
        },
        TransitionContractSnapshot {
            media_type: TransitionMediaType::Image,
            parameters: vec![contract],
        },
    );
    placement.parameters.insert(
        "amount".to_string(),
        AutomatableParameter {
            value: PropertyValue::Number(OrderedFloat(0.5)),
            automation: None,
        },
    );
    let (transition_id, _) = service.add_transition(placement).unwrap();

    assert!(
        service
            .set_transition_parameter_value(
                transition_id,
                "amount",
                PropertyValue::String("wrong".to_string())
            )
            .is_err()
    );
    let invalid_automation = AutomationTrack {
        keyframes: vec![AutomationKeyframe::new(
            seconds(5),
            PropertyValue::Number(OrderedFloat(1.0)),
            EasingFunction::Linear,
        )],
    };
    assert!(
        service
            .set_transition_parameter_automation(transition_id, "amount", Some(invalid_automation))
            .is_err()
    );
    let transition = &service.snapshot().unwrap().transitions[&transition_id];
    assert_eq!(
        transition.parameters["amount"].value,
        PropertyValue::Number(OrderedFloat(0.5))
    );
    assert!(transition.parameters["amount"].automation.is_none());
}
