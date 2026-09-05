use super::*;

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).expect("whole seconds")
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

fn track_layers(
    project: &AuthoringProject,
    track_id: TimelineTrackId,
) -> Vec<(i64, TimelineItemId)> {
    let mut layers = project
        .items
        .values()
        .filter(|item| item.track_id == track_id)
        .map(|item| (item.layer, item.id))
        .collect::<Vec<_>>();
    layers.sort_by_key(|entry| entry.0);
    layers
}

#[test]
fn inserting_and_reordering_items_keeps_one_unambiguous_layer_per_row() {
    let service = TimelineEditorService::create_default("layer rows").unwrap();
    let project = service.snapshot().unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);

    let mut ids = Vec::new();
    for index in 0..3 {
        let (item_id, _) = service
            .add_item(
                track_id,
                format!("Clip {index}"),
                solid(index),
                TimelineInterval::new(seconds(index.into()), seconds(2)).unwrap(),
                0,
            )
            .unwrap();
        ids.push(item_id);
    }

    let before_move = service.snapshot().unwrap();
    assert_eq!(
        track_layers(&before_move, track_id)
            .iter()
            .map(|entry| entry.0)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    let untouched_starts = ids
        .iter()
        .map(|id| (*id, before_move.items[id].interval.start))
        .collect::<HashMap<_, _>>();
    let projected_order = track_item_ids_after_placement(&before_move, track_id, ids[0], 2);

    service.move_item(ids[0], track_id, seconds(9), 2).unwrap();
    let moved = service.snapshot().unwrap();
    assert_eq!(moved.items[&ids[0]].interval.start, seconds(9));
    assert_eq!(moved.items[&ids[0]].layer, 2);
    for item_id in &ids[1..] {
        assert_eq!(
            moved.items[item_id].interval.start, untouched_starts[item_id],
            "moving one clip must not move a sibling"
        );
    }
    assert_eq!(
        track_layers(&moved, track_id)
            .iter()
            .map(|entry| entry.0)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert_eq!(
        ordered_track_item_ids(&moved, track_id, None),
        projected_order,
        "the pure placement query must match the committed service order"
    );
}

#[test]
fn a_layer_move_and_its_reindexing_are_one_undo_step() {
    let service = TimelineEditorService::create_default("atomic layer move").unwrap();
    let project = service.snapshot().unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let (first, _) = service
        .add_item(
            track_id,
            "First".to_string(),
            solid(1),
            TimelineInterval::new(seconds(0), seconds(2)).unwrap(),
            0,
        )
        .unwrap();
    service
        .add_item(
            track_id,
            "Second".to_string(),
            solid(2),
            TimelineInterval::new(seconds(3), seconds(2)).unwrap(),
            1,
        )
        .unwrap();
    let before = service.snapshot().unwrap();

    service.move_item(first, track_id, seconds(7), 1).unwrap();
    service.undo().unwrap().expect("one move transaction");

    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}

#[test]
fn grouped_move_preserves_offsets_and_order_across_tracks_in_one_undo_step() {
    let service = TimelineEditorService::create_default("group move").unwrap();
    let project = service.snapshot().unwrap();
    let timeline_id = project.root_timeline_id;
    let source_track_id = project.timelines[&timeline_id].track_order[0];
    drop(project);
    let (target_track_id, _) = service
        .add_track(
            timeline_id,
            "Video 2".to_string(),
            TimelineTrackKind::Visual,
        )
        .unwrap();
    let (first, _) = service
        .add_item(
            source_track_id,
            "First".to_string(),
            solid(1),
            TimelineInterval::new(seconds(2), seconds(2)).unwrap(),
            0,
        )
        .unwrap();
    let (primary, _) = service
        .add_item(
            source_track_id,
            "Primary".to_string(),
            solid(2),
            TimelineInterval::new(seconds(5), seconds(2)).unwrap(),
            1,
        )
        .unwrap();
    let (remaining, _) = service
        .add_item(
            source_track_id,
            "Remaining".to_string(),
            solid(3),
            TimelineInterval::new(seconds(8), seconds(2)).unwrap(),
            2,
        )
        .unwrap();
    let (target_existing, _) = service
        .add_item(
            target_track_id,
            "Target existing".to_string(),
            solid(4),
            TimelineInterval::new(seconds(1), seconds(2)).unwrap(),
            0,
        )
        .unwrap();
    let before_move = service.snapshot().unwrap();
    let before_revision = service.revision().unwrap();

    service
        .move_items(&[first, primary], primary, target_track_id, seconds(20), 1)
        .unwrap();

    assert_eq!(service.revision().unwrap().get(), before_revision.get() + 1);
    let moved = service.snapshot().unwrap();
    assert_eq!(moved.items[&first].track_id, target_track_id);
    assert_eq!(moved.items[&primary].track_id, target_track_id);
    assert_eq!(moved.items[&first].interval.start, seconds(17));
    assert_eq!(moved.items[&primary].interval.start, seconds(20));
    assert_eq!(moved.items[&first].layer, 0);
    assert_eq!(moved.items[&primary].layer, 1);
    assert_eq!(moved.items[&target_existing].layer, 2);
    assert_eq!(moved.items[&remaining].track_id, source_track_id);
    assert_eq!(moved.items[&remaining].layer, 0);

    service
        .undo()
        .unwrap()
        .expect("one grouped move transaction");
    assert_eq!(service.snapshot().unwrap().as_ref(), before_move.as_ref());
}

#[test]
fn grouped_horizontal_move_does_not_collapse_noncontiguous_layers() {
    let service = TimelineEditorService::create_default("horizontal group move").unwrap();
    let project = service.snapshot().unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let mut item_ids = Vec::new();
    for index in 0..3 {
        let (item_id, _) = service
            .add_item(
                track_id,
                format!("Clip {index}"),
                solid(index),
                TimelineInterval::new(seconds(index.into()), seconds(2)).unwrap(),
                index.into(),
            )
            .unwrap();
        item_ids.push(item_id);
    }

    service
        .move_items(
            &[item_ids[0], item_ids[2]],
            item_ids[2],
            track_id,
            seconds(7),
            2,
        )
        .unwrap();

    let moved = service.snapshot().unwrap();
    assert_eq!(moved.items[&item_ids[0]].layer, 0);
    assert_eq!(moved.items[&item_ids[1]].layer, 1);
    assert_eq!(moved.items[&item_ids[2]].layer, 2);
    assert_eq!(moved.items[&item_ids[0]].interval.start, seconds(5));
    assert_eq!(moved.items[&item_ids[1]].interval.start, seconds(1));
    assert_eq!(moved.items[&item_ids[2]].interval.start, seconds(7));
}

#[test]
fn blend_mode_is_owned_by_one_placement_and_is_undoable() {
    let service = TimelineEditorService::create_default("placement blend").unwrap();
    let project = service.snapshot().unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let (first, _) = service
        .add_item(
            track_id,
            "First".to_string(),
            solid(32),
            TimelineInterval::new(seconds(0), seconds(2)).unwrap(),
            0,
        )
        .unwrap();
    let (sibling, _) = service
        .add_item(
            track_id,
            "Sibling".to_string(),
            solid(64),
            TimelineInterval::new(seconds(0), seconds(2)).unwrap(),
            1,
        )
        .unwrap();

    service
        .set_item_blend_mode(first, BlendMode::Multiply)
        .unwrap();
    let changed = service.snapshot().unwrap();
    assert_eq!(changed.items[&first].blend_mode, BlendMode::Multiply);
    assert_eq!(changed.items[&sibling].blend_mode, BlendMode::Normal);

    service.undo().unwrap().expect("blend transaction");
    let undone = service.snapshot().unwrap();
    assert_eq!(undone.items[&first].blend_mode, BlendMode::Normal);
    assert_eq!(undone.items[&sibling].blend_mode, BlendMode::Normal);
}

#[test]
fn direct_manipulation_updates_position_and_scale_in_one_transaction() {
    let service = TimelineEditorService::create_default("atomic transform gesture").unwrap();
    let project = service.snapshot().unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let (item_id, _) = service
        .add_item(
            track_id,
            "Item".to_string(),
            solid(255),
            TimelineInterval::new(seconds(0), seconds(2)).unwrap(),
            0,
        )
        .unwrap();
    let vector = |x, y| {
        PropertyValue::Vec2(crate::model::property::Vec2 {
            x: ordered_float::OrderedFloat(x),
            y: ordered_float::OrderedFloat(y),
        })
    };
    service
        .set_authored_property_constant(
            AuthoringPropertyOwner::Item(item_id),
            "position".to_string(),
            vector(0.0, 0.0),
        )
        .unwrap();
    service
        .set_authored_property_constant(
            AuthoringPropertyOwner::Item(item_id),
            "scale".to_string(),
            vector(1.0, 1.0),
        )
        .unwrap();
    let before = service.revision().unwrap();

    service
        .apply_authored_property_values(
            AuthoringPropertyOwner::Item(item_id),
            vec![
                AuthoringPropertyValueUpdate {
                    key: "position".to_string(),
                    value: vector(25.0, 0.0),
                    target: AuthoringPropertyValueTarget::Constant,
                },
                AuthoringPropertyValueUpdate {
                    key: "scale".to_string(),
                    value: vector(1.5, 1.0),
                    target: AuthoringPropertyValueTarget::Constant,
                },
            ],
        )
        .unwrap();

    assert_eq!(service.revision().unwrap().get(), before.get() + 1);
    let changed = service.snapshot().unwrap();
    assert_eq!(
        changed.items[&item_id]
            .authored_properties
            .get("position")
            .unwrap()
            .evaluate_at(0.0)
            .unwrap(),
        vector(25.0, 0.0)
    );
    assert_eq!(
        changed.items[&item_id]
            .authored_properties
            .get("scale")
            .unwrap()
            .evaluate_at(0.0)
            .unwrap(),
        vector(1.5, 1.0)
    );

    service.undo().unwrap().expect("one transform undo entry");
    let undone = service.snapshot().unwrap();
    assert_eq!(
        undone.items[&item_id]
            .authored_properties
            .get("position")
            .unwrap()
            .evaluate_at(0.0)
            .unwrap(),
        vector(0.0, 0.0)
    );
    assert_eq!(
        undone.items[&item_id]
            .authored_properties
            .get("scale")
            .unwrap()
            .evaluate_at(0.0)
            .unwrap(),
        vector(1.0, 1.0)
    );
}

#[test]
fn direct_manipulation_does_not_replace_expression_ownership() {
    let service = TimelineEditorService::create_default("expression ownership").unwrap();
    let project = service.snapshot().unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let (item_id, _) = service
        .add_item(
            track_id,
            "Item".to_string(),
            solid(255),
            TimelineInterval::new(seconds(0), seconds(2)).unwrap(),
            0,
        )
        .unwrap();
    let value = PropertyValue::Vec2(crate::model::property::Vec2 {
        x: ordered_float::OrderedFloat(1.0),
        y: ordered_float::OrderedFloat(1.0),
    });
    service
        .set_authored_property(
            AuthoringPropertyOwner::Item(item_id),
            "scale".to_string(),
            Property::expression("signal".to_string(), value.clone()),
        )
        .unwrap();
    let before = service.revision().unwrap();

    let error = service
        .apply_authored_property_values(
            AuthoringPropertyOwner::Item(item_id),
            vec![AuthoringPropertyValueUpdate {
                key: "scale".to_string(),
                value,
                target: AuthoringPropertyValueTarget::Constant,
            }],
        )
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("changed from Constant to 'expression'")
    );
    assert_eq!(service.revision().unwrap(), before);
    assert_eq!(
        service.snapshot().unwrap().items[&item_id]
            .authored_properties
            .get("scale")
            .unwrap()
            .evaluator,
        "expression"
    );
}
