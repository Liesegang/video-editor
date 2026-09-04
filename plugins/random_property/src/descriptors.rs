//! Shared constructors for descriptor property definitions.

use ruvie_plugin_api::{PropertyDefinitionV1, PropertyUiV1};

pub(super) fn color_property(r: u8, g: u8, b: u8, a: u8) -> PropertyDefinitionV1 {
    PropertyDefinitionV1 {
        name: "color".to_string(),
        label: "Color".to_string(),
        ui: PropertyUiV1::Color,
        default: serde_json::json!({"r": r, "g": g, "b": b, "a": a}),
    }
}

pub(super) struct FloatPropertySpec<'a> {
    pub(super) name: &'a str,
    pub(super) label: &'a str,
    pub(super) min: f64,
    pub(super) max: f64,
    pub(super) step: f64,
    pub(super) suffix: &'a str,
    pub(super) min_hard_limit: bool,
    pub(super) max_hard_limit: bool,
    pub(super) default: f64,
}

pub(super) fn float_property(spec: FloatPropertySpec<'_>) -> PropertyDefinitionV1 {
    let FloatPropertySpec {
        name,
        label,
        min,
        max,
        step,
        suffix,
        min_hard_limit,
        max_hard_limit,
        default,
    } = spec;
    PropertyDefinitionV1 {
        name: name.to_string(),
        label: label.to_string(),
        ui: PropertyUiV1::Float {
            min,
            max,
            step,
            suffix: suffix.to_string(),
            min_hard_limit,
            max_hard_limit,
        },
        default: serde_json::json!(default),
    }
}

pub(super) fn dropdown_property(
    name: &str,
    label: &str,
    options: &[&str],
    default: &str,
) -> PropertyDefinitionV1 {
    PropertyDefinitionV1 {
        name: name.to_string(),
        label: label.to_string(),
        ui: PropertyUiV1::Dropdown {
            options: options.iter().map(|option| (*option).to_string()).collect(),
        },
        default: serde_json::json!(default),
    }
}
