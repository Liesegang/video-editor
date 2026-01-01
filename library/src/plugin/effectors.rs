use crate::core::ensemble::effectors::OpacityMode;
use crate::core::ensemble::types::EffectorConfig;
use crate::model::project::ensemble::EffectorInstance;
use crate::model::project::property::{PropertyDefinition, PropertyUiType};
use crate::plugin::entity_converter::FrameEvaluationContext;
use crate::plugin::{Plugin, PluginCategory};

pub trait EffectorPlugin: Plugin {
    fn properties(&self) -> Vec<PropertyDefinition>;

    fn convert(
        &self,
        context: &FrameEvaluationContext,
        instance: &EffectorInstance,
        eval_time: f64,
    ) -> Option<EffectorConfig>;

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
                },
                "Translate X",
            ),
            PropertyDefinition::new(
                "ty",
                PropertyUiType::Float {
                    min: -1000.0,
                    max: 1000.0,
                    step: 1.0,
                    suffix: "px".into(),
                },
                "Translate Y",
            ),
            PropertyDefinition::new(
                "scale_x",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 10.0,
                    step: 0.1,
                    suffix: "".into(),
                },
                "Scale X",
            ),
            PropertyDefinition::new(
                "scale_y",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 10.0,
                    step: 0.1,
                    suffix: "".into(),
                },
                "Scale Y",
            ),
            PropertyDefinition::new(
                "rotation",
                PropertyUiType::Float {
                    min: -360.0,
                    max: 360.0,
                    step: 1.0,
                    suffix: "°".into(),
                },
                "Rotation",
            ),
        ]
    }

    fn convert(
        &self,
        context: &FrameEvaluationContext,
        instance: &EffectorInstance,
        eval_time: f64,
    ) -> Option<EffectorConfig> {
        let tx = context.evaluate_number(&instance.properties, "tx", eval_time, 0.0) as f32;
        let ty = context.evaluate_number(&instance.properties, "ty", eval_time, 0.0) as f32;
        let r = context.evaluate_number(&instance.properties, "rotation", eval_time, 0.0) as f32;
        let sx = context.evaluate_number(&instance.properties, "scale_x", eval_time, 1.0) as f32;
        let sy = context.evaluate_number(&instance.properties, "scale_y", eval_time, 1.0) as f32;

        Some(EffectorConfig::Transform {
            translate: (tx, ty),
            rotate: r,
            scale: (sx, sy),
            target: Default::default(),
        })
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
                },
                "Delay per Char",
            ),
            PropertyDefinition::new(
                "duration",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 5.0,
                    step: 0.05,
                    suffix: "s".into(),
                },
                "Duration",
            ),
            PropertyDefinition::new(
                "from_opacity",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: "%".into(),
                },
                "From Opacity",
            ),
            PropertyDefinition::new(
                "to_opacity",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: "%".into(),
                },
                "To Opacity",
            ),
        ]
    }

    fn convert(
        &self,
        context: &FrameEvaluationContext,
        instance: &EffectorInstance,
        eval_time: f64,
    ) -> Option<EffectorConfig> {
        let delay = context.evaluate_number(&instance.properties, "delay", eval_time, 0.1) as f32;
        let duration =
            context.evaluate_number(&instance.properties, "duration", eval_time, 1.0) as f32;
        let from_opacity =
            context.evaluate_number(&instance.properties, "from_opacity", eval_time, 0.0) as f32;
        let to_opacity =
            context.evaluate_number(&instance.properties, "to_opacity", eval_time, 100.0) as f32;

        Some(EffectorConfig::StepDelay {
            delay_per_element: delay,
            duration,
            from_opacity,
            to_opacity,
            target: Default::default(),
        })
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
                },
                "Seed",
            ),
            PropertyDefinition::new(
                "amount",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 1.0,
                    step: 0.01,
                    suffix: "".into(),
                },
                "Amount",
            ),
            PropertyDefinition::new(
                "translate_range",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 500.0,
                    step: 1.0,
                    suffix: "px".into(),
                },
                "Translate Range",
            ),
            PropertyDefinition::new(
                "rotate_range",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 360.0,
                    step: 1.0,
                    suffix: "deg".into(),
                },
                "Rotate Range",
            ),
        ]
    }

    fn convert(
        &self,
        context: &FrameEvaluationContext,
        instance: &EffectorInstance,
        eval_time: f64,
    ) -> Option<EffectorConfig> {
        let seed = context.evaluate_number(&instance.properties, "seed", eval_time, 0.0) as u64;
        let amount = context.evaluate_number(&instance.properties, "amount", eval_time, 1.0) as f32;
        let tr_val = context.evaluate_number(
            &instance.properties,
            "translate_range",
            eval_time,
            100.0 * amount as f64,
        ) as f32;
        let rr_val = context.evaluate_number(
            &instance.properties,
            "rotate_range",
            eval_time,
            360.0 * amount as f64,
        ) as f32;
        let sr_val = context.evaluate_number(
            &instance.properties,
            "scale_range",
            eval_time,
            0.5 * amount as f64,
        ) as f32;

        Some(EffectorConfig::Randomize {
            translate_range: (tr_val, tr_val),
            rotate_range: rr_val,
            scale_range: (sr_val, sr_val),
            seed,
            target: Default::default(),
        })
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
        vec![PropertyDefinition::new(
            "opacity",
            PropertyUiType::Float {
                min: 0.0,
                max: 100.0,
                step: 1.0,
                suffix: "%".into(),
            },
            "Opacity",
        )]
    }

    fn convert(
        &self,
        context: &FrameEvaluationContext,
        instance: &EffectorInstance,
        eval_time: f64,
    ) -> Option<EffectorConfig> {
        let target_opacity =
            context.evaluate_number(&instance.properties, "opacity", eval_time, 100.0) as f32;
        Some(EffectorConfig::Opacity {
            target_opacity,
            mode: OpacityMode::Set,
            target: Default::default(),
        })
    }
}
