//! Drawing utilities for the node editor.

use egui::{Color32, Pos2, Rect, Stroke, Vec2};

/// Draw a background grid.
pub fn draw_grid(painter: &egui::Painter, rect: Rect, pan: Vec2, color: Color32, spacing: f32) {
    let start_x = rect.min.x + (pan.x % spacing);
    let start_y = rect.min.y + (pan.y % spacing);

    let mut x = start_x;
    while x < rect.max.x {
        painter.line_segment(
            [Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)],
            Stroke::new(1.0, color),
        );
        x += spacing;
    }

    let mut y = start_y;
    while y < rect.max.y {
        painter.line_segment(
            [Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)],
            Stroke::new(1.0, color),
        );
        y += spacing;
    }
}

/// Draw a cubic bezier connection between two points.
pub fn draw_bezier_connection(painter: &egui::Painter, from: Pos2, to: Pos2, color: Color32) {
    let dx = (to.x - from.x).abs() * 0.5;
    let cp1 = Pos2::new(from.x + dx, from.y);
    let cp2 = Pos2::new(to.x - dx, to.y);

    let segments = 20;
    let mut points = Vec::with_capacity(segments + 1);
    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let t2 = t * t;
        let t3 = t2 * t;
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        let mt3 = mt2 * mt;

        let x = mt3 * from.x + 3.0 * mt2 * t * cp1.x + 3.0 * mt * t2 * cp2.x + t3 * to.x;
        let y = mt3 * from.y + 3.0 * mt2 * t * cp1.y + 3.0 * mt * t2 * cp2.y + t3 * to.y;
        points.push(Pos2::new(x, y));
    }

    for window in points.windows(2) {
        painter.line_segment([window[0], window[1]], Stroke::new(2.0, color));
    }
}
