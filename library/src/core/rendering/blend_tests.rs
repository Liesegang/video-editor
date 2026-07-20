use crate::model::BlendMode;
use crate::model::frame::Image;
use crate::model::frame::color::Color;
use crate::rendering::renderer::{Affine2D, RenderOutput, Renderer};
use crate::rendering::skia_renderer::SkiaRenderer;
use sha2::{Digest, Sha256};

type Rgba = [f64; 4];
type Rgb = [f64; 3];

fn transparent() -> Color {
    Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    }
}

fn image(width: u32, height: u32, data: Vec<u8>) -> RenderOutput {
    RenderOutput::Image(Image::new(width, height, data))
}

fn render_layers(
    width: u32,
    height: u32,
    base: &RenderOutput,
    source: &RenderOutput,
    opacity: f64,
    mode: BlendMode,
) -> Image {
    let mut renderer = SkiaRenderer::new(width, height, transparent(), false, None, None).unwrap();
    renderer
        .draw_layer_affine_with_blend(base, &Affine2D::IDENTITY, 1.0, BlendMode::Normal)
        .unwrap();
    renderer
        .draw_layer_affine_with_blend(source, &Affine2D::IDENTITY, opacity, mode)
        .unwrap();
    let RenderOutput::Image(output) = renderer.finalize().unwrap() else {
        panic!("CPU renderer must finalize to an Image");
    };
    output
}

fn rgba8(pixel: [u8; 4]) -> Rgba {
    pixel.map(|channel| f64::from(channel) / 255.0)
}

fn to_rgba8(pixel: Rgba) -> [u8; 4] {
    pixel.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn map3(a: Rgb, b: Rgb, f: impl Fn(f64, f64) -> f64) -> Rgb {
    [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])]
}

fn lum(color: Rgb) -> f64 {
    0.3 * color[0] + 0.59 * color[1] + 0.11 * color[2]
}

fn sat(color: Rgb) -> f64 {
    color.into_iter().fold(f64::NEG_INFINITY, f64::max)
        - color.into_iter().fold(f64::INFINITY, f64::min)
}

fn clip_color(mut color: Rgb) -> Rgb {
    let luminance = lum(color);
    let minimum = color.into_iter().fold(f64::INFINITY, f64::min);
    let maximum = color.into_iter().fold(f64::NEG_INFINITY, f64::max);
    if minimum < 0.0 {
        for channel in &mut color {
            *channel = luminance + (*channel - luminance) * luminance / (luminance - minimum);
        }
    }
    if maximum > 1.0 {
        for channel in &mut color {
            *channel =
                luminance + (*channel - luminance) * (1.0 - luminance) / (maximum - luminance);
        }
    }
    color
}

fn set_lum(mut color: Rgb, luminance: f64) -> Rgb {
    let delta = luminance - lum(color);
    for channel in &mut color {
        *channel += delta;
    }
    clip_color(color)
}

fn set_sat(color: Rgb, saturation: f64) -> Rgb {
    let mut indices = [0, 1, 2];
    indices.sort_by(|left, right| color[*left].total_cmp(&color[*right]));
    let (minimum, middle, maximum) = (indices[0], indices[1], indices[2]);
    let mut result = [0.0; 3];
    if color[maximum] > color[minimum] {
        result[middle] =
            (color[middle] - color[minimum]) * saturation / (color[maximum] - color[minimum]);
        result[maximum] = saturation;
    }
    result[minimum] = 0.0;
    result
}

fn burn(base: f64, source: f64) -> f64 {
    if source <= 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - base) / source).min(1.0)
    }
}

fn dodge(base: f64, source: f64) -> f64 {
    if source >= 1.0 {
        1.0
    } else {
        (base / (1.0 - source)).min(1.0)
    }
}

fn soft_light(base: f64, source: f64) -> f64 {
    if source <= 0.5 {
        base - (1.0 - 2.0 * source) * base * (1.0 - base)
    } else {
        let d = if base <= 0.25 {
            ((16.0 * base - 12.0) * base + 4.0) * base
        } else {
            base.sqrt()
        };
        base + (2.0 * source - 1.0) * (d - base)
    }
}

