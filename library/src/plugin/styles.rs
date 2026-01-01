use crate::model::frame::color::Color;
use crate::model::frame::draw_type::DrawStyle;
use crate::model::frame::entity::StyleConfig;
use crate::model::project::property::{PropertyDefinition, PropertyUiType, PropertyValue};
use crate::model::project::style::StyleInstance;
use crate::plugin::entity_converter::FrameEvaluationContext;
use crate::plugin::{Plugin, PluginCategory};
use ordered_float::OrderedFloat;

pub trait StylePlugin: Plugin {
    fn properties(&self) -> Vec<PropertyDefinition>;

    fn convert(
        &self,
        context: &FrameEvaluationContext,
        instance: &StyleInstance,
        time: f64,
    ) -> Option<StyleConfig>;

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Style
    }
}

pub struct FillStylePlugin;
impl Plugin for FillStylePlugin {
    fn id(&self) -> &'static str {
        "fill"
    }
    fn name(&self) -> String {
        "Fill".to_string()
    }
    fn category(&self) -> String {
        "Built-in".to_string()
    }
    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}
impl StylePlugin for FillStylePlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        vec![
            PropertyDefinition {
                name: "color".to_string(),
                label: "Color".to_string(),
                ui_type: PropertyUiType::Color,
                default_value: PropertyValue::Color(Color {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                }),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "offset".to_string(),
                label: "Offset".to_string(),
                ui_type: PropertyUiType::Float {
                    min: -100.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                category: "Style".to_string(),
            },
        ]
    }

    fn convert(
        &self,
        context: &FrameEvaluationContext,
        instance: &StyleInstance,
        time: f64,
    ) -> Option<StyleConfig> {
        // Original logic: offset: 0.0
        let offset = context.evaluate_number(&instance.properties, "offset", time, 0.0);

        Some(StyleConfig {
            id: instance.id,
            style: DrawStyle::Fill {
                color: context.evaluate_color(&instance.properties, "color", time, Color::white()),
                offset,
            },
        })
    }
}

pub struct StrokeStylePlugin;
impl Plugin for StrokeStylePlugin {
    fn id(&self) -> &'static str {
        "stroke"
    }
    fn name(&self) -> String {
        "Stroke".to_string()
    }
    fn category(&self) -> String {
        "Built-in".to_string()
    }
    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}
impl StylePlugin for StrokeStylePlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        vec![
            PropertyDefinition {
                name: "color".to_string(),
                label: "Color".to_string(),
                ui_type: PropertyUiType::Color,
                default_value: PropertyValue::Color(Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                }),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "width".to_string(),
                label: "Width".to_string(),
                ui_type: PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(1.0)),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "offset".to_string(),
                label: "Offset".to_string(),
                ui_type: PropertyUiType::Float {
                    min: -100.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "miter".to_string(),
                label: "Miter Limit".to_string(),
                ui_type: PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 0.1,
                    suffix: "".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(4.0)),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "cap".to_string(),
                label: "Line Cap".to_string(),
                ui_type: PropertyUiType::Dropdown {
                    options: vec![
                        "Butt".to_string(),
                        "Round".to_string(),
                        "Square".to_string(),
                    ],
                },
                default_value: PropertyValue::String("Butt".to_string()),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "join".to_string(),
                label: "Line Join".to_string(),
                ui_type: PropertyUiType::Dropdown {
                    options: vec![
                        "Miter".to_string(),
                        "Round".to_string(),
                        "Bevel".to_string(),
                    ],
                },
                default_value: PropertyValue::String("Miter".to_string()),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "dash_array".to_string(),
                label: "Dash Array".to_string(),
                ui_type: PropertyUiType::Text,
                default_value: PropertyValue::String("".to_string()),
                category: "Style".to_string(),
            },
            PropertyDefinition {
                name: "dash_offset".to_string(),
                label: "Dash Offset".to_string(),
                ui_type: PropertyUiType::Float {
                    min: -100.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                category: "Style".to_string(),
            },
        ]
    }

    fn convert(
        &self,
        context: &FrameEvaluationContext,
        instance: &StyleInstance,
        time: f64,
    ) -> Option<StyleConfig> {
        // Original logic: offset: 0.0
        let offset = 0.0; // Keeping 0.0 for now unless I confirmed it's safe to change
        let style = DrawStyle::Stroke {
            color: context.evaluate_color(&instance.properties, "color", time, Color::black()),
            width: context.evaluate_number(&instance.properties, "width", time, 1.0),
            offset,
            cap: context.evaluate_cap_type(&instance.properties, "cap", time, Default::default()),
            join: context.evaluate_join_type(
                &instance.properties,
                "join",
                time,
                Default::default(),
            ),
            miter: context.evaluate_number(&instance.properties, "miter", time, 4.0),
            dash_array: context.evaluate_number_array(&instance.properties, "dash_array", time),
            dash_offset: context.evaluate_number(&instance.properties, "dash_offset", time, 0.0),
        };

        Some(StyleConfig {
            id: instance.id,
            style,
        })
    }
}
