use crate::model::vector::{ControlPoint, PointType, VectorEditorState};
use skia_safe::PathVerb;

pub fn parse_svg_path(path_data: &str) -> VectorEditorState {
    let path = match skia_safe::utils::parse_path::from_svg(path_data) {
        Some(p) => p,
        None => return VectorEditorState::default(),
    };

    let mut points = Vec::new();
    let mut is_closed = false;

    let iter = path.iter();

    for rec in iter {
        let verb = rec.verb();
        let pts = rec.points();
        match verb {
            PathVerb::Move => {
                let p = pts[0];
                points.push(ControlPoint {
                    position: [p.x, p.y],
                    handle_in: [0.0, 0.0],
                    handle_out: [0.0, 0.0],
                    point_type: PointType::Corner,
                });
            }
            PathVerb::Line => {
                let p = pts[1];
                points.push(ControlPoint {
                    position: [p.x, p.y],
                    handle_in: [0.0, 0.0],
                    handle_out: [0.0, 0.0],
                    point_type: PointType::Corner,
                });
            }
            PathVerb::Quad => {
                let p0 = pts[0];
                let p1 = pts[1];
                let p2 = pts[2];

                let c1 = p0 + (p1 - p0) * (2.0 / 3.0);
                let c2 = p2 + (p1 - p2) * (2.0 / 3.0);

                if let Some(last) = points.last_mut() {
                    last.handle_out = [c1.x - p0.x, c1.y - p0.y];
                }

                points.push(ControlPoint {
                    position: [p2.x, p2.y],
                    handle_in: [c2.x - p2.x, c2.y - p2.y],
                    handle_out: [0.0, 0.0],
                    point_type: PointType::Smooth,
                });
            }
            PathVerb::Conic => {
                let p = pts.last().unwrap();
                points.push(ControlPoint {
                    position: [p.x, p.y],
                    handle_in: [0.0, 0.0],
                    handle_out: [0.0, 0.0],
                    point_type: PointType::Corner,
                });
            }
            PathVerb::Cubic => {
                let p0 = pts[0];
                let c1 = pts[1];
                let c2 = pts[2];
                let p3 = pts[3];

                if let Some(last) = points.last_mut() {
                    last.handle_out = [c1.x - p0.x, c1.y - p0.y];
                }

                points.push(ControlPoint {
                    position: [p3.x, p3.y],
                    handle_in: [c2.x - p3.x, c2.y - p3.y],
                    handle_out: [0.0, 0.0],
                    point_type: PointType::Smooth,
                });
            }
            PathVerb::Close => {
                is_closed = true;
            }
        }
    }

    for pt in &mut points {
        if is_collinear_opposite(pt.handle_in, pt.handle_out) {
            if is_same_length(pt.handle_in, pt.handle_out) {
                pt.point_type = PointType::Symmetric;
            } else {
                pt.point_type = PointType::Smooth;
            }
        } else if is_zero(pt.handle_in) && is_zero(pt.handle_out) {
            pt.point_type = PointType::Corner;
        } else {
            pt.point_type = PointType::Corner;
        }
    }

    VectorEditorState {
        points,
        is_closed,
        ..Default::default()
    }
}

fn is_zero(v: [f32; 2]) -> bool {
    v[0].abs() < 0.001 && v[1].abs() < 0.001
}

fn is_collinear_opposite(v1: [f32; 2], v2: [f32; 2]) -> bool {
    if is_zero(v1) || is_zero(v2) {
        return false;
    }
    let n1 = normalize(v1);
    let n2 = normalize(v2);
    let dot = n1[0] * n2[0] + n1[1] * n2[1];
    (dot + 1.0).abs() < 0.01
}

fn is_same_length(v1: [f32; 2], v2: [f32; 2]) -> bool {
    let l1 = (v1[0] * v1[0] + v1[1] * v1[1]).sqrt();
    let l2 = (v2[0] * v2[0] + v2[1] * v2[1]).sqrt();
    (l1 - l2).abs() < 0.01
}

fn normalize(v: [f32; 2]) -> [f32; 2] {
    let l = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if l < 0.0001 {
        [0.0, 0.0]
    } else {
        [v[0] / l, v[1] / l]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::panels::preview::vector_editor::svg_writer::to_svg_path;

    #[test]
    fn test_svg_round_trip() {
        let original_path = "M 10,10 L 90,10 L 90,90 L 10,90 Z";
        // Note: Parser might produce C for everything or keep L
        // With current implementation:
        // Move -> Corner
        // Line -> Corner
        // Close -> is_closed

        let state = parse_svg_path(original_path);
        assert_eq!(state.points.len(), 4);
        assert!(state.is_closed);

        let generated = to_svg_path(&state);
        // Expect: M 10,10 L 90,10 L 90,90 L 10,90 Z
        // Note: Floats might format differently.
        assert!(generated.contains("M 10,10"));
        assert!(generated.contains("Z"));
    }
}
