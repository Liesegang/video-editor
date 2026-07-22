//! Explicit SVG path-data boundary for canonical [`super::PathValue`] values.
//!
//! SVG path data has no command for a general weighted rational quadratic.
//! [`SvgPathEnvelope`] therefore keeps a valid, interoperable SVG `d` string
//! together with lossless conic hints. The envelope round trip is lossless;
//! copying only [`SvgPathEnvelope::path_data`] intentionally discards those
//! extensions and may change the rendered geometry because its ordinary
//! quadratic fallback has no rational weight. The legacy string writer rejects
//! conics instead of hiding that loss.

use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use skia_safe::PathVerb;

use super::{FillRule, PathContour, PathPoint, PathSegment, PathValidationError, PathValue};

/// Lossless in-process envelope around interoperable SVG path data.
///
/// `path_data` remains a valid SVG `d` value. `fill_rule` models the separate
/// SVG presentation property, while private conic hints retain information
/// that SVG path syntax cannot express. Serializing the complete envelope is
/// lossless for a value decoded through this codec; serializing only
/// `path_data` is not lossless for conics and its fallback can render different
/// geometry. Parsing raw SVG uses Skia's `f32` coordinate boundary, so this is
/// not a general-purpose `f64` interchange.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SvgPathEnvelope {
    path_data: String,
    fill_rule: FillRule,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    conic_hints: Vec<ConicHint>,
}

impl SvgPathEnvelope {
    /// Wrap raw SVG `d` data. Raw SVG has no RuViE conic extensions.
    pub fn new(path_data: impl Into<String>, fill_rule: FillRule) -> Self {
        Self {
            path_data: path_data.into(),
            fill_rule,
            conic_hints: Vec::new(),
        }
    }

    pub fn path_data(&self) -> &str {
        &self.path_data
    }

    pub fn fill_rule(&self) -> FillRule {
        self.fill_rule
    }

    pub fn into_path_data(self) -> String {
        self.path_data
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConicHint {
    contour_index: usize,
    segment_index: usize,
    weight: OrderedFloat<f64>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SvgPathCodecError {
    #[error("SVG path data contains an interior NUL byte")]
    InteriorNul,
    #[error("invalid SVG path data")]
    InvalidSyntax,
    #[error("SVG path backend produced non-finite geometry")]
    NonFiniteBackendPath,
    #[error("SVG path verb {verb_index} ({verb}) appeared before an explicit move")]
    MissingContourStart {
        verb_index: usize,
        verb: &'static str,
    },
    #[error(
        "SVG path verb {verb_index} ({verb}) returned {actual} points; expected at least {required}"
    )]
    MalformedVerb {
        verb_index: usize,
        verb: &'static str,
        required: usize,
        actual: usize,
    },
    #[error("path {location} value {value} cannot be represented by the SVG/Skia f32 boundary")]
    CoordinateOutOfRange { location: String, value: String },
    #[error(
        "SVG conic hint for contour {contour_index} segment {segment_index} is invalid: {reason}"
    )]
    InvalidConicHint {
        contour_index: usize,
        segment_index: usize,
        reason: &'static str,
    },
    #[error(
        "legacy SVG path strings cannot losslessly represent contour {contour_index} segment {segment_index} conic weight {weight}; use SvgPathEnvelope"
    )]
    LegacyConicUnsupported {
        contour_index: usize,
        segment_index: usize,
        weight: OrderedFloat<f64>,
    },
    #[error(transparent)]
    InvalidPath(#[from] PathValidationError),
}

pub fn decode_svg_path(source: &SvgPathEnvelope) -> Result<PathValue, SvgPathCodecError> {
    if source.path_data.as_bytes().contains(&0) {
        return Err(SvgPathCodecError::InteriorNul);
    }
    let path = skia_safe::utils::parse_path::from_svg(&source.path_data)
        .ok_or(SvgPathCodecError::InvalidSyntax)?;
    if !path.is_finite() {
        return Err(SvgPathCodecError::NonFiniteBackendPath);
    }
    let decoded = path_value_from_skia(&path, source.fill_rule)?;
    restore_conic_hints(decoded, &source.conic_hints)
}

pub fn encode_svg_path(path: &PathValue) -> Result<SvgPathEnvelope, SvgPathCodecError> {
    path.validate()?;
    let (path_data, conic_hints) = write_svg_path_data(path)?;
    Ok(SvgPathEnvelope {
        path_data,
        fill_rule: path.fill_rule(),
        conic_hints,
    })
}

