use crate::core::ensemble::decorators::{BackplateFit, BackplateTarget};
use crate::core::ensemble::types::DecoratorConfig;
use crate::model::property::{
    PropertyDefinition, PropertyMap, PropertyUiType, PropertyValue, Vec2,
};
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
    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        OperationDescriptor::backplate(self.id(), self.name(), self.properties())
    }

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
                "offset",
                PropertyUiType::vec2("px"),
                "Offset",
                PropertyValue::Vec2(Vec2 {
                    x: ordered_float::OrderedFloat(0.0),
                    y: ordered_float::OrderedFloat(0.0),
                }),
            ),
            PropertyDefinition::new(
                "fit",
                PropertyUiType::Dropdown {
                    options: vec![
                        "Stretch".to_string(),
                        "Contain".to_string(),
                        "Cover".to_string(),
                    ],
                },
                "Fit",
                PropertyValue::String("Stretch".to_string()),
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
        let padding_val = context.evaluate_number(properties, "padding", eval_time, 0.0) as f32;
        let [offset_x, offset_y] = context.evaluate_vec2(properties, "offset", eval_time, [0.0; 2]);

        let target_str = context
            .require_string(properties, "target", eval_time, "Block")
            .unwrap_or("Block".to_string());

        let target = match target_str.as_str() {
            "Char" => BackplateTarget::Char,
            "Line" => BackplateTarget::Line,
            _ => BackplateTarget::Block,
        };

        let fit_str = context
            .require_string(properties, "fit", eval_time, "Stretch")
            .unwrap_or("Stretch".to_string());
        let fit = match fit_str.as_str() {
            "Contain" => BackplateFit::Contain,
            "Cover" => BackplateFit::Cover,
            _ => BackplateFit::Stretch,
        };

        Some(DecoratorConfig::Backplate {
            target,
            padding: (padding_val, padding_val, padding_val, padding_val),
            offset: (offset_x as f32, offset_y as f32),
            fit,
        })
    }
}
