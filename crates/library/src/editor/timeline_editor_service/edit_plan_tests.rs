use super::*;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::model::authoring::{
    InstanceLocator, ItemOutputStage, MediaInputBinding, MediaOutputKind, ModuleDefinitionSharing,
    ModuleInterface, ModuleTemplateOrigin, PublishedMediaInput, PublishedMediaInputId, Transition,
    TransitionAlignment, TransitionId, TransitionProcessor,
};
use crate::model::project::PortDataType;

fn time(value: i64, timescale: u32) -> MediaTime {
    MediaTime::new(value, timescale).expect("exact test time")
}

fn item_state(item: &TimelineItem) -> TimelineItemEditState {
    TimelineItemEditState {
        track_id: item.track_id,
        interval: item.interval,
        time_map: item.time_map,
        layer: item.layer,
    }
}

fn solid(red: u8) -> SourceRef {
    SourceRef::Solid {
        color: Color {
            r: red,
            g: 0,
            b: 0,
            a: 255,
        },
    }
}

fn project_with_items() -> (
    AuthoringProject,
    TimelineTrackId,
    [TimelineItemId; 3],
    ModuleDefinitionId,
) {
    let mut project = AuthoringProject::new(
        "edit plan",
        1920,
        1080,
        RationalRate::new(30, 1).unwrap(),
        time(60, 1),
    )
    .unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let starts = [time(1, 3), time(3, 1), time(6, 1)];
    let ids = [
        TimelineItemId::new(),
        TimelineItemId::new(),
        TimelineItemId::new(),
    ];
    for (index, item_id) in ids.into_iter().enumerate() {
        project.items.insert(
            item_id,
            TimelineItem {
                id: item_id,
                track_id,
                name: format!("Clip {index}"),
                source: solid(index as u8),
                interval: TimelineInterval::new(starts[index], time(2, 1)).unwrap(),
                time_map: TimeMap::default(),
                layer: index as i64,
                parent: None,
                blend_mode: BlendMode::Normal,
                authored_properties: PropertyMap::new(),
            },
        );
    }
    let (definition, _) = ModuleDefinition::new_project_image("untouched definition");
    let definition_id = definition.id;
    project.module_definitions.insert(definition_id, definition);
    project.validate().unwrap();
    (project, track_id, ids, definition_id)
}

fn adjacent_transition_project() -> (
    AuthoringProject,
    TimelineTrackId,
    TimelineItemId,
    TimelineItemId,
) {
    let mut project = AuthoringProject::new(
        "transition edit plan",
        1920,
        1080,
        RationalRate::new(30, 1).unwrap(),
        time(20, 1),
    )
    .unwrap();
    let timeline_id = project.root_timeline_id;
    let track_id = project.timelines[&timeline_id].track_order[0];
    let from = TimelineItemId::new();
    let to = TimelineItemId::new();
    for (item_id, name, interval, layer) in [
        (
            from,
            "From",
            TimelineInterval::new(time(0, 1), time(5, 1)).unwrap(),
            0,
        ),
        (
            to,
            "To",
            TimelineInterval::new(time(5, 1), time(5, 1)).unwrap(),
            1,
        ),
    ] {
        project.items.insert(
            item_id,
            TimelineItem {
                id: item_id,
                track_id,
                name: name.to_string(),
                source: solid(128),
                interval,
                time_map: TimeMap::default(),
                layer,
                parent: None,
                blend_mode: BlendMode::Normal,
                authored_properties: PropertyMap::new(),
            },
        );
    }
    let transition_id = TransitionId::new();
    project.transitions.insert(
        transition_id,
        Transition {
            id: transition_id,
            timeline_id,
            from_item_id: from,
            to_item_id: to,
            edit_point: time(5, 1),
            duration: time(4, 1),
            alignment: TransitionAlignment::CenteredOnEdit,
            processor: TransitionProcessor::cross_dissolve(),
            parameters: HashMap::new(),
        },
    );
    project.validate().unwrap();
    (project, track_id, from, to)
}