fn blend_rgb(mode: BlendMode, base: Rgb, source: Rgb) -> Rgb {
    match mode {
        BlendMode::Normal | BlendMode::Dissolve => source,
        BlendMode::Darken => map3(base, source, f64::min),
        BlendMode::Multiply => map3(base, source, |base, source| base * source),
        BlendMode::ColorBurn => map3(base, source, burn),
        BlendMode::LinearBurn => map3(base, source, |base, source| (base + source - 1.0).max(0.0)),
        BlendMode::DarkerColor => {
            if base.into_iter().sum::<f64>() <= source.into_iter().sum::<f64>() {
                base
            } else {
                source
            }
        }
        BlendMode::Lighten => map3(base, source, f64::max),
        BlendMode::Screen => map3(base, source, |base, source| base + source - base * source),
        BlendMode::ColorDodge => map3(base, source, dodge),
        BlendMode::LinearDodge => map3(base, source, |base, source| (base + source).min(1.0)),
        BlendMode::LighterColor => {
            if base.into_iter().sum::<f64>() >= source.into_iter().sum::<f64>() {
                base
            } else {
                source
            }
        }
        BlendMode::Overlay => map3(base, source, |base, source| {
            if base <= 0.5 {
                2.0 * base * source
            } else {
                1.0 - 2.0 * (1.0 - base) * (1.0 - source)
            }
        }),
        BlendMode::SoftLight => map3(base, source, soft_light),
        BlendMode::HardLight => map3(base, source, |base, source| {
            if source <= 0.5 {
                2.0 * base * source
            } else {
                1.0 - 2.0 * (1.0 - base) * (1.0 - source)
            }
        }),
        BlendMode::VividLight => map3(base, source, |base, source| {
            if source <= 0.5 {
                burn(base, 2.0 * source)
            } else {
                dodge(base, 2.0 * source - 1.0)
            }
        }),
        BlendMode::LinearLight => map3(base, source, |base, source| {
            (base + 2.0 * source - 1.0).clamp(0.0, 1.0)
        }),
        BlendMode::PinLight => map3(base, source, |base, source| {
            if source <= 0.5 {
                base.min(2.0 * source)
            } else {
                base.max(2.0 * source - 1.0)
            }
        }),
        BlendMode::HardMix => map3(
            base,
            source,
            |base, source| {
                if base + source >= 1.0 { 1.0 } else { 0.0 }
            },
        ),
        BlendMode::Difference => map3(base, source, |base, source| (base - source).abs()),
        BlendMode::Exclusion => map3(base, source, |base, source| {
            base + source - 2.0 * base * source
        }),
        BlendMode::Subtract => map3(base, source, |base, source| (base - source).max(0.0)),
        BlendMode::Divide => map3(base, source, |base, source| {
            if source <= 0.0 {
                1.0
            } else {
                (base / source).min(1.0)
            }
        }),
        BlendMode::Hue => set_lum(set_sat(source, sat(base)), lum(base)),
        BlendMode::Saturation => set_lum(set_sat(base, sat(source)), lum(base)),
        BlendMode::Color => set_lum(source, lum(base)),
        BlendMode::Luminosity => set_lum(base, lum(source)),
        BlendMode::Behind | BlendMode::Clear => unreachable!("Porter-Duff special mode"),
    }
}

