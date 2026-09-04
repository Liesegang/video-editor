//! Fill and stroke style descriptors, evaluation, and renderer safety checks.

use ruvie_plugin_api::{
    ComponentDescriptorV1, PropertyDefinitionV1, PropertyUiV1, RuvieCallResult, StrokeCapV1,
    StrokeJoinV1, StyleEvaluateRequestV1, StyleOutputV1, MAX_STYLE_DASH_INTERVALS_V1,
    STYLE_CATEGORY, STYLE_EVALUATE_V1,
};

use crate::component_request::{
    finite_f32, has_exact_properties, invalid_request, property_color, property_number,
    property_string, valid_config_metadata,
};
use crate::descriptors::{color_property, dropdown_property, float_property, FloatPropertySpec};

pub(super) const FILL_COMPONENT_ID: &str = "runtime_fill_style";
pub(super) const STROKE_COMPONENT_ID: &str = "runtime_stroke_style";

pub(super) fn fill_descriptor() -> ComponentDescriptorV1 {
    ComponentDescriptorV1 {
        id: FILL_COMPONENT_ID.to_string(),
        name: "Runtime Fill".to_string(),
        category: STYLE_CATEGORY.to_string(),
        group: "Style".to_string(),
        version: "0.1.0".to_string(),
        operations: vec![STYLE_EVALUATE_V1.to_string()],
        properties: vec![
            color_property(255, 128, 32, 255),
            float_property(FloatPropertySpec {
                name: "offset",
                label: "Offset",
                min: -100.0,
                max: 100.0,
                step: 1.0,
                suffix: "px",
                min_hard_limit: false,
                max_hard_limit: false,
                default: 2.0,
            }),
        ],
        output_default: None,
    }
}

pub(super) fn stroke_descriptor() -> ComponentDescriptorV1 {
    ComponentDescriptorV1 {
        id: STROKE_COMPONENT_ID.to_string(),
        name: "Runtime Stroke".to_string(),
        category: STYLE_CATEGORY.to_string(),
        group: "Style".to_string(),
        version: "0.1.0".to_string(),
        operations: vec![STYLE_EVALUATE_V1.to_string()],
        properties: vec![
            color_property(32, 128, 255, 255),
            float_property(FloatPropertySpec {
                name: "width",
                label: "Width",
                min: 0.0,
                max: 100.0,
                step: 0.5,
                suffix: "px",
                min_hard_limit: true,
                max_hard_limit: false,
                default: 3.0,
            }),
            float_property(FloatPropertySpec {
                name: "offset",
                label: "Offset",
                min: -100.0,
                max: 100.0,
                step: 1.0,
                suffix: "px",
                min_hard_limit: false,
                max_hard_limit: false,
                default: 0.0,
            }),
            dropdown_property("cap", "Cap", &["Round", "Square", "Butt"], "Round"),
            dropdown_property("join", "Join", &["Round", "Bevel", "Miter"], "Miter"),
            float_property(FloatPropertySpec {
                name: "miter",
                label: "Miter",
                min: 0.0,
                max: 100.0,
                step: 0.5,
                suffix: "",
                min_hard_limit: true,
                max_hard_limit: false,
                default: 4.0,
            }),
            PropertyDefinitionV1 {
                name: "dash_array".to_string(),
                label: "Dash Array".to_string(),
                ui: PropertyUiV1::Text,
                default: serde_json::json!("3 2"),
            },
            float_property(FloatPropertySpec {
                name: "dash_offset",
                label: "Dash Offset",
                min: -1_000.0,
                max: 1_000.0,
                step: 1.0,
                suffix: "px",
                min_hard_limit: false,
                max_hard_limit: false,
                default: 1.0,
            }),
        ],
        output_default: None,
    }
}

pub(super) fn evaluate_fill(payload: serde_json::Value) -> RuvieCallResult {
    let payload: StyleEvaluateRequestV1 = match serde_json::from_value(payload) {
        Ok(payload) => payload,
        Err(error) => return invalid_request(error),
    };
    if !valid_config_metadata(payload.time, payload.fps)
        || !has_exact_properties(&payload.properties, &["color", "offset"])
    {
        return invalid_request("Fill request does not match its descriptor");
    }
    let Some(color) = property_color(&payload.properties, "color") else {
        return invalid_request("Fill color is invalid");
    };
    let Some(offset) = property_number(&payload.properties, "offset") else {
        return invalid_request("Fill offset is invalid");
    };
    if finite_f32(offset).is_none() || finite_f32(offset * 2.0).is_none() {
        return invalid_request("Fill offset is outside the renderer f32 contract");
    }
    RuvieCallResult::ok_json(&StyleOutputV1::Fill { color, offset })
}

