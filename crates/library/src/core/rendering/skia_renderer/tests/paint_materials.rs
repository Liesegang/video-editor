//! Managed Paint pixel oracles shared by Shape and transformed Text.

use super::layer_styles::{
    alpha_bounds, opaque, pixel, point, render_shape, render_text_content, white_fill,
};
use super::*;
use crate::model::property::{
    ColorSpaceRef, ColorValue, GradientGeometry, GradientSpread, Paint, PatternKind, PatternValue,
};

fn horizontal_gradient() -> crate::model::property::GradientValue {
    gradient_value(
        GradientGeometry::Linear {
            start: point(0.0, 0.5),
            end: point(1.0, 0.5),
        },
        GradientSpread::Pad,
        &[(0.0, opaque(255, 0, 0)), (1.0, opaque(0, 0, 255))],
    )
}

#[test]
fn fill_and_stroke_share_managed_gradient_and_pattern_materials() {
    let gradient_fill = render_shape(vec![DrawStyle::Fill {
        paint: Paint::Gradient(horizontal_gradient()),
        opacity: 1.0,
        offset: 0.0,
    }]);
    let left = pixel(&gradient_fill, 22, 32);
    let right = pixel(&gradient_fill, 42, 32);
    assert!(
        left[0] > left[2],
        "Gradient Fill left edge is not red: {left:?}"
    );
    assert!(
        right[2] > right[0],
        "Gradient Fill right edge is not blue: {right:?}"
    );

    let gradient_stroke = render_shape(vec![DrawStyle::Stroke {
        paint: Paint::Gradient(horizontal_gradient()),
        opacity: 1.0,
        width: 4.0,
        offset: 0.0,
        cap: Default::default(),
        join: Default::default(),
        miter: 4.0,
        dash_array: Vec::new(),
        dash_offset: 0.0,
    }]);
    let left = pixel(&gradient_stroke, 22, 20);
    let right = pixel(&gradient_stroke, 42, 20);
    assert!(
        left[0] > left[2],
        "Gradient Stroke left edge is not red: {left:?}"
    );
    assert!(
        right[2] > right[0],
        "Gradient Stroke right edge is not blue: {right:?}"
    );

    let pattern = PatternValue::new(
        PatternKind::Stripes,
        managed_color(opaque(255, 255, 255)),
        managed_color(opaque(0, 0, 0)),
        point(8.0, 8.0),
        point(0.0, 0.0),
        0.0,
        0.5,
    )
    .expect("valid test Pattern");
    let pattern_fill = render_shape(vec![DrawStyle::Fill {
        paint: Paint::Pattern(pattern),
        opacity: 1.0,
        offset: 0.0,
    }]);
    assert!(pixel(&pattern_fill, 24, 32)[0] > 0.95);
    assert!(pixel(&pattern_fill, 30, 32)[0] < 0.01);
}

#[test]
fn managed_solid_keeps_extended_values_and_pattern_applies_opacity_once() {
    let authored = ColorValue::new(ColorSpaceRef::srgb(), [-0.2, 1.5, 0.3, 0.5])
        .expect("finite managed test color");
    let solid = render_shape(vec![DrawStyle::Fill {
        paint: Paint::Solid(authored),
        opacity: 0.8,
        offset: 0.0,
    }]);
    let actual = pixel(&solid, 32, 32);
    let converted = source_processor(SRGB_SPACE_ID)
        .transform_rgb([-0.2, 1.5, 0.3])
        .expect("test sRGB to working conversion");
    let alpha = 0.4_f32;
    assert_pixel_near(
        actual,
        [
            converted[0] as f32 * alpha,
            converted[1] as f32 * alpha,
            converted[2] as f32 * alpha,
            alpha,
        ],
    );

    let pattern = PatternValue::new(
        PatternKind::Stripes,
        managed_color(opaque(255, 255, 255)),
        managed_color(opaque(0, 0, 0)),
        point(8.0, 8.0),
        point(0.0, 0.0),
        0.0,
        0.5,
    )
    .expect("valid test Pattern");
    let pixels = render_shape(vec![DrawStyle::Fill {
        paint: Paint::Pattern(pattern),
        opacity: 0.5,
        offset: 0.0,
    }]);
    let foreground = pixel(&pixels, 24, 32);
    let background = pixel(&pixels, 30, 32);
    assert!(
        (foreground[3] - 0.5).abs() <= 0.01,
        "Pattern foreground applied opacity more than once: {foreground:?}"
    );
    assert!(
        (background[3] - 0.5).abs() <= 0.01,
        "Pattern background opacity differs from the foreground: {background:?}"
    );
}

