use library::model::vector::{ControlPoint, PointType, VectorPath};
use skia_safe::PathVerb;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SvgPathParseError {
    InvalidSyntax,
    InvalidVerbRecord {
        verb: &'static str,
        required_index: usize,
        point_count: usize,
    },
}

impl fmt::Display for SvgPathParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSyntax => formatter.write_str("invalid SVG path syntax"),
            Self::InvalidVerbRecord {
                verb,
                required_index,
                point_count,
            } => write!(
                formatter,
                "Skia returned an invalid {verb} record: point index {required_index} is missing from {point_count} points"
            ),
        }
    }
}

impl std::error::Error for SvgPathParseError {}

fn verb_point(
    points: &[skia_safe::Point],
    index: usize,
    verb: &'static str,
) -> Result<skia_safe::Point, SvgPathParseError> {
    points
        .get(index)
        .copied()
        .ok_or(SvgPathParseError::InvalidVerbRecord {
            verb,
            required_index: index,
            point_count: points.len(),
        })
}

pub fn parse_svg_path(path_data: &str) -> Result<VectorPath, SvgPathParseError> {
    let path = skia_safe::utils::parse_path::from_svg(path_data)
        .ok_or(SvgPathParseError::InvalidSyntax)?;

    let mut points = Vec::new();
    let mut is_closed = false;

    let iter = path.iter();

    for rec in iter {
        let verb = rec.verb();
        let pts = rec.points();
        match verb {
            PathVerb::Move => {
                let p = verb_point(pts, 0, "move")?;
                points.push(ControlPoint {
                    position: [p.x, p.y],
                    handle_in: [0.0, 0.0],
                    handle_out: [0.0, 0.0],
                    point_type: PointType::Corner,
                });
            }
            PathVerb::Line => {
                let p = verb_point(pts, 1, "line")?;
                points.push(ControlPoint {
                    position: [p.x, p.y],
                    handle_in: [0.0, 0.0],
                    handle_out: [0.0, 0.0],
                    point_type: PointType::Corner,
                });
            }
            PathVerb::Quad => {
                let p0 = verb_point(pts, 0, "quadratic curve")?;
                let p1 = verb_point(pts, 1, "quadratic curve")?;
                let p2 = verb_point(pts, 2, "quadratic curve")?;

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
                let p = verb_point(pts, 2, "conic curve")?;
                points.push(ControlPoint {
                    position: [p.x, p.y],
                    handle_in: [0.0, 0.0],
                    handle_out: [0.0, 0.0],
                    point_type: PointType::Corner,
                });
            }
            PathVerb::Cubic => {
                let p0 = verb_point(pts, 0, "cubic curve")?;
                let c1 = verb_point(pts, 1, "cubic curve")?;
                let c2 = verb_point(pts, 2, "cubic curve")?;
                let p3 = verb_point(pts, 3, "cubic curve")?;

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

    // A curved SVG closing segment must name its first endpoint in the final
    // `C` command before `Z`. Skia reports that endpoint as another Cubic/Line
    // record. Fold it back into the first logical vertex instead of allowing
    // parse -> edit -> write cycles to grow one point per frame.
    if is_closed
        && points.len() > 1
        && points
            .first()
            .zip(points.last())
            .is_some_and(|(first, last)| same_position(first.position, last.position))
    {
        if let Some(repeated_first) = points.pop() {
            if let Some(first) = points.first_mut() {
                first.handle_in = repeated_first.handle_in;
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
        } else {
            pt.point_type = PointType::Corner;
        }
    }

    Ok(VectorPath { points, is_closed })
}

fn is_zero(v: [f32; 2]) -> bool {
    v[0].abs() < 0.001 && v[1].abs() < 0.001
}

fn same_position(left: [f32; 2], right: [f32; 2]) -> bool {
    (left[0] - right[0]).abs() < 0.001 && (left[1] - right[1]).abs() < 0.001
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

        let result = parse_svg_path(original_path);
        assert!(result
            .as_ref()
            .is_ok_and(|path| path.points.len() == 4 && path.is_closed));
        let generated = result.as_ref().map(to_svg_path);
        // Expect: M 10,10 L 90,10 L 90,90 L 10,90 Z
        // Note: Floats might format differently.
        assert!(generated.is_ok_and(|path| path.contains("M 10,10") && path.contains('Z')));
    }

    #[test]
    fn invalid_svg_is_reported_instead_of_becoming_an_empty_shape() {
        assert!(matches!(
            parse_svg_path("not a path"),
            Err(SvgPathParseError::InvalidSyntax)
        ));
    }

    #[test]
    fn malformed_skia_verb_record_is_rejected_before_indexing() {
        assert!(matches!(
            verb_point(&[], 2, "conic curve"),
            Err(SvgPathParseError::InvalidVerbRecord {
                verb: "conic curve",
                required_index: 2,
                point_count: 0,
            })
        ));
    }

    #[test]
    fn closed_path_round_trip_never_grows_duplicate_first_points() {
        let mut encoded = "M 0 0 H 160 V 90 H 0 Z".to_string();
        for _ in 0..32 {
            let parsed = parse_svg_path(&encoded).expect("closed path remains parseable");
            assert_eq!(parsed.points.len(), 4);
            assert_eq!(
                parsed
                    .points
                    .iter()
                    .map(|point| point.position)
                    .collect::<Vec<_>>(),
                vec![[0.0, 0.0], [160.0, 0.0], [160.0, 90.0], [0.0, 90.0]]
            );
            encoded = to_svg_path(&parsed);
        }
        assert!(!encoded.contains("L 0,0 Z"));
    }

    #[test]
    fn curved_closing_segment_round_trip_keeps_first_in_handle() {
        let encoded = "M 0,0 L 100,0 C 100,80 -20,60 0,0 Z";
        let parsed = parse_svg_path(encoded).expect("curved close parses");
        assert_eq!(parsed.points.len(), 2);
        assert_eq!(parsed.points[0].handle_in, [-20.0, 60.0]);
        let reparsed = parse_svg_path(&to_svg_path(&parsed)).expect("written close parses");
        assert_eq!(reparsed.points.len(), 2);
        assert_eq!(reparsed.points[0].handle_in, [-20.0, 60.0]);
    }
}