fn reference_source_over(mode: BlendMode, base: Rgba, mut source: Rgba, opacity: f64) -> Rgba {
    source[3] *= opacity;
    let base_rgb = [base[0], base[1], base[2]];
    let source_rgb = [source[0], source[1], source[2]];
    let base_alpha = base[3];
    let source_alpha = source[3];
    match mode {
        BlendMode::Behind => {
            let alpha = base_alpha + source_alpha * (1.0 - base_alpha);
            let premul = map3(base_rgb, source_rgb, |base, source| {
                base * base_alpha + source * source_alpha * (1.0 - base_alpha)
            });
            if alpha > 0.0 {
                [
                    premul[0] / alpha,
                    premul[1] / alpha,
                    premul[2] / alpha,
                    alpha,
                ]
            } else {
                [0.0; 4]
            }
        }
        BlendMode::Clear => {
            let alpha = base_alpha * (1.0 - source_alpha);
            if alpha > 0.0 {
                [base_rgb[0], base_rgb[1], base_rgb[2], alpha]
            } else {
                [0.0; 4]
            }
        }
        _ => {
            let blended = blend_rgb(mode, base_rgb, source_rgb);
            let alpha = source_alpha + base_alpha * (1.0 - source_alpha);
            let premul = [0, 1, 2].map(|index| {
                source_alpha
                    * ((1.0 - base_alpha) * source_rgb[index] + base_alpha * blended[index])
                    + (1.0 - source_alpha) * base_alpha * base_rgb[index]
            });
            if alpha > 0.0 {
                [
                    premul[0] / alpha,
                    premul[1] / alpha,
                    premul[2] / alpha,
                    alpha,
                ]
            } else {
                [0.0; 4]
            }
        }
    }
}

fn assert_pixel_near(actual: &[u8], expected: [u8; 4], tolerance: u8, mode: BlendMode) {
    for channel in 0..4 {
        assert!(
            actual[channel].abs_diff(expected[channel]) <= tolerance,
            "{} channel {channel}: actual {:?}, expected {:?}",
            mode.label(),
            &actual[..4],
            expected
        );
    }
}

#[test]
fn all_29_modes_render_deliberate_pixels_with_correct_straight_alpha_semantics() {
    let base: [u8; 4] = [47, 139, 221, 153];
    let source: [u8; 4] = [211, 67, 29, 179];
    let opacity = 0.73;
    for mode in BlendMode::ALL {
        let output = render_layers(
            1,
            1,
            &image(1, 1, base.to_vec()),
            &image(1, 1, source.to_vec()),
            opacity,
            mode,
        );
        if mode == BlendMode::Dissolve {
            let near = |expected: [u8; 4]| {
                output.data[..4]
                    .iter()
                    .zip(expected)
                    .all(|(actual, expected)| actual.abs_diff(expected) <= 3)
            };
            let base_or_opaque_source = near(base) || near([source[0], source[1], source[2], 255]);
            assert!(base_or_opaque_source, "Dissolve returned {:?}", output.data);
            continue;
        }
        let expected = to_rgba8(reference_source_over(
            mode,
            rgba8(base),
            rgba8(source),
            opacity,
        ));
        // Skia's 8-bit premul/unpremul path may round twice.
        assert_pixel_near(&output.data, expected, 3, mode);
    }
}

#[test]
fn dissolve_is_repeatable_spatial_and_tracks_opacity_without_frame_rng() {
    const SIDE: u32 = 64;
    let pixels = usize::try_from(SIDE * SIDE).unwrap();
    let base = image(SIDE, SIDE, [0, 0, 255, 255].repeat(pixels));
    let source = image(SIDE, SIDE, [255, 0, 0, 255].repeat(pixels));
    let count_red = |opacity| {
        let output = render_layers(SIDE, SIDE, &base, &source, opacity, BlendMode::Dissolve);
        let red = output
            .data
            .chunks_exact(4)
            .filter(|pixel| pixel[0] > 240 && pixel[2] < 15 && pixel[3] == 255)
            .count();
        (red, output.data)
    };
    let (red_25, bytes_25) = count_red(0.25);
    let (red_25_repeat, repeat_25) = count_red(0.25);
    let (red_50, _) = count_red(0.50);
    let (red_75, _) = count_red(0.75);
    assert_eq!(bytes_25, repeat_25);
    assert_eq!(red_25, red_25_repeat);
    assert!((820..=1_230).contains(&red_25), "25% coverage was {red_25}");
    assert!(
        (1_843..=2_253).contains(&red_50),
        "50% coverage was {red_50}"
    );
    assert!(
        (2_867..=3_277).contains(&red_75),
        "75% coverage was {red_75}"
    );
    assert!(red_25 < red_50 && red_50 < red_75);
}

