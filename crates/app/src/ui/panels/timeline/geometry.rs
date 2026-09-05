use library::model::authoring::{
    AuthoringProject, MediaTime, RationalRate, TimelineId, TimelineInterval, TimelineItemId,
    TimelineTrackId,
};

use crate::state::authoring::AuthoringTimelineView;

pub(super) const SIDEBAR_WIDTH: f32 = 188.0;
pub(super) const RULER_HEIGHT: f32 = 26.0;
pub(super) const EDGE_WIDTH: f32 = 7.0;
pub(super) const MIN_CLIP_WIDTH: f32 = 7.0;

const BASE_ROW_HEIGHT: f32 = 32.0;
const BASE_ROW_GAP: f32 = 2.0;
pub(super) const MIN_VERTICAL_ZOOM: f32 = 0.55;
pub(super) const MAX_VERTICAL_ZOOM: f32 = 3.0;

/// The sole authority for Timeline row geometry.
///
/// Rows live in canvas-world points and are projected with the shared
/// `CanvasState` Y zoom. Painting, scrolling, hit testing, drag/drop, reorder
/// markers, and QA metadata all consume this value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TimelineRowMetrics {
    vertical_zoom: f32,
}

impl TimelineRowMetrics {
    pub(super) fn from_view(view: &AuthoringTimelineView) -> Self {
        Self {
            vertical_zoom: view
                .vertical_zoom
                .clamp(MIN_VERTICAL_ZOOM, MAX_VERTICAL_ZOOM),
        }
    }

    pub(super) const fn world_row_height(self) -> f32 {
        BASE_ROW_HEIGHT
    }

    pub(super) const fn world_gap(self) -> f32 {
        BASE_ROW_GAP
    }

    pub(super) const fn world_stride(self) -> f32 {
        BASE_ROW_HEIGHT + BASE_ROW_GAP
    }

    pub(super) fn row_height(self) -> f32 {
        self.world_row_height() * self.vertical_zoom
    }

    pub(super) const fn minimum_row_height() -> f32 {
        BASE_ROW_HEIGHT * MIN_VERTICAL_ZOOM
    }

    pub(super) const fn maximum_row_height() -> f32 {
        BASE_ROW_HEIGHT * MAX_VERTICAL_ZOOM
    }

    pub(super) fn gap(self) -> f32 {
        self.world_gap() * self.vertical_zoom
    }

    pub(super) fn stride(self) -> f32 {
        self.world_stride() * self.vertical_zoom
    }

    pub(super) fn content_height(self, row_count: usize) -> f32 {
        row_count as f32 * self.stride()
    }

    pub(super) fn row_index_at(
        self,
        pointer_y: f32,
        content_top: f32,
        vertical_scroll: f32,
    ) -> Option<usize> {
        let index = ((pointer_y - content_top + vertical_scroll) / self.stride()).floor();
        if index.is_finite() && index >= 0.0 {
            Some(index as usize)
        } else {
            None
        }
    }

    pub(super) fn boundary_y(
        self,
        boundary_row: usize,
        content_top: f32,
        vertical_scroll: f32,
    ) -> f32 {
        content_top + boundary_row as f32 * self.stride() - vertical_scroll
    }

    pub(super) const fn vertical_zoom(self) -> f32 {
        self.vertical_zoom
    }
}

pub(crate) fn next_layer(project: &AuthoringProject, track_id: TimelineTrackId) -> i64 {
    project
        .items
        .values()
        .filter(|item| item.track_id == track_id)
        .map(|item| item.layer)
        .max()
        .map_or(0, |layer| layer.saturating_add(1))
}

/// Authoritative, un-clipped screen geometry shared by clip painting and hit
/// testing. `row_rect` is already projected by the Timeline canvas. Consumers
/// clip this rectangle only when painting or registering a visible response.
pub(super) fn clip_rect(
    interval: TimelineInterval,
    layer: i64,
    row_rect: egui::Rect,
    view: &AuthoringTimelineView,
    summary: bool,
) -> egui::Rect {
    let x = super::viewport::seconds_to_screen_x(
        interval.start.to_seconds_f64() as f32,
        row_rect,
        view,
    );
    let width =
        (interval.duration.to_seconds_f64() as f32 * view.pixels_per_second).max(MIN_CLIP_WIDTH);
    let vertical_inset = if summary {
        3.0 + layer.rem_euclid(3) as f32
    } else {
        3.0
    };
    egui::Rect::from_min_size(
        egui::Pos2::new(x, row_rect.top() + vertical_inset),
        egui::Vec2::new(width, row_rect.height() - vertical_inset - 3.0),
    )
}

/// Visible, non-overlapping drag targets for a clip's start and end edges.
///
/// The targets are derived from the un-clipped clip rectangle so a viewport
/// boundary can never masquerade as a trim handle for an off-screen edge.
pub(super) fn trim_edge_rects(
    clip_rect: egui::Rect,
    content_rect: egui::Rect,
) -> (egui::Rect, egui::Rect) {
    let handle_width = EDGE_WIDTH.min(clip_rect.width() * 0.5);
    let start = egui::Rect::from_min_max(
        clip_rect.min,
        egui::Pos2::new(clip_rect.left() + handle_width, clip_rect.bottom()),
    )
    .intersect(content_rect);
    let end = egui::Rect::from_min_max(
        egui::Pos2::new(clip_rect.right() - handle_width, clip_rect.top()),
        clip_rect.max,
    )
    .intersect(content_rect);
    (start, end)
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

    #[test]
    fn row_metrics_project_height_gap_and_hit_testing_together() {
        let view = AuthoringTimelineView {
            vertical_zoom: 2.25,
            ..AuthoringTimelineView::default()
        };
        let metrics = TimelineRowMetrics::from_view(&view);
        let top = 180.0;
        let scroll = 47.0;

        assert_eq!(metrics.row_height(), metrics.world_row_height() * 2.25);
        assert_eq!(metrics.gap(), metrics.world_gap() * 2.25);
        assert_eq!(metrics.stride(), metrics.row_height() + metrics.gap());
        for row in 0..7 {
            let center = top + row as f32 * metrics.stride() + metrics.row_height() / 2.0 - scroll;
            assert_eq!(metrics.row_index_at(center, top, scroll), Some(row));
        }
        assert_eq!(
            TimelineRowMetrics::minimum_row_height(),
            metrics.world_row_height() * MIN_VERTICAL_ZOOM
        );
        assert_eq!(
            TimelineRowMetrics::maximum_row_height(),
            metrics.world_row_height() * MAX_VERTICAL_ZOOM
        );
    }
}
