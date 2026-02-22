use crate::plugin::{Plugin, PluginCategory};
use crate::project::property::{PropertyDefinition, PropertyUiType, PropertyValue};
use crate::runtime::color::Color;

pub trait StylePlugin: Plugin {
    fn properties(&self) -> Vec<PropertyDefinition>;

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
        ]
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
        ]
    }
}