#[test]
fn dissolve_transformed_cubic_sampling_outputs_clamped_straight_rgba() {
    let source = image(
        3,
        3,
        vec![
            255, 0, 240, 255, // opaque magenta
            0, 255, 20, 8, // almost-transparent green
            255, 220, 0, 255, // opaque yellow
            0, 10, 255, 4, // almost-transparent blue
            0, 0, 0, 0, // transparent hole
            250, 0, 255, 255, // opaque violet
            255, 0, 3, 255, // opaque red
            0, 0, 255, 0, // authored transparent blue, canonicalized by Image
            255, 255, 255, 255, // opaque white
        ],
    );
    let mut renderer = SkiaRenderer::new(8, 8, transparent(), false, None, None).unwrap();
    renderer
        .draw_layer_affine_with_blend(
            &source,
            &Affine2D {
                scale_x: 1.9,
                skew_x: 0.17,
                translate_x: 0.4,
                skew_y: -0.11,
                scale_y: 1.8,
                translate_y: 0.7,
            },
            0.64,
            BlendMode::Dissolve,
        )
        .unwrap();
    let RenderOutput::Image(output) = renderer.finalize().unwrap() else {
        panic!("CPU renderer must finalize to an Image");
    };
    assert!(output.data.chunks_exact(4).any(|pixel| pixel[3] == 255));
    for pixel in output.data.chunks_exact(4) {
        if pixel[3] == 0 {
            assert_eq!(pixel, [0, 0, 0, 0]);
        }
    }
    // Golden includes cubic overshoot-prone samples, geometric edge coverage,
    // and canonical straight-RGBA conversion after shader-side RGB clamping.
    let checksum = format!("{:x}", Sha256::digest(&output.data));
    assert_eq!(
        checksum,
        "f1a90855c6ee616f111bda4c37ce839ca7920b37bb01e620f12500703776bb4b"
    );
}

#[test]
fn clear_erases_only_covered_pixels_and_preserves_transparent_source_holes() {
    let base = image(3, 1, [20, 200, 40, 255].repeat(3));
    let source = image(3, 1, vec![255, 0, 0, 255, 0, 0, 0, 0, 255, 0, 0, 128]);
    let output = render_layers(3, 1, &base, &source, 1.0, BlendMode::Clear);
    assert_eq!(&output.data[0..4], &[0, 0, 0, 0]);
    assert_eq!(&output.data[4..8], &[20, 200, 40, 255]);
    assert_pixel_near(&output.data[8..12], [20, 200, 40, 127], 1, BlendMode::Clear);
}

#[test]
fn behind_places_partial_source_beneath_partial_destination() {
    let base = image(1, 1, vec![0, 0, 255, 128]);
    let source = image(1, 1, vec![255, 0, 0, 128]);
    let output = render_layers(1, 1, &base, &source, 1.0, BlendMode::Behind);
    assert_pixel_near(&output.data, [85, 0, 170, 192], 2, BlendMode::Behind);
}

#[test]
fn custom_formula_edges_distinguish_whole_color_and_hard_mix_threshold() {
    assert_eq!(
        blend_rgb(BlendMode::DarkerColor, [0.9, 0.0, 0.0], [0.4, 0.4, 0.4]),
        [0.9, 0.0, 0.0]
    );
    assert_eq!(
        blend_rgb(BlendMode::LighterColor, [0.9, 0.0, 0.0], [0.4, 0.4, 0.4]),
        [0.4, 0.4, 0.4]
    );
    assert_eq!(
        blend_rgb(BlendMode::HardMix, [0.49, 0.50, 0.51], [0.50; 3]),
        [0.0, 1.0, 1.0]
    );
}

#[test]
fn divide_defines_every_zero_denominator_as_white_including_zero_over_zero() {
    assert_eq!(
        blend_rgb(BlendMode::Divide, [0.0, 0.4, 1.0], [0.0, 0.2, 0.0]),
        [1.0, 1.0, 1.0]
    );
}