fn same_timeline_input_module() -> (ModuleDefinition, ModuleOutputId, PublishedMediaInputId) {
    let (mut definition, output_id) = ModuleDefinition::new_image(
        "Scoped input",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
    );
    let input_id = PublishedMediaInputId::new();
    definition.interface = ModuleInterface {
        parameters: Vec::new(),
        media_inputs: vec![PublishedMediaInput {
            id: input_id,
            name: "Image".to_string(),
            data_type: PortDataType::Image,
            target: definition
                .output(output_id)
                .unwrap()
                .target(PortDataType::Image)
                .unwrap(),
            required: true,
            primary: true,
        }],
        signals: Vec::new(),
        actions: Vec::new(),
    };
    (definition, output_id, input_id)
}

fn invalid_request_message(error: TimelineEditError) -> String {
    match error {
        TimelineEditError::InvalidRequest(message) => message,
        other => panic!("expected InvalidRequest, got {other:?}"),
    }
}

#[test]
fn move_preview_matches_commit_and_is_one_undo_without_touching_modules() {
    let (project, track_id, ids, definition_id) = project_with_items();
    let service = TimelineEditorService::new(project).unwrap();
    let before = service.snapshot().unwrap();
    let definition_before = before.module_definitions[&definition_id].clone();
    let request = TimelineEditRequest::move_item(
        service.revision().unwrap(),
        ids[0],
        track_id,
        time(7, 3),
        2,
    );

    let plan = service.plan_timeline_edit(request).unwrap();
    let projection = service.project_edit_plan(&plan).unwrap();
    assert_eq!(
        projection.len(),
        3,
        "the two reindexed siblings are projected"
    );
    let changes = service.commit_edit_plan(&plan).unwrap();

    let committed = service.snapshot().unwrap();
    for (item_id, projected_state) in projection.items() {
        assert_eq!(item_state(&committed.items[&item_id]), *projected_state);
    }
    assert_eq!(committed.items[&ids[0]].interval.start, time(7, 3));
    assert_eq!(committed.items[&ids[0]].layer, 2);
    assert_eq!(
        committed.module_definitions[&definition_id], definition_before,
        "Timeline placement edits cannot mutate reusable Module topology"
    );
    assert_eq!(changes.revision.get(), 1);
    assert_eq!(changes.invalidations.len(), 1);
    assert_eq!(
        changes.invalidations[0],
        ProjectInvalidation::TimelineRange {
            timeline_id: before.root_timeline_id,
            start: time(1, 3),
            duration: time(23, 3),
        }
    );

    service.undo().unwrap().expect("one edit-plan transaction");
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
    assert!(
        !service.can_undo().unwrap(),
        "one undo exhausts the clean fixture"
    );
}

#[test]
fn trim_projection_preserves_exact_local_time_and_matches_commit() {
    let (mut project, _, ids, _) = project_with_items();
    let item = project.items.get_mut(&ids[0]).unwrap();
    item.time_map = TimeMap {
        source_start: time(2, 7),
        playback_rate: RationalRate::new(3, 2).unwrap(),
    };
    project.validate().unwrap();
    let service = TimelineEditorService::new(project).unwrap();
    let interval = TimelineInterval::new(time(5, 6), time(4, 3)).unwrap();
    let request = TimelineEditRequest::trim_item(service.revision().unwrap(), ids[0], interval);

    let plan = service.plan_timeline_edit(request).unwrap();
    let projection = service.project_edit_plan(&plan).unwrap();
    let projected = projection.item(ids[0]).unwrap();
    let expected_source_start = time(2, 7)
        .checked_add(
            time(5, 6)
                .checked_sub(time(1, 3))
                .unwrap()
                .checked_mul_rate(RationalRate::new(3, 2).unwrap())
                .unwrap(),
        )
        .unwrap();
    assert_eq!(projected.interval, interval);
    assert_eq!(projected.time_map.source_start, expected_source_start);

    let changes = service.commit_edit_plan(&plan).unwrap();
    let committed = service.snapshot().unwrap();
    assert_eq!(item_state(&committed.items[&ids[0]]), *projected);
    assert_eq!(changes.invalidations.len(), 1);
    assert!(matches!(
        changes.invalidations[0],
        ProjectInvalidation::TimelineRange { .. }
    ));
}

