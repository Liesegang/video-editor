//! Canonical [`ColorValue`](crate::model::property::ColorValue) adapter for the
//! Project-independent color backend.
//!
//! Keep UI and graph code on this boundary. Transfer math belongs to the
//! `ruvie-color-management` crate so Preview and export can adopt the same
//! implementation when their render surfaces become scene-linear float.

use std::fmt;

use crate::model::frame::color::Color;
use crate::model::property::{ColorSpaceRef, ColorValue, ColorValueError};
use ruvie_color_management::{
    BackendCapabilities, BuiltinColorTransform, ColorManagementError, ColorTransformBackend,
    LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID,
};

pub use ruvie_color_management::{
    AlphaRepresentation, ColorPipelineContract, ComponentStorage, TARGET_COLOR_PIPELINE,
};

const BACKEND: BuiltinColorTransform = BuiltinColorTransform;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColorTransformError {
    Backend(ColorManagementError),
    InvalidResult(ColorValueError),
}

impl fmt::Display for ColorTransformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backend(error) => error.fmt(formatter),
            Self::InvalidResult(error) => write!(formatter, "invalid transformed color: {error}"),
        }
    }
}

impl std::error::Error for ColorTransformError {}

impl From<ColorManagementError> for ColorTransformError {
    fn from(error: ColorManagementError) -> Self {
        Self::Backend(error)
    }
}

impl From<ColorValueError> for ColorTransformError {
    fn from(error: ColorValueError) -> Self {
        Self::InvalidResult(error)
    }
}

pub fn backend_capabilities() -> BackendCapabilities {
    BACKEND.capabilities()
}

pub fn available_color_spaces()
-> Result<Vec<ruvie_color_management::ColorSpaceInfo>, ColorTransformError> {
    BACKEND.available_color_spaces().map_err(Into::into)
}

/// Convert without clipping, premultiplying alpha, or changing Project shape.
pub fn transform_color(
    source: &ColorValue,
    target_space: &ColorSpaceRef,
) -> Result<ColorValue, ColorTransformError> {
    let rgba = BACKEND.transform_rgba(
        source.rgba(),
        source.color_space().as_str(),
        target_space.as_str(),
    )?;
    ColorValue::new(target_space.clone(), rgba).map_err(Into::into)
}

/// Convert a Project color into the encoded display-sRGB editing domain.
///
/// RGB remains extended range; a UI may clamp only its temporary picker draft.
pub fn to_display_srgb(source: &ColorValue) -> Result<[f64; 4], ColorTransformError> {
    transform_color(source, &ColorSpaceRef::srgb()).map(|value| value.rgba())
}

/// Convert an encoded display-sRGB picker result back into an authored space.
pub fn from_display_srgb(
    display_rgba: [f64; 4],
    authored_space: &ColorSpaceRef,
) -> Result<ColorValue, ColorTransformError> {
    let display = ColorValue::new(ColorSpaceRef::srgb(), display_rgba)?;
    transform_color(&display, authored_space)
}

/// Convert an authored built-in color to the current encoded-sRGB u8 raster
/// boundary. Gamut mapping is not implemented yet, so this terminal adapter
/// uses channel clipping only after the explicit color-space transform.
/// The authoritative [`ColorValue`] remains floating-point and unchanged.
pub fn to_renderer_srgba8(source: &ColorValue) -> Result<Color, ColorTransformError> {
    let display = transform_color(source, &ColorSpaceRef::srgb())?;
    let [r, g, b, a] = display.rgba();
    Ok(Color {
        r: quantize_terminal(r),
        g: quantize_terminal(g),
        b: quantize_terminal(b),
        a: quantize_terminal(a),
    })
}

fn quantize_terminal(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub fn to_working_linear_srgb(source: &ColorValue) -> Result<ColorValue, ColorTransformError> {
    transform_color(source, &ColorSpaceRef::linear_srgb())
}

pub fn from_working_linear_srgb(
    source: &ColorValue,
    target_space: &ColorSpaceRef,
) -> Result<ColorValue, ColorTransformError> {
    if source.color_space().as_str() != LINEAR_SRGB_SPACE_ID {
        return Err(ColorManagementError::UnsupportedTransform {
            source: source.color_space().to_string(),
            target: target_space.to_string(),
        }
        .into());
    }
    transform_color(source, target_space)
}

pub const fn encoded_srgb_space_id() -> &'static str {
    SRGB_SPACE_ID
}

pub const fn working_linear_srgb_space_id() -> &'static str {
    LINEAR_SRGB_SPACE_ID
}

#[cfg(test)]
mod tests {
    use super::{
        from_display_srgb, from_working_linear_srgb, to_display_srgb, to_renderer_srgba8,
        to_working_linear_srgb, transform_color,
    };
    use crate::model::property::{ColorSpaceRef, ColorValue};

    fn assert_near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() <= 1.0e-12);
    }

    #[test]
    fn picker_and_working_helpers_share_one_reversible_transform()
    -> Result<(), Box<dyn std::error::Error>> {
        let encoded = ColorValue::new(ColorSpaceRef::srgb(), [-0.25, 0.5, 2.0, 0.4])?;
        let working = to_working_linear_srgb(&encoded)?;
        assert_eq!(working.color_space(), &ColorSpaceRef::linear_srgb());
        let restored = from_working_linear_srgb(&working, &ColorSpaceRef::srgb())?;
        for (actual, expected) in restored.rgba().into_iter().zip(encoded.rgba()) {
            assert_near(actual, expected);
        }

        let display = to_display_srgb(&working)?;
        let picker_restored = from_display_srgb(display, &ColorSpaceRef::linear_srgb())?;
        for (actual, expected) in picker_restored.rgba().into_iter().zip(working.rgba()) {
            assert_near(actual, expected);
        }
        Ok(())
    }

    #[test]
    fn adapter_never_retags_an_unsupported_conversion() -> Result<(), Box<dyn std::error::Error>> {
        let aces = ColorValue::new(ColorSpaceRef::new("acescg")?, [0.5, 0.25, 2.0, 1.0])?;
        assert!(transform_color(&aces, &ColorSpaceRef::linear_srgb()).is_err());
        assert!(transform_color(&aces, &ColorSpaceRef::new("acescg")?).is_err());
        Ok(())
    }

    #[test]
    fn renderer_terminal_transforms_display_p3_then_clips_only_at_output()
    -> Result<(), Box<dyn std::error::Error>> {
        let p3 = ColorValue::new(ColorSpaceRef::new("display-p3")?, [0.8, 0.4, 0.2, 0.5])?;
        assert_eq!(
            to_renderer_srgba8(&p3)?,
            crate::model::frame::color::Color {
                r: 219,
                g: 94,
                b: 31,
                a: 128,
            }
        );
        Ok(())
    }
}
