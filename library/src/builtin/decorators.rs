use crate::plugin::{Plugin, PluginCategory};
use crate::project::property::{PropertyDefinition, PropertyUiType, PropertyValue};
use crate::runtime::color::Color;

pub trait DecoratorPlugin: Plugin {
    fn properties(&self) -> Vec<PropertyDefinition>;

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
}
