//! Validation and typed access for descriptor-driven JSON requests.

use std::collections::BTreeMap;

use ruvie_plugin_api::{ColorV1, PropertyValueV1, RuvieCallResult, STATUS_INVALID_REQUEST};

pub(super) fn valid_config_metadata(time: f64, fps: f64) -> bool {
    time.is_finite() && fps.is_finite() && fps > 0.0
}

pub(super) fn finite_f32(value: f64) -> Option<f32> {
    let value = value as f32;
    value.is_finite().then_some(value)
}

pub(super) fn has_exact_properties(
    properties: &BTreeMap<String, PropertyValueV1>,
    expected: &[&str],
) -> bool {
    properties.len() == expected.len() && expected.iter().all(|name| properties.contains_key(*name))
}

pub(super) fn property_number(
    properties: &BTreeMap<String, PropertyValueV1>,
    name: &str,
) -> Option<f64> {
    match properties.get(name) {
        Some(PropertyValueV1::Number { value }) if value.is_finite() => Some(*value),
        _ => None,
    }
}

pub(super) fn property_string<'a>(
    properties: &'a BTreeMap<String, PropertyValueV1>,
    name: &str,
) -> Option<&'a str> {
    match properties.get(name) {
        Some(PropertyValueV1::String { value }) => Some(value),
        _ => None,
    }
}

pub(super) fn property_color(
    properties: &BTreeMap<String, PropertyValueV1>,
    name: &str,
) -> Option<ColorV1> {
    match properties.get(name) {
        Some(PropertyValueV1::Color { r, g, b, a }) => Some(ColorV1 {
            r: *r,
            g: *g,
            b: *b,
            a: *a,
        }),
        _ => None,
    }
}

pub(super) fn invalid_request(detail: impl std::fmt::Display) -> RuvieCallResult {
    RuvieCallResult::error(STATUS_INVALID_REQUEST, detail.to_string())
}
