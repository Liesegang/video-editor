use crate::core::ensemble::effectors::OpacityMode;
use crate::core::ensemble::target::EffectorTarget;
use crate::core::ensemble::types::EffectorConfig;
use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue};
use crate::plugin::{
    EvaluatedOperation, OperationDescriptor, OperationDescriptorError, Plugin, PluginCategory,
};
use uuid::Uuid;

fn target_property() -> PropertyDefinition {
    target_property_with_default("Block")
}

fn target_property_with_default(default: &str) -> PropertyDefinition {
    PropertyDefinition::new(
        "target",
        PropertyUiType::Dropdown {
            options: vec!["Block".to_string(), "Line".to_string(), "Char".to_string()],
        },
        "Target",
        PropertyValue::String(default.to_string()),
    )
}

fn evaluate_target(context: &EvaluatedOperation<'_>) -> EffectorTarget {
    evaluate_target_or(context, EffectorTarget::Block)
}

fn evaluate_target_or(context: &EvaluatedOperation<'_>, default: EffectorTarget) -> EffectorTarget {
    match context.string("target").as_deref() {
        Some("Line") => EffectorTarget::Line,
        Some("Char") => EffectorTarget::Char,
        Some("Block") => EffectorTarget::Block,
        _ => default,
    }
}

pub trait EffectorPlugin: Plugin {
    fn properties(&self) -> Vec<PropertyDefinition>;

    /// Authoritative graph operation identity, typed ports, property metadata,
    /// and defaults. The default preserves existing built-in/runtime plugin
    /// implementations while every manager/factory path consumes the
    /// validated descriptor.
    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        OperationDescriptor::effector(self.id(), self.name(), self.properties())
    }

    /// Evaluates one explicit Effector operation Node from its direct authored
    /// properties. No embedded instance model exists.
    fn evaluate_source(
        &self,
        context: &EvaluatedOperation<'_>,
        source_id: Uuid,
    ) -> Option<EffectorConfig>;

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Effector
    }
}

// Optional per-element transform modulation. Base whole-Shape placement is
// owned by the distinct native Transform operation.
pub struct TransformEffectorPlugin;
impl Plugin for TransformEffectorPlugin {
    fn id(&self) -> &'static str {
        "transform"
    }
    fn name(&self) -> String {
        "Transform Modulation".to_string()
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
                    suffix: "°".into(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Rotation",
                PropertyValue::from(0.0),
            ),
            target_property(),
        ]
    }

    fn evaluate_source(
        &self,
        context: &EvaluatedOperation<'_>,
        _source_id: Uuid,
    ) -> Option<EffectorConfig> {
        let tx = context.number("tx").unwrap_or(0.0) as f32;
        let ty = context.number("ty").unwrap_or(0.0) as f32;
        let r = context.number("rotation").unwrap_or(0.0) as f32;
        let sx = context.number("scale_x").unwrap_or(1.0) as f32;
        let sy = context.number("scale_y").unwrap_or(1.0) as f32;

        Some(EffectorConfig::Transform {
            translate: (tx, ty),
            rotate: r,
            scale: (sx, sy),
            target: evaluate_target(context),
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
            target_property(),
        ]
    }

    fn evaluate_source(
        &self,
        context: &EvaluatedOperation<'_>,
        _source_id: Uuid,
    ) -> Option<EffectorConfig> {
        let delay = context.number("delay").unwrap_or(0.05) as f32;
        let duration = context.number("duration").unwrap_or(0.2) as f32;
        let from_opacity = context.number("from_opacity").unwrap_or(0.0) as f32;
        let to_opacity = context.number("to_opacity").unwrap_or(100.0) as f32;

        Some(EffectorConfig::StepDelay {
            delay_per_element: delay,
            duration,
            from_opacity,
            to_opacity,
            target: evaluate_target(context),
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
            target_property(),
        ]
    }

    fn evaluate_source(
        &self,
        context: &EvaluatedOperation<'_>,
        _source_id: Uuid,
    ) -> Option<EffectorConfig> {
        let seed = context.number("seed").unwrap_or(0.0) as u64;
        let amount = context.number("amount").unwrap_or(1.0) as f32;
        let tr_val = context.number("translate_range").unwrap_or(50.0) as f32;
        let rr_val = context.number("rotate_range").unwrap_or(15.0) as f32;
        let sr_val = context.number("scale_range").unwrap_or(0.5) as f32;

        Some(EffectorConfig::Randomize {
            translate_range: (tr_val * amount, tr_val * amount),
            rotate_range: rr_val * amount,
            scale_range: (sr_val * amount, sr_val * amount),
            seed,
            target: evaluate_target(context),
        })
    }
}

// Optional per-element opacity modulation. Base/static opacity is Style-owned.
pub struct OpacityEffectorPlugin;
impl Plugin for OpacityEffectorPlugin {
    fn id(&self) -> &'static str {
        "opacity"
    }
    fn name(&self) -> String {
        "Opacity Modulation".to_string()
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
                "Element Opacity",
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
            target_property(),
        ]
    }

    fn evaluate_source(
        &self,
        context: &EvaluatedOperation<'_>,
        _source_id: Uuid,
    ) -> Option<EffectorConfig> {
        let target_opacity = context.number("opacity").unwrap_or(100.0) as f32;
        let mode_str = context.string("mode").unwrap_or_else(|| "Set".to_string());

        let mode = match mode_str.as_str() {
            "Add" => OpacityMode::Add,
            "Multiply" => OpacityMode::Multiply,
            _ => OpacityMode::Set,
        };

        Some(EffectorConfig::Opacity {
            target_opacity,
            mode,
            target: evaluate_target(context),
        })
    }
}

