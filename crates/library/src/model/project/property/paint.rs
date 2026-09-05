use std::fmt;

use ordered_float::OrderedFloat;
use serde::de::Error as _;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{ColorValue, Vec2};

const PROPERTY_TYPE_FIELD: &str = "$type";
const GRADIENT_VALUE_TAG: &str = "gradient_value";
const PATTERN_VALUE_TAG: &str = "pattern_value";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GradientSpread {
    Pad,
    Repeat,
    Reflect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GradientGeometry {
    /// Start and end are normalized to the rendered shape surface, where
    /// `(0, 0)` is top-left and `(1, 1)` is bottom-right.
    Linear { start: Vec2, end: Vec2 },
    /// Center and radius are normalized to the rendered shape surface.
    Radial {
        center: Vec2,
        radius: OrderedFloat<f64>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GradientStop {
    offset: OrderedFloat<f64>,
    color: ColorValue,
}

impl<'de> Deserialize<'de> for GradientStop {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            offset: OrderedFloat<f64>,
            color: ColorValue,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.offset.into_inner(), wire.color).map_err(D::Error::custom)
    }
}

impl GradientStop {
    pub fn new(offset: f64, color: ColorValue) -> Result<Self, PaintValueError> {
        if !offset.is_finite() || !(0.0..=1.0).contains(&offset) {
            return Err(PaintValueError::InvalidGradientStopOffset);
        }
        Ok(Self {
            offset: OrderedFloat(offset),
            color,
        })
    }

    pub fn offset(&self) -> f64 {
        self.offset.into_inner()
    }

    pub fn color(&self) -> &ColorValue {
        &self.color
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GradientValue {
    geometry: GradientGeometry,
    spread: GradientSpread,
    stops: Vec<GradientStop>,
}

impl GradientValue {
    pub fn new(
        geometry: GradientGeometry,
        spread: GradientSpread,
        stops: Vec<GradientStop>,
    ) -> Result<Self, PaintValueError> {
        validate_gradient_geometry(geometry)?;
        if stops.len() < 2 {
            return Err(PaintValueError::TooFewGradientStops);
        }
        if stops.windows(2).any(|pair| pair[0].offset > pair[1].offset) {
            return Err(PaintValueError::UnsortedGradientStops);
        }
        Ok(Self {
            geometry,
            spread,
            stops,
        })
    }

    pub fn geometry(&self) -> GradientGeometry {
        self.geometry
    }

    pub fn spread(&self) -> GradientSpread {
        self.spread
    }

    pub fn stops(&self) -> &[GradientStop] {
        &self.stops
    }
}

impl Serialize for GradientValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("GradientValue", 4)?;
        state.serialize_field(PROPERTY_TYPE_FIELD, GRADIENT_VALUE_TAG)?;
        state.serialize_field("geometry", &self.geometry)?;
        state.serialize_field("spread", &self.spread)?;
        state.serialize_field("stops", &self.stops)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for GradientValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            #[serde(rename = "$type")]
            value_type: String,
            geometry: GradientGeometry,
            spread: GradientSpread,
            stops: Vec<GradientStop>,
        }

        let wire = Wire::deserialize(deserializer)?;
        if wire.value_type != GRADIENT_VALUE_TAG {
            return Err(D::Error::custom(format!(
                "gradient value tag must be {GRADIENT_VALUE_TAG:?}, got {:?}",
                wire.value_type
            )));
        }
        Self::new(wire.geometry, wire.spread, wire.stops).map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternKind {
    Checker,
    Stripes,
    Dots,
    Grid,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PatternValue {
    kind: PatternKind,
    foreground: ColorValue,
    background: ColorValue,
    scale: Vec2,
    phase: Vec2,
    angle: OrderedFloat<f64>,
    duty: OrderedFloat<f64>,
}

impl PatternValue {
    pub fn new(
        kind: PatternKind,
        foreground: ColorValue,
        background: ColorValue,
        scale: Vec2,
        phase: Vec2,
        angle: f64,
        duty: f64,
    ) -> Result<Self, PaintValueError> {
        let finite = [scale.x, scale.y, phase.x, phase.y]
            .into_iter()
            .all(|value| value.into_inner().is_finite())
            && angle.is_finite()
            && duty.is_finite();
        if !finite {
            return Err(PaintValueError::NonFinitePatternGeometry);
        }
        if scale.x <= OrderedFloat(0.0) || scale.y <= OrderedFloat(0.0) {
            return Err(PaintValueError::NonPositivePatternScale);
        }
        if !(0.0..=1.0).contains(&duty) {
            return Err(PaintValueError::InvalidPatternDuty);
        }
        Ok(Self {
            kind,
            foreground,
            background,
            scale,
            phase,
            angle: OrderedFloat(angle),
            duty: OrderedFloat(duty),
        })
    }

    pub fn kind(&self) -> PatternKind {
        self.kind
    }

    pub fn foreground(&self) -> &ColorValue {
        &self.foreground
    }

    pub fn background(&self) -> &ColorValue {
        &self.background
    }

    pub fn scale(&self) -> Vec2 {
        self.scale
    }

    pub fn phase(&self) -> Vec2 {
        self.phase
    }

    pub fn angle(&self) -> f64 {
        self.angle.into_inner()
    }

    pub fn duty(&self) -> f64 {
        self.duty.into_inner()
    }
}

impl Serialize for PatternValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("PatternValue", 8)?;
        state.serialize_field(PROPERTY_TYPE_FIELD, PATTERN_VALUE_TAG)?;
        state.serialize_field("kind", &self.kind)?;
        state.serialize_field("foreground", &self.foreground)?;
        state.serialize_field("background", &self.background)?;
        state.serialize_field("scale", &self.scale)?;
        state.serialize_field("phase", &self.phase)?;
        state.serialize_field("angle", &self.angle)?;
        state.serialize_field("duty", &self.duty)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for PatternValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            #[serde(rename = "$type")]
            value_type: String,
            kind: PatternKind,
            foreground: ColorValue,
            background: ColorValue,
            scale: Vec2,
            phase: Vec2,
            angle: OrderedFloat<f64>,
            duty: OrderedFloat<f64>,
        }

        let wire = Wire::deserialize(deserializer)?;
        if wire.value_type != PATTERN_VALUE_TAG {
            return Err(D::Error::custom(format!(
                "pattern value tag must be {PATTERN_VALUE_TAG:?}, got {:?}",
                wire.value_type
            )));
        }
        Self::new(
            wire.kind,
            wire.foreground,
            wire.background,
            wire.scale,
            wire.phase,
            wire.angle.into_inner(),
            wire.duty.into_inner(),
        )
        .map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintValueError {
    NonFiniteGradientGeometry,
    DegenerateGradientGeometry,
    TooFewGradientStops,
    InvalidGradientStopOffset,
    UnsortedGradientStops,
    NonFinitePatternGeometry,
    NonPositivePatternScale,
    InvalidPatternDuty,
}

impl fmt::Display for PaintValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NonFiniteGradientGeometry => "gradient geometry must be finite",
            Self::DegenerateGradientGeometry => "gradient geometry must have a positive extent",
            Self::TooFewGradientStops => "gradient requires at least two color stops",
            Self::InvalidGradientStopOffset => {
                "gradient stop offset must be finite and between zero and one"
            }
            Self::UnsortedGradientStops => "gradient stops must be sorted by offset",
            Self::NonFinitePatternGeometry => "pattern geometry must be finite",
            Self::NonPositivePatternScale => "pattern scale must be positive on both axes",
            Self::InvalidPatternDuty => "pattern duty must be between zero and one",
        })
    }
}

