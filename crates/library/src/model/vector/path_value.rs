//! Loss-aware projection between canonical [`PathValue`] data and the direct
//! manipulation representation used by the Preview Path Editor.
//!
//! The Project remains authoritative. A projection edits one contour while
//! every other contour and the fill rule are retained verbatim. General
//! rational conics have no exact cubic Bezier representation and are rejected
//! instead of being silently rewritten.

use std::fmt;

use crate::model::path::{PathContour, PathPoint, PathSegment, PathValue};

use super::{ControlPoint, PointType, VectorPath};

const EPSILON: f32 = 1.0e-4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathProjectionError {
    MissingContour {
        contour_index: usize,
    },
    EmptyContour {
        contour_index: usize,
    },
    RationalConic {
        contour_index: usize,
        segment_index: usize,
    },
    CoordinateOutsideEditor {
        contour_index: usize,
        location: String,
    },
    InvalidPath(String),
}

impl fmt::Display for PathProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingContour { contour_index } => {
                write!(formatter, "Path contour {contour_index} does not exist")
            }
            Self::EmptyContour { contour_index } => {
                write!(
                    formatter,
                    "Path contour {contour_index} has no editable vertices"
                )
            }
            Self::RationalConic {
                contour_index,
                segment_index,
            } => write!(
                formatter,
                "Path contour {contour_index} segment {segment_index} is a rational conic and cannot be edited losslessly with Bezier handles"
            ),
            Self::CoordinateOutsideEditor {
                contour_index,
                location,
            } => write!(
                formatter,
                "Path contour {contour_index} {location} is outside the finite f32 editor boundary"
            ),
            Self::InvalidPath(error) => write!(formatter, "Invalid edited Path: {error}"),
        }
    }
}

impl std::error::Error for PathProjectionError {}

/// Project one canonical contour into editable vertices and cubic handles.
///
/// Quadratics are represented as their exact cubic equivalent. A commit can
/// therefore normalize a touched quadratic to a cubic without changing its
/// geometry. Rational conics are refused because no such exact projection
/// exists.
pub fn project_path_contour(
    path: &PathValue,
    contour_index: usize,
) -> Result<VectorPath, PathProjectionError> {
    let contour = path
        .contours()
        .get(contour_index)
        .ok_or(PathProjectionError::MissingContour { contour_index })?;
    let start = editor_point(contour.start(), contour_index, "start")?;
    let mut points = vec![control_point(start)];

    for (segment_index, segment) in contour.segments().iter().enumerate() {
        let current = points
            .last()
            .map(|point| point.position)
            .ok_or(PathProjectionError::EmptyContour { contour_index })?;
        let (to, outgoing, incoming) = match segment {
            PathSegment::Line { to } => (
                editor_point(*to, contour_index, &format!("segment {segment_index} to"))?,
                [0.0, 0.0],
                [0.0, 0.0],
            ),
            PathSegment::Quadratic { control, to } => {
                let control = editor_point(
                    *control,
                    contour_index,
                    &format!("segment {segment_index} control"),
                )?;
                let to = editor_point(*to, contour_index, &format!("segment {segment_index} to"))?;
                (
                    to,
                    scale(subtract(control, current), 2.0 / 3.0),
                    scale(subtract(control, to), 2.0 / 3.0),
                )
            }
            PathSegment::Conic { .. } => {
                return Err(PathProjectionError::RationalConic {
                    contour_index,
                    segment_index,
                });
            }
            PathSegment::Cubic {
                control1,
                control2,
                to,
            } => {
                let control1 = editor_point(
                    *control1,
                    contour_index,
                    &format!("segment {segment_index} control1"),
                )?;
                let control2 = editor_point(
                    *control2,
                    contour_index,
                    &format!("segment {segment_index} control2"),
                )?;
                let to = editor_point(*to, contour_index, &format!("segment {segment_index} to"))?;
                (to, subtract(control1, current), subtract(control2, to))
            }
        };

        if let Some(current) = points.last_mut() {
            current.handle_out = outgoing;
        }
        let closes_at_start = contour.is_closed()
            && segment_index + 1 == contour.segments().len()
            && same_position(to, start);
        if closes_at_start {
            points[0].handle_in = incoming;
        } else {
            let mut next = control_point(to);
            next.handle_in = incoming;
            points.push(next);
        }
    }

    if points.is_empty() {
        return Err(PathProjectionError::EmptyContour { contour_index });
    }
    infer_point_types(&mut points);
    Ok(VectorPath {
        points,
        is_closed: contour.is_closed(),
    })
}

