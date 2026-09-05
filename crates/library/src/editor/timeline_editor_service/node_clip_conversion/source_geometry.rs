//! Validation and literal geometry extraction at the Clip-to-Module boundary.

use super::*;

pub(super) fn shape_path(
    shape: &ShapeSource,
) -> Result<crate::model::path::PathValue, LibraryError> {
    match shape.shape_kind {
        ShapeKind::Path => match shape.parameters.get("path") {
            Some(PropertyValue::Path(path)) => Ok(path.clone()),
            _ => Err(LibraryError::Validation(
                "Path source has no canonical Path value".to_string(),
            )),
        },
        kind => {
            let width = shape_number(shape, "width", 100.0)?;
            let height = shape_number(shape, "height", 100.0)?;
            let path =
                crate::plugin::entity_converter::primitive_shape_path_data(kind, width, height)?;
            crate::model::path::parse_legacy_svg_path_data(&path)
                .map_err(|error| LibraryError::Validation(error.to_string()))
        }
    }
}

pub(super) fn validate_shape_parameters(shape: &ShapeSource) -> Result<(), LibraryError> {
    let allowed = match shape.shape_kind {
        ShapeKind::Rectangle | ShapeKind::Ellipse => ["width", "height", "color"].as_slice(),
        ShapeKind::Path => ["path", "width", "height", "color"].as_slice(),
    };
    let mut unsupported = shape
        .parameters
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    unsupported.sort();
    if unsupported.is_empty() {
        Ok(())
    } else {
        Err(LibraryError::Validation(format!(
            "Shape source has unsupported parameters that cannot be moved atomically: {}",
            unsupported.join(", ")
        )))
    }
}

pub(super) fn shape_number(
    shape: &ShapeSource,
    key: &str,
    fallback: f64,
) -> Result<f64, LibraryError> {
    let value = match shape.parameters.get(key) {
        None => fallback,
        Some(PropertyValue::Number(value)) => value.into_inner(),
        Some(PropertyValue::Integer(value)) => *value as f64,
        Some(_) => {
            return Err(LibraryError::Validation(format!(
                "Shape source parameter '{key}' is not numeric"
            )));
        }
    };
    if !value.is_finite() || value <= 0.0 {
        return Err(LibraryError::Validation(format!(
            "Shape source parameter '{key}' must be positive and finite"
        )));
    }
    Ok(value)
}

pub(super) fn positive_shape_extent(
    shape: &ShapeSource,
    key: &str,
    fallback: f64,
) -> Result<u64, LibraryError> {
    let value = shape_number(shape, key, fallback)?.ceil();
    if value > u64::MAX as f64 {
        return Err(LibraryError::Validation(format!(
            "Shape source parameter '{key}' is too large"
        )));
    }
    Ok(value as u64)
}
