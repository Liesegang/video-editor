use super::*;
use crate::model::property::{
    ColorValue, GradientGeometry, GradientSpread, GradientStop, GradientValue, Vec2,
};

pub(super) fn test_gradient(spread: GradientSpread, stops: &[(f64, Color)]) -> GradientValue {
    GradientValue::new(
        GradientGeometry::Linear {
            start: Vec2 {
                x: OrderedFloat(0.0),
                y: OrderedFloat(0.5),
            },
            end: Vec2 {
                x: OrderedFloat(1.0),
                y: OrderedFloat(0.5),
            },
        },
        spread,
        stops
            .iter()
            .map(|(offset, color)| {
                GradientStop::new(*offset, ColorValue::from_straight_srgba8(color)).unwrap()
            })
            .collect(),
    )
    .unwrap()
}

pub(super) fn test_random(seed: u32, serial: u32, channel: u32) -> f32 {
    fn hash(mut value: u32) -> u32 {
        value ^= value >> 16;
        value = value.wrapping_mul(0x7feb_352d);
        value ^= value >> 15;
        value = value.wrapping_mul(0x846c_a68b);
        value ^ (value >> 16)
    }
    let bits = hash(seed ^ hash(serial.wrapping_add(channel.wrapping_mul(0x9e37_79b9))));
    (bits & 0x00ff_ffff) as f32 / 16_777_216.0
}
