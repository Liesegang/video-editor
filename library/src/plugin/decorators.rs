use crate::core::ensemble::decorators::{BackplateFit, BackplateShape, BackplateTarget};
use crate::core::ensemble::types::DecoratorConfig;
use crate::model::frame::color::Color;
use crate::model::property::{ColorValue, PropertyDefinition, PropertyUiType, PropertyValue, Vec2};
use crate::plugin::{
    EvaluatedOperation, OperationDescriptor, OperationDescriptorError, Plugin, PluginCategory,
};
use uuid::Uuid;

pub trait DecoratorPlugin: Plugin {
    fn properties(&self) -> Vec<PropertyDefinition>;

    /// Authoritative graph operation contract.
    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        OperationDescriptor::decorator(self.id(), self.name(), self.properties())
    }

    /// Descriptor used when a Text source supplies the operation's only Shape
    /// input. Most one-Shape Decorators can use their graph contract directly.
    /// A multi-input Decorator may expose a self-contained form through this
    /// same component instead of requiring a second, duplicated plugin.
    fn inline_text_descriptor(
        &self,
    ) -> Result<Option<OperationDescriptor>, OperationDescriptorError> {
        let descriptor = self.descriptor()?;
        Ok(
            crate::model::authoring::text_ensemble_direct_contract_is_compatible(
                descriptor.declared_ports(),
            )
            .then_some(descriptor),
        )
    }

    /// Evaluates one explicit Decorator operation Node from its direct authored
    /// properties. No embedded instance model exists.
    fn evaluate_source(
        &self,
        context: &EvaluatedOperation<'_>,
        source_id: Uuid,
    ) -> Option<DecoratorConfig>;

    /// Execute the self-contained Text form declared above. The default keeps
    /// one-Shape plugins on their existing executor; multi-input plugins only
    /// override the part whose semantics genuinely differ without creating a
    /// parallel component.
    fn evaluate_inline_text_source(
        &self,
        context: &EvaluatedOperation<'_>,
        source_id: Uuid,
    ) -> Option<DecoratorConfig> {
        self.evaluate_source(context, source_id)
    }

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

    fn inline_text_descriptor(
        &self,
    ) -> Result<Option<OperationDescriptor>, OperationDescriptorError> {
        OperationDescriptor::decorator(
            self.id(),
            self.name(),
            self_contained_backplate_properties(),
        )
        .map(Some)
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
        context: &EvaluatedOperation<'_>,
        _source_id: Uuid,
    ) -> Option<DecoratorConfig> {
        let padding_val = context.number("padding").unwrap_or(0.0) as f32;
        let [offset_x, offset_y] = context.vec2("offset").unwrap_or([0.0; 2]);

        let target_str = context
            .string("target")
            .unwrap_or_else(|| "Block".to_string());

        let target = match target_str.as_str() {
            "Char" => BackplateTarget::Char,
            "Line" => BackplateTarget::Line,
            _ => BackplateTarget::Block,
        };

        let fit_str = context
            .string("fit")
            .unwrap_or_else(|| "Stretch".to_string());
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

    fn evaluate_inline_text_source(
        &self,
        context: &EvaluatedOperation<'_>,
        _source_id: Uuid,
    ) -> Option<DecoratorConfig> {
        let target = match context.string("target").as_deref() {
            Some("Char") => BackplateTarget::Char,
            Some("Line") => BackplateTarget::Line,
            _ => BackplateTarget::Block,
        };
        let shape = match context.string("shape").as_deref() {
            Some("RoundRect") => BackplateShape::RoundedRect,
            Some("Circle") => BackplateShape::Circle,
            _ => BackplateShape::Rect,
        };
        let color = context
            .properties()
            .get("color")
            .and_then(|value| match value {
                PropertyValue::Color(color) => Some(color.clone()),
                PropertyValue::ColorValue(color) => {
                    crate::color_management::to_renderer_srgba8(color).ok()
                }
                _ => None,
            })
            .unwrap_or_else(Color::black);
        let padding = context.number("padding").unwrap_or(0.0) as f32;
        let corner_radius = context.number("radius").unwrap_or(0.0) as f32;
        Some(DecoratorConfig::LegacyBackplate {
            target,
            shape,
            color,
            padding: (padding, padding, padding, padding),
            corner_radius,
        })
    }
}

/// The production self-contained Backplate controls used by Text Ensemble.
/// The graph form of the same component keeps its arbitrary background Shape
/// input, while this form owns only appearance that can be authored without a
/// second source.
fn self_contained_backplate_properties() -> Vec<PropertyDefinition> {
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
            PropertyUiType::ColorValue,
            "Color",
            PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&Color::black())),
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
