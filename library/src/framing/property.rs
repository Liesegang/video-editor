use log::{debug, warn}; // Ensure debug is imported
use std::collections::HashMap;

use crate::animation::EasingFunction;
use crate::model::project::property::{Property, PropertyMap, PropertyValue, Vec2, Vec3};
use crate::model::frame::color::Color;

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

pub struct ConstantEvaluator;
pub struct KeyframeEvaluator;
pub struct ExpressionEvaluator;

impl PropertyEvaluator for ConstantEvaluator {
    fn evaluate(&self, property: &Property, _time: f64, _ctx: &EvaluationContext) -> PropertyValue {
        property.value().cloned().unwrap_or_else(|| {
            warn!("Constant evaluator missing 'value'; using 0");
            PropertyValue::Number(0.0)
        })
    }
}

impl PropertyEvaluator for KeyframeEvaluator {
    fn evaluate(&self, property: &Property, time: f64, _ctx: &EvaluationContext) -> PropertyValue {
        evaluate_keyframes(property, time)
    }
}

impl PropertyEvaluator for ExpressionEvaluator {
    fn evaluate(&self, property: &Property, time: f64, _ctx: &EvaluationContext) -> PropertyValue {
        warn!(
            "Expression evaluator not implemented for property '{}' at time {}",
            property.evaluator, time
        );
        PropertyValue::Number(0.0)
    }
}

pub fn register_builtin_evaluators(registry: &mut PropertyEvaluatorRegistry) {
    registry.register("constant", Box::new(ConstantEvaluator));
    registry.register("keyframe", Box::new(KeyframeEvaluator));
    registry.register("expression", Box::new(ExpressionEvaluator));
}

fn evaluate_keyframes(property: &Property, time: f64) -> PropertyValue {
    let keyframes = property.keyframes();
    debug!("evaluate_keyframes for property {:?} at time {}", property, time); // Added debug log

    if keyframes.is_empty() {
        debug!("evaluate_keyframes: keyframes empty, returning 0.0"); // Added debug log
        return PropertyValue::Number(0.0);
    }
    if time <= keyframes[0].time {
        debug!("evaluate_keyframes: time <= first keyframe, returning first keyframe value {:?}", keyframes[0].value); // Added debug log
        return keyframes[0].value.clone();
    }
    if time >= keyframes.last().unwrap().time {
        debug!("evaluate_keyframes: time >= last keyframe, returning last keyframe value {:?}", keyframes.last().unwrap().value); // Added debug log
        return keyframes.last().unwrap().value.clone();
    }

    let current = keyframes.iter().rev().find(|k| k.time <= time).unwrap();
    let next = keyframes.iter().find(|k| k.time > time).unwrap();
    let t = (time - current.time) / (next.time - current.time);
    let interpolated = interpolate_property_values(&current.value, &next.value, t, &current.easing);
    debug!("evaluate_keyframes: interpolated value {:?} for time {}", interpolated, time); // Added debug log
    interpolated
}
fn interpolate_property_values(
    start: &PropertyValue,
    end: &PropertyValue,
    t: f64,
    easing: &EasingFunction,
) -> PropertyValue {
    let t = easing.apply(t);

    match (start, end) {
        (PropertyValue::Number(s), PropertyValue::Number(e)) => {
            PropertyValue::Number(s + (e - s) * t)
        }
        (PropertyValue::Integer(s), PropertyValue::Integer(e)) => {
            PropertyValue::Number(*s as f64 + (*e as f64 - *s as f64) * t)
        }
        (PropertyValue::Vec2(Vec2 { x: sx, y: sy }), PropertyValue::Vec2(Vec2 { x: ex, y: ey })) => {
            PropertyValue::Vec2(Vec2 {
                x: sx + (ex - sx) * t,
                y: sy + (ey - sy) * t,
            })
        }
        (
            PropertyValue::Vec3(Vec3 { x: sx, y: sy, z: sz }),
            PropertyValue::Vec3(Vec3 { x: ex, y: ey, z: ez }),
        ) => PropertyValue::Vec3(Vec3 {
            x: sx + (ex - sx) * t,
            y: sy + (ey - sy) * t,
            z: sz + (ez - sz) * t,
        }),
        (
            PropertyValue::Color(Color { r: sr, g: sg, b: sb, a: sa }),
            PropertyValue::Color(Color { r: er, g: eg, b: eb, a: ea }),
        ) => PropertyValue::Color(Color {
            r: ((*sr as f64) + (*er as f64 - *sr as f64) * t).round() as u8,
            g: ((*sg as f64) + (*eg as f64 - *sg as f64) * t).round() as u8,
            b: ((*sb as f64) + (*eb as f64 - *sb as f64) * t).round() as u8,
            a: ((*sa as f64) + (*ea as f64 - *sa as f64) * t).round() as u8,
        }),
        (PropertyValue::Array(s), PropertyValue::Array(e)) => PropertyValue::Array(
            s.iter()
                .zip(e.iter())
                .map(|(start, end)| interpolate_property_values(start, end, t, easing))
                .collect(),
        ),
        (PropertyValue::Map(s), PropertyValue::Map(e)) => PropertyValue::Map(
            s.iter()
                .zip(e.iter())
                .map(|((k, sv), (_, ev))| {
                    (k.clone(), interpolate_property_values(sv, ev, t, easing))
                })
                .collect(),
        ),
        _ => start.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::project::property::Keyframe;

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
