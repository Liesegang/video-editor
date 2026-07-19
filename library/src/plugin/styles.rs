use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{CapType, DrawStyle, JoinType};
use crate::model::frame::entity::StyleConfig;
use crate::model::property::{PropertyDefinition, PropertyMap, PropertyUiType, PropertyValue};
use crate::plugin::entity_converter::FrameEvaluationContext;
use crate::plugin::{OperationDescriptor, OperationDescriptorError, Plugin, PluginCategory};
use uuid::Uuid;

pub trait StylePlugin: Plugin {
    /// Authoritative operation identity, ports, property metadata, and
    /// defaults for this Style producer.
    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError>;

    fn properties(&self) -> Vec<PropertyDefinition> {
        match self.descriptor() {
            Ok(descriptor) => descriptor.properties().to_vec(),
            Err(error) => {
                log::error!("Invalid Style descriptor for {}: {error}", self.id());
                Vec::new()
            }
        }
    }

    /// Evaluates one explicit Style operation Node from its direct properties.
    fn evaluate_source(
        &self,
        context: &FrameEvaluationContext,
        source_id: Uuid,
        properties: &PropertyMap,
        eval_time: f64,
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
    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        OperationDescriptor::style(
            self.id(),
            self.name(),
            vec![
                PropertyDefinition::new(
                    "color",
                    PropertyUiType::Color,
                    "Color",
                    PropertyValue::Color(Color::white()),
                ),
                PropertyDefinition::new(
                    "opacity",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 1.0,
                        step: 0.01,
                        suffix: "".into(),
                        min_hard_limit: true,
                        max_hard_limit: true,
                    },
                    "Opacity",
                    PropertyValue::from(1.0),
                ),
                PropertyDefinition::new(
                    "offset",
                    PropertyUiType::Float {
                        min: -50.0,
                        max: 50.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Offset",
                    PropertyValue::from(0.0),
                ),
            ],
        )
    }

    fn evaluate_source(
        &self,
        context: &FrameEvaluationContext,
        source_id: Uuid,
        properties: &PropertyMap,
        eval_time: f64,
    ) -> Option<StyleConfig> {
        let color = context.evaluate_color(properties, "color", eval_time, Color::white());
        let opacity = context.evaluate_number(properties, "opacity", eval_time, 1.0);
        let offset = context.evaluate_number(properties, "offset", eval_time, 0.0) as f32;

        // Apply opacity to color
        let mut final_color = color;
        final_color.a = (final_color.a as f32 * opacity as f32) as u8;

        Some(StyleConfig {
            id: source_id,
            style: DrawStyle::Fill {
                color: final_color,
                offset: offset as f64,
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
    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        OperationDescriptor::style(
            self.id(),
            self.name(),
            vec![
                PropertyDefinition::new(
                    "color",
                    PropertyUiType::Color,
                    "Color",
                    PropertyValue::Color(Color::white()),
                ),
                PropertyDefinition::new(
                    "width",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Width",
                    PropertyValue::from(1.0),
                ),
                PropertyDefinition::new(
                    "opacity",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 1.0,
                        step: 0.01,
                        suffix: "".into(),
                        min_hard_limit: true,
                        max_hard_limit: true,
                    },
                    "Opacity",
                    PropertyValue::from(1.0),
                ),
                PropertyDefinition::new(
                    "offset",
                    PropertyUiType::Float {
                        min: -50.0,
                        max: 50.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Offset",
                    PropertyValue::from(0.0),
                ),
                PropertyDefinition::new(
                    "join",
                    PropertyUiType::Dropdown {
                        options: vec![
                            "Miter".to_string(),
                            "Round".to_string(),
                            "Bevel".to_string(),
                        ],
                    },
                    "Join",
                    PropertyValue::String("Round".to_string()),
                ),
                PropertyDefinition::new(
                    "cap",
                    PropertyUiType::Dropdown {
                        options: vec![
                            "Butt".to_string(),
                            "Round".to_string(),
                            "Square".to_string(),
                        ],
                    },
                    "Cap",
                    PropertyValue::String("Round".to_string()),
                ),
                PropertyDefinition::new(
                    "miter_limit",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 0.1,
                        suffix: "".into(),
                        min_hard_limit: true,
                        max_hard_limit: false,
                    },
                    "Miter Limit",
                    PropertyValue::from(4.0),
                ),
                PropertyDefinition::new(
                    "dash_array",
                    PropertyUiType::Text,
                    "Dash Array",
                    PropertyValue::String("".to_string()),
                ),
                PropertyDefinition::new(
                    "dash_offset",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 1000.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Dash Offset",
                    PropertyValue::from(0.0),
                ),
            ],
        )
    }

    fn evaluate_source(
        &self,
        context: &FrameEvaluationContext,
        source_id: Uuid,
        properties: &PropertyMap,
        eval_time: f64,
    ) -> Option<StyleConfig> {
        let color = context.evaluate_color(properties, "color", eval_time, Color::white());
        let width = context.evaluate_number(properties, "width", eval_time, 1.0) as f32;
        let opacity = context.evaluate_number(properties, "opacity", eval_time, 1.0);
        let offset = context.evaluate_number(properties, "offset", eval_time, 0.0) as f32;
        let join_str = context
            .require_string(properties, "join", eval_time, "Round")
            .unwrap_or("Round".to_string());
        let cap_str = context
            .require_string(properties, "cap", eval_time, "Round")
            .unwrap_or("Round".to_string());

        let miter = context.evaluate_number(properties, "miter_limit", eval_time, 4.0) as f32;
        let dash_array_str = context
            .require_string(properties, "dash_array", eval_time, "0 0")
            .unwrap_or("".to_string());
        let dash_offset = context.evaluate_number(properties, "dash_offset", eval_time, 0.0) as f32;

        let dash_array: Vec<f32> = dash_array_str
            .split_whitespace()
            .filter_map(|s| s.parse::<f32>().ok())
            .collect();

        let join = match join_str.as_str() {
            "Miter" => JoinType::Miter,
            "Bevel" => JoinType::Bevel,
            _ => JoinType::Round,
        };

        let cap = match cap_str.as_str() {
            "Butt" => CapType::Butt,
            "Square" => CapType::Square,
            _ => CapType::Round,
        };

        // Apply opacity to color
        let mut final_color = color;
        final_color.a = (final_color.a as f32 * opacity as f32) as u8;

        Some(StyleConfig {
            id: source_id,
            style: DrawStyle::Stroke {
                color: final_color,
                width: width as f64,
                offset: offset as f64,
                join,
                cap,
                miter: miter as f64,
                dash_array: dash_array.into_iter().map(|v| v as f64).collect(),
                dash_offset: dash_offset as f64,
            },
        })
    }
}
