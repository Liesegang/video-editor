use crate::state::authoring::{AuthoringUiState, TimelineGestureKind, TimelineItemGesture};

use super::geometry::{trim_edge_rects, TimelineRowMetrics};
use super::interaction::{
    destination_index_after_removal, drop_target, item_gesture_kind, update_item_projection,
};
use super::tests::fixture;
use super::{display_rows, RowKind};

#[test]
fn removal_adjustment_keeps_adjacent_slots_stable() {
    assert_eq!(destination_index_after_removal(2, 0, 4), Some(0));
    assert_eq!(destination_index_after_removal(0, 3, 4), Some(2));
    assert_eq!(destination_index_after_removal(1, 1, 4), Some(1));
    assert_eq!(destination_index_after_removal(1, 2, 4), Some(1));
    assert_eq!(destination_index_after_removal(4, 0, 4), None);
}

#[test]
fn clip_press_origin_selects_nearest_edge_before_body_drag() {
    let clip = egui::Rect::from_min_max(egui::pos2(10.0, 20.0), egui::pos2(110.0, 50.0));
    assert_eq!(
        item_gesture_kind(clip, egui::pos2(13.0, 35.0)),
        TimelineGestureKind::TrimStart
    );
    assert_eq!(
        item_gesture_kind(clip, egui::pos2(107.0, 35.0)),
        TimelineGestureKind::TrimEnd
    );
    assert_eq!(
        item_gesture_kind(clip, egui::pos2(50.0, 35.0)),
        TimelineGestureKind::Move
    );

    let narrow = egui::Rect::from_min_max(egui::pos2(10.0, 20.0), egui::pos2(17.0, 50.0));
    assert_eq!(
        item_gesture_kind(narrow, egui::pos2(12.0, 35.0)),
        TimelineGestureKind::TrimStart
    );
    assert_eq!(
        item_gesture_kind(narrow, egui::pos2(15.0, 35.0)),
        TimelineGestureKind::TrimEnd
    );
}

#[test]
fn offscreen_clip_edge_does_not_expose_viewport_boundary_as_trim_handle() {
    let clip = egui::Rect::from_min_max(egui::pos2(-30.0, 20.0), egui::pos2(80.0, 50.0));
    let viewport = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0));
    let (start, end) = trim_edge_rects(clip, viewport);
    assert!(!start.is_positive());
    assert!(end.is_positive());
    assert_eq!(end.right(), 80.0);
}

#[test]
fn left_and_right_trim_projection_change_only_the_requested_edge() {
    let (project, track_id, item_ids) = fixture();
    let item = project.items[&item_ids[2]].clone();
    let mut state = AuthoringUiState::new(project.root_timeline_id);
    state.timeline.pixels_per_second = 30.0;

    for (kind, pointer_x, expected_start, expected_end) in [
        (TimelineGestureKind::TrimStart, 130.0, 3.0, 7.0),
        (TimelineGestureKind::TrimEnd, 70.0, 2.0, 6.0),
    ] {
        state.timeline.item_gesture = Some(TimelineItemGesture {
            item_id: item.id,
            kind,
            pointer_origin: egui::pos2(100.0, 100.0),
            original_track_id: track_id,
            original_layer: item.layer,
            original_interval: item.interval,
            projected_track_id: track_id,
            projected_layer: item.layer,
            projected_interval: item.interval,
        });
        update_item_projection(&project, &mut state, egui::pos2(pointer_x, -1.0), 0.0);
        let projected = state
            .timeline
            .item_gesture
            .as_ref()
            .expect("trim projection")
            .projected_interval;
        assert_eq!(projected.start.to_seconds_f64(), expected_start);
        assert_eq!(projected.duration.to_seconds_f64(), 4.0);
        assert_eq!(projected.end().expect("end").to_seconds_f64(), expected_end);
    }
}

#[test]
fn drop_hit_uses_scaled_rows_and_vertical_scroll() {
    let (project, track_id, _) = fixture();
    let mut state = AuthoringUiState::new(project.root_timeline_id);
    state.timeline.expanded_tracks.insert(track_id);
    state.timeline.vertical_zoom = 2.25;
    state.timeline.vertical_scroll = 47.0;
    let rows = display_rows(
        &project,
        project.root_timeline_id,
        &state.timeline.expanded_tracks,
        &state.timeline.expanded_items,
        state.active_instance_path.as_ref(),
    );
    let metrics = TimelineRowMetrics::from_view(&state.timeline);
    let first_row_top = 180.0;

    for (row_index, row) in rows.iter().enumerate().skip(1) {
        let RowKind::Clip { item_id, .. } = row.kind else {
            panic!("expanded child must be a clip row");
        };
        let pointer_y =
            first_row_top + row_index as f32 * metrics.stride() + metrics.row_height() / 2.0
                - state.timeline.vertical_scroll;
        assert_eq!(
            drop_target(&project, &rows, &state, pointer_y, first_row_top),
            Some((track_id, project.items[&item_id].layer + 1))
        );
    }
}