/// Replace exactly one contour in a canonical path from an editor projection.
pub fn replace_path_contour(
    original: &PathValue,
    contour_index: usize,
    edited: &VectorPath,
) -> Result<PathValue, PathProjectionError> {
    if contour_index >= original.contours().len() {
        return Err(PathProjectionError::MissingContour { contour_index });
    }
    let Some(first) = edited.points.first() else {
        return Err(PathProjectionError::EmptyContour { contour_index });
    };
    if edited.points.iter().any(|point| {
        point
            .position
            .into_iter()
            .chain(point.handle_in)
            .chain(point.handle_out)
            .any(|value| !value.is_finite())
    }) {
        return Err(PathProjectionError::CoordinateOutsideEditor {
            contour_index,
            location: "edited vertex".to_string(),
        });
    }

    let mut segments = Vec::new();
    let edge_count = if edited.is_closed {
        edited.points.len()
    } else {
        edited.points.len().saturating_sub(1)
    };
    for index in 0..edge_count {
        let current = &edited.points[index];
        let next_index = (index + 1) % edited.points.len();
        let next = &edited.points[next_index];
        let closing_straight = edited.is_closed
            && next_index == 0
            && is_zero(current.handle_out)
            && is_zero(next.handle_in);
        if closing_straight {
            continue;
        }
        let to = canonical_point(next.position);
        if is_zero(current.handle_out) && is_zero(next.handle_in) {
            segments.push(PathSegment::line(to));
        } else {
            segments.push(PathSegment::cubic(
                canonical_point(add(current.position, current.handle_out)),
                canonical_point(add(next.position, next.handle_in)),
                to,
            ));
        }
    }

    let replacement = PathContour::new(canonical_point(first.position), segments, edited.is_closed);
    let contours = original
        .contours()
        .iter()
        .enumerate()
        .map(|(index, contour)| {
            if index == contour_index {
                replacement.clone()
            } else {
                PathContour::new(
                    contour.start(),
                    contour.segments().to_vec(),
                    contour.is_closed(),
                )
            }
        })
        .collect();
    PathValue::new(original.fill_rule(), contours)
        .map_err(|error| PathProjectionError::InvalidPath(error.to_string()))
}

fn control_point(position: [f32; 2]) -> ControlPoint {
    ControlPoint {
        position,
        handle_in: [0.0, 0.0],
        handle_out: [0.0, 0.0],
        point_type: PointType::Corner,
    }
}

fn editor_point(
    point: PathPoint,
    contour_index: usize,
    location: &str,
) -> Result<[f32; 2], PathProjectionError> {
    let x = point.x() as f32;
    let y = point.y() as f32;
    if x.is_finite() && y.is_finite() {
        Ok([x, y])
    } else {
        Err(PathProjectionError::CoordinateOutsideEditor {
            contour_index,
            location: location.to_string(),
        })
    }
}

fn canonical_point(point: [f32; 2]) -> PathPoint {
    PathPoint::new(f64::from(point[0]), f64::from(point[1]))
}

fn infer_point_types(points: &mut [ControlPoint]) {
    for point in points {
        point.point_type = if collinear_opposite(point.handle_in, point.handle_out) {
            if (length(point.handle_in) - length(point.handle_out)).abs() <= 0.01 {
                PointType::Symmetric
            } else {
                PointType::Smooth
            }
        } else {
            PointType::Corner
        };
    }
}