#[test]
fn stale_plan_is_rejected_without_project_or_undo_mutation() {
    let (project, track_id, ids, _) = project_with_items();
    let service = TimelineEditorService::new(project).unwrap();
    let plan = service
        .plan_timeline_edit(TimelineEditRequest::move_item(
            service.revision().unwrap(),
            ids[0],
            track_id,
            time(10, 1),
            1,
        ))
        .unwrap();
    service
        .add_track(
            service.snapshot().unwrap().root_timeline_id,
            "Audio".to_string(),
            TimelineTrackKind::Audio,
        )
        .unwrap();
    let after_intervening_edit = service.snapshot().unwrap();
    let revision = service.revision().unwrap();

    assert_eq!(
        service.commit_edit_plan(&plan).unwrap_err(),
        TimelineEditError::StaleRevision {
            base_revision: ProjectRevision::initial(),
            current_revision: revision,
        }
    );
    assert_eq!(
        service.snapshot().unwrap().as_ref(),
        after_intervening_edit.as_ref()
    );
    assert_eq!(service.revision().unwrap(), revision);

    service
        .undo()
        .unwrap()
        .expect("only the intervening edit exists");
    assert!(!service.can_undo().unwrap());
}

#[test]
fn noop_plan_does_not_advance_revision_or_create_undo() {
    let (project, track_id, ids, _) = project_with_items();
    let service = TimelineEditorService::new(project).unwrap();
    let before = service.snapshot().unwrap();
    let revision = service.revision().unwrap();
    let item = &before.items[&ids[1]];
    let plan = service
        .plan_timeline_edit(TimelineEditRequest::move_item(
            revision,
            item.id,
            track_id,
            item.interval.start,
            item.layer,
        ))
        .unwrap();

    assert!(plan.is_noop());
    assert!(service.project_edit_plan(&plan).unwrap().is_empty());
    let changes = service.commit_edit_plan(&plan).unwrap();
    assert_eq!(changes.revision, revision);
    assert!(changes.invalidations.is_empty());
    assert_eq!(service.revision().unwrap(), revision);
    assert!(!service.can_undo().unwrap());
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}

#[test]
fn expected_before_rejects_a_same_revision_project_with_different_item_state() {
    let (project, track_id, ids, _) = project_with_items();
    let source = TimelineEditorService::new(project.clone()).unwrap();
    let plan = source
        .plan_timeline_edit(TimelineEditRequest::move_item(
            ProjectRevision::initial(),
            ids[0],
            track_id,
            time(9, 1),
            2,
        ))
        .unwrap();
    let mut divergent = project;
    divergent.items.get_mut(&ids[0]).unwrap().interval.start = time(1, 2);
    divergent.validate().unwrap();
    let target = TimelineEditorService::new(divergent).unwrap();

    assert_eq!(
        target.commit_edit_plan(&plan).unwrap_err(),
        TimelineEditError::ExpectedItemChanged(ids[0])
    );
    assert_eq!(target.revision().unwrap(), ProjectRevision::initial());
    assert!(!target.can_undo().unwrap());
}

#[test]
fn transition_participant_move_and_trim_are_rejected_during_planning() {
    let (project, track_id, from, _) = adjacent_transition_project();
    for operation in [
        TimelineEditOperation::MoveItem {
            item_id: from,
            track_id,
            start: time(4, 1),
            layer: 0,
        },
        TimelineEditOperation::TrimItem {
            item_id: from,
            interval: TimelineInterval::new(time(0, 1), time(4, 1)).unwrap(),
        },
    ] {
        let service = TimelineEditorService::new(project.clone()).unwrap();
        let before = service.snapshot().unwrap();
        let revision = service.revision().unwrap();

        let message = invalid_request_message(
            service
                .plan_timeline_edit(TimelineEditRequest {
                    base_revision: revision,
                    operation,
                })
                .unwrap_err(),
        );
        assert!(message.contains("Transition") && message.contains("from item"));
        assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
        assert_eq!(service.revision().unwrap(), revision);
        assert!(!service.can_undo().unwrap());
    }
}

