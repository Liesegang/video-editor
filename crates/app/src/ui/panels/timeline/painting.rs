use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use egui_phosphor::regular as icons;
use library::model::asset::AssetKind;
use library::model::authoring::{AuthoringProject, MediaTime, SourceRef, Timeline, TimelineItem};

use crate::state::authoring::AuthoringUiState;
use crate::ui::time_ruler::{TimeRuler, TimeRulerTick};

use super::geometry::{
    format_time, frame_for_seconds, major_tick_seconds, TimelineRowMetrics, RULER_HEIGHT,
};
use super::viewport::{
    canvas_transform, grid_config, row_top, screen_x_to_seconds, seconds_to_screen_x,
};
pub(super) fn paint_background(
    ui: &egui::Ui,
    canvas_rect: Rect,
    content_rect: Rect,
    row_count: usize,
    state: &AuthoringUiState,
) {
    let painter = ui.painter();
    let metrics = TimelineRowMetrics::from_view(&state.timeline);
    painter.rect_filled(canvas_rect, 0.0, ui.visuals().extreme_bg_color);
    painter.rect_filled(
        Rect::from_min_max(
            canvas_rect.min,
            Pos2::new(content_rect.min.x, canvas_rect.max.y),
        ),
        0.0,
        ui.visuals().panel_fill,
    );
    painter.line_segment(
        [
            Pos2::new(content_rect.min.x, canvas_rect.min.y),
            Pos2::new(content_rect.min.x, canvas_rect.max.y),
        ],
        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
    );
    for index in 0..row_count {
        let y = row_top(content_rect, &state.timeline, index);
        let rect = Rect::from_min_size(
            Pos2::new(canvas_rect.min.x, y),
            Vec2::new(canvas_rect.width(), metrics.row_height()),
        );
        if rect.intersects(canvas_rect) {
            let gray = if index.is_multiple_of(2) { 27 } else { 31 };
            painter.rect_filled(rect, 0.0, Color32::from_gray(gray));
        }
    }
    let grid_painter = painter.with_clip_rect(content_rect);
    for line in pan_zoom_ui::grid_lines(
        content_rect,
        canvas_transform(content_rect, &state.timeline),
        grid_config(&state.timeline),
    ) {
        if line.axis != pan_zoom_ui::GridAxis::X {
            continue;
        }
        let color = match line.kind {
            pan_zoom_ui::GridLineKind::Origin => Color32::from_gray(72),
            pan_zoom_ui::GridLineKind::Major => Color32::from_gray(50),
            pan_zoom_ui::GridLineKind::Minor => Color32::from_gray(37),
        };
        grid_painter.line_segment(
            [
                Pos2::new(line.screen_position, content_rect.top()),
                Pos2::new(line.screen_position, content_rect.bottom()),
            ],
            Stroke::new(1.0, color),
        );
    }
}

pub(super) fn draw_ruler(
    ui: &mut egui::Ui,
    timeline: &Timeline,
    state: &mut AuthoringUiState,
    canvas_rect: Rect,
    content_rect: Rect,
) {
    let ruler_rect = Rect::from_min_max(
        Pos2::new(content_rect.min.x, canvas_rect.min.y),
        Pos2::new(content_rect.max.x, content_rect.min.y),
    );
    let seconds_per_major = major_tick_seconds(state.timeline.pixels_per_second);
    let first =
        (state.timeline.horizontal_scroll / state.timeline.pixels_per_second / seconds_per_major)
            .floor() as i64
            - 1;
    let count = (ruler_rect.width() / (seconds_per_major * state.timeline.pixels_per_second)).ceil()
        as i64
        + 3;
    let ticks = (first..(first + count))
        .filter(|index| *index >= 0)
        .map(|index| {
            let seconds = index as f32 * seconds_per_major;
            TimeRulerTick {
                x: seconds_to_screen_x(seconds, content_rect, &state.timeline),
                label: format_time(seconds),
            }
        })
        .collect::<Vec<_>>();
    let duration_x = seconds_to_screen_x(
        timeline.duration.to_seconds_f64() as f32,
        content_rect,
        &state.timeline,
    );
    let response = TimeRuler::new("authoring_timeline_ruler", "timeline.ruler", &ticks)
        .with_duration_x(Some(duration_x))
        .show(ui, ruler_rect);
    if response.clicked() || response.dragged() {
        if let Some(pointer) = response.interact_pointer_pos() {
            let seconds = screen_x_to_seconds(pointer.x, content_rect, &state.timeline);
            state
                .timeline
                .seek_frame(frame_for_seconds(seconds, timeline.fps));
        }
    }
}

