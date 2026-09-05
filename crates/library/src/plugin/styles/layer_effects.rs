//! Descriptor-backed Photoshop-style alpha-mask appearances.

use crate::model::BlendMode;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{BevelDirection, BevelStyle, BevelTechnique, DrawStyle};
use crate::model::frame::entity::StyleConfig;
use crate::model::property::{ColorValue, PropertyDefinition, PropertyUiType, PropertyValue};
use crate::plugin::{
    EvaluatedOperation, OperationDescriptor, OperationDescriptorError, Plugin, StylePlugin,
};
use uuid::Uuid;

fn color_default(color: Color) -> PropertyValue {
    PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&color))
}

pub(super) fn color_property(name: &str, label: &str, color: Color) -> PropertyDefinition {
    PropertyDefinition::new(
        name,
        PropertyUiType::ColorValue,
        label,
        color_default(color),
    )
}

pub(super) fn float_property(
    name: &str,
    label: &str,
    default: f64,
    min: f64,
    max: f64,
    suffix: &str,
    hard: bool,
) -> PropertyDefinition {
    PropertyDefinition::new(
        name,
        PropertyUiType::Float {
            min,
            max,
            step: if suffix == "°" { 1.0 } else { 0.1 },
            suffix: suffix.to_string(),
            min_hard_limit: hard,
            max_hard_limit: hard,
        },
        label,
        PropertyValue::from(default),
    )
}

fn dropdown_property(
    name: &str,
    label: &str,
    default: &str,
    options: &[&str],
) -> PropertyDefinition {
    PropertyDefinition::new(
        name,
        PropertyUiType::Dropdown {
            options: options.iter().map(|option| (*option).to_string()).collect(),
        },
        label,
        PropertyValue::String(default.to_string()),
    )
}

fn bool_property(name: &str, label: &str, default: bool) -> PropertyDefinition {
    PropertyDefinition::new(
        name,
        PropertyUiType::Bool,
        label,
        PropertyValue::Boolean(default),
    )
}

pub(super) fn blend_property(default: &str) -> PropertyDefinition {
    dropdown_property(
        "blend_mode",
        "Blend Mode",
        default,
        &["Normal", "Multiply", "Screen", "Linear Dodge (Add)"],
    )
}

fn shadow_properties(inner: bool) -> Vec<PropertyDefinition> {
    vec![
        color_property("color", "Color", Color::black()),
        float_property("opacity", "Opacity", 0.75, 0.0, 1.0, "", true),
        blend_property("Multiply"),
        float_property("angle", "Angle", 120.0, -360.0, 360.0, "°", false),
        float_property("distance", "Distance", 5.0, 0.0, 1000.0, " px", true),
        float_property(
            "spread",
            if inner { "Choke" } else { "Spread" },
            0.0,
            0.0,
            1.0,
            "",
            true,
        ),
        float_property("size", "Size", 5.0, 0.0, 1000.0, " px", true),
    ]
}

fn glow_properties(inner: bool) -> Vec<PropertyDefinition> {
    vec![
        color_property("color", "Color", Color::white()),
        float_property("opacity", "Opacity", 0.75, 0.0, 1.0, "", true),
        blend_property("Screen"),
        float_property(
            "spread",
            if inner { "Choke" } else { "Spread" },
            0.0,
            0.0,
            1.0,
            "",
            true,
        ),
        float_property("size", "Size", 5.0, 0.0, 1000.0, " px", true),
    ]
}

fn satin_properties() -> Vec<PropertyDefinition> {
    vec![
        color_property("color", "Color", Color::black()),
        float_property("opacity", "Opacity", 0.5, 0.0, 1.0, "", true),
        blend_property("Multiply"),
        float_property("angle", "Angle", 19.0, -360.0, 360.0, "°", false),
        float_property("distance", "Distance", 11.0, 0.0, 1000.0, " px", true),
        float_property("size", "Size", 14.0, 0.0, 1000.0, " px", true),
        bool_property("invert", "Invert", true),
    ]
}

