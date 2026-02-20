use egui::{Color32, Painter, Rect};

use super::super::geometry::TimelineGeometry;

pub(super) fn draw_track_backgrounds(
    painter: &Painter,
    content_rect: Rect,
    num_tracks: usize,
    geo: &TimelineGeometry,
    scroll_offset: egui::Vec2,
    duration_sec: f64,
) {
    let row_height = geo.row_height;
    let track_spacing = geo.track_spacing;
    let pixels_per_unit = geo.pixels_per_unit;

    // 1. Draw Track Rows
    for i in 0..num_tracks {
        let y = content_rect.min.y + (i as f32 * (row_height + track_spacing)) - scroll_offset.y;
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
    let end_x_screen = content_start_x - scroll_offset.x + (duration_sec as f32 * pixels_per_unit);

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
