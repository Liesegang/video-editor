use std::fmt;
use std::hash::Hash;

use ordered_float::OrderedFloat;
use serde::de::Error as _;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::model::frame::color::Color;

const COLOR_VALUE_TAG_FIELD: &str = "$type";
const COLOR_VALUE_TAG: &str = "color_value";
const SRGB_COLOR_SPACE: &str = "srgb";
const COMPONENT_NAMES: [&str; 4] = ["r", "g", "b", "a"];

/// Stable reference to the color space in which a graph color is encoded.
///
/// Resolution of the identifier belongs to the color-management service. The
/// Project model only guarantees that the authored reference is nonempty.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ColorSpaceRef(String);

impl ColorSpaceRef {
    pub fn new(value: impl Into<String>) -> Result<Self, ColorValueError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ColorValueError::EmptyColorSpace);
        }
        Ok(Self(value))
    }

    /// The canonical encoded sRGB space used by the legacy straight RGBA8
    /// boundary. This is not linear-light sRGB.
    pub fn srgb() -> Self {
        Self(SRGB_COLOR_SPACE.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ColorSpaceRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<'de> Deserialize<'de> for ColorSpaceRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Validation failure for an authored graph color or an exact legacy adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColorValueError {
    EmptyColorSpace,
    NonFiniteComponent { component: &'static str },
    AlphaOutOfRange,
    NotStraightSrgba8ColorSpace,
    NotExactlyRepresentableAsStraightSrgba8 { component: &'static str },
}

impl fmt::Display for ColorValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyColorSpace => formatter.write_str("color space must not be empty"),
            Self::NonFiniteComponent { component } => {
                write!(formatter, "color component {component} must be finite")
            }
            Self::AlphaOutOfRange => {
                formatter.write_str("straight alpha must be between 0 and 1 inclusive")
            }
            Self::NotStraightSrgba8ColorSpace => {
                formatter.write_str("color space is not the legacy straight sRGBA8 space")
            }
            Self::NotExactlyRepresentableAsStraightSrgba8 { component } => write!(
                formatter,
                "color component {component} is not exactly representable as straight sRGBA8"
            ),
        }
    }
}

impl std::error::Error for ColorValueError {}

/// Project-authoritative straight-alpha floating-point graph color.
///
/// RGB values may be negative or greater than one so scene-linear and HDR
/// values are not clipped by the model. All components must be finite and
/// alpha is always constrained to `[0, 1]`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ColorValue {
    color_space: ColorSpaceRef,
    rgba: [OrderedFloat<f64>; 4],
}

impl ColorValue {
    pub fn new(color_space: ColorSpaceRef, rgba: [f64; 4]) -> Result<Self, ColorValueError> {
        let rgba = rgba.map(OrderedFloat);
        Self::from_ordered(color_space, rgba)
    }

    fn from_ordered(
        color_space: ColorSpaceRef,
        rgba: [OrderedFloat<f64>; 4],
    ) -> Result<Self, ColorValueError> {
        for (component, value) in COMPONENT_NAMES.into_iter().zip(rgba) {
            if !value.is_finite() {
                return Err(ColorValueError::NonFiniteComponent { component });
            }
        }
        if !(0.0..=1.0).contains(&rgba[3].into_inner()) {
            return Err(ColorValueError::AlphaOutOfRange);
        }
        Ok(Self { color_space, rgba })
    }

    pub fn color_space(&self) -> &ColorSpaceRef {
        &self.color_space
    }

    pub fn rgba(&self) -> [f64; 4] {
        self.rgba.map(OrderedFloat::into_inner)
    }

    /// Losslessly embeds a legacy, encoded straight sRGBA8 value. RGB is not
    /// multiplied by alpha and no color-space transform is performed.
    pub fn from_straight_srgba8(color: &Color) -> Self {
        let normalize = |component| OrderedFloat(f64::from(component) / 255.0);
        Self {
            color_space: ColorSpaceRef::srgb(),
            rgba: [
                normalize(color.r),
                normalize(color.g),
                normalize(color.b),
                normalize(color.a),
            ],
        }
    }

    /// Converts to the legacy boundary only when no color transform,
    /// quantization, clipping, or alpha premultiplication would be required.
    pub fn try_to_straight_srgba8(&self) -> Result<Color, ColorValueError> {
        if self.color_space.as_str() != SRGB_COLOR_SPACE {
            return Err(ColorValueError::NotStraightSrgba8ColorSpace);
        }
        let [r, g, b, a] = self.rgba;
        Ok(Color {
            r: exact_srgba8_component(r, "r")?,
            g: exact_srgba8_component(g, "g")?,
            b: exact_srgba8_component(b, "b")?,
            a: exact_srgba8_component(a, "a")?,
        })
    }
}

fn exact_srgba8_component(
    value: OrderedFloat<f64>,
    component: &'static str,
) -> Result<u8, ColorValueError> {
    let scaled = value.into_inner() * 255.0;
    if !(0.0..=255.0).contains(&scaled) || scaled.fract() != 0.0 {
        return Err(ColorValueError::NotExactlyRepresentableAsStraightSrgba8 { component });
    }
    Ok(scaled as u8)
}

impl Serialize for ColorValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("ColorValue", 3)?;
        state.serialize_field(COLOR_VALUE_TAG_FIELD, COLOR_VALUE_TAG)?;
        state.serialize_field("space", &self.color_space)?;
        state.serialize_field("rgba", &self.rgba)?;
        state.end()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColorValueWire {
    #[serde(rename = "$type")]
    value_type: String,
    space: ColorSpaceRef,
    rgba: [OrderedFloat<f64>; 4],
}

impl<'de> Deserialize<'de> for ColorValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ColorValueWire::deserialize(deserializer)?;
        if wire.value_type != COLOR_VALUE_TAG {
            return Err(D::Error::custom(format_args!(
                "unsupported tagged color value {:?}",
                wire.value_type
            )));
        }
        Self::from_ordered(wire.space, wire.rgba).map_err(D::Error::custom)
    }
}

pub(super) fn is_tagged_color_value_json(value: &serde_json::Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    // Reserve only the complete wire envelope. A user's ordinary Map may use
    // the same `$type` string with a partial or extended shape; those values
    // remain Maps. Once the exact envelope is present, malformed color data is
    // rejected rather than silently falling through the untagged Map variant.
    object.len() == 3
        && object.contains_key("space")
        && object.contains_key("rgba")
        && object
            .get(COLOR_VALUE_TAG_FIELD)
            .and_then(serde_json::Value::as_str)
            == Some(COLOR_VALUE_TAG)
}
