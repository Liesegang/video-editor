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

/// Evaluate a cubic bezier at parameter t.
fn cubic_bezier(p0: Pos2, cp1: Pos2, cp2: Pos2, p1: Pos2, t: f32) -> Pos2 {
    let t2 = t * t;
    let t3 = t2 * t;
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    let mt3 = mt2 * mt;
    Pos2::new(
        mt3 * p0.x + 3.0 * mt2 * t * cp1.x + 3.0 * mt * t2 * cp2.x + t3 * p1.x,
        mt3 * p0.y + 3.0 * mt2 * t * cp1.y + 3.0 * mt * t2 * cp2.y + t3 * p1.y,
    )
}

/// Distance from a point to a line segment.
fn point_to_segment_dist(point: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let ap = point - a;
    let len_sq = ab.length_sq();
    if len_sq < 1e-8 {
        return ap.length();
    }
    let t = (ap.x * ab.x + ap.y * ab.y) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let proj = Pos2::new(a.x + t * ab.x, a.y + t * ab.y);
    point.distance(proj)
}

/// Compute the minimum distance from a point to a cubic bezier connection.
///
/// Uses the same control point convention as `draw_bezier_connection`.
pub fn bezier_distance_to_point(from: Pos2, to: Pos2, point: Pos2) -> f32 {
    let dx = (to.x - from.x).abs() * 0.5;
    let cp1 = Pos2::new(from.x + dx, from.y);
    let cp2 = Pos2::new(to.x - dx, to.y);

    let segments = 20;
    let mut min_dist = f32::MAX;
    for i in 0..segments {
        let t0 = i as f32 / segments as f32;
        let t1 = (i + 1) as f32 / segments as f32;
        let p0 = cubic_bezier(from, cp1, cp2, to, t0);
        let p1 = cubic_bezier(from, cp1, cp2, to, t1);
        min_dist = min_dist.min(point_to_segment_dist(point, p0, p1));
    }
    min_dist
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
        points.push(cubic_bezier(from, cp1, cp2, to, t));
    }

    for window in points.windows(2) {
        painter.line_segment([window[0], window[1]], Stroke::new(2.0, color));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bezier_distance_on_straight_line_midpoint() {
        let from = Pos2::new(0.0, 0.0);
        let to = Pos2::new(100.0, 0.0);
        // For a horizontal line, the bezier is a straight line
        let midpoint = Pos2::new(50.0, 0.0);
        assert!(bezier_distance_to_point(from, to, midpoint) < 2.0);
    }

    #[test]
    fn test_bezier_distance_at_endpoints() {
        let from = Pos2::new(0.0, 0.0);
        let to = Pos2::new(100.0, 0.0);
        assert!(bezier_distance_to_point(from, to, from) < 1.0);
        assert!(bezier_distance_to_point(from, to, to) < 1.0);
    }

    #[test]
    fn test_bezier_distance_far_away() {
        let from = Pos2::new(0.0, 0.0);
        let to = Pos2::new(100.0, 0.0);
        let far = Pos2::new(50.0, 100.0);
        assert!(bezier_distance_to_point(from, to, far) > 30.0);
    }

    #[test]
    fn test_bezier_distance_slightly_off_curve() {
        let from = Pos2::new(0.0, 0.0);
        let to = Pos2::new(200.0, 0.0);
        // Point slightly above the midpoint of a horizontal bezier
        let near = Pos2::new(100.0, 3.0);
        assert!(bezier_distance_to_point(from, to, near) < 5.0);
    }

    #[test]
    fn test_bezier_distance_vertical_connection() {
        let from = Pos2::new(0.0, 0.0);
        let to = Pos2::new(0.0, 100.0);
        // Even with a vertical connection, the control points go horizontal
        // so a point at (0, 50) may not be on the curve — but should be close
        let mid = Pos2::new(0.0, 50.0);
        // The curve bows out, so the midpoint is off, but distance should still be reasonable
        assert!(bezier_distance_to_point(from, to, mid) < 30.0);
    }

    #[test]
    fn test_point_to_segment_dist_on_segment() {
        let a = Pos2::new(0.0, 0.0);
        let b = Pos2::new(10.0, 0.0);
        let p = Pos2::new(5.0, 0.0);
        assert!(point_to_segment_dist(p, a, b) < 0.01);
    }

    #[test]
    fn test_point_to_segment_dist_perpendicular() {
        let a = Pos2::new(0.0, 0.0);
        let b = Pos2::new(10.0, 0.0);
        let p = Pos2::new(5.0, 3.0);
        assert!((point_to_segment_dist(p, a, b) - 3.0).abs() < 0.01);
    }
}
