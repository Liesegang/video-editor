use super::*;

fn root_track(service: &TimelineEditorService) -> TimelineTrackId {
    let project = service.snapshot().unwrap();
    project.timelines[&project.root_timeline_id].track_order[0]
}

#[test]
fn track_visual_enabled_is_one_undoable_sparse_authored_property() {
    let service = TimelineEditorService::create_default("Track visibility").unwrap();
    let track_id = root_track(&service);
    service
        .add_item(
            track_id,
            "Owned source".to_string(),
            SourceRef::Solid {
                color: Color::white(),
            },
            TimelineInterval::new(MediaTime::zero(), MediaTime::new(1, 1).unwrap()).unwrap(),
            0,
        )
        .unwrap();
    let original = service.snapshot().unwrap();
    assert!(original.tracks[&track_id].is_visually_enabled().unwrap());
    assert!(
        original.tracks[&track_id]
            .authored_properties
            .get(TRACK_VISIBILITY_PROPERTY)
            .is_none()
    );

    let changes = service.set_track_visual_enabled(track_id, false).unwrap();
    assert_eq!(
        changes.invalidations,
        vec![ProjectInvalidation::TimelineStructure {
            timeline_id: original.root_timeline_id,
        }]
    );
    let hidden = service.snapshot().unwrap();
    assert!(!hidden.tracks[&track_id].is_visually_enabled().unwrap());
    assert_eq!(hidden.items, original.items);
    assert_eq!(hidden.module_definitions, original.module_definitions);

    service.undo().unwrap().expect("visibility undo");
    let restored = service.snapshot().unwrap();
    assert!(restored.tracks[&track_id].is_visually_enabled().unwrap());
    assert!(
        restored.tracks[&track_id]
            .authored_properties
            .get(TRACK_VISIBILITY_PROPERTY)
            .is_none()
    );
    service.redo().unwrap().expect("visibility redo");
    assert!(
        !service.snapshot().unwrap().tracks[&track_id]
            .is_visually_enabled()
            .unwrap()
    );

    service.set_track_visual_enabled(track_id, true).unwrap();
    let enabled = service.snapshot().unwrap();
    assert!(enabled.tracks[&track_id].is_visually_enabled().unwrap());
    assert!(
        enabled.tracks[&track_id]
            .authored_properties
            .get(TRACK_VISIBILITY_PROPERTY)
            .is_none(),
        "enabled is the sparse default, not a second persisted state"
    );
}

#[test]
fn track_visual_visibility_round_trips_through_project_storage() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("visibility.ruvie");
    let service = TimelineEditorService::create_default("Track visibility save").unwrap();
    let track_id = root_track(&service);
    service.set_track_visual_enabled(track_id, false).unwrap();
    service.save_as(&path).unwrap();

    let loaded = TimelineEditorService::open(&path).unwrap();
    assert!(
        !loaded.snapshot().unwrap().tracks[&track_id]
            .is_visually_enabled()
            .unwrap()
    );
}

#[test]
fn track_visual_visibility_rejects_untyped_or_animated_state_atomically() {
    let service = TimelineEditorService::create_default("Track visibility validation").unwrap();
    let track_id = root_track(&service);
    for property in [
        Property::constant(PropertyValue::Number(1.0.into())),
        Property::expression("true".to_string(), PropertyValue::Boolean(true)),
    ] {
        let before = service.snapshot().unwrap();
        let error = service
            .set_authored_property(
                AuthoringPropertyOwner::Track(track_id),
                TRACK_VISIBILITY_PROPERTY.to_string(),
                property,
            )
            .expect_err("visibility schema must reject invalid state")
            .to_string();
        assert!(error.contains("Constant Boolean"), "{error}");
        assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
    }
    let before = service.snapshot().unwrap();
    let error = service
        .set_authored_property_keyframe_mode(
            AuthoringPropertyOwner::Track(track_id),
            TRACK_VISIBILITY_PROPERTY.to_string(),
            MediaTime::zero(),
            PropertyValue::Boolean(false),
        )
        .expect_err("visibility must not become keyframed")
        .to_string();
    assert!(error.contains("Constant Boolean"), "{error}");
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}

#[test]
fn track_reorder_preserves_every_clip_placement_and_is_one_undoable_edit() {
    let service = TimelineEditorService::create_default("Track reorder").unwrap();
    let before_tracks = service.snapshot().unwrap();
    let timeline_id = before_tracks.root_timeline_id;
    let back = before_tracks.timelines[&timeline_id].track_order[0];
    let (middle, _) = service
        .add_track(
            timeline_id,
            "Middle".to_string(),
            TimelineTrackKind::AudioVisual,
        )
        .unwrap();
    let (front, _) = service
        .add_track(timeline_id, "Front".to_string(), TimelineTrackKind::Visual)
        .unwrap();
    service
        .add_item(
            back,
            "Back clip".to_string(),
            SourceRef::Solid {
                color: Color::white(),
            },
            TimelineInterval::new(MediaTime::new(2, 1).unwrap(), MediaTime::new(3, 1).unwrap())
                .unwrap(),
            4,
        )
        .unwrap();
    service
        .add_item(
            middle,
            "Middle clip".to_string(),
            SourceRef::Solid {
                color: Color::black(),
            },
            TimelineInterval::new(MediaTime::new(1, 1).unwrap(), MediaTime::new(2, 1).unwrap())
                .unwrap(),
            -3,
        )
        .unwrap();
    let original = service.snapshot().unwrap();
    assert_eq!(
        original.timelines[&timeline_id].track_order,
        vec![back, middle, front]
    );

    let changes = service.reorder_track(timeline_id, back, 2).unwrap();
    assert_eq!(
        changes.invalidations,
        vec![ProjectInvalidation::TimelineStructure { timeline_id }]
    );
    let reordered = service.snapshot().unwrap();
    assert_eq!(
        reordered.timelines[&timeline_id].track_order,
        vec![middle, front, back]
    );
    assert_eq!(
        reordered.items, original.items,
        "Track order must not rewrite Clip track, time, source, parent, or layer placement"
    );
    assert_eq!(reordered.tracks, original.tracks);
    assert_eq!(reordered.module_definitions, original.module_definitions);
    assert_eq!(reordered.module_instances, original.module_instances);

    service.undo().unwrap().expect("one reorder undo");
    assert_eq!(service.snapshot().unwrap().as_ref(), original.as_ref());
    service.redo().unwrap().expect("one reorder redo");
    assert_eq!(service.snapshot().unwrap().as_ref(), reordered.as_ref());
}
