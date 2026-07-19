use egui::{Color32, Painter, Rect};

#[derive(Clone, Copy)]
pub(super) struct TrackBackgroundLayout {
    pub(super) content_rect: Rect,
    pub(super) num_rows: usize,
    pub(super) row_height: f32,
    pub(super) row_spacing: f32,
    pub(super) scroll_offset: egui::Vec2,
    pub(super) pixels_per_unit: f32,
    pub(super) duration_seconds: f64,
}

pub(super) fn draw_track_backgrounds(painter: &Painter, layout: TrackBackgroundLayout) {
    let TrackBackgroundLayout {
        content_rect,
        num_rows,
        row_height,
        row_spacing,
        scroll_offset,
        pixels_per_unit,
        duration_seconds,
    } = layout;

    // 1. Draw Track Rows
    for i in 0..num_rows {
        let y = content_rect.min.y + (i as f32 * (row_height + row_spacing)) - scroll_offset.y;
        let track_rect = Rect::from_min_size(
            egui::pos2(content_rect.min.x, y),
            egui::vec2(content_rect.width(), row_height),
        );
        painter.rect_filled(
            track_rect,
            0.0,
            if i % 2 == 0 {
                Color32::from_gray(50)
            } else {
                Color32::from_gray(60)
            },
        );
    }

    // 2. Draw Duration Visuals (End Line + Dimming)
    let content_start_x = content_rect.min.x;
    let end_x_screen =
        content_start_x - scroll_offset.x + (duration_seconds as f32 * pixels_per_unit);

    // Ensure we are within bounds visually if needed, though painter clips usually.
    // Drawing overlay for "out of bounds" area (right of duration)
    if end_x_screen < content_rect.max.x {
        let dim_rect = Rect::from_min_max(
            egui::pos2(end_x_screen.max(content_rect.min.x), content_rect.min.y),
            content_rect.max,
        );
        painter.rect_filled(
            dim_rect,
            0.0,
            Color32::from_rgba_premultiplied(0, 0, 0, 100), // Semi-transparent black
        );
    }

    // Duration Line
    if end_x_screen >= content_rect.min.x && end_x_screen <= content_rect.max.x {
        painter.line_segment(
            [
                egui::pos2(end_x_screen, content_rect.min.y),
                egui::pos2(end_x_screen, content_rect.max.y),
            ],
            egui::Stroke::new(1.5, Color32::from_rgb(100, 100, 100)), // Grey line
        );
    }
}
