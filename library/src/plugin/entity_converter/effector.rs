use crate::core::ensemble::effectors::OpacityMode;
use crate::core::ensemble::types::EffectorConfig;
use crate::model::project::ensemble::EffectorInstance;
use crate::plugin::entity_converter::FrameEvaluationContext;

pub trait EffectorConverter: Send + Sync {
    fn convert(
        &self,
        context: &FrameEvaluationContext,
        instance: &EffectorInstance,
        eval_time: f64,
    ) -> Option<EffectorConfig>;
}

// Transform Effector
pub struct TransformEffectorConverter;
impl EffectorConverter for TransformEffectorConverter {
    fn convert(
        &self,
        context: &FrameEvaluationContext,
        instance: &EffectorInstance,
        eval_time: f64,
    ) -> Option<EffectorConfig> {
        let tx =
            context.evaluate_number(&instance.properties, "translate_x", eval_time, 0.0) as f32;
        let ty =
            context.evaluate_number(&instance.properties, "translate_y", eval_time, 0.0) as f32;
        let r = context.evaluate_number(&instance.properties, "rotate", eval_time, 0.0) as f32;
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
pub struct StepDelayEffectorConverter;
impl EffectorConverter for StepDelayEffectorConverter {
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

// Opacity Effector
pub struct OpacityEffectorConverter;
impl EffectorConverter for OpacityEffectorConverter {
    fn convert(
        &self,
        context: &FrameEvaluationContext,
        instance: &EffectorInstance,
        eval_time: f64,
    ) -> Option<EffectorConfig> {
        // Implementation based on assumption (not visible in original snippet, but inferred standard)
        let target_opacity =
            context.evaluate_number(&instance.properties, "opacity", eval_time, 100.0) as f32;
        Some(EffectorConfig::Opacity {
            target_opacity,
            mode: OpacityMode::Set,
            target: Default::default(),
        })
    }
}

// Randomize Effector
pub struct RandomizeEffectorConverter;
impl EffectorConverter for RandomizeEffectorConverter {
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

pub fn get_converter(type_name: &str) -> Option<Box<dyn EffectorConverter>> {
    match type_name {
        "transform" => Some(Box::new(TransformEffectorConverter)),
        "step_delay" => Some(Box::new(StepDelayEffectorConverter)),
        "randomize" => Some(Box::new(RandomizeEffectorConverter)),
        "opacity" => Some(Box::new(OpacityEffectorConverter)),
        _ => None,
    }
}