fn bevel_properties() -> Vec<PropertyDefinition> {
    vec![
        dropdown_property(
            "style",
            "Style",
            "Inner Bevel",
            &[
                "Inner Bevel",
                "Outer Bevel",
                "Emboss",
                "Pillow Emboss",
                "Stroke Emboss",
            ],
        ),
        dropdown_property(
            "technique",
            "Technique",
            "Smooth",
            &["Smooth", "Chisel Hard", "Chisel Soft"],
        ),
        float_property("depth", "Depth", 1.0, 0.0, 1.0, "", true),
        dropdown_property("direction", "Direction", "Up", &["Up", "Down"]),
        float_property("size", "Size", 5.0, 0.0, 1000.0, " px", true),
        float_property("soften", "Soften", 0.0, 0.0, 1000.0, " px", true),
        float_property("angle", "Angle", 120.0, -360.0, 360.0, "°", false),
        float_property("altitude", "Altitude", 30.0, -90.0, 90.0, "°", true),
        color_property("highlight_color", "Highlight Color", Color::white()),
        float_property(
            "highlight_opacity",
            "Highlight Opacity",
            0.75,
            0.0,
            1.0,
            "",
            true,
        ),
        dropdown_property(
            "highlight_blend_mode",
            "Highlight Blend",
            "Screen",
            &["Normal", "Screen", "Linear Dodge (Add)"],
        ),
        color_property("shadow_color", "Shadow Color", Color::black()),
        float_property("shadow_opacity", "Shadow Opacity", 0.75, 0.0, 1.0, "", true),
        dropdown_property(
            "shadow_blend_mode",
            "Shadow Blend",
            "Multiply",
            &["Normal", "Multiply"],
        ),
    ]
}

pub(super) fn number(context: &EvaluatedOperation<'_>, key: &str) -> Option<f64> {
    match context.properties().get(key)? {
        PropertyValue::Number(value) => Some(value.into_inner()),
        PropertyValue::Integer(value) => Some(*value as f64),
        _ => None,
    }
}

fn text<'a>(context: &'a EvaluatedOperation<'_>, key: &str) -> Option<&'a str> {
    match context.properties().get(key)? {
        PropertyValue::String(value) => Some(value),
        _ => None,
    }
}

fn boolean(context: &EvaluatedOperation<'_>, key: &str) -> Option<bool> {
    match context.properties().get(key)? {
        PropertyValue::Boolean(value) => Some(*value),
        _ => None,
    }
}

pub(super) fn color(context: &EvaluatedOperation<'_>, key: &str) -> Option<Color> {
    match context.properties().get(key)? {
        PropertyValue::Color(color) => Some(color.clone()),
        PropertyValue::ColorValue(color) => crate::color_management::to_renderer_srgba8(color).ok(),
        _ => None,
    }
}

pub(super) fn blend(context: &EvaluatedOperation<'_>, key: &str) -> Option<BlendMode> {
    match text(context, key)? {
        "Normal" => Some(BlendMode::Normal),
        "Multiply" => Some(BlendMode::Multiply),
        "Screen" => Some(BlendMode::Screen),
        "Linear Dodge (Add)" => Some(BlendMode::LinearDodge),
        _ => None,
    }
}

fn shadow_style(context: &EvaluatedOperation<'_>, inner: bool) -> Option<DrawStyle> {
    let fields = (
        color(context, "color")?,
        number(context, "opacity")?,
        blend(context, "blend_mode")?,
        number(context, "angle")?,
        number(context, "distance")?,
        number(context, "spread")?,
        number(context, "size")?,
    );
    Some(if inner {
        DrawStyle::InnerShadow {
            color: fields.0,
            opacity: fields.1,
            blend_mode: fields.2,
            angle: fields.3,
            distance: fields.4,
            spread: fields.5,
            size: fields.6,
        }
    } else {
        DrawStyle::DropShadow {
            color: fields.0,
            opacity: fields.1,
            blend_mode: fields.2,
            angle: fields.3,
            distance: fields.4,
            spread: fields.5,
            size: fields.6,
        }
    })
}

fn glow_style(context: &EvaluatedOperation<'_>, inner: bool) -> Option<DrawStyle> {
    let fields = (
        color(context, "color")?,
        number(context, "opacity")?,
        blend(context, "blend_mode")?,
        number(context, "spread")?,
        number(context, "size")?,
    );
    Some(if inner {
        DrawStyle::InnerGlow {
            color: fields.0,
            opacity: fields.1,
            blend_mode: fields.2,
            spread: fields.3,
            size: fields.4,
        }
    } else {
        DrawStyle::OuterGlow {
            color: fields.0,
            opacity: fields.1,
            blend_mode: fields.2,
            spread: fields.3,
            size: fields.4,
        }
    })
}

fn satin_style(context: &EvaluatedOperation<'_>) -> Option<DrawStyle> {
    Some(DrawStyle::Satin {
        color: color(context, "color")?,
        opacity: number(context, "opacity")?,
        blend_mode: blend(context, "blend_mode")?,
        angle: number(context, "angle")?,
        distance: number(context, "distance")?,
        size: number(context, "size")?,
        invert: boolean(context, "invert")?,
    })
}