#[test]
fn transition_invariant_is_rechecked_when_projecting_and_committing_a_plan() {
    let (project, track_id, from, _) = adjacent_transition_project();
    let mut without_transition = project.clone();
    without_transition.transitions.clear();
    without_transition.validate().unwrap();
    let source = TimelineEditorService::new(without_transition).unwrap();
    let plan = source
        .plan_timeline_edit(TimelineEditRequest::move_item(
            ProjectRevision::initial(),
            from,
            track_id,
            time(4, 1),
            0,
        ))
        .unwrap();
    let target = TimelineEditorService::new(project).unwrap();
    let before = target.snapshot().unwrap();

    let preview_error = invalid_request_message(target.project_edit_plan(&plan).unwrap_err());
    let commit_error = invalid_request_message(target.commit_edit_plan(&plan).unwrap_err());
    assert_eq!(preview_error, commit_error);
    assert!(preview_error.contains("Transition") && preview_error.contains("from item"));
    assert_eq!(target.snapshot().unwrap().as_ref(), before.as_ref());
    assert_eq!(target.revision().unwrap(), ProjectRevision::initial());
    assert!(!target.can_undo().unwrap());
}

#[test]
fn partial_visible_overlap_is_rejected_during_planning() {
    let (project, _, from, _) = adjacent_transition_project();
    let service = TimelineEditorService::new(project).unwrap();
    let before = service.snapshot().unwrap();
    let interval = TimelineInterval::new(time(0, 1), time(6, 1)).unwrap();
    let message = invalid_request_message(
        service
            .plan_timeline_edit(TimelineEditRequest::trim_item(
                ProjectRevision::initial(),
                from,
                interval,
            ))
            .unwrap_err(),
    );

    assert!(message.contains("visible overlap exactly equal to its interval"));
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
    assert!(!service.can_undo().unwrap());
}

#[test]
fn exact_transition_interval_overlap_is_valid() {
    let (mut project, _, from, to) = adjacent_transition_project();
    project.items.get_mut(&from).unwrap().interval =
        TimelineInterval::new(time(0, 1), time(7, 1)).unwrap();
    project.items.get_mut(&to).unwrap().interval =
        TimelineInterval::new(time(3, 1), time(7, 1)).unwrap();

    project.validate().unwrap();
    TimelineEditorService::new(project).unwrap();
}

#[test]
fn overlapping_same_media_transitions_on_one_item_fail_full_and_overlay_validation() {
    let (mut project, track_id, _, shared) = adjacent_transition_project();
    let timeline_id = project.root_timeline_id;
    project.items.get_mut(&shared).unwrap().interval =
        TimelineInterval::new(time(5, 1), time(2, 1)).unwrap();
    let third = TimelineItemId::new();
    project.items.insert(
        third,
        TimelineItem {
            id: third,
            track_id,
            name: "Third".to_string(),
            source: solid(192),
            interval: TimelineInterval::new(time(7, 1), time(5, 1)).unwrap(),
            time_map: TimeMap::default(),
            layer: 2,
            parent: None,
            blend_mode: BlendMode::Normal,
            authored_properties: PropertyMap::new(),
        },
    );
    let transition_id = TransitionId::new();
    project.transitions.insert(
        transition_id,
        Transition {
            id: transition_id,
            timeline_id,
            from_item_id: shared,
            to_item_id: third,
            edit_point: time(7, 1),
            duration: time(4, 1),
            alignment: TransitionAlignment::CenteredOnEdit,
            processor: TransitionProcessor::cross_dissolve(),
            parameters: HashMap::new(),
        },
    );

    let full_error = project.validate().unwrap_err();
    assert!(full_error.contains("overlap") && full_error.contains("Image media"));

    let index = TimelineEditPlanningIndex::build(&project, ProjectRevision::initial()).unwrap();
    let replacements = BTreeMap::from([(shared, item_state(&project.items[&shared]))]);
    let overlay_error = project
        .validate_timeline_item_placement_overlay(&index, &replacements)
        .unwrap_err();
    assert_eq!(overlay_error, full_error);
}

