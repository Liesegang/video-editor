use std::collections::HashSet;

use library::animation::EasingFunction;
use library::model::authoring::{
    AuthoringProject, MediaTime, RationalRate, SourceRef, TimelineInterval, TimelineItem,
    TimelineItemId, TimelineTrackId,
};
use library::model::property::{Keyframe, Property, PropertyMap, PropertyValue};

use crate::state::authoring::{TimelineGestureKind, TimelineItemGesture};

use super::interaction::timeline_row_projection;
use super::rows::property_row_items;
use super::{display_rows, projected_gesture_for_item, RowKind};

pub(super) fn fixture() -> (AuthoringProject, TimelineTrackId, Vec<TimelineItemId>) {
    let mut project = AuthoringProject::new(
        "Timeline rows",
        1920,
        1080,
        RationalRate::new(30, 1).expect("valid frame rate"),
        MediaTime::new(30, 1).expect("valid duration"),
    )
    .expect("valid project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let mut item_ids = Vec::new();
    for (name, layer, start) in [("bottom", 0, 0), ("top", 2, 1), ("middle", 1, 2)] {
        let item = TimelineItem {
            id: TimelineItemId::new(),
            track_id,
            name: name.to_string(),
            source: SourceRef::Text {
                text: name.to_string(),
                appearance_operations: Vec::new(),
                ensemble_operations: Vec::new(),
            },
            interval: TimelineInterval::new(
                MediaTime::new(start, 1).expect("valid start"),
                MediaTime::new(5, 1).expect("valid clip duration"),
            )
            .expect("valid interval"),
            time_map: Default::default(),
            layer,
            parent: None,
            blend_mode: library::model::BlendMode::Normal,
            authored_properties: PropertyMap::new(),
        };
        item_ids.push(item.id);
        project.items.insert(item.id, item);
    }
    (project, track_id, item_ids)
}

fn keyframe_one_property(project: &mut AuthoringProject, item_id: TimelineItemId) {
    project
        .items
        .get_mut(&item_id)
        .expect("item")
        .authored_properties
        .set(
            "position".to_string(),
            Property::keyframe(vec![
                Keyframe::new(0.0, PropertyValue::from(0.0), EasingFunction::Linear),
                Keyframe::new(1.0, PropertyValue::from(1.0), EasingFunction::Linear),
            ]),
        );
}

#[test]
fn expanded_track_exposes_each_clip_as_its_own_layer_row() {
    let (project, track_id, _) = fixture();
    let rows = display_rows(
        &project,
        project.root_timeline_id,
        &HashSet::from([track_id]),
        &HashSet::new(),
        None,
    );

    assert_eq!(rows.len(), 4, "one Track row plus its three Clip rows");
    assert!(matches!(
        rows[0].kind,
        RowKind::Track {
            track_id: id,
            expanded: true
        } if id == track_id
    ));
    let layers = rows[1..]
        .iter()
        .map(|row| match row.kind {
            RowKind::Clip { item_id, .. } => project.items[&item_id].layer,
            RowKind::Track { .. } | RowKind::Property { .. } => {
                panic!("expanded children must be Clip rows")
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(layers, vec![2, 1, 0]);
}

#[test]
fn collapsed_track_is_one_compact_track_row() {
    let (project, track_id, _) = fixture();
    let rows = display_rows(
        &project,
        project.root_timeline_id,
        &HashSet::new(),
        &HashSet::new(),
        None,
    );

    assert_eq!(rows.len(), 1);
    assert!(matches!(
        rows[0].kind,
        RowKind::Track {
            track_id: id,
            expanded: false
        } if id == track_id
    ));
}

#[test]
fn expanded_clip_adds_shared_property_rows_directly_after_its_clip() {
    let (mut project, track_id, item_ids) = fixture();
    keyframe_one_property(&mut project, item_ids[2]);
    let rows = display_rows(
        &project,
        project.root_timeline_id,
        &HashSet::from([track_id]),
        &HashSet::from([item_ids[2]]),
        None,
    );

    let clip_row = rows
        .iter()
        .position(|row| matches!(row.kind, RowKind::Clip { item_id, .. } if item_id == item_ids[2]))
        .expect("expanded clip row");
    assert!(matches!(
        &rows[clip_row + 1].kind,
        RowKind::Property { item_id, lane }
            if *item_id == item_ids[2]
                && lane.target == crate::state::authoring::AutomationTarget::AuthoredProperty("position".to_string())
    ));
}

#[test]
fn keyframe_mode_keeps_constant_properties_out_of_the_dope_sheet() {
    let (mut project, track_id, item_ids) = fixture();
    project
        .items
        .get_mut(&item_ids[0])
        .unwrap()
        .authored_properties
        .set(
            "opacity".to_string(),
            Property::constant(PropertyValue::from(0.5)),
        );
    keyframe_one_property(&mut project, item_ids[2]);
    let mut view = crate::state::authoring::AuthoringTimelineView::default();
    view.expanded_tracks.insert(track_id);
    view.track_display_modes.insert(
        track_id,
        crate::state::authoring::TimelineClipDisplayMode::Keyframes,
    );
    let property_items = property_row_items(&project, project.root_timeline_id, &view);
    let rows = display_rows(
        &project,
        project.root_timeline_id,
        &view.expanded_tracks,
        &property_items,
        None,
    );

    assert!(rows.iter().any(|row| matches!(
        &row.kind,
        RowKind::Property { item_id, lane }
            if *item_id == item_ids[2]
                && lane.target == crate::state::authoring::AutomationTarget::AuthoredProperty("position".to_string())
    )));
    assert!(!rows.iter().any(|row| matches!(
        &row.kind,
        RowKind::Property { item_id, lane }
            if *item_id == item_ids[0]
                && lane.target == crate::state::authoring::AutomationTarget::AuthoredProperty("opacity".to_string())
    )));
}

#[test]
fn clip_drag_projection_never_leaks_to_a_sibling_clip() {
    let (project, track_id, item_ids) = fixture();
    let dragged = &project.items[&item_ids[0]];
    let sibling = &project.items[&item_ids[1]];
    let gesture = TimelineItemGesture {
        item_id: dragged.id,
        kind: TimelineGestureKind::Move,
        pointer_origin: egui::pos2(40.0, 50.0),
        original_track_id: track_id,
        original_layer: dragged.layer,
        original_interval: dragged.interval,
        projected_track_id: track_id,
        projected_layer: dragged.layer,
        projected_interval: TimelineInterval::new(
            MediaTime::new(8, 1).expect("valid projected start"),
            dragged.interval.duration,
        )
        .expect("valid projected interval"),
    };

    assert!(projected_gesture_for_item(Some(&gesture), dragged.id).is_some());
    assert!(projected_gesture_for_item(Some(&gesture), sibling.id).is_none());
    assert_eq!(
        sibling.interval.start,
        MediaTime::new(1, 1).expect("valid sibling start")
    );
}

#[test]
fn live_reorder_projection_displaces_siblings_without_mutating_the_project() {
    let (project, track_id, item_ids) = fixture();
    let original_project = project.clone();
    let dragged = &project.items[&item_ids[0]];
    let gesture = TimelineItemGesture {
        item_id: dragged.id,
        kind: TimelineGestureKind::Move,
        pointer_origin: egui::pos2(40.0, 50.0),
        original_track_id: track_id,
        original_layer: dragged.layer,
        original_interval: dragged.interval,
        projected_track_id: track_id,
        projected_layer: 1,
        projected_interval: dragged.interval,
    };
    let expanded = HashSet::from([track_id]);
    let rows = display_rows(
        &project,
        project.root_timeline_id,
        &expanded,
        &HashSet::new(),
        None,
    );
    let projection =
        timeline_row_projection(&project, &rows, &expanded, &HashSet::new(), Some(&gesture))
            .expect("active move projection");

    assert_eq!(projection.row_for_item(item_ids[1]), Some(1));
    assert_eq!(projection.row_for_item(item_ids[0]), Some(2));
    assert_eq!(projection.row_for_item(item_ids[2]), Some(3));
    assert_eq!(project, original_project, "preview must stay UI-only");

    assert!(
        timeline_row_projection(&project, &rows, &expanded, &HashSet::new(), None).is_none(),
        "cancelling drops the projection so canonical rows render again"
    );
    let canonical_items = rows[1..]
        .iter()
        .map(|row| match row.kind {
            RowKind::Clip { item_id, .. } => item_id,
            RowKind::Track { .. } | RowKind::Property { .. } => {
                panic!("expanded child must be a Clip row")
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(canonical_items, vec![item_ids[1], item_ids[2], item_ids[0]]);
}

#[test]
fn live_reorder_projection_moves_clip_and_property_rows_as_one_block() {
    let (mut project, track_id, item_ids) = fixture();
    keyframe_one_property(&mut project, item_ids[0]);
    let dragged = &project.items[&item_ids[0]];
    let gesture = TimelineItemGesture {
        item_id: dragged.id,
        kind: TimelineGestureKind::Move,
        pointer_origin: egui::pos2(40.0, 50.0),
        original_track_id: track_id,
        original_layer: dragged.layer,
        original_interval: dragged.interval,
        projected_track_id: track_id,
        projected_layer: 2,
        projected_interval: dragged.interval,
    };
    let expanded_tracks = HashSet::from([track_id]);
    let expanded_items = HashSet::from([item_ids[0]]);
    let rows = display_rows(
        &project,
        project.root_timeline_id,
        &expanded_tracks,
        &expanded_items,
        None,
    );
    let projection = timeline_row_projection(
        &project,
        &rows,
        &expanded_tracks,
        &expanded_items,
        Some(&gesture),
    )
    .expect("projection");
    let target = crate::state::authoring::AutomationLaneId {
        owner: crate::state::authoring::AutomationOwner::Item(item_ids[0]),
        target: crate::state::authoring::AutomationTarget::AuthoredProperty("position".to_string()),
    };

    assert_eq!(projection.row_for_item(item_ids[0]), Some(1));
    assert_eq!(projection.row_for_property(item_ids[0], &target), Some(2));
    assert_eq!(projection.row_for_item(item_ids[1]), Some(3));
    assert_eq!(projection.visible_row_count(), rows.len());
}