fn bevel_style(context: &EvaluatedOperation<'_>) -> Option<DrawStyle> {
    Some(DrawStyle::BevelEmboss {
        style: match text(context, "style")? {
            "Inner Bevel" => BevelStyle::InnerBevel,
            "Outer Bevel" => BevelStyle::OuterBevel,
            "Emboss" => BevelStyle::Emboss,
            "Pillow Emboss" => BevelStyle::PillowEmboss,
            "Stroke Emboss" => BevelStyle::StrokeEmboss,
            _ => return None,
        },
        technique: match text(context, "technique")? {
            "Smooth" => BevelTechnique::Smooth,
            "Chisel Hard" => BevelTechnique::ChiselHard,
            "Chisel Soft" => BevelTechnique::ChiselSoft,
            _ => return None,
        },
        depth: number(context, "depth")?,
        direction: match text(context, "direction")? {
            "Up" => BevelDirection::Up,
            "Down" => BevelDirection::Down,
            _ => return None,
        },
        size: number(context, "size")?,
        soften: number(context, "soften")?,
        angle: number(context, "angle")?,
        altitude: number(context, "altitude")?,
        highlight_color: color(context, "highlight_color")?,
        highlight_opacity: number(context, "highlight_opacity")?,
        highlight_blend_mode: blend(context, "highlight_blend_mode")?,
        shadow_color: color(context, "shadow_color")?,
        shadow_opacity: number(context, "shadow_opacity")?,
        shadow_blend_mode: blend(context, "shadow_blend_mode")?,
    })
}

macro_rules! layer_effect_plugin {
    ($type:ident, $id:literal, $name:literal, $properties:expr, $evaluate:expr) => {
        pub struct $type;

        impl Plugin for $type {
            fn id(&self) -> &'static str {
                $id
            }
            fn name(&self) -> String {
                $name.to_string()
            }
            fn category(&self) -> String {
                "Layer Style".to_string()
            }
            fn version(&self) -> (u32, u32, u32) {
                (0, 1, 0)
            }
        }

        impl StylePlugin for $type {
            fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
                OperationDescriptor::style(self.id(), self.name(), $properties)
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

layer_effect_plugin!(
    DropShadowStylePlugin,
    "drop_shadow",
    "Drop Shadow",
    shadow_properties(false),
    |context| shadow_style(context, false)
);
layer_effect_plugin!(
    InnerShadowStylePlugin,
    "inner_shadow",
    "Inner Shadow",
    shadow_properties(true),
    |context| shadow_style(context, true)
);
layer_effect_plugin!(
    OuterGlowStylePlugin,
    "outer_glow",
    "Outer Glow",
    glow_properties(false),
    |context| glow_style(context, false)
);
layer_effect_plugin!(
    InnerGlowStylePlugin,
    "inner_glow",
    "Inner Glow",
    glow_properties(true),
    |context| glow_style(context, true)
);
layer_effect_plugin!(
    SatinStylePlugin,
    "satin",
    "Satin",
    satin_properties(),
    satin_style
);
layer_effect_plugin!(
    BevelEmbossStylePlugin,
    "bevel_emboss",
    "Bevel & Emboss",
    bevel_properties(),
    bevel_style
);

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn every_alpha_mask_style_materializes_its_typed_default_contract() {
        let plugins: Vec<Box<dyn StylePlugin>> = vec![
            Box::new(DropShadowStylePlugin),
            Box::new(InnerShadowStylePlugin),
            Box::new(OuterGlowStylePlugin),
            Box::new(InnerGlowStylePlugin),
            Box::new(SatinStylePlugin),
            Box::new(BevelEmbossStylePlugin),
        ];
        for plugin in plugins {
            let descriptor = plugin.descriptor().expect("valid descriptor");
            let values = descriptor
                .properties()
                .iter()
                .map(|definition| {
                    (
                        definition.name().to_string(),
                        definition.default_value().clone(),
                    )
                })
                .collect::<HashMap<_, _>>();
            let context = EvaluatedOperation::new(&values, 0.0, 60.0, (1920, 1080));
            assert!(
                plugin.evaluate_values(&context, Uuid::new_v4()).is_some(),
                "{} default did not evaluate",
                plugin.id()
            );
        }
    }

    #[test]
    fn style_units_and_blend_names_map_to_the_runtime_contract() {
        let plugin = DropShadowStylePlugin;
        let descriptor = plugin.descriptor().expect("descriptor");
        let mut values = descriptor
            .properties()
            .iter()
            .map(|definition| {
                (
                    definition.name().to_string(),
                    definition.default_value().clone(),
                )
            })
            .collect::<HashMap<_, _>>();
        values.insert(
            "blend_mode".to_string(),
            PropertyValue::String("Linear Dodge (Add)".to_string()),
        );
        values.insert("spread".to_string(), PropertyValue::from(0.4));
        let context = EvaluatedOperation::new(&values, 0.0, 24.0, (640, 360));
        let style = plugin
            .evaluate_values(&context, Uuid::new_v4())
            .expect("style");
        assert!(matches!(
            style.style,
            DrawStyle::DropShadow {
                blend_mode: BlendMode::LinearDodge,
                spread,
                ..
            } if spread == 0.4
        ));
    }
}