#[test]
fn inactive_item_between_transition_participant_layers_is_allowed_during_planning() {
    let (mut project, track_id, _, to) = adjacent_transition_project();
    project.items.get_mut(&to).unwrap().layer = 2;
    let inserted = TimelineItemId::new();
    project.items.insert(
        inserted,
        TimelineItem {
            id: inserted,
            track_id,
            name: "Inserted".to_string(),
            source: solid(64),
            interval: TimelineInterval::new(time(12, 1), time(1, 1)).unwrap(),
            time_map: TimeMap::default(),
            layer: 1,
            parent: None,
            blend_mode: BlendMode::Normal,
            authored_properties: PropertyMap::new(),
        },
    );
    project.validate().unwrap();
    let service = TimelineEditorService::new(project).unwrap();

    service
        .plan_timeline_edit(TimelineEditRequest::move_item(
            ProjectRevision::initial(),
            inserted,
            track_id,
            time(13, 1),
            1,
        ))
        .expect("an inactive layer cannot alter Transition compositing");
}

#[test]
fn active_item_between_transition_participant_layers_is_rejected_during_planning() {
    let (mut project, track_id, _, to) = adjacent_transition_project();
    project.items.get_mut(&to).unwrap().layer = 2;
    let inserted = TimelineItemId::new();
    project.items.insert(
        inserted,
        TimelineItem {
            id: inserted,
            track_id,
            name: "Inserted".to_string(),
            source: solid(64),
            interval: TimelineInterval::new(time(12, 1), time(2, 1)).unwrap(),
            time_map: TimeMap::default(),
            layer: 1,
            parent: None,
            blend_mode: BlendMode::Normal,
            authored_properties: PropertyMap::new(),
        },
    );
    project.validate().unwrap();
    let service = TimelineEditorService::new(project).unwrap();
    let before = service.snapshot().unwrap();

    let message = invalid_request_message(
        service
            .plan_timeline_edit(TimelineEditRequest::move_item(
                ProjectRevision::initial(),
                inserted,
                track_id,
                time(4, 1),
                1,
            ))
            .unwrap_err(),
    );

    assert!(message.contains("active item between its participant layers"));
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
    assert!(!service.can_undo().unwrap());
}

#[test]
fn moving_a_child_across_timelines_is_rejected_before_preview() {
    let service = TimelineEditorService::create_default("parent scope").unwrap();
    let initial = service.snapshot().unwrap();
    let root_track = initial.timelines[&initial.root_timeline_id].track_order[0];
    drop(initial);
    let (parent, _) = service
        .add_item(
            root_track,
            "Parent".to_string(),
            solid(1),
            TimelineInterval::new(time(0, 1), time(5, 1)).unwrap(),
            0,
        )
        .unwrap();
    let (child, _) = service
        .add_item(
            root_track,
            "Child".to_string(),
            solid(2),
            TimelineInterval::new(time(0, 1), time(5, 1)).unwrap(),
            1,
        )
        .unwrap();
    service.set_item_parent(child, Some(parent)).unwrap();
    let before_add_timeline = service.snapshot().unwrap();
    let (_, other_track, _) = service
        .add_timeline(
            "Other".to_string(),
            1920,
            1080,
            RationalRate::new(30, 1).unwrap(),
            time(20, 1),
        )
        .unwrap();
    let before_failure = service.snapshot().unwrap();
    let revision = service.revision().unwrap();

    let message = invalid_request_message(
        service
            .plan_timeline_edit(TimelineEditRequest::move_item(
                revision,
                child,
                other_track,
                time(0, 1),
                0,
            ))
            .unwrap_err(),
    );
    assert!(message.contains("invalid parent"));
    assert_eq!(
        service.snapshot().unwrap().as_ref(),
        before_failure.as_ref()
    );
    assert_eq!(service.revision().unwrap(), revision);
    service
        .undo()
        .unwrap()
        .expect("only add_timeline is newest");
    assert_eq!(
        service.snapshot().unwrap().as_ref(),
        before_add_timeline.as_ref()
    );
}