#[test]
fn text_gradient_fill_uses_one_shared_ink_domain() {
    let pixels = render_text_content(
        vec![DrawStyle::Fill {
            paint: Paint::Gradient(horizontal_gradient()),
            opacity: 1.0,
            offset: 0.0,
        }],
        // Four narrow glyphs fit the bounded raster, so both normalized
        // endpoints are observable instead of clipping the blue half.
        "IIII",
        None,
    );
    let (left, top, right, bottom) = alpha_bounds(&pixels);
    let span = (right - left + 1).max(1);
    let average = |range: std::ops::RangeInclusive<usize>| {
        let mut total = [0.0_f32; 3];
        let mut count = 0_u32;
        for y in top..=bottom {
            for x in range.clone() {
                let sample = pixel(&pixels, x, y);
                if sample[3] > 0.1 {
                    for channel in 0..3 {
                        total[channel] += sample[channel] / sample[3];
                    }
                    count += 1;
                }
            }
        }
        assert!(count > 0, "Gradient Text sample region has no ink");
        total.map(|value| value / count as f32)
    };
    let left_color = average(left..=left + span / 5);
    let right_color = average(right - span / 5..=right);
    assert!(
        left_color[0] > left_color[2] * 1.5,
        "Text Gradient must start red once across the complete ink: {left_color:?}"
    );
    assert!(
        right_color[2] > right_color[0] * 1.5,
        "Text Gradient must end blue once across the complete ink: {right_color:?}"
    );
}

#[test]
fn ensemble_transforms_keep_fill_material_in_the_complete_text_domain() {
    use crate::core::ensemble::TransformData;

    let ensemble = crate::core::ensemble::EnsembleData {
        enabled: true,
        effector_configs: Vec::new(),
        decorator_configs: Vec::new(),
        patches: std::collections::HashMap::from([
            (
                0,
                TransformData {
                    translate: (-3.0, 4.0),
                    rotate: 20.0,
                    ..TransformData::identity()
                },
            ),
            (
                1,
                TransformData {
                    translate: (12.0, -3.0),
                    rotate: -25.0,
                    ..TransformData::identity()
                },
            ),
        ]),
    };
    let gradient = horizontal_gradient();
    let fill = render_text_content(
        vec![DrawStyle::Fill {
            paint: Paint::Gradient(gradient.clone()),
            opacity: 1.0,
            offset: 0.0,
        }],
        "II",
        Some(ensemble.clone()),
    );
    let overlay = render_text_content(
        vec![
            white_fill(),
            DrawStyle::GradientOverlay {
                gradient,
                opacity: 1.0,
                blend_mode: BlendMode::Normal,
            },
        ],
        "II",
        Some(ensemble.clone()),
    );
    assert_material_pixels_match("Gradient", &fill, &overlay);

    let pattern = PatternValue::new(
        PatternKind::Stripes,
        managed_color(opaque(255, 255, 255)),
        managed_color(opaque(0, 0, 0)),
        point(11.0, 11.0),
        point(2.0, 1.0),
        17.0,
        0.55,
    )
    .expect("valid transformed Text Pattern");
    let fill = render_text_content(
        vec![DrawStyle::Fill {
            paint: Paint::Pattern(pattern.clone()),
            opacity: 1.0,
            offset: 0.0,
        }],
        "II",
        Some(ensemble.clone()),
    );
    let overlay = render_text_content(
        vec![
            white_fill(),
            DrawStyle::PatternOverlay {
                pattern,
                opacity: 1.0,
                blend_mode: BlendMode::Normal,
            },
        ],
        "II",
        Some(ensemble),
    );
    assert_material_pixels_match("Pattern", &fill, &overlay);
}

fn assert_material_pixels_match(label: &str, fill: &[[f32; 4]], overlay: &[[f32; 4]]) {
    let mut compared = 0;
    for (index, (fill, overlay)) in fill.iter().zip(overlay).enumerate() {
        if fill[3].min(overlay[3]) < 0.999 {
            continue;
        }
        compared += 1;
        for channel in 0..4 {
            assert!(
                (fill[channel] - overlay[channel]).abs() <= 0.015,
                "{label} Fill left the object-space Image overlay oracle at pixel {index}, channel {channel}: fill={fill:?}, overlay={overlay:?}"
            );
        }
    }
    assert!(
        compared >= 8,
        "{label} transformed Text oracle had only {compared} opaque comparison pixels"
    );
}