/// Read an existing SVG `d` string whose historic contract implied non-zero
/// fill. New code should retain [`SvgPathEnvelope`] so its fill rule and any
/// conic extensions remain explicit.
pub fn parse_legacy_svg_path_data(data: &str) -> Result<PathValue, SvgPathCodecError> {
    decode_svg_path(&SvgPathEnvelope::new(data, FillRule::NonZero))
}

/// Produce the SVG `d` string still consumed by the current renderer and Shape
/// converter. The caller remains responsible for carrying `path.fill_rule()`.
/// General conic segments return an explicit error because SVG `d` alone
/// cannot retain their weight.
pub fn write_legacy_svg_path_data(path: &PathValue) -> Result<String, SvgPathCodecError> {
    reject_legacy_conics(path)?;
    encode_svg_path(path).map(SvgPathEnvelope::into_path_data)
}

fn write_svg_path_data(path: &PathValue) -> Result<(String, Vec<ConicHint>), SvgPathCodecError> {
    let mut path_data = String::new();
    let mut conic_hints = Vec::new();
    for (contour_index, contour) in path.contours().iter().enumerate() {
        push_svg_command(
            &mut path_data,
            'M',
            &[svg_point(
                contour.start(),
                format!("contour {contour_index} start"),
            )?],
        );
        for (segment_index, segment) in contour.segments().iter().enumerate() {
            let location = || format!("contour {contour_index} segment {segment_index}");
            match segment {
                PathSegment::Line { to } => {
                    push_svg_command(
                        &mut path_data,
                        'L',
                        &[svg_point(*to, format!("{} to", location()))?],
                    );
                }
                PathSegment::Quadratic { control, to } => {
                    push_svg_command(
                        &mut path_data,
                        'Q',
                        &[
                            svg_point(*control, format!("{} control", location()))?,
                            svg_point(*to, format!("{} to", location()))?,
                        ],
                    );
                }
                PathSegment::Conic {
                    control,
                    to,
                    weight,
                } => {
                    let _ = backend_scalar(weight.into_inner(), format!("{} weight", location()))?;
                    push_svg_command(
                        &mut path_data,
                        'Q',
                        &[
                            svg_point(*control, format!("{} control", location()))?,
                            svg_point(*to, format!("{} to", location()))?,
                        ],
                    );
                    conic_hints.push(ConicHint {
                        contour_index,
                        segment_index,
                        weight: *weight,
                    });
                }
                PathSegment::Cubic {
                    control1,
                    control2,
                    to,
                } => {
                    push_svg_command(
                        &mut path_data,
                        'C',
                        &[
                            svg_point(*control1, format!("{} control1", location()))?,
                            svg_point(*control2, format!("{} control2", location()))?,
                            svg_point(*to, format!("{} to", location()))?,
                        ],
                    );
                }
            }
        }
        if contour.is_closed() {
            path_data.push('Z');
            path_data.push(' ');
        }
    }
    Ok((path_data.trim_end().to_owned(), conic_hints))
}

/// Convert a finite backend path into canonical Project geometry at the
/// explicit Skia f32 boundary used by native boolean Path operations.
pub(crate) fn path_value_from_skia(
    path: &skia_safe::Path,
    fill_rule: FillRule,
) -> Result<PathValue, SvgPathCodecError> {
    let mut contours = Vec::new();
    let mut current = None::<PathContourBuilder>;
    for (verb_index, record) in path.iter().enumerate() {
        match record.verb() {
            PathVerb::Move => {
                finish_contour(&mut current, &mut contours);
                current = Some(PathContourBuilder::new(path_point(
                    record.points(),
                    0,
                    verb_index,
                    "move",
                )?));
            }
            PathVerb::Line => current_contour(&mut current, verb_index, "line")?
                .segments
                .push(PathSegment::line(path_point(
                    record.points(),
                    1,
                    verb_index,
                    "line",
                )?)),
            PathVerb::Quad => {
                let control = path_point(record.points(), 1, verb_index, "quadratic")?;
                let to = path_point(record.points(), 2, verb_index, "quadratic")?;
                current_contour(&mut current, verb_index, "quadratic")?
                    .segments
                    .push(PathSegment::quadratic(control, to));
            }
            PathVerb::Conic => {
                let control = path_point(record.points(), 1, verb_index, "conic")?;
                let to = path_point(record.points(), 2, verb_index, "conic")?;
                current_contour(&mut current, verb_index, "conic")?
                    .segments
                    .push(PathSegment::conic(
                        control,
                        to,
                        f64::from(record.conic_weight()),
                    ));
            }
            PathVerb::Cubic => {
                let control1 = path_point(record.points(), 1, verb_index, "cubic")?;
                let control2 = path_point(record.points(), 2, verb_index, "cubic")?;
                let to = path_point(record.points(), 3, verb_index, "cubic")?;
                current_contour(&mut current, verb_index, "cubic")?
                    .segments
                    .push(PathSegment::cubic(control1, control2, to));
            }
            PathVerb::Close => {
                current_contour(&mut current, verb_index, "close")?.closed = true;
                finish_contour(&mut current, &mut contours);
            }
        }
    }
    finish_contour(&mut current, &mut contours);
    PathValue::new(fill_rule, contours).map_err(Into::into)
}

