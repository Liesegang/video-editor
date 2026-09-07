use std::fmt;

use ordered_float::OrderedFloat;
use serde::de::Error as _;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{ColorValue, PropertyValue, Vec2};

const PROPERTY_TYPE_FIELD: &str = "$type";
const GRADIENT_VALUE_TAG: &str = "gradient_value";
const PATTERN_VALUE_TAG: &str = "pattern_value";
const PAINT_VALUE_TAG: &str = "paint_value";

/// A reusable authored paint. Every variant retains managed colors and exact
/// typed geometry; consumers never flatten Gradient or Pattern to a
/// representative Solid swatch.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub enum Paint {
    Solid(ColorValue),
    Gradient(GradientValue),
    Pattern(PatternValue),
}

impl Default for Paint {
    fn default() -> Self {
        Self::Solid(ColorValue::from_straight_srgba8(
            &crate::model::frame::color::Color::white(),
        ))
    }
}

impl Serialize for Paint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let (kind, value) = match self {
            Self::Solid(value) => ("solid", serde_json::to_value(value)),
            Self::Gradient(value) => ("gradient", serde_json::to_value(value)),
            Self::Pattern(value) => ("pattern", serde_json::to_value(value)),
        };
        let mut state = serializer.serialize_struct("Paint", 3)?;
        state.serialize_field(PROPERTY_TYPE_FIELD, PAINT_VALUE_TAG)?;
        state.serialize_field("kind", kind)?;
        state.serialize_field("value", &value.map_err(serde::ser::Error::custom)?)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Paint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            #[serde(rename = "$type")]
            value_type: String,
            kind: String,
            value: serde_json::Value,
        }

        let wire = Wire::deserialize(deserializer)?;
        if wire.value_type != PAINT_VALUE_TAG {
            return Err(D::Error::custom(format!(
                "paint value tag must be {PAINT_VALUE_TAG:?}, got {:?}",
                wire.value_type
            )));
        }
        match wire.kind.as_str() {
            "solid" => serde_json::from_value(wire.value)
                .map(Self::Solid)
                .map_err(D::Error::custom),
            "gradient" => serde_json::from_value(wire.value)
                .map(Self::Gradient)
                .map_err(D::Error::custom),
            "pattern" => serde_json::from_value(wire.value)
                .map(Self::Pattern)
                .map_err(D::Error::custom),
            kind => Err(D::Error::custom(format!(
                "unknown paint value kind {kind:?}"
            ))),
        }
    }
}

impl From<ColorValue> for Paint {
    fn from(value: ColorValue) -> Self {
        Self::Solid(value)
    }
}

impl From<crate::model::frame::color::Color> for Paint {
    fn from(value: crate::model::frame::color::Color) -> Self {
        Self::Solid(ColorValue::from_straight_srgba8(&value))
    }
}

impl From<GradientValue> for Paint {
    fn from(value: GradientValue) -> Self {
        Self::Gradient(value)
    }
}

impl From<PatternValue> for Paint {
    fn from(value: PatternValue) -> Self {
        Self::Pattern(value)
    }
}

impl Paint {
    /// Lossless one-way injection from a concrete graph value into Paint.
    /// Paint is never flattened back to a representative Color or Gradient.
    pub fn from_property_value(value: &PropertyValue) -> Option<Self> {
        match value {
            PropertyValue::Paint(paint) => Some(paint.clone()),
            PropertyValue::ColorValue(color) => Some(Self::Solid(color.clone())),
            PropertyValue::Color(color) => {
                Some(Self::Solid(ColorValue::from_straight_srgba8(color)))
            }
            PropertyValue::Gradient(gradient) => Some(Self::Gradient(gradient.clone())),
            PropertyValue::Pattern(pattern) => Some(Self::Pattern(pattern.clone())),
            PropertyValue::Integer(_)
            | PropertyValue::Number(_)
            | PropertyValue::String(_)
            | PropertyValue::Boolean(_)
            | PropertyValue::Vec2(_)
            | PropertyValue::Vec3(_)
            | PropertyValue::Vec4(_)
            | PropertyValue::Path(_)
            | PropertyValue::Array(_)
            | PropertyValue::Map(_)
            | PropertyValue::OpaqueJson(_) => None,
        }
    }

    /// Interpolate only the existing managed Solid-color case. Structural
    /// paints and changes between variants retain keyframe step semantics.
    pub fn interpolate_solid(&self, end: &Self, t: f64) -> Option<Self> {
        match (self, end) {
            (Self::Solid(start), Self::Solid(end)) => {
                start.interpolate_same_space(end, t).map(Self::Solid)
            }
            _ => None,
        }
    }
}

pub(crate) fn has_paint_value_tag_json(value: &serde_json::Value) -> bool {
    value
        .as_object()
        .and_then(|object| object.get(PROPERTY_TYPE_FIELD))
        .and_then(serde_json::Value::as_str)
        == Some(PAINT_VALUE_TAG)
}

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

