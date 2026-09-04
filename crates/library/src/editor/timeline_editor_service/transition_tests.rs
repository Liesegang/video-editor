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
            TimelineInterval::new(seconds(0), seconds(10)).unwrap(),
            0,
        )
        .unwrap();
    let (to, _) = service
        .add_item(
            track_id,
            "To".to_string(),
            solid(224),
            TimelineInterval::new(seconds(0), seconds(10)).unwrap(),
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
fn item_edits_cannot_leave_a_dangling_or_uncovered_transition() {
    let (service, track_id, from, to) = overlapping_items();
    let (transition_id, _) = service.add_transition(cross_dissolve(from, to)).unwrap();
    let before = service.snapshot().unwrap();

    assert!(service.delete_item(from).is_err());
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
    for (index, path) in ["from.wav", "to.wav"].into_iter().enumerate() {
        let asset = Asset::new(path, path, AssetKind::Audio);
        let asset_id = asset.id;
        service.add_asset(asset).unwrap();
        let (item_id, _) = service
            .add_item(
                track_id,
                format!("Audio {index}"),
                SourceRef::Asset { asset_id },
                TimelineInterval::new(seconds(0), seconds(10)).unwrap(),
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
fn transition_parameters_and_local_automation_are_contract_checked() {
    let (service, _, from, to) = overlapping_items();
    let contract = ProcessorParameterContract {
        key: "amount".to_string(),
        data_type: PortDataType::Number,
        default_value: PropertyValue::Number(OrderedFloat(1.0)),
    };
    let mut placement = cross_dissolve(from, to);
    placement.processor = TransitionProcessor {
        operation: OperationRef {
            category: TRANSITION_CATEGORY.to_string(),
            component_id: "test_transition".to_string(),
            operation: TRANSITION_APPLY_OPERATION.to_string(),
            version: "1".to_string(),
        },
        contract: TransitionContractSnapshot {
            media_type: TransitionMediaType::Image,
            parameters: vec![contract],
        },
    };
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
