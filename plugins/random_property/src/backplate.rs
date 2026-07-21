//! Dual-protocol Backplate fixture over one descriptor property contract.

use ruvie_plugin_api::{
    BackplateFitV2, BackplateOffsetV2, BackplateShapeV1, ColorV1, ComponentDescriptorV1,
    DecoratorEvaluateRequestV1, DecoratorEvaluateRequestV2, DecoratorOutputV1, DecoratorOutputV2,
    DecoratorTargetV1, DecoratorTargetV2, InsetsV1, InsetsV2, PropertyDefinitionV1, PropertyUiV1,
    PropertyValueV1, RuvieCallResult, DECORATOR_CATEGORY, DECORATOR_EVALUATE_V1,
    DECORATOR_EVALUATE_V2,
};

use super::{
    dropdown_property, finite_f32, has_exact_properties, invalid_request, property_string,
    valid_config_metadata, BACKPLATE_COMPONENT_ID,
};

pub(super) fn descriptor() -> ComponentDescriptorV1 {
    ComponentDescriptorV1 {
        id: BACKPLATE_COMPONENT_ID.to_string(),
        name: "Runtime Backplate".to_string(),
        category: DECORATOR_CATEGORY.to_string(),
        group: "Decorator".to_string(),
        version: "0.1.0".to_string(),
        operations: vec![
            DECORATOR_EVALUATE_V1.to_string(),
            DECORATOR_EVALUATE_V2.to_string(),
        ],
        properties: vec![
            dropdown_property("target", "Target", &["Block", "Line", "Char"], "Block"),
            PropertyDefinitionV1 {
                name: "padding".to_string(),
                label: "Padding".to_string(),
                ui: PropertyUiV1::Vec4 {
                    min: -1_000_000.0,
                    max: 1_000_000.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                default: serde_json::json!({"x": 4.0, "y": 6.0, "z": 4.0, "w": 6.0}),
            },
            PropertyDefinitionV1 {
                name: "offset".to_string(),
                label: "Offset".to_string(),
                ui: PropertyUiV1::Vec2 {
                    min: -1_000_000.0,
                    max: 1_000_000.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                default: serde_json::json!({"x": 2.0, "y": -1.0}),
            },
            dropdown_property("fit", "Fit", &["Stretch", "Contain", "Cover"], "Contain"),
        ],
        output_default: None,
    }
}

pub(super) fn evaluate_v1(payload: serde_json::Value) -> RuvieCallResult {
    let payload: DecoratorEvaluateRequestV1 = match serde_json::from_value(payload) {
        Ok(payload) => payload,
        Err(error) => return invalid_request(error),
    };
    let resolved = match resolve(payload.time, payload.fps, &payload.properties) {
        Ok(resolved) => resolved,
        Err(error) => return invalid_request(error),
    };
    let output = v1_output(resolved);
    if !valid_v1_geometry(output.padding(), 3.0) {
        return invalid_request("Backplate v1 renderer-derived geometry is unsafe");
    }
    RuvieCallResult::ok_json(&output)
}

pub(super) fn evaluate_v2(payload: serde_json::Value) -> RuvieCallResult {
    let payload: DecoratorEvaluateRequestV2 = match serde_json::from_value(payload) {
        Ok(payload) => payload,
        Err(error) => return invalid_request(error),
    };
    let resolved = match resolve(payload.time, payload.fps, &payload.properties) {
        Ok(resolved) => resolved,
        Err(error) => return invalid_request(error),
    };
    RuvieCallResult::ok_json(&v2_output(resolved))
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ResolvedBackplate {
    target: DecoratorTargetV2,
    padding: InsetsV2,
    offset: BackplateOffsetV2,
    fit: BackplateFitV2,
}

fn resolve(
    time: f64,
    fps: f64,
    properties: &std::collections::BTreeMap<String, PropertyValueV1>,
) -> Result<ResolvedBackplate, &'static str> {
    let expected = ["target", "padding", "offset", "fit"];
    if !valid_config_metadata(time, fps) || !has_exact_properties(properties, &expected) {
        return Err("Backplate request does not match its descriptor");
    }
    let target = match property_string(properties, "target") {
        Some("Block") => DecoratorTargetV2::Block,
        Some("Line") => DecoratorTargetV2::Line,
        Some("Char") => DecoratorTargetV2::Char,
        _ => return Err("Backplate target is invalid"),
    };
    let padding = match properties.get("padding") {
        Some(PropertyValueV1::Vec4 { x, y, z, w }) => match (
            finite_f32(*x),
            finite_f32(*y),
            finite_f32(*z),
            finite_f32(*w),
        ) {
            (Some(top), Some(right), Some(bottom), Some(left)) => InsetsV2 {
                top,
                right,
                bottom,
                left,
            },
            _ => return Err("Backplate padding is outside the f32 contract"),
        },
        _ => return Err("Backplate padding is invalid"),
    };
    let offset = match properties.get("offset") {
        Some(PropertyValueV1::Vec2 { x, y }) => match (finite_f32(*x), finite_f32(*y)) {
            (Some(x), Some(y)) => BackplateOffsetV2 { x, y },
            _ => return Err("Backplate offset is outside the f32 contract"),
        },
        _ => return Err("Backplate offset is invalid"),
    };
    let fit = match property_string(properties, "fit") {
        Some("Stretch") => BackplateFitV2::Stretch,
        Some("Contain") => BackplateFitV2::Contain,
        Some("Cover") => BackplateFitV2::Cover,
        _ => return Err("Backplate fit is invalid"),
    };
    Ok(ResolvedBackplate {
        target,
        padding,
        offset,
        fit,
    })
}

fn v2_output(resolved: ResolvedBackplate) -> DecoratorOutputV2 {
    DecoratorOutputV2::Backplate {
        target: resolved.target,
        padding: resolved.padding,
        offset: resolved.offset,
        fit: resolved.fit,
    }
}

fn v1_output(resolved: ResolvedBackplate) -> DecoratorOutputV1 {
    let target = match resolved.target {
        DecoratorTargetV2::Block => DecoratorTargetV1::Block,
        DecoratorTargetV2::Line => DecoratorTargetV1::Line,
        DecoratorTargetV2::Char => DecoratorTargetV1::Char,
    };
    let InsetsV2 {
        top,
        right,
        bottom,
        left,
    } = resolved.padding;
    // v1 has no offset, fit, or background-Shape fields. The compatibility
    // fallback deliberately retains only target/padding and substitutes the
    // fixture's historical fixed appearance.
    let _discarded_v2_geometry = (resolved.offset, resolved.fit);
    DecoratorOutputV1::Backplate {
        target,
        shape: BackplateShapeV1::RoundedRect,
        color: ColorV1 {
            r: 0,
            g: 0,
            b: 0,
            a: 192,
        },
        padding: InsetsV1 {
            top,
            right,
            bottom,
            left,
        },
        corner_radius: 3.0,
    }
}

trait V1OutputPadding {
    fn padding(&self) -> InsetsV1;
}

impl V1OutputPadding for DecoratorOutputV1 {
    fn padding(&self) -> InsetsV1 {
        match self {
            DecoratorOutputV1::Backplate { padding, .. } => *padding,
            DecoratorOutputV1::NoOutput => InsetsV1 {
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
                left: 0.0,
            },
        }
    }
}

fn valid_v1_geometry(padding: InsetsV1, corner_radius: f32) -> bool {
    let InsetsV1 {
        top,
        right,
        bottom,
        left,
    } = padding;
    let padded_left = -1.0_f32 - left;
    let padded_top = -2.0_f32 - top;
    let padded_right = 3.0_f32 + right;
    let padded_bottom = 4.0_f32 + bottom;
    [
        left + right,
        top + bottom,
        padded_left,
        padded_top,
        padded_right,
        padded_bottom,
        padded_right - padded_left,
        padded_bottom - padded_top,
        corner_radius * 2.0,
    ]
    .into_iter()
    .all(f32::is_finite)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved() -> ResolvedBackplate {
        resolve(
            0.25,
            30.0,
            &std::collections::BTreeMap::from([
                (
                    "target".to_string(),
                    PropertyValueV1::String {
                        value: "Line".to_string(),
                    },
                ),
                (
                    "padding".to_string(),
                    PropertyValueV1::Vec4 {
                        x: 1.0,
                        y: 2.0,
                        z: 3.0,
                        w: 4.0,
                    },
                ),
                (
                    "offset".to_string(),
                    PropertyValueV1::Vec2 { x: 5.0, y: -6.0 },
                ),
                (
                    "fit".to_string(),
                    PropertyValueV1::String {
                        value: "Cover".to_string(),
                    },
                ),
            ]),
        )
        .expect("descriptor defaults resolve for both protocols")
    }

    #[test]
    fn descriptor_advertises_ordered_dual_protocol_with_v2_properties() {
        let descriptor = descriptor();
        assert_eq!(
            descriptor.operations,
            [DECORATOR_EVALUATE_V1, DECORATOR_EVALUATE_V2]
        );
        assert_eq!(
            descriptor
                .properties
                .iter()
                .map(|property| property.name.as_str())
                .collect::<Vec<_>>(),
            ["target", "padding", "offset", "fit"]
        );
        let PropertyUiV1::Dropdown { options } = &descriptor.properties[0].ui else {
            panic!("Backplate target must be a dropdown")
        };
        assert_eq!(options, &["Block", "Line", "Char"]);
    }

    #[test]
    fn dual_outputs_share_resolution_and_v1_fallback_is_intentionally_lossy() {
        let resolved = resolved();
        assert_eq!(
            v2_output(resolved),
            DecoratorOutputV2::Backplate {
                target: DecoratorTargetV2::Line,
                padding: InsetsV2 {
                    top: 1.0,
                    right: 2.0,
                    bottom: 3.0,
                    left: 4.0,
                },
                offset: BackplateOffsetV2 { x: 5.0, y: -6.0 },
                fit: BackplateFitV2::Cover,
            }
        );
        assert_eq!(
            v1_output(resolved),
            DecoratorOutputV1::Backplate {
                target: DecoratorTargetV1::Line,
                shape: BackplateShapeV1::RoundedRect,
                color: ColorV1 {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 192,
                },
                padding: InsetsV1 {
                    top: 1.0,
                    right: 2.0,
                    bottom: 3.0,
                    left: 4.0,
                },
                corner_radius: 3.0,
            }
        );
    }

    #[test]
    fn v1_fallback_rejects_renderer_derived_overflow() {
        assert!(!valid_v1_geometry(
            InsetsV1 {
                top: 0.0,
                right: f32::MAX,
                bottom: 0.0,
                left: f32::MAX,
            },
            3.0,
        ));
        assert!(valid_v1_geometry(
            InsetsV1 {
                top: 1.0,
                right: 2.0,
                bottom: 3.0,
                left: 4.0,
            },
            3.0,
        ));
    }
}
