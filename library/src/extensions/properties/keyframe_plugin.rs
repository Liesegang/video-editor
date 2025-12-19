use crate::extensions::traits::{Plugin, PropertyPlugin};
use crate::animation::EasingFunction;
use crate::core::frame::color::Color;
use crate::core::model::property::{Property, PropertyValue, Vec2, Vec3, Vec4};
use crate::extensions::traits::{EvaluationContext, PropertyEvaluator};
use log::debug;
use ordered_float::OrderedFloat;
use std::sync::Arc;

pub struct KeyframePropertyPlugin;

impl KeyframePropertyPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for KeyframePropertyPlugin {
    fn id(&self) -> &'static str {
        "keyframe"
    }

    fn name(&self) -> String {
        "Keyframe Property".to_string()
    }

    fn category(&self) -> String {
        "Property".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl PropertyPlugin for KeyframePropertyPlugin {
    fn get_evaluator_instance(&self) -> Arc<dyn PropertyEvaluator> {
        Arc::new(KeyframeEvaluator)
    }
}

pub struct KeyframeEvaluator;

impl PropertyEvaluator for KeyframeEvaluator {
    fn evaluate(&self, property: &Property, time: f64, _ctx: &EvaluationContext) -> PropertyValue {
        evaluate_keyframes(property, time)
    }
}

fn evaluate_keyframes(property: &Property, time: f64) -> PropertyValue {
    let keyframes = property.keyframes();
    debug!(
        "evaluate_keyframes for property {:?} at time {}",
        property, time
    );

    if keyframes.is_empty() {
        return PropertyValue::Number(OrderedFloat(0.0));
    }

    if time <= *keyframes[0].time {
        return keyframes[0].value.clone();
    }
    if time >= *keyframes.last().unwrap().time {
        return keyframes.last().unwrap().value.clone();
    }

    // Find the keyframe before and after the current time
    let current = keyframes.iter().rev().find(|k| *k.time <= time).unwrap();
    let next = keyframes.iter().find(|k| *k.time > time).unwrap();
    let t = (time - *current.time) / (*next.time - *current.time);
    let interpolated = interpolate_property_values(&current.value, &next.value, t, &current.easing);
    debug!(
        "evaluate_keyframes: interpolated value {:?} for time {}",
        interpolated, time
    );
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
            PropertyValue::Number(OrderedFloat(s.0 + (e.0 - s.0) * t))
        }
        (
            PropertyValue::Vec2(Vec2 { x: sx, y: sy }),
            PropertyValue::Vec2(Vec2 { x: ex, y: ey }),
        ) => PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(sx.0 + (ex.0 - sx.0) * t),
            y: OrderedFloat(sy.0 + (ey.0 - sy.0) * t),
        }),
        (
            PropertyValue::Vec3(Vec3 {
                x: sx,
                y: sy,
                z: sz,
            }),
            PropertyValue::Vec3(Vec3 {
                x: ex,
                y: ey,
                z: ez,
            }),
        ) => PropertyValue::Vec3(Vec3 {
            x: OrderedFloat(sx.0 + (ex.0 - sx.0) * t),
            y: OrderedFloat(sy.0 + (ey.0 - sy.0) * t),
            z: OrderedFloat(sz.0 + (ez.0 - sz.0) * t),
        }),
        (
            PropertyValue::Vec4(Vec4 {
                x: sx,
                y: sy,
                z: sz,
                w: sw,
            }),
            PropertyValue::Vec4(Vec4 {
                x: ex,
                y: ey,
                z: ez,
                w: ew,
            }),
        ) => PropertyValue::Vec4(Vec4 {
            x: OrderedFloat(sx.0 + (ex.0 - sx.0) * t),
            y: OrderedFloat(sy.0 + (ey.0 - sy.0) * t),
            z: OrderedFloat(sz.0 + (ez.0 - sz.0) * t),
            w: OrderedFloat(sw.0 + (ew.0 - sw.0) * t),
        }),
        (
            PropertyValue::Color(Color {
                r: sr,
                g: sg,
                b: sb,
                a: sa,
            }),
            PropertyValue::Color(Color {
                r: er,
                g: eg,
                b: eb,
                a: ea,
            }),
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
