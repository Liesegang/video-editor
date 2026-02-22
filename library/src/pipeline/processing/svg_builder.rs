//! SVG path string builders for geometric shapes.

use crate::pipeline::processing::ensemble::decorators::BackplateShape;

/// Build an SVG rectangle (or rounded-rect/circle) path string.
pub fn build_rect_svg(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    shape: &BackplateShape,
    radius: f32,
) -> String {
    match shape {
        BackplateShape::Rect => {
            format!(
                "M {:.2} {:.2} L {:.2} {:.2} L {:.2} {:.2} L {:.2} {:.2} Z",
                x,
                y,
                x + w,
                y,
                x + w,
                y + h,
                x,
                y + h
            )
        }
        BackplateShape::RoundedRect => {
            let r = radius.min(w / 2.0).min(h / 2.0);
            format!(
                "M {:.2} {:.2} L {:.2} {:.2} Q {:.2} {:.2} {:.2} {:.2} \
                 L {:.2} {:.2} Q {:.2} {:.2} {:.2} {:.2} \
                 L {:.2} {:.2} Q {:.2} {:.2} {:.2} {:.2} \
                 L {:.2} {:.2} Q {:.2} {:.2} {:.2} {:.2} Z",
                x + r,
                y,
                x + w - r,
                y,
                x + w,
                y,
                x + w,
                y + r,
                x + w,
                y + h - r,
                x + w,
                y + h,
                x + w - r,
                y + h,
                x + r,
                y + h,
                x,
                y + h,
                x,
                y + h - r,
                x,
                y + r,
                x,
                y,
                x + r,
                y,
            )
        }
        BackplateShape::Circle => {
            let cx = x + w / 2.0;
            let cy = y + h / 2.0;
            let r = (w.min(h) / 2.0).max(0.0);
            // Approximate circle with 4 cubic Bezier curves
            let k = 0.5522847498; // (4/3)*tan(pi/8)
            let kr = k * r;
            format!(
                "M {:.2} {:.2} \
                 C {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} \
                 C {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} \
                 C {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} \
                 C {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} Z",
                cx,
                cy - r,
                cx + kr,
                cy - r,
                cx + r,
                cy - kr,
                cx + r,
                cy,
                cx + r,
                cy + kr,
                cx + kr,
                cy + r,
                cx,
                cy + r,
                cx - kr,
                cy + r,
                cx - r,
                cy + kr,
                cx - r,
                cy,
                cx - r,
                cy - kr,
                cx - kr,
                cy - r,
                cx,
                cy - r,
            )
        }
    }
}
