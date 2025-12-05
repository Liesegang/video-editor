use std::collections::HashMap;
use log::warn; // Ensure debug is imported
use crate::model::project::property::{Property, PropertyMap, PropertyValue};

pub struct PropertyEvaluatorRegistry {
    evaluators: HashMap<&'static str, Box<dyn PropertyEvaluator>>,
}

impl PropertyEvaluatorRegistry {
    pub fn new() -> Self {
        Self {
            evaluators: HashMap::new(),
        }
    }

    pub fn register(&mut self, key: &'static str, evaluator: Box<dyn PropertyEvaluator>) {
        self.evaluators.insert(key, evaluator);
    }

    pub fn evaluate(
        &self,
        property: &Property,
        time: f64,
        ctx: &EvaluationContext,
    ) -> PropertyValue {
        let key = property.evaluator.as_str();
        match self.evaluators.get(key) {
            Some(evaluator) => evaluator.evaluate(property, time, ctx),
            None => {
                warn!("Unknown property evaluator '{}'", key);
                PropertyValue::Number(0.0)
            }
        }
    }
}

pub trait PropertyEvaluator: Send + Sync {
    fn evaluate(&self, property: &Property, time: f64, ctx: &EvaluationContext) -> PropertyValue;
}

pub struct EvaluationContext<'a> {
    pub property_map: &'a PropertyMap,
}


#[cfg(test)]
mod tests {
    use crate::animation::EasingFunction;
    use super::*;
    use crate::model::project::property::Keyframe;
    use crate::plugin::properties::builtin::register_builtin_evaluators;

    #[test]
    fn constant_evaluator_returns_value() {
        let mut map = PropertyMap::new();
        map.set(
            "value_prop".into(),
            Property::constant(PropertyValue::Number(42.0)),
        );

        let mut registry = PropertyEvaluatorRegistry::new();
        register_builtin_evaluators(&mut registry);
        let property = map.get("value_prop").unwrap();
        let ctx = EvaluationContext { property_map: &map };

        let result = registry.evaluate(property, 0.0, &ctx);
        assert!(matches!(result, PropertyValue::Number(v) if (v - 42.0).abs() < f64::EPSILON));
    }

    #[test]
    fn keyframe_evaluator_interpolates_linearly() {
        let keyframes = vec![
            Keyframe {
                time: 0.0,
                value: PropertyValue::Number(0.0),
                easing: EasingFunction::Linear,
            },
            Keyframe {
                time: 10.0,
                value: PropertyValue::Number(10.0),
                easing: EasingFunction::Linear,
            },
        ];
        let mut map = PropertyMap::new();
        map.set("anim".into(), Property::keyframe(keyframes));

        let mut registry = PropertyEvaluatorRegistry::new();
        register_builtin_evaluators(&mut registry);
        let property = map.get("anim").unwrap();
        let ctx = EvaluationContext { property_map: &map };

        let result = registry.evaluate(property, 5.0, &ctx);
        match result {
            PropertyValue::Number(v) => assert!((v - 5.0).abs() < f64::EPSILON),
            other => panic!("Expected number, got {:?}", other),
        }
    }
}
