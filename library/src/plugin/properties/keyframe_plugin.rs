use super::super::{Plugin, PropertyPlugin};
use crate::animation::EasingFunction;
use crate::model::frame::color::Color;
use crate::model::project::property::{Property, PropertyValue, Vec2, Vec3, Vec4};
use crate::plugin::{EvaluationContext, PropertyEvaluator};
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
    // debug!(
    //     "evaluate_keyframes for property {:?} at time {}",
    //     property, time
    // );

    if time.is_nan() || keyframes.is_empty() {
        return PropertyValue::Number(OrderedFloat(0.0));
    }

    // Keyframes might not be strictly sorted if modified recently?
    // Usually they should be, but let's be safe or just reference them.
    // For performance, we assume property.keyframes() returns a reference to a Vec.
    // If we can't assume sort, we must sort indices.
    // But evaluating every frame with a sort is slow.
    // Let's assume they are sorted for now, but handle the find safely.
    // Actually, `GraphEditor` sorts them manually before use. `TrackClip` doesn't enforce sort on add?
    // ProjectService::add_keyframe pushes and doesn't sort?
    // Let's check ProjectService. If it doesn't sort, we are in trouble.
    // But for now, let's just make this function safe.

    // We'll collect and sort references to be robust against unsorted input.
    // Note: This allocation is per-evaluation per-property. Ideally data is kept sorted.
    let mut sorted_refs: Vec<_> = keyframes.iter().collect();
    sorted_refs.sort_by(|a, b| a.time.cmp(&b.time));

    let first = sorted_refs[0];
    let last = sorted_refs[sorted_refs.len() - 1];

    if time <= *first.time {
        return first.value.clone();
    }
    if time >= *last.time {
        return last.value.clone();
    }

    // Find the keyframe before and after the current time
    // Now we are sure they exist because time is strictly between first and last.
    // We want the *last* keyframe <= time as 'current'.
    let current_idx = sorted_refs.iter().rposition(|k| *k.time <= time);

    let current = if let Some(idx) = current_idx {
        sorted_refs[idx]
    } else {
        // Should be impossible given checks above, but safe fallback
        first
    };

    // 'next' is the one immediately after
    let next = if let Some(idx) = current_idx {
        if idx + 1 < sorted_refs.len() {
            sorted_refs[idx + 1]
        } else {
            last
        }
    } else {
        // Fallback
        if sorted_refs.len() > 1 {
            sorted_refs[1]
        } else {
            last
        }
    };

    // Safety check for zero duration
    let duration = *next.time - *current.time;
    let t = if duration <= 1e-6 {
        0.0
    } else {
        (time - *current.time) / duration
    };

    // Get Interpolation Mode
    let mode = property
        .properties
        .get("interpolation")
        .and_then(|v| v.get_as::<String>())
        .unwrap_or_else(|| "linear".to_string());

    let interpolated =
        interpolate_property_values(&current.value, &next.value, t, &current.easing, &mode);
    // debug!(
    //     "evaluate_keyframes: interpolated value {:?} for time {}",
    //     interpolated, time
    // );
    interpolated
}

fn interpolate_property_values(
    start: &PropertyValue,
    end: &PropertyValue,
    t: f64,
    easing: &EasingFunction,
    mode: &str,
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
        (PropertyValue::Color(start_color), PropertyValue::Color(end_color)) => {
            if mode == "hsv" {
                interpolate_color_hsv(start_color, end_color, t)
            } else {
                interpolate_color_rgb(start_color, end_color, t)
            }
        }
        (PropertyValue::Array(s), PropertyValue::Array(e)) => PropertyValue::Array(
            s.iter()
                .zip(e.iter())
                .map(|(start, end)| interpolate_property_values(start, end, t, easing, mode))
                .collect(),
        ),
        (PropertyValue::Map(s), PropertyValue::Map(e)) => PropertyValue::Map(
            s.iter()
                .zip(e.iter())
                .map(|((k, sv), (_, ev))| {
                    (
                        k.clone(),
                        interpolate_property_values(sv, ev, t, easing, mode),
                    )
                })
                .collect(),
        ),
        _ => start.clone(),
    }
}

fn interpolate_color_rgb(start: &Color, end: &Color, t: f64) -> PropertyValue {
    PropertyValue::Color(Color {
        r: ((start.r as f64) + (end.r as f64 - start.r as f64) * t).round() as u8,
        g: ((start.g as f64) + (end.g as f64 - start.g as f64) * t).round() as u8,
        b: ((start.b as f64) + (end.b as f64 - start.b as f64) * t).round() as u8,
        a: ((start.a as f64) + (end.a as f64 - start.a as f64) * t).round() as u8,
    })
}

fn interpolate_color_hsv(start: &Color, end: &Color, t: f64) -> PropertyValue {
    let (h1, s1, v1) = rgb_to_hsv(start.r, start.g, start.b);
    let (h2, s2, v2) = rgb_to_hsv(end.r, end.g, end.b);

    // Interpolate H (shortest path)
    let mut diff = h2 - h1;
    if diff > 180.0 {
        diff -= 360.0;
    } else if diff < -180.0 {
        diff += 360.0;
    }
    let h = (h1 + diff * t).rem_euclid(360.0);

    let s = s1 + (s2 - s1) * t;
    let v = v1 + (v2 - v1) * t;

    // Interpolate Alpha usually linear
    let a = (start.a as f64) + (end.a as f64 - start.a as f64) * t;

    let (r, g, b) = hsv_to_rgb(h, s, v);

    PropertyValue::Color(Color {
        r: r as u8,
        g: g as u8,
        b: b as u8,
        a: a.round() as u8,
    })
}

fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let r = r as f64 / 255.0;
    let g = g as f64 / 255.0;
    let b = b as f64 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let h = if delta == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };

    let s = if max == 0.0 { 0.0 } else { delta / max };
    let v = max;

    // h in [0, 360] (can be negative before rem_euclid but here usually result of % 6.0 logic)
    // Actually the % 6.0 logic can yield negative?
    // ((g - b) / delta) if b > g is negative.
    // % 6.0 keeps sign.
    // So h can be negative.
    let h = (h + 360.0) % 360.0;

    (h, s, v)
}

fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (r_prime, g_prime, b_prime) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    (
        ((r_prime + m) * 255.0).round() as u8,
        ((g_prime + m) * 255.0).round() as u8,
        ((b_prime + m) * 255.0).round() as u8,
    )
}