pub(super) fn draw_playhead(
    ui: &egui::Ui,
    project: &AuthoringProject,
    state: &AuthoringUiState,
    content_rect: Rect,
) {
    let Some(timeline) = project.timelines.get(&state.active_timeline_id) else {
        return;
    };
    let seconds = MediaTime::from_frame_index(state.timeline.current_frame, timeline.fps)
        .map_or(0.0, MediaTime::to_seconds_f64);
    let x = seconds_to_screen_x(seconds as f32, content_rect, &state.timeline);
    if x >= content_rect.left() && x <= content_rect.right() {
        ui.painter().line_segment(
            [
                Pos2::new(x, content_rect.top() - RULER_HEIGHT),
                Pos2::new(x, content_rect.bottom()),
            ],
            Stroke::new(1.5, Color32::from_rgb(255, 82, 82)),
        );
        ui.painter().circle_filled(
            Pos2::new(x, content_rect.top() - 3.0),
            4.0,
            Color32::from_rgb(255, 82, 82),
        );
    }
}

pub(super) fn item_colors(project: &AuthoringProject, item: &TimelineItem) -> (Color32, Color32) {
    match &item.source {
        SourceRef::Asset { asset_id } => project
            .assets
            .iter()
            .find(|asset| asset.id == *asset_id)
            .map_or(
                (
                    Color32::from_rgb(48, 79, 96),
                    Color32::from_rgb(80, 177, 214),
                ),
                |asset| match asset.kind {
                    AssetKind::Audio => (
                        Color32::from_rgb(57, 89, 63),
                        Color32::from_rgb(92, 196, 112),
                    ),
                    AssetKind::Image => (
                        Color32::from_rgb(76, 62, 100),
                        Color32::from_rgb(169, 119, 226),
                    ),
                    _ => (
                        Color32::from_rgb(48, 79, 96),
                        Color32::from_rgb(80, 177, 214),
                    ),
                },
            ),
        SourceRef::Composition(_) => (
            Color32::from_rgb(85, 61, 91),
            Color32::from_rgb(210, 126, 220),
        ),
        SourceRef::Module(_) => (
            Color32::from_rgb(91, 66, 43),
            Color32::from_rgb(244, 163, 64),
        ),
        SourceRef::Text { .. } => (
            Color32::from_rgb(69, 65, 102),
            Color32::from_rgb(151, 137, 241),
        ),
        SourceRef::Shape { .. } | SourceRef::Solid { .. } => (
            Color32::from_rgb(67, 82, 68),
            Color32::from_rgb(127, 199, 136),
        ),
    }
}

pub(super) fn item_icon(item: &TimelineItem) -> &'static str {
    match &item.source {
        SourceRef::Asset { .. } => icons::FILE_VIDEO,
        SourceRef::Composition(_) => icons::FILM_STRIP,
        SourceRef::Module(_) => icons::SHARE_NETWORK,
        SourceRef::Text { .. } => icons::TEXT_T,
        SourceRef::Shape { .. } | SourceRef::Solid { .. } => icons::SQUARE,
    }
}

pub(super) fn open_icon(item: &TimelineItem) -> &'static str {
    match &item.source {
        SourceRef::Composition(_) => icons::FILM_STRIP,
        SourceRef::Module(_) => icons::SHARE_NETWORK,
        _ => icons::ARROW_SQUARE_OUT,
    }
}
