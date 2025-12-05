use log::{debug, warn};
use crate::animation::EasingFunction;
use super::super::{Plugin, PluginCategory, PropertyPlugin};
use crate::framing::property::{PropertyEvaluatorRegistry};
use crate::framing::{EvaluationContext, PropertyEvaluator};
use crate::model::frame::color::Color;
use crate::model::project::property::{Property, PropertyValue, Vec2, Vec3};

pub struct BuiltinPropertyPlugin;

impl BuiltinPropertyPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for BuiltinPropertyPlugin {
    fn id(&self) -> &'static str {
        "builtin_properties"
    }

    fn category(&self) -> PluginCategory {
        PluginCategory::Property
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl PropertyPlugin for BuiltinPropertyPlugin {
    fn register(&self, registry: &mut PropertyEvaluatorRegistry) {
        register_builtin_evaluators(registry);
    }
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
