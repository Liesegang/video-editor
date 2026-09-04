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
