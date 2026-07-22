//! Explicit conversion from Project-authoritative path data to Skia geometry.
//!
//! This is a renderer boundary: the graph retains f64 coordinates and exact
//! rational-conic weights, while Skia consumes f32 scalars for one frame.

use skia_safe::{Path, PathBuilder, PathFillType, Point};

use crate::error::LibraryError;
use crate::model::path::{FillRule, PathPoint, PathSegment, PathValue};

pub(crate) fn to_skia_path(value: &PathValue) -> Result<Path, LibraryError> {
    value
        .validate()
        .map_err(|error| LibraryError::Render(format!("Invalid canonical path: {error}")))?;
    let fill_type = match value.fill_rule() {
        FillRule::NonZero => PathFillType::Winding,
        FillRule::EvenOdd => PathFillType::EvenOdd,
    };
    let mut builder = PathBuilder::new_with_fill_type(fill_type);
    for (contour_index, contour) in value.contours().iter().enumerate() {
        builder.move_to(point(contour.start(), contour_index, "start")?);
        for (segment_index, segment) in contour.segments().iter().enumerate() {
            let location = |field| format!("segment {segment_index} {field}");
            match segment {
                PathSegment::Line { to } => {
                    builder.line_to(point(*to, contour_index, &location("to"))?);
                }
                PathSegment::Quadratic { control, to } => {
                    builder.quad_to(
                        point(*control, contour_index, &location("control"))?,
                        point(*to, contour_index, &location("to"))?,
                    );
                }
                PathSegment::Conic {
                    control,
                    to,
                    weight,
                } => {
                    let weight =
                        renderer_scalar(weight.into_inner(), contour_index, &location("weight"))?;
                    builder.conic_to(
                        point(*control, contour_index, &location("control"))?,
                        point(*to, contour_index, &location("to"))?,
                        weight,
                    );
                }
                PathSegment::Cubic {
                    control1,
                    control2,
                    to,
                } => {
                    builder.cubic_to(
                        point(*control1, contour_index, &location("control1"))?,
                        point(*control2, contour_index, &location("control2"))?,
                        point(*to, contour_index, &location("to"))?,
                    );
                }
            }
        }
        if contour.is_closed() {
            builder.close();
        }
    }
    let path = builder.detach();
    if !path.is_finite() {
        return Err(LibraryError::Render(
            "Canonical path became non-finite at the Skia f32 boundary".to_string(),
        ));
    }
    Ok(path)
}

/// Convert a native Path operation result back into canonical Project data.
/// Boolean PathOps normalize their result to non-zero winding geometry, so
/// this boundary never infers a fill rule from renderer-only inverse modes.
pub(crate) fn from_skia_boolean_path(value: &Path) -> Result<PathValue, LibraryError> {
    if !value.is_finite() {
        return Err(LibraryError::Render(
            "Native Path operation produced non-finite geometry".to_string(),
        ));
    }
    crate::model::path::path_value_from_skia(value, FillRule::NonZero)
        .map_err(|error| LibraryError::Render(format!("Invalid native Path result: {error}")))
}

pub(crate) fn resolve_renderer_path(
    canonical: Option<&PathValue>,
    legacy_svg: &str,
) -> Result<Path, LibraryError> {
    let path = if let Some(canonical) = canonical {
        to_skia_path(canonical)?
    } else {
        Path::from_svg(legacy_svg).ok_or_else(|| {
            LibraryError::Render(format!("Invalid or empty SVG path data: {legacy_svg:?}"))
        })?
    };
    if path.is_empty() {
        return Err(LibraryError::Render(format!(
            "Invalid or empty path geometry: {legacy_svg:?}"
        )));
    }
    Ok(path)
}

fn point(value: PathPoint, contour_index: usize, location: &str) -> Result<Point, LibraryError> {
    Ok(Point::new(
        renderer_scalar(value.x(), contour_index, &format!("{location}.x"))?,
        renderer_scalar(value.y(), contour_index, &format!("{location}.y"))?,
    ))
}

fn renderer_scalar(value: f64, contour_index: usize, location: &str) -> Result<f32, LibraryError> {
    let converted = value as f32;
    if !converted.is_finite() {
        return Err(LibraryError::Render(format!(
            "Canonical path contour {contour_index} {location} value {value} is outside the Skia f32 boundary"
        )));
    }
    Ok(converted)
}

#[cfg(test)]
mod tests {
    use skia_safe::{PathFillType, PathVerb};

    use super::*;
    use crate::model::path::{PathContour, PathPoint};

    #[test]
    fn preserves_fill_rule_and_arbitrary_conic_weight() -> Result<(), LibraryError> {
        let value = PathValue::new(
            FillRule::EvenOdd,
            vec![PathContour::new(
                PathPoint::new(0.0, 0.0),
                vec![PathSegment::conic(
                    PathPoint::new(5.0, 10.0),
                    PathPoint::new(10.0, 0.0),
                    0.375,
                )],
                true,
            )],
        )
        .map_err(|error| LibraryError::Render(error.to_string()))?;
        let path = to_skia_path(&value)?;
        assert_eq!(path.fill_type(), PathFillType::EvenOdd);
        let conic = path
            .iter()
            .find(|record| record.verb() == PathVerb::Conic)
            .ok_or_else(|| LibraryError::Render("Skia path lost conic verb".to_string()))?;
        assert!((conic.conic_weight() - 0.375).abs() <= f32::EPSILON);
        Ok(())
    }
}
