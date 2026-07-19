use crate::core::ensemble::decorators::{BackplateShape, BackplateTarget};
use crate::core::ensemble::types::DecoratorConfig;
use crate::model::frame::color::Color;
use crate::model::property::{PropertyDefinition, PropertyMap, PropertyUiType, PropertyValue};
use crate::plugin::entity_converter::FrameEvaluationContext;
use crate::plugin::{OperationDescriptor, OperationDescriptorError, Plugin, PluginCategory};
use uuid::Uuid;

pub trait DecoratorPlugin: Plugin {
    fn properties(&self) -> Vec<PropertyDefinition>;

    /// Authoritative graph operation contract.
    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        OperationDescriptor::decorator(self.id(), self.name(), self.properties())
    }

    /// Evaluates one explicit Decorator operation Node from its direct authored
    /// properties. No embedded instance model exists.
    fn evaluate_source(
        &self,
        context: &FrameEvaluationContext,
        source_id: Uuid,
        properties: &PropertyMap,
        eval_time: f64,
    ) -> Option<DecoratorConfig>;

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Decorator
    }
}

pub struct BackplateDecoratorPlugin;
impl Plugin for BackplateDecoratorPlugin {
    fn id(&self) -> &'static str {
        "backplate"
    }
    fn name(&self) -> String {
        "Backplate".to_string()
    }
    fn category(&self) -> String {
        "Built-in".to_string()
    }
    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}
impl DecoratorPlugin for BackplateDecoratorPlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        vec![
            PropertyDefinition::new(
                "target",
                PropertyUiType::Dropdown {
                    options: vec!["Char".to_string(), "Line".to_string(), "Block".to_string()],
                },
                "Target",
                PropertyValue::String("Block".to_string()),
            ),
            PropertyDefinition::new(
                "shape",
                PropertyUiType::Dropdown {
                    options: vec![
                        "Rect".to_string(),
                        "RoundRect".to_string(),
                        "Circle".to_string(),
                    ],
                },
                "Shape",
                PropertyValue::String("Rect".to_string()),
            ),
            PropertyDefinition::new(
                "color",
                PropertyUiType::Color,
                "Color",
                PropertyValue::Color(Color::black()),
            ),
            PropertyDefinition::new(
                "padding",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: "px".into(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Padding",
                PropertyValue::from(0.0),
            ),
            PropertyDefinition::new(
                "radius",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 50.0,
                    step: 1.0,
                    suffix: "px".into(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Corner Radius",
                PropertyValue::from(0.0),
            ),
        ]
    }

    fn evaluate_source(
        &self,
        context: &FrameEvaluationContext,
        _source_id: Uuid,
        properties: &PropertyMap,
        eval_time: f64,
    ) -> Option<DecoratorConfig> {
        let color = context.evaluate_color(properties, "color", eval_time, Color::black());

        let padding_val = context.evaluate_number(properties, "padding", eval_time, 0.0) as f32;
        let radius = context.evaluate_number(properties, "radius", eval_time, 0.0) as f32;

        let target_str = context
            .require_string(properties, "target", eval_time, "Block")
            .unwrap_or("Block".to_string());

        let target = match target_str.as_str() {
            "Char" => BackplateTarget::Char,
            "Line" => BackplateTarget::Line,
            _ => BackplateTarget::Block,
        };

        let shape_str = context
            .require_string(properties, "shape", eval_time, "Rect")
            .unwrap_or("Rect".to_string());

        let shape = match shape_str.as_str() {
            "RoundRect" => BackplateShape::RoundedRect,
            "Circle" => BackplateShape::Circle,
            _ => BackplateShape::Rect,
        };

        Some(DecoratorConfig::Backplate {
            target,
            shape,
            color,
            padding: (padding_val, padding_val, padding_val, padding_val),
            corner_radius: radius,
        })
    }
}
