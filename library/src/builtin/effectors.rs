use crate::plugin::{Plugin, PluginCategory};
use crate::project::property::{PropertyDefinition, PropertyUiType, PropertyValue};

pub trait EffectorPlugin: Plugin {
    fn properties(&self) -> Vec<PropertyDefinition>;

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Effector
    }
}

// Transform Effector
pub struct TransformEffectorPlugin;
impl Plugin for TransformEffectorPlugin {
    fn id(&self) -> &'static str {
        "transform"
    }
    fn name(&self) -> String {
        "Transform".to_string()
    }
    fn category(&self) -> String {
        "Built-in".to_string()
    }
    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}
impl EffectorPlugin for TransformEffectorPlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        vec![
            PropertyDefinition::new(
                "tx",
                PropertyUiType::Float {
                    min: -1000.0,
                    max: 1000.0,
                    step: 1.0,
                    suffix: "px".into(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Translate X",
                PropertyValue::from(0.0),
            ),
            PropertyDefinition::new(
                "ty",
                PropertyUiType::Float {
                    min: -1000.0,
                    max: 1000.0,
                    step: 1.0,
                    suffix: "px".into(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Translate Y",
                PropertyValue::from(0.0),
            ),
            PropertyDefinition::new(
                "scale_x",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 10.0,
                    step: 0.1,
                    suffix: "".into(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Scale X",
                PropertyValue::from(1.0),
            ),
            PropertyDefinition::new(
                "scale_y",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 10.0,
                    step: 0.1,
                    suffix: "".into(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Scale Y",
                PropertyValue::from(1.0),
            ),
            PropertyDefinition::new(
                "rotation",
                PropertyUiType::Float {
                    min: -360.0,
                    max: 360.0,
                    step: 1.0,
                    suffix: "\u{b0}".into(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Rotation",
                PropertyValue::from(0.0),
            ),
        ]
    }
}

// StepDelay Effector
pub struct StepDelayEffectorPlugin;
impl Plugin for StepDelayEffectorPlugin {
    fn id(&self) -> &'static str {
        "step_delay"
    }
    fn name(&self) -> String {
        "Step Delay".to_string()
    }
    fn category(&self) -> String {
        "Built-in".to_string()
    }
    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}
impl EffectorPlugin for StepDelayEffectorPlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        vec![
            PropertyDefinition::new(
                "delay",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 5.0,
                    step: 0.05,
                    suffix: "s".into(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Delay per Char",
                PropertyValue::from(0.05),
            ),
            PropertyDefinition::new(
                "duration",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 5.0,
                    step: 0.05,
                    suffix: "s".into(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Duration",
                PropertyValue::from(0.2),
            ),
            PropertyDefinition::new(
                "from_opacity",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: "%".into(),
                    min_hard_limit: true,
                    max_hard_limit: true,
                },
                "From Opacity",
                PropertyValue::from(0.0),
            ),
            PropertyDefinition::new(
                "to_opacity",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: "%".into(),
                    min_hard_limit: true,
                    max_hard_limit: true,
                },
                "To Opacity",
                PropertyValue::from(100.0),
            ),
        ]
    }
}

// Randomize Effector
pub struct RandomizeEffectorPlugin;
impl Plugin for RandomizeEffectorPlugin {
    fn id(&self) -> &'static str {
        "randomize"
    }
    fn name(&self) -> String {
        "Randomize".to_string()
    }
    fn category(&self) -> String {
        "Built-in".to_string()
    }
    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}
impl EffectorPlugin for RandomizeEffectorPlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        vec![
            PropertyDefinition::new(
                "seed",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: "".into(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Seed",
                PropertyValue::from(0.0),
            ),
            PropertyDefinition::new(
                "amount",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 1.0,
                    step: 0.01,
                    suffix: "".into(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Amount",
                PropertyValue::from(1.0),
            ),
            PropertyDefinition::new(
                "translate_range",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 500.0,
                    step: 1.0,
                    suffix: "px".into(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Translate Range",
                PropertyValue::from(50.0),
            ),
            PropertyDefinition::new(
                "rotate_range",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 360.0,
                    step: 1.0,
                    suffix: "deg".into(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Rotate Range",
                PropertyValue::from(15.0),
            ),
            PropertyDefinition::new(
                "scale_range",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 5.0,
                    step: 0.1,
                    suffix: "".into(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Scale Range",
                PropertyValue::from(0.5),
            ),
        ]
    }
}

// Opacity Effector
pub struct OpacityEffectorPlugin;
impl Plugin for OpacityEffectorPlugin {
    fn id(&self) -> &'static str {
        "opacity"
    }
    fn name(&self) -> String {
        "Opacity".to_string()
    }
    fn category(&self) -> String {
        "Built-in".to_string()
    }
    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}
impl EffectorPlugin for OpacityEffectorPlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        vec![
            PropertyDefinition::new(
                "opacity",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: "%".into(),
                    min_hard_limit: true,
                    max_hard_limit: true,
                },
                "Opacity",
                PropertyValue::from(0.0),
            ),
            PropertyDefinition::new(
                "mode",
                PropertyUiType::Dropdown {
                    options: vec!["Set".to_string(), "Add".to_string(), "Multiply".to_string()],
                },
                "Mode",
                PropertyValue::String("Set".to_string()),
            ),
        ]
    }
}
