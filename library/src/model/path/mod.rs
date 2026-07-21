//! Canonical authored two-dimensional path values.
//!
//! A [`PathValue`] is Project data, not a renderer object or an SVG string.
//! Backend and interchange formats are converted only at explicit boundaries.

use ordered_float::OrderedFloat;
use serde::{Deserialize, Deserializer, Serialize};

mod svg;

pub use svg::{
    SvgPathCodecError, SvgPathEnvelope, decode_svg_path, encode_svg_path,
    parse_legacy_svg_path_data, write_legacy_svg_path_data,
};

#[cfg(test)]
mod tests;

/// Fill rule applied across every contour of one path value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

/// One finite point in authored path coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathPoint {
    x: OrderedFloat<f64>,
    y: OrderedFloat<f64>,
}

impl PathPoint {
    pub fn new(x: f64, y: f64) -> Self {
        Self {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
        }
    }

    pub fn x(self) -> f64 {
        self.x.into_inner()
    }

    pub fn y(self) -> f64 {
        self.y.into_inner()
    }
}

/// One segment whose start is the preceding segment's end, or its contour's
/// explicit start for the first segment.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PathSegment {
    Line {
        to: PathPoint,
    },
    Quadratic {
        control: PathPoint,
        to: PathPoint,
    },
    Conic {
        control: PathPoint,
        to: PathPoint,
        weight: OrderedFloat<f64>,
    },
    Cubic {
        control1: PathPoint,
        control2: PathPoint,
        to: PathPoint,
    },
}

impl PathSegment {
    pub fn line(to: PathPoint) -> Self {
        Self::Line { to }
    }

    pub fn quadratic(control: PathPoint, to: PathPoint) -> Self {
        Self::Quadratic { control, to }
    }

    pub fn conic(control: PathPoint, to: PathPoint, weight: f64) -> Self {
        Self::Conic {
            control,
            to,
            weight: OrderedFloat(weight),
        }
    }

    pub fn cubic(control1: PathPoint, control2: PathPoint, to: PathPoint) -> Self {
        Self::Cubic {
            control1,
            control2,
            to,
        }
    }
}

/// One independent contour. Closure belongs to each contour rather than to
/// the complete path, so compound paths can mix open and closed geometry.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathContour {
    start: PathPoint,
    segments: Vec<PathSegment>,
    closed: bool,
}

impl PathContour {
    pub fn new(start: PathPoint, segments: Vec<PathSegment>, closed: bool) -> Self {
        Self {
            start,
            segments,
            closed,
        }
    }

    pub fn start(&self) -> PathPoint {
        self.start
    }

    pub fn segments(&self) -> &[PathSegment] {
        &self.segments
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

/// Canonical Project-side path geometry.
///
/// Construction and deserialization reject every non-finite coordinate or
/// conic weight. Empty paths and move-only contours remain legal values;
/// consumers decide whether those values produce visible output.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PathValue {
    fill_rule: FillRule,
    contours: Vec<PathContour>,
}

impl PathValue {
    pub fn new(
        fill_rule: FillRule,
        contours: Vec<PathContour>,
    ) -> Result<Self, PathValidationError> {
        let value = Self {
            fill_rule,
            contours,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn empty(fill_rule: FillRule) -> Self {
        Self {
            fill_rule,
            contours: Vec::new(),
        }
    }

    pub fn fill_rule(&self) -> FillRule {
        self.fill_rule
    }

    pub fn contours(&self) -> &[PathContour] {
        &self.contours
    }

    pub fn validate(&self) -> Result<(), PathValidationError> {
        for (contour_index, contour) in self.contours.iter().enumerate() {
            validate_point(contour.start, contour_index, None, "start")?;
            for (segment_index, segment) in contour.segments.iter().enumerate() {
                match segment {
                    PathSegment::Line { to } => {
                        validate_point(*to, contour_index, Some(segment_index), "to")?;
                    }
                    PathSegment::Quadratic { control, to } => {
                        validate_point(*control, contour_index, Some(segment_index), "control")?;
                        validate_point(*to, contour_index, Some(segment_index), "to")?;
                    }
                    PathSegment::Conic {
                        control,
                        to,
                        weight,
                    } => {
                        validate_point(*control, contour_index, Some(segment_index), "control")?;
                        validate_point(*to, contour_index, Some(segment_index), "to")?;
                        if !weight.is_finite() {
                            return Err(PathValidationError::NonFiniteConicWeight {
                                contour_index,
                                segment_index,
                                value: *weight,
                            });
                        }
                    }
                    PathSegment::Cubic {
                        control1,
                        control2,
                        to,
                    } => {
                        validate_point(*control1, contour_index, Some(segment_index), "control1")?;
                        validate_point(*control2, contour_index, Some(segment_index), "control2")?;
                        validate_point(*to, contour_index, Some(segment_index), "to")?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathValueData {
    fill_rule: FillRule,
    contours: Vec<PathContour>,
}

impl<'de> Deserialize<'de> for PathValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = PathValueData::deserialize(deserializer)?;
        Self::new(data.fill_rule, data.contours).map_err(serde::de::Error::custom)
    }
}

/// Exact location and value of malformed canonical path data.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PathValidationError {
    #[error("path contour {contour_index} {location}.{axis} must be finite, got {value}")]
    NonFiniteCoordinate {
        contour_index: usize,
        location: String,
        axis: &'static str,
        value: OrderedFloat<f64>,
    },
    #[error(
        "path contour {contour_index} segment {segment_index} conic weight must be finite, got {value}"
    )]
    NonFiniteConicWeight {
        contour_index: usize,
        segment_index: usize,
        value: OrderedFloat<f64>,
    },
}

fn validate_point(
    point: PathPoint,
    contour_index: usize,
    segment_index: Option<usize>,
    field: &str,
) -> Result<(), PathValidationError> {
    let location = segment_index.map_or_else(
        || field.to_string(),
        |index| format!("segment {index} {field}"),
    );
    for (axis, value) in [("x", point.x), ("y", point.y)] {
        if !value.is_finite() {
            return Err(PathValidationError::NonFiniteCoordinate {
                contour_index,
                location,
                axis,
                value,
            });
        }
    }
    Ok(())
}