/// Horizontal character spacing applied after text layout. This stays an
/// Ensemble Effector so the same descriptor is authorable inline on Text and
/// as a Node in a promoted Node Clip.
pub struct TrackingEffectorPlugin;

impl Plugin for TrackingEffectorPlugin {
    fn id(&self) -> &'static str {
        "tracking"
    }

    fn name(&self) -> String {
        "Tracking".to_string()
    }

    fn category(&self) -> String {
        "Built-in".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EffectorPlugin for TrackingEffectorPlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        vec![
            PropertyDefinition::new(
                "amount",
                PropertyUiType::Float {
                    min: -500.0,
                    max: 500.0,
                    step: 1.0,
                    suffix: "px".into(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Tracking",
                PropertyValue::from(0.0),
            ),
            target_property_with_default("Line"),
        ]
    }

    fn evaluate_source(
        &self,
        context: &EvaluatedOperation<'_>,
        _source_id: Uuid,
    ) -> Option<EffectorConfig> {
        Some(EffectorConfig::Tracking {
            amount: context.number("amount").unwrap_or(0.0) as f32,
            target: evaluate_target_or(context, EffectorTarget::Line),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn transform_effector_keeps_translate_scale_and_target_independent() {
        let values = HashMap::from([
            ("tx".to_string(), PropertyValue::from(13.0)),
            ("ty".to_string(), PropertyValue::from(-29.0)),
            ("rotation".to_string(), PropertyValue::from(17.0)),
            ("scale_x".to_string(), PropertyValue::from(0.5)),
            ("scale_y".to_string(), PropertyValue::from(1.75)),
            (
                "target".to_string(),
                PropertyValue::String("Line".to_string()),
            ),
        ]);
        let context = EvaluatedOperation::new(&values, 0.0, 30.0, (1920, 1080));
        let config = TransformEffectorPlugin
            .evaluate_source(&context, Uuid::new_v4())
            .expect("Transform config");

        let EffectorConfig::Transform {
            translate,
            rotate,
            scale,
            target,
        } = config
        else {
            panic!("wrong Effector config");
        };
        assert_eq!(translate, (13.0, -29.0));
        assert_eq!(rotate, 17.0);
        assert_eq!(scale, (0.5, 1.75));
        assert_eq!(target, EffectorTarget::Line);
    }

    #[test]
    fn target_property_selects_each_ensemble_scope() {
        for (authored, expected) in [
            ("Block", EffectorTarget::Block),
            ("Line", EffectorTarget::Line),
            ("Char", EffectorTarget::Char),
        ] {
            let values = HashMap::from([(
                "target".to_string(),
                PropertyValue::String(authored.to_string()),
            )]);
            let context = EvaluatedOperation::new(&values, 0.0, 30.0, (1920, 1080));
            assert_eq!(evaluate_target(&context), expected);
        }
    }

    #[test]
    fn tracking_descriptor_and_evaluation_share_amount_and_target() {
        let descriptor = TrackingEffectorPlugin.descriptor().unwrap();
        assert_eq!(descriptor.component_id(), "tracking");
        assert_eq!(descriptor.properties().len(), 2);
        assert_eq!(
            descriptor.properties()[1].default_value(),
            &PropertyValue::String("Line".to_string())
        );

        let values = HashMap::from([
            ("amount".to_string(), PropertyValue::from(24.0)),
            (
                "target".to_string(),
                PropertyValue::String("Line".to_string()),
            ),
        ]);
        let context = EvaluatedOperation::new(&values, 0.0, 30.0, (1920, 1080));
        assert!(matches!(
            TrackingEffectorPlugin.evaluate_source(&context, Uuid::new_v4()),
            Some(EffectorConfig::Tracking {
                amount: 24.0,
                target: EffectorTarget::Line,
            })
        ));
    }
}
