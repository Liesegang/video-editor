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
    OutsideStraightSrgba8Range { component: &'static str },
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
            Self::OutsideStraightSrgba8Range { component } => write!(
                formatter,
                "color component {component} is outside the legacy straight sRGBA8 range"
            ),
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

    /// Interpolate two authored colors without leaving their declared color
    /// space. Color-space conversion is deliberately not guessed here: a
    /// mixed-space keyframe pair has no well-defined interpolation until the
    /// color-management service supplies an explicit transform.
    pub fn interpolate_same_space(&self, other: &Self, amount: f64) -> Option<Self> {
        if self.color_space != other.color_space || !amount.is_finite() {
            return None;
        }
        let start = self.rgba();
        let end = other.rgba();
        let mut rgba =
            std::array::from_fn(|index| start[index] + (end[index] - start[index]) * amount);
        // Easing functions may overshoot. RGB remains an HDR-capable float,
        // while straight alpha's model invariant stays bounded and continuous.
        rgba[3] = rgba[3].clamp(0.0, 1.0);
        Self::new(self.color_space.clone(), rgba).ok()
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

    /// Cross the current u8 renderer boundary with an explicit round-to-nearest
    /// conversion. The authoritative graph value remains unchanged. Values
    /// that require a color-space transform or HDR/range clipping are rejected
    /// rather than silently clamped or replaced.
    pub fn try_to_renderer_srgba8(&self) -> Result<Color, ColorValueError> {
        if self.color_space.as_str() != SRGB_COLOR_SPACE {
            return Err(ColorValueError::NotStraightSrgba8ColorSpace);
        }
        let [r, g, b, a] = self.rgba;
        Ok(Color {
            r: renderer_srgba8_component(r, "r")?,
            g: renderer_srgba8_component(g, "g")?,
            b: renderer_srgba8_component(b, "b")?,
            a: renderer_srgba8_component(a, "a")?,
        })
    }
}

fn renderer_srgba8_component(
    value: OrderedFloat<f64>,
    component: &'static str,
) -> Result<u8, ColorValueError> {
    let value = value.into_inner();
    if !(0.0..=1.0).contains(&value) {
        return Err(ColorValueError::OutsideStraightSrgba8Range { component });
    }
    Ok((value * 255.0).round() as u8)
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
    // remain Maps. A valid exact envelope becomes ColorValue; a malformed one
    // is retained as an authored Map so it can survive loading and be repaired.
    object.len() == 3
        && object.contains_key("space")
        && object.contains_key("rgba")
        && has_color_value_tag_json(value)
}

pub(super) fn has_color_value_tag_json(value: &serde_json::Value) -> bool {
    value
        .as_object()
        .and_then(|object| object.get(COLOR_VALUE_TAG_FIELD))
        .and_then(serde_json::Value::as_str)
        == Some(COLOR_VALUE_TAG)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_boundary_rounds_ordinary_srgb_but_rejects_color_management_loss()
    -> Result<(), Box<dyn std::error::Error>> {
        let ordinary = ColorValue::new(ColorSpaceRef::srgb(), [0.5, 0.25, 1.0, 0.5])?;
        assert_eq!(
            ordinary.try_to_renderer_srgba8(),
            Ok(Color {
                r: 128,
                g: 64,
                b: 255,
                a: 128,
            })
        );

        let hdr = ColorValue::new(ColorSpaceRef::srgb(), [-0.1, 2.0, 0.0, 1.0])?;
        assert!(matches!(
            hdr.try_to_renderer_srgba8(),
            Err(ColorValueError::OutsideStraightSrgba8Range { .. })
        ));
        let other_space = ColorValue::new(
            ColorSpaceRef::new("scene_linear_ap1")?,
            [0.5, 0.25, 1.0, 0.5],
        )?;
        assert_eq!(
            other_space.try_to_renderer_srgba8(),
            Err(ColorValueError::NotStraightSrgba8ColorSpace)
        );
        Ok(())
    }

    #[test]
    fn same_space_interpolation_keeps_float_graph_values() -> Result<(), Box<dyn std::error::Error>>
    {
        let start = ColorValue::new(ColorSpaceRef::srgb(), [-1.0, 0.0, 1.0, 0.0])?;
        let end = ColorValue::new(ColorSpaceRef::srgb(), [3.0, 2.0, 1.0, 1.0])?;
        let middle = start
            .interpolate_same_space(&end, 0.25)
            .ok_or("same-space interpolation failed")?;
        assert_eq!(middle.rgba(), [0.0, 0.5, 1.0, 0.25]);
        Ok(())
    }
}
