//! Descriptor-backed Color, Gradient, and procedural Pattern overlays.

use uuid::Uuid;

use crate::model::frame::color::Color;
use crate::model::frame::draw_type::DrawStyle;
use crate::model::frame::entity::StyleConfig;
use crate::model::property::{
    GradientValue, PatternValue, PropertyDefinition, PropertyUiType, PropertyValue,
};
use crate::plugin::{
    EvaluatedOperation, OperationDescriptor, OperationDescriptorError, Plugin, StylePlugin,
};

use super::layer_effects::{blend, blend_property, color, color_property, float_property, number};

pub const COLOR_OVERLAY_COMPONENT_ID: &str = "color_overlay";
pub const GRADIENT_OVERLAY_COMPONENT_ID: &str = "gradient_overlay";
pub const PATTERN_OVERLAY_COMPONENT_ID: &str = "pattern_overlay";

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
        PropertyValue::Gradient(GradientValue::default()),
    )];
    properties.extend(common_overlay_properties());
    properties
}

fn pattern_overlay_properties() -> Vec<PropertyDefinition> {
    let mut properties = vec![PropertyDefinition::new(
        "pattern",
        PropertyUiType::Pattern,
        "Pattern",
        PropertyValue::Pattern(PatternValue::default()),
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
    Some(DrawStyle::GradientOverlay {
        gradient,
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
        pattern,
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
                OperationDescriptor::image_style(self.id(), self.name(), $properties())
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