impl std::error::Error for PaintValueError {}

fn validate_gradient_geometry(geometry: GradientGeometry) -> Result<(), PaintValueError> {
    let values = match geometry {
        GradientGeometry::Linear { start, end } => [start.x, start.y, end.x, end.y],
        GradientGeometry::Radial { center, radius } => {
            if radius <= OrderedFloat(0.0) {
                return Err(PaintValueError::DegenerateGradientGeometry);
            }
            [center.x, center.y, radius, radius]
        }
    };
    if !values
        .into_iter()
        .all(|value| value.into_inner().is_finite())
    {
        return Err(PaintValueError::NonFiniteGradientGeometry);
    }
    let degenerate_linear = matches!(
        geometry,
        GradientGeometry::Linear { start, end } if start == end
    );
    if degenerate_linear {
        return Err(PaintValueError::DegenerateGradientGeometry);
    }
    Ok(())
}

pub(crate) fn has_gradient_value_tag_json(value: &serde_json::Value) -> bool {
    value
        .as_object()
        .and_then(|object| object.get(PROPERTY_TYPE_FIELD))
        .and_then(serde_json::Value::as_str)
        == Some(GRADIENT_VALUE_TAG)
}

pub(crate) fn has_pattern_value_tag_json(value: &serde_json::Value) -> bool {
    value
        .as_object()
        .and_then(|object| object.get(PROPERTY_TYPE_FIELD))
        .and_then(serde_json::Value::as_str)
        == Some(PATTERN_VALUE_TAG)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::property::{ColorSpaceRef, PropertyValue};

    fn color(value: f64) -> ColorValue {
        ColorValue::new(ColorSpaceRef::linear_srgb(), [value, 0.25, 0.5, 1.0])
            .expect("managed color")
    }

    fn point(x: f64, y: f64) -> Vec2 {
        Vec2 {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
        }
    }

    #[test]
    fn gradient_round_trip_preserves_managed_stops_and_geometry() {
        let value = GradientValue::new(
            GradientGeometry::Linear {
                start: point(0.0, 0.0),
                end: point(640.0, 360.0),
            },
            GradientSpread::Reflect,
            vec![
                GradientStop::new(0.0, color(-0.5)).unwrap(),
                GradientStop::new(1.0, color(2.0)).unwrap(),
            ],
        )
        .unwrap();
        let encoded = serde_json::to_string(&value).unwrap();
        assert!(encoded.contains("gradient_value"));
        assert_eq!(
            serde_json::from_str::<GradientValue>(&encoded).unwrap(),
            value
        );
    }

    #[test]
    fn invalid_gradient_and_pattern_geometry_is_rejected_on_load() {
        assert!(
            serde_json::from_value::<GradientStop>(serde_json::json!({
                "offset": 1.5,
                "color": color(1.0),
            }))
            .is_err(),
            "deserialization must not bypass the GradientStop invariant"
        );
        assert!(
            GradientValue::new(
                GradientGeometry::Linear {
                    start: point(0.0, 0.0),
                    end: point(0.0, 0.0),
                },
                GradientSpread::Pad,
                vec![
                    GradientStop::new(0.0, color(0.0)).unwrap(),
                    GradientStop::new(1.0, color(1.0)).unwrap(),
                ],
            )
            .is_err()
        );
        assert!(
            PatternValue::new(
                PatternKind::Checker,
                color(1.0),
                color(0.0),
                point(0.0, 12.0),
                point(0.0, 0.0),
                0.0,
                0.5,
            )
            .is_err()
        );
    }

    #[test]
    fn property_value_round_trip_is_typed_and_malformed_envelopes_are_preserved() {
        let gradient = GradientValue::new(
            GradientGeometry::Radial {
                center: point(0.5, 0.5),
                radius: OrderedFloat(0.5),
            },
            GradientSpread::Repeat,
            vec![
                GradientStop::new(0.0, color(0.0)).unwrap(),
                GradientStop::new(1.0, color(1.0)).unwrap(),
            ],
        )
        .unwrap();
        let property = PropertyValue::Gradient(gradient);
        let encoded = serde_json::to_string(&property).unwrap();
        assert_eq!(
            serde_json::from_str::<PropertyValue>(&encoded).unwrap(),
            property
        );

        let malformed = serde_json::json!({
            "$type": "gradient_value",
            "geometry": {
                "kind": "linear",
                "start": { "x": 0.0, "y": 0.0 },
                "end": { "x": 0.0, "y": 0.0 }
            },
            "spread": "pad",
            "stops": []
        });
        assert_eq!(
            serde_json::from_value::<PropertyValue>(malformed.clone()).unwrap(),
            PropertyValue::OpaqueJson(malformed)
        );
    }
}
