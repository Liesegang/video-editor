use egui::Rect;
use library::model::authoring::{
    AuthoringProject, MediaTime, RationalRate, TimelineId, TimelineItemId, TimelineTrackId,
};

use crate::state::authoring::AuthoringUiState;

pub(super) const SIDEBAR_WIDTH: f32 = 188.0;
pub(super) const RULER_HEIGHT: f32 = 26.0;
pub(super) const ROW_HEIGHT: f32 = 32.0;
pub(super) const ROW_GAP: f32 = 2.0;
pub(super) const EDGE_WIDTH: f32 = 7.0;
pub(super) const MIN_CLIP_WIDTH: f32 = 7.0;

pub(super) fn next_layer(project: &AuthoringProject, track_id: TimelineTrackId) -> i64 {
    project
        .items
        .values()
        .filter(|item| item.track_id == track_id)
        .map(|item| item.layer)
        .max()
        .map_or(0, |layer| layer.saturating_add(1))
}

pub(super) fn snap_seconds(
    project: &AuthoringProject,
    timeline_id: TimelineId,
    moving_id: TimelineItemId,
    seconds: f64,
    frame_seconds: f64,
    pixels_per_second: f32,
) -> f64 {
    let frame_snapped = (seconds / frame_seconds).round() * frame_seconds;
    let threshold = 7.0 / f64::from(pixels_per_second);
    let mut best = frame_snapped;
    let mut distance = (seconds - frame_snapped).abs();
    for item in project.items.values().filter(|item| {
        item.id != moving_id
            && project
                .tracks
                .get(&item.track_id)
                .is_some_and(|track| track.timeline_id == timeline_id)
    }) {
        for candidate in [
            item.interval.start.to_seconds_f64(),
            item.interval.end().map_or(0.0, MediaTime::to_seconds_f64),
        ] {
            let candidate_distance = (seconds - candidate).abs();
            if candidate_distance < distance && candidate_distance <= threshold {
                best = candidate;
                distance = candidate_distance;
            }
        }
    }
    best.max(0.0)
}

pub(super) fn row_top(content_rect: Rect, state: &AuthoringUiState, index: usize) -> f32 {
    content_rect.top() + index as f32 * (ROW_HEIGHT + ROW_GAP) - state.timeline.vertical_scroll
}

pub(super) fn screen_x_to_seconds(x: f32, content_rect: Rect, state: &AuthoringUiState) -> f32 {
    ((x - content_rect.left() + state.timeline.horizontal_scroll)
        / state.timeline.pixels_per_second)
        .max(0.0)
}

pub(super) fn seconds_to_screen_x(
    seconds: f32,
    content_rect: Rect,
    state: &AuthoringUiState,
) -> f32 {
    content_rect.left() + seconds * state.timeline.pixels_per_second
        - state.timeline.horizontal_scroll
}

pub(super) fn frame_for_seconds(seconds: f32, fps: RationalRate) -> i64 {
    (f64::from(seconds) * fps.to_f64()).round().max(0.0) as i64
}

pub(super) fn major_tick_seconds(pixels_per_second: f32) -> f32 {
    [0.1_f32, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0]
        .into_iter()
        .find(|seconds| *seconds * pixels_per_second >= 65.0)
        .unwrap_or(60.0)
}

pub(super) fn format_time(seconds: f32) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u64;
    let remainder = seconds - minutes as f32 * 60.0;
    format!("{minutes:02}:{remainder:05.2}")
}

#[cfg(test)]
mod tests {
    use library::editor::TimelineEditorService;
    use library::model::authoring::{SourceRef, TimelineInterval};
    use library::model::frame::color::Color;

    use super::*;

    #[test]
    fn snapping_threshold_is_seven_screen_pixels_at_every_zoom() {
        let project = AuthoringProject::new(
            "Snap test",
            1920,
            1080,
            RationalRate::new(30, 1).expect("frame rate"),
            MediaTime::new(10, 1).expect("duration"),
        )
        .expect("Project");
        let timeline_id = project.root_timeline_id;
        let track_id = project.timelines[&timeline_id].track_order[0];
        let service = TimelineEditorService::new(project).expect("service");
        service
            .add_item(
                track_id,
                "Snap target".to_string(),
                SourceRef::Solid {
                    color: Color::black(),
                },
                TimelineInterval::new(
                    MediaTime::new(11, 10).expect("start"),
                    MediaTime::new(1, 1).expect("item duration"),
                )
                .expect("interval"),
                0,
            )
            .expect("item");
        let project = service.snapshot().expect("snapshot");
        let moving_id = TimelineItemId::new();

        let low_zoom = snap_seconds(&project, timeline_id, moving_id, 1.06, 1.0, 80.0);
        let high_zoom = snap_seconds(&project, timeline_id, moving_id, 1.06, 1.0, 200.0);

        assert!((low_zoom - 1.1).abs() < f64::EPSILON);
        assert!((high_zoom - 1.0).abs() < f64::EPSILON);
    }
}
