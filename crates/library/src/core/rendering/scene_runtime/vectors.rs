//! Checked vector transport shared by Point sources, forces, and collisions.

use crate::error::LibraryError;
use crate::model::property::Vec3;

pub(super) fn vec3_f32(value: Vec3, label: &str) -> Result<[f32; 3], LibraryError> {
    let converted = [
        value.x.into_inner() as f32,
        value.y.into_inner() as f32,
        value.z.into_inner() as f32,
    ];
    converted
        .iter()
        .all(|component| component.is_finite())
        .then_some(converted)
        .ok_or_else(|| {
            LibraryError::Validation(format!("GPU Point {label} must fit finite GPU floats"))
        })
}

pub(super) fn normalized_direction(value: Vec3, label: &str) -> Result<[f32; 3], LibraryError> {
    let components = [
        value.x.into_inner(),
        value.y.into_inner(),
        value.z.into_inner(),
    ];
    let scale = components
        .iter()
        .copied()
        .map(f64::abs)
        .fold(0.0_f64, f64::max);
    if !scale.is_finite() || scale == 0.0 || components.iter().any(|value| !value.is_finite()) {
        return Err(LibraryError::Validation(format!(
            "{label} must be finite and non-zero"
        )));
    }
    // Normalize before converting to f32 so small authored directions cannot
    // underflow into a zero vector at the shader boundary.
    let scaled = components.map(|value| value / scale);
    let length = scaled.iter().map(|value| value * value).sum::<f64>().sqrt();
    Ok(scaled.map(|value| (value / length) as f32))
}