impl Default for GradientValue {
    fn default() -> Self {
        Self {
            geometry: GradientGeometry::Linear {
                start: Vec2 {
                    x: OrderedFloat(0.0),
                    y: OrderedFloat(0.5),
                },
                end: Vec2 {
                    x: OrderedFloat(1.0),
                    y: OrderedFloat(0.5),
                },
            },
            spread: GradientSpread::Pad,
            stops: vec![
                GradientStop {
                    offset: OrderedFloat(0.0),
                    color: ColorValue::from_straight_srgba8(&crate::model::frame::color::Color {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 255,
                    }),
                },
                GradientStop {
                    offset: OrderedFloat(1.0),
                    color: ColorValue::from_straight_srgba8(
                        &crate::model::frame::color::Color::white(),
                    ),
                },
            ],
        }
    }
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

impl Default for PatternValue {
    fn default() -> Self {
        Self {
            kind: PatternKind::Checker,
            foreground: ColorValue::from_straight_srgba8(
                &crate::model::frame::color::Color::white(),
            ),
            background: ColorValue::from_straight_srgba8(
                &crate::model::frame::color::Color::black(),
            ),
            scale: Vec2 {
                x: OrderedFloat(32.0),
                y: OrderedFloat(32.0),
            },
            phase: Vec2 {
                x: OrderedFloat(0.0),
                y: OrderedFloat(0.0),
            },
            angle: OrderedFloat(0.0),
            duty: OrderedFloat(0.5),
        }
    }
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
    fn shared_gradient_default_preserves_the_bundled_black_to_white_ramp() {
        let value = GradientValue::default();
        assert_eq!(
            value.geometry(),
            GradientGeometry::Linear {
                start: point(0.0, 0.5),
                end: point(1.0, 0.5),
            }
        );
        assert_eq!(value.spread(), GradientSpread::Pad);
        assert_eq!(value.stops().len(), 2);
        assert_eq!(
            value.stops()[0].color(),
            &ColorValue::from_straight_srgba8(&crate::model::frame::color::Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            })
        );
        assert_eq!(
            value.stops()[1].color(),
            &ColorValue::from_straight_srgba8(&crate::model::frame::color::Color::white())
        );
    }

    #[test]
    fn shared_pattern_default_preserves_the_bundled_checker() {
        let value = PatternValue::default();
        assert_eq!(value.kind(), PatternKind::Checker);
        assert_eq!(
            value.foreground(),
            &ColorValue::from_straight_srgba8(&crate::model::frame::color::Color::white())
        );
        assert_eq!(
            value.background(),
            &ColorValue::from_straight_srgba8(&crate::model::frame::color::Color::black())
        );
        assert_eq!(value.scale(), point(32.0, 32.0));
        assert_eq!(value.phase(), point(0.0, 0.0));
        assert_eq!(value.angle(), 0.0);
        assert_eq!(value.duty(), 0.5);
    }

    #[test]
    fn paint_property_round_trip_and_injections_preserve_the_selected_variant() {
        let solid = Paint::from(color(0.75));
        let property = PropertyValue::Paint(solid.clone());
        let encoded = serde_json::to_value(&property).unwrap();
        assert_eq!(encoded["$type"], "paint_value");
        assert_eq!(encoded["kind"], "solid");
        assert_eq!(
            serde_json::from_value::<PropertyValue>(encoded).unwrap(),
            property
        );
        assert_eq!(Paint::from_property_value(&property), Some(solid));
        assert!(matches!(
            Paint::from_property_value(&PropertyValue::Gradient(GradientValue::default())),
            Some(Paint::Gradient(_))
        ));
        assert!(matches!(
            Paint::from_property_value(&PropertyValue::Pattern(PatternValue::default())),
            Some(Paint::Pattern(_))
        ));
    }

    #[test]
    fn malformed_paint_envelopes_are_preserved_as_opaque_json() {
        let malformed = serde_json::json!({
            "$type": "paint_value",
            "kind": "solid",
            "value": color(0.5),
            "unexpected": true,
        });
        assert_eq!(
            serde_json::from_value::<PropertyValue>(malformed.clone()).unwrap(),
            PropertyValue::OpaqueJson(malformed)
        );
        assert!(
            serde_json::from_value::<Paint>(serde_json::json!({
                "kind": "solid",
                "value": color(0.5),
            }))
            .is_err(),
            "canonical Paint must require its explicit type tag"
        );
    }

    #[test]
    fn paint_interpolation_preserves_solid_color_and_steps_structural_variants() {
        let start = PropertyValue::Paint(Paint::Solid(color(0.0)));
        let end = PropertyValue::Paint(Paint::Solid(color(1.0)));
        let PropertyValue::Paint(Paint::Solid(middle)) =
            PropertyValue::interpolate(&start, &end, 0.5)
        else {
            panic!("Solid Paint interpolation changed type");
        };
        assert_eq!(middle.rgba(), [0.5, 0.25, 0.5, 1.0]);

        let gradient = PropertyValue::Paint(Paint::Gradient(GradientValue::default()));
        let pattern = PropertyValue::Paint(Paint::Pattern(PatternValue::default()));
        assert_eq!(
            PropertyValue::interpolate(&gradient, &pattern, 0.5),
            gradient
        );
        assert_eq!(
            PropertyValue::interpolate(&gradient, &pattern, 1.0),
            pattern
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
