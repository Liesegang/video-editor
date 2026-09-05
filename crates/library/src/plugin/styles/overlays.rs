//! Descriptor-backed Color, Gradient, and procedural Pattern overlays.

use ordered_float::OrderedFloat;
use uuid::Uuid;

use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{DrawStyle, GradientStyle, GradientStyleStop, PatternStyle};
use crate::model::frame::entity::StyleConfig;
use crate::model::property::{
    ColorValue, GradientGeometry, GradientSpread, GradientStop, GradientValue, PatternKind,
    PatternValue, PropertyDefinition, PropertyUiType, PropertyValue, Vec2,
};
use crate::plugin::{
    EvaluatedOperation, OperationDescriptor, OperationDescriptorError, Plugin, StylePlugin,
};

use super::layer_effects::{blend, blend_property, color, color_property, float_property, number};

pub const COLOR_OVERLAY_COMPONENT_ID: &str = "color_overlay";
pub const GRADIENT_OVERLAY_COMPONENT_ID: &str = "gradient_overlay";
pub const PATTERN_OVERLAY_COMPONENT_ID: &str = "pattern_overlay";

fn point(x: f64, y: f64) -> Vec2 {
    Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    }
}

#[expect(
    clippy::expect_used,
    reason = "bundled literal Gradient defaults are checked here and by descriptor tests"
)]
fn default_gradient() -> GradientValue {
    GradientValue::new(
        GradientGeometry::Linear {
            start: point(0.0, 0.5),
            end: point(1.0, 0.5),
        },
        GradientSpread::Pad,
        vec![
            GradientStop::new(
                0.0,
                ColorValue::from_straight_srgba8(&Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                }),
            )
            .expect("valid bundled Gradient stop"),
            GradientStop::new(1.0, ColorValue::from_straight_srgba8(&Color::white()))
                .expect("valid bundled Gradient stop"),
        ],
    )
    .expect("valid bundled Gradient")
}

#[expect(
    clippy::expect_used,
    reason = "bundled literal Pattern defaults are checked here and by descriptor tests"
)]
fn default_pattern() -> PatternValue {
    PatternValue::new(
        PatternKind::Checker,
        ColorValue::from_straight_srgba8(&Color::white()),
        ColorValue::from_straight_srgba8(&Color {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        }),
        point(32.0, 32.0),
        point(0.0, 0.0),
        0.0,
        0.5,
    )
    .expect("valid bundled Pattern")
}

fn common_overlay_properties() -> Vec<PropertyDefinition> {
    vec![
        float_property("opacity", "Opacity", 1.0, 0.0, 1.0, "", true),
        blend_property("Normal"),
    ]
}

fn color_overlay_properties() -> Vec<PropertyDefinition> {
    let mut properties = vec![color_property("color", "Color", Color::white())];
    properties.extend(common_overlay_properties());
    properties
}

fn gradient_overlay_properties() -> Vec<PropertyDefinition> {
    let mut properties = vec![PropertyDefinition::new(
        "gradient",
        PropertyUiType::Gradient,
        "Gradient",
        PropertyValue::Gradient(default_gradient()),
    )];
    properties.extend(common_overlay_properties());
    properties
}

fn pattern_overlay_properties() -> Vec<PropertyDefinition> {
    let mut properties = vec![PropertyDefinition::new(
        "pattern",
        PropertyUiType::Pattern,
        "Pattern",
        PropertyValue::Pattern(default_pattern()),
    )];
    properties.extend(common_overlay_properties());
    properties
}

fn evaluate_color_overlay(context: &EvaluatedOperation<'_>) -> Option<DrawStyle> {
    Some(DrawStyle::ColorOverlay {
        color: color(context, "color")?,
        opacity: number(context, "opacity")?,
        blend_mode: blend(context, "blend_mode")?,
    })
}

fn evaluate_gradient_overlay(context: &EvaluatedOperation<'_>) -> Option<DrawStyle> {
    let gradient = context
        .properties()
        .get("gradient")?
        .get_as::<GradientValue>()?;
    let stops = gradient
        .stops()
        .iter()
        .map(|stop| {
            crate::color_management::to_renderer_srgba8(stop.color())
                .ok()
                .map(|color| GradientStyleStop {
                    offset: OrderedFloat(stop.offset()),
                    color,
                })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(DrawStyle::GradientOverlay {
        gradient: GradientStyle {
            geometry: gradient.geometry(),
            spread: gradient.spread(),
            stops,
        },
        opacity: number(context, "opacity")?,
        blend_mode: blend(context, "blend_mode")?,
    })
}

fn evaluate_pattern_overlay(context: &EvaluatedOperation<'_>) -> Option<DrawStyle> {
    let pattern = context
        .properties()
        .get("pattern")?
        .get_as::<PatternValue>()?;
    Some(DrawStyle::PatternOverlay {
        pattern: PatternStyle {
            kind: pattern.kind(),
            foreground: crate::color_management::to_renderer_srgba8(pattern.foreground()).ok()?,
            background: crate::color_management::to_renderer_srgba8(pattern.background()).ok()?,
            scale: pattern.scale(),
            phase: pattern.phase(),
            angle: OrderedFloat(pattern.angle()),
            duty: OrderedFloat(pattern.duty()),
        },
        opacity: number(context, "opacity")?,
        blend_mode: blend(context, "blend_mode")?,
    })
}

macro_rules! overlay_plugin {
    ($type:ident, $id:expr, $name:expr, $properties:ident, $evaluate:ident) => {
        pub struct $type;

        impl Plugin for $type {
            fn id(&self) -> &'static str {
                $id
            }

            fn name(&self) -> String {
                $name.to_string()
            }

            fn category(&self) -> String {
                "Built-in".to_string()
            }

            fn version(&self) -> (u32, u32, u32) {
                (0, 1, 0)
            }
        }

        impl StylePlugin for $type {
            fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
                OperationDescriptor::style(self.id(), self.name(), $properties())
            }

            fn evaluate_values(
                &self,
                context: &EvaluatedOperation<'_>,
                source_id: Uuid,
            ) -> Option<StyleConfig> {
                Some(StyleConfig {
                    id: source_id,
                    style: $evaluate(context)?,
                })
            }
        }
    };
}

overlay_plugin!(
    ColorOverlayStylePlugin,
    COLOR_OVERLAY_COMPONENT_ID,
    "Color Overlay",
    color_overlay_properties,
    evaluate_color_overlay
);
overlay_plugin!(
    GradientOverlayStylePlugin,
    GRADIENT_OVERLAY_COMPONENT_ID,
    "Gradient Overlay",
    gradient_overlay_properties,
    evaluate_gradient_overlay
);
overlay_plugin!(
    PatternOverlayStylePlugin,
    PATTERN_OVERLAY_COMPONENT_ID,
    "Pattern Overlay",
    pattern_overlay_properties,
    evaluate_pattern_overlay
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_descriptors_expose_typed_paint_properties() {
        let gradient = GradientOverlayStylePlugin.descriptor().unwrap();
        assert!(matches!(
            gradient.properties()[0].default_value(),
            PropertyValue::Gradient(_)
        ));
        let pattern = PatternOverlayStylePlugin.descriptor().unwrap();
        assert!(matches!(
            pattern.properties()[0].default_value(),
            PropertyValue::Pattern(_)
        ));
    }
}