fn restore_conic_hints(
    path: PathValue,
    hints: &[ConicHint],
) -> Result<PathValue, SvgPathCodecError> {
    let mut contours = path
        .contours()
        .iter()
        .map(|contour| {
            PathContour::new(
                contour.start(),
                contour.segments().to_vec(),
                contour.is_closed(),
            )
        })
        .collect::<Vec<_>>();
    for hint in hints {
        let segment = contours
            .get_mut(hint.contour_index)
            .and_then(|contour| contour.segments.get_mut(hint.segment_index))
            .ok_or(SvgPathCodecError::InvalidConicHint {
                contour_index: hint.contour_index,
                segment_index: hint.segment_index,
                reason: "segment does not exist",
            })?;
        let (control, to) = match segment {
            PathSegment::Quadratic { control, to } => (*control, *to),
            _ => {
                return Err(SvgPathCodecError::InvalidConicHint {
                    contour_index: hint.contour_index,
                    segment_index: hint.segment_index,
                    reason: "target is not the quadratic SVG fallback",
                });
            }
        };
        *segment = PathSegment::Conic {
            control,
            to,
            weight: hint.weight,
        };
    }
    PathValue::new(path.fill_rule(), contours).map_err(Into::into)
}

fn reject_legacy_conics(path: &PathValue) -> Result<(), SvgPathCodecError> {
    for (contour_index, contour) in path.contours().iter().enumerate() {
        for (segment_index, segment) in contour.segments().iter().enumerate() {
            if let PathSegment::Conic { weight, .. } = segment {
                return Err(SvgPathCodecError::LegacyConicUnsupported {
                    contour_index,
                    segment_index,
                    weight: *weight,
                });
            }
        }
    }
    Ok(())
}

struct PathContourBuilder {
    start: PathPoint,
    segments: Vec<PathSegment>,
    closed: bool,
}

impl PathContourBuilder {
    fn new(start: PathPoint) -> Self {
        Self {
            start,
            segments: Vec::new(),
            closed: false,
        }
    }
}

fn current_contour<'a>(
    current: &'a mut Option<PathContourBuilder>,
    verb_index: usize,
    verb: &'static str,
) -> Result<&'a mut PathContourBuilder, SvgPathCodecError> {
    current
        .as_mut()
        .ok_or(SvgPathCodecError::MissingContourStart { verb_index, verb })
}

fn finish_contour(current: &mut Option<PathContourBuilder>, contours: &mut Vec<PathContour>) {
    if let Some(contour) = current.take() {
        contours.push(PathContour::new(
            contour.start,
            contour.segments,
            contour.closed,
        ));
    }
}

fn path_point(
    points: &[skia_safe::Point],
    index: usize,
    verb_index: usize,
    verb: &'static str,
) -> Result<PathPoint, SvgPathCodecError> {
    points
        .get(index)
        .map(|point| PathPoint::new(f64::from(point.x), f64::from(point.y)))
        .ok_or(SvgPathCodecError::MalformedVerb {
            verb_index,
            verb,
            required: index + 1,
            actual: points.len(),
        })
}

fn svg_point(point: PathPoint, location: String) -> Result<(f32, f32), SvgPathCodecError> {
    Ok((
        backend_scalar(point.x(), format!("{location}.x"))?,
        backend_scalar(point.y(), format!("{location}.y"))?,
    ))
}

fn push_svg_command(path_data: &mut String, command: char, points: &[(f32, f32)]) {
    path_data.push(command);
    for (x, y) in points {
        path_data.push_str(&x.to_string());
        path_data.push(' ');
        path_data.push_str(&y.to_string());
        path_data.push(' ');
    }
}

fn backend_scalar(value: f64, location: String) -> Result<f32, SvgPathCodecError> {
    let converted = value as f32;
    if converted.is_finite() {
        Ok(converted)
    } else {
        Err(SvgPathCodecError::CoordinateOutOfRange {
            location,
            value: value.to_string(),
        })
    }
}