fn collinear_opposite(left: [f32; 2], right: [f32; 2]) -> bool {
    let left_length = length(left);
    let right_length = length(right);
    if left_length <= EPSILON || right_length <= EPSILON {
        return false;
    }
    let dot = (left[0] / left_length) * (right[0] / right_length)
        + (left[1] / left_length) * (right[1] / right_length);
    (dot + 1.0).abs() <= 0.01
}

fn same_position(left: [f32; 2], right: [f32; 2]) -> bool {
    (left[0] - right[0]).abs() <= EPSILON && (left[1] - right[1]).abs() <= EPSILON
}

fn is_zero(value: [f32; 2]) -> bool {
    length(value) <= EPSILON
}

fn length(value: [f32; 2]) -> f32 {
    value[0].hypot(value[1])
}

fn subtract(left: [f32; 2], right: [f32; 2]) -> [f32; 2] {
    [left[0] - right[0], left[1] - right[1]]
}

fn add(left: [f32; 2], right: [f32; 2]) -> [f32; 2] {
    [left[0] + right[0], left[1] + right[1]]
}

fn scale(value: [f32; 2], amount: f32) -> [f32; 2] {
    [value[0] * amount, value[1] * amount]
}

#[cfg(test)]
mod tests {
    use ordered_float::OrderedFloat;

    use super::*;
    use crate::model::path::FillRule;

    fn rectangle() -> PathValue {
        PathValue::new(
            FillRule::NonZero,
            vec![PathContour::new(
                PathPoint::new(0.0, 0.0),
                vec![
                    PathSegment::line(PathPoint::new(160.0, 0.0)),
                    PathSegment::line(PathPoint::new(160.0, 90.0)),
                    PathSegment::line(PathPoint::new(0.0, 90.0)),
                ],
                true,
            )],
        )
        .unwrap()
    }

    #[test]
    fn closed_line_contour_round_trips_without_duplicate_start() {
        let original = rectangle();
        let projected = project_path_contour(&original, 0).unwrap();
        assert_eq!(projected.points.len(), 4);
        assert!(projected.is_closed);
        assert_eq!(
            replace_path_contour(&original, 0, &projected).unwrap(),
            original
        );
    }

    #[test]
    fn cubic_handles_and_unedited_contours_are_preserved() {
        let second = PathContour::new(
            PathPoint::new(200.0, 200.0),
            vec![PathSegment::line(PathPoint::new(220.0, 220.0))],
            false,
        );
        let original = PathValue::new(
            FillRule::EvenOdd,
            vec![
                PathContour::new(
                    PathPoint::new(0.0, 0.0),
                    vec![PathSegment::cubic(
                        PathPoint::new(20.0, 30.0),
                        PathPoint::new(80.0, 30.0),
                        PathPoint::new(100.0, 0.0),
                    )],
                    false,
                ),
                second.clone(),
            ],
        )
        .unwrap();
        let projected = project_path_contour(&original, 0).unwrap();
        let restored = replace_path_contour(&original, 0, &projected).unwrap();
        assert_eq!(restored, original);
        assert_eq!(restored.contours()[1], second);
        assert_eq!(restored.fill_rule(), FillRule::EvenOdd);
    }

    #[test]
    fn rational_conic_is_rejected_without_projection() {
        let original = PathValue::new(
            FillRule::NonZero,
            vec![PathContour::new(
                PathPoint::new(0.0, 0.0),
                vec![PathSegment::Conic {
                    control: PathPoint::new(50.0, 100.0),
                    to: PathPoint::new(100.0, 0.0),
                    weight: OrderedFloat(0.75),
                }],
                false,
            )],
        )
        .unwrap();
        assert!(matches!(
            project_path_contour(&original, 0),
            Err(PathProjectionError::RationalConic { .. })
        ));
    }
}