#[test]
fn moving_a_node_clip_input_across_timelines_is_rejected_before_preview() {
    let service = TimelineEditorService::create_default("Node Clip scope").unwrap();
    let initial = service.snapshot().unwrap();
    let root_track = initial.timelines[&initial.root_timeline_id].track_order[0];
    drop(initial);
    let (source_item, _) = service
        .add_item(
            root_track,
            "Source".to_string(),
            solid(8),
            TimelineInterval::new(time(0, 1), time(5, 1)).unwrap(),
            0,
        )
        .unwrap();
    let (definition, output_id, input_id) = same_timeline_input_module();
    let definition_id = definition.id;
    service.add_module_definition(definition).unwrap();
    service
        .place_module_item(
            definition_id,
            ModuleItemPlacement {
                track_id: root_track,
                name: "Dependent Node Clip".to_string(),
                output_id,
                interval: TimelineInterval::new(time(0, 1), time(5, 1)).unwrap(),
                layer: 1,
                parameter_overrides: HashMap::new(),
                input_bindings: HashMap::from([(
                    input_id,
                    MediaInputBinding::TimelineItemOutput {
                        locator: InstanceLocator::SameTimeline,
                        item_id: source_item,
                        output: MediaOutputKind::Image,
                        stage: ItemOutputStage::PostTransform,
                    },
                )]),
            },
        )
        .unwrap();
    let before_add_timeline = service.snapshot().unwrap();
    let (_, other_track, _) = service
        .add_timeline(
            "Other".to_string(),
            1920,
            1080,
            RationalRate::new(30, 1).unwrap(),
            time(20, 1),
        )
        .unwrap();
    let before_failure = service.snapshot().unwrap();
    let revision = service.revision().unwrap();

    let message = invalid_request_message(
        service
            .plan_timeline_edit(TimelineEditRequest::move_item(
                revision,
                source_item,
                other_track,
                time(0, 1),
                0,
            ))
            .unwrap_err(),
    );
    assert!(message.contains("Same-Timeline media binding"));
    assert_eq!(
        service.snapshot().unwrap().as_ref(),
        before_failure.as_ref()
    );
    assert_eq!(service.revision().unwrap(), revision);
    service
        .undo()
        .unwrap()
        .expect("only add_timeline is newest");
    assert_eq!(
        service.snapshot().unwrap().as_ref(),
        before_add_timeline.as_ref()
    );
}

#[test]
fn ten_thousand_item_horizontal_preview_reuses_index_and_validates_one_item() {
    const ITEM_COUNT: usize = 10_000;
    const MOVED_INDEX: usize = 5_000;
    let mut project = AuthoringProject::new(
        "10k sparse planning",
        1920,
        1080,
        RationalRate::new(60, 1).unwrap(),
        time(60, 1),
    )
    .unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let mut moved_item_id = None;
    for layer in 0..ITEM_COUNT {
        let item_id = TimelineItemId::new();
        if layer == MOVED_INDEX {
            moved_item_id = Some(item_id);
        }
        project.items.insert(
            item_id,
            TimelineItem {
                id: item_id,
                track_id,
                name: format!("Clip {layer}"),
                source: solid((layer % 255) as u8),
                interval: TimelineInterval::new(time(0, 1), time(1, 1)).unwrap(),
                time_map: TimeMap::default(),
                layer: layer as i64,
                parent: None,
                blend_mode: BlendMode::Normal,
                authored_properties: PropertyMap::new(),
            },
        );
    }
    project.validate().unwrap();
    let service = TimelineEditorService::new(project).unwrap();
    let first_index = service.timeline_edit_planning_index().unwrap();
    assert_eq!(first_index.indexed_item_count(), ITEM_COUNT);

    let plan = service
        .plan_timeline_edit(TimelineEditRequest::move_item(
            ProjectRevision::initial(),
            moved_item_id.unwrap(),
            track_id,
            time(1, 3),
            MOVED_INDEX as i64,
        ))
        .unwrap();
    assert_eq!(plan.changed_item_count(), 1);
    assert_eq!(
        plan.validation_scope(),
        EditPlanValidationScope {
            items: 1,
            transitions: 0,
            attachments: 0,
            composition_parameters: 0,
            moved_compositions: 0,
        }
    );
    let second_index = service.timeline_edit_planning_index().unwrap();
    assert!(
        Arc::ptr_eq(&first_index, &second_index),
        "pointer previews at one revision must reuse the immutable index"
    );
}