pub(super) fn evaluate_stroke(payload: serde_json::Value) -> RuvieCallResult {
    let payload: StyleEvaluateRequestV1 = match serde_json::from_value(payload) {
        Ok(payload) => payload,
        Err(error) => return invalid_request(error),
    };
    let expected = [
        "color",
        "width",
        "offset",
        "cap",
        "join",
        "miter",
        "dash_array",
        "dash_offset",
    ];
    if !valid_config_metadata(payload.time, payload.fps)
        || !has_exact_properties(&payload.properties, &expected)
    {
        return invalid_request("Stroke request does not match its descriptor");
    }
    let Some(color) = property_color(&payload.properties, "color") else {
        return invalid_request("Stroke color is invalid");
    };
    let Some(width) = property_number(&payload.properties, "width") else {
        return invalid_request("Stroke width is invalid");
    };
    if width < 0.0 {
        return invalid_request("Stroke width must be non-negative");
    }
    let Some(offset) = property_number(&payload.properties, "offset") else {
        return invalid_request("Stroke offset is invalid");
    };
    if !valid_stroke_geometry(width, offset) {
        return invalid_request("Stroke renderer-derived widths are unsafe");
    }
    let cap = match property_string(&payload.properties, "cap") {
        Some("Round") => StrokeCapV1::Round,
        Some("Square") => StrokeCapV1::Square,
        Some("Butt") => StrokeCapV1::Butt,
        _ => return invalid_request("Stroke cap is invalid"),
    };
    let join = match property_string(&payload.properties, "join") {
        Some("Round") => StrokeJoinV1::Round,
        Some("Bevel") => StrokeJoinV1::Bevel,
        Some("Miter") => StrokeJoinV1::Miter,
        _ => return invalid_request("Stroke join is invalid"),
    };
    let Some(miter) = property_number(&payload.properties, "miter") else {
        return invalid_request("Stroke miter is invalid");
    };
    if miter < 0.0 || finite_f32(miter).is_none() {
        return invalid_request("Stroke miter must be a non-negative f32");
    }
    let Some(dash_array) = property_string(&payload.properties, "dash_array").and_then(|value| {
        value
            .split_whitespace()
            .map(str::parse::<f64>)
            .collect::<Result<Vec<_>, _>>()
            .ok()
    }) else {
        return invalid_request("Stroke dash array is invalid");
    };
    if !valid_dash_array(&dash_array) {
        return invalid_request("Stroke dash intervals violate the ABI work or period limits");
    }
    let Some(dash_offset) = property_number(&payload.properties, "dash_offset") else {
        return invalid_request("Stroke dash offset is invalid");
    };
    if finite_f32(dash_offset).is_none() {
        return invalid_request("Stroke dash offset is outside the f32 contract");
    }
    RuvieCallResult::ok_json(&StyleOutputV1::Stroke {
        color,
        width,
        offset,
        cap,
        join,
        miter,
        dash_array,
        dash_offset,
    })
}

fn valid_stroke_geometry(width: f64, offset: f64) -> bool {
    let finite_scalar = |value: f64| value.is_finite() && (value as f32).is_finite();
    if width < 0.0 || !finite_scalar(width) || !finite_scalar(offset) {
        return false;
    }
    if !finite_scalar((width + offset * 2.0).max(0.0)) {
        return false;
    }
    if width <= 0.0 || offset == 0.0 {
        return true;
    }
    let half_width = width / 2.0;
    let outer_radius = offset.abs() + half_width;
    let inner_radius = offset.abs() - half_width;
    finite_scalar(outer_radius * 2.0) && (inner_radius <= 0.0 || finite_scalar(inner_radius * 2.0))
}

fn valid_dash_array(values: &[f64]) -> bool {
    if values.is_empty() {
        return true;
    }
    if values.len() > MAX_STYLE_DASH_INTERVALS_V1 || !values.len().is_multiple_of(2) {
        return false;
    }
    let mut period = 0.0_f32;
    values.iter().all(|value| {
        let interval = *value as f32;
        if !value.is_finite() || !interval.is_finite() || interval <= 0.0 {
            return false;
        }
        period += interval;
        period.is_finite()
    }) && period > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn float_ui<'a>(component: &'a ComponentDescriptorV1, name: &str) -> &'a PropertyUiV1 {
        &component
            .properties
            .iter()
            .find(|property| property.name == name)
            .expect("test property is declared")
            .ui
    }

    #[test]
    fn config_descriptor_metadata_matches_runtime_safety_contracts() {
        let stroke = stroke_descriptor();
        for name in ["width", "miter"] {
            assert!(matches!(
                float_ui(&stroke, name),
                PropertyUiV1::Float {
                    min: 0.0,
                    min_hard_limit: true,
                    ..
                }
            ));
        }
        for name in ["offset", "dash_offset"] {
            assert!(matches!(
                float_ui(&stroke, name),
                PropertyUiV1::Float {
                    min_hard_limit: false,
                    ..
                }
            ));
        }
    }

    #[test]
    fn stroke_fixture_honors_the_abi_dash_work_and_period_limits() {
        assert!(valid_dash_array(&[]));
        assert!(valid_dash_array(&vec![1.0; MAX_STYLE_DASH_INTERVALS_V1]));
        assert!(!valid_dash_array(&[f32::MAX as f64, f32::MAX as f64]));
        assert!(!valid_dash_array(&vec![
            1.0;
            MAX_STYLE_DASH_INTERVALS_V1 + 2
        ]));
    }

    #[test]
    fn stroke_fixture_rejects_renderer_derived_overflow() {
        assert!(!valid_stroke_geometry(1.0, -(f32::MAX as f64)));
        assert!(valid_stroke_geometry(1.0, -(f32::MAX as f64) / 4.0));
    }
}
