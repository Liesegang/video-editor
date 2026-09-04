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
