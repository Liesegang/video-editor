//! Pixel oracles for the shaped Text/Ensemble rendering boundary.
//!
//! Enabling Ensemble must not change shaping, fallback, line layout, or style
//! rendering while every evaluated element transform is neutral.

use std::collections::HashMap;

use super::vector_layer_native::{renderer, working_pixels};
use super::*;
use crate::core::ensemble::TransformData;
use crate::core::ensemble::effectors::OpacityMode;
use crate::core::ensemble::target::EffectorTarget;
use crate::core::ensemble::types::{EffectorConfig, EnsembleData};
use crate::model::frame::draw_type::{CapType, JoinType};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 220;

fn style(style: DrawStyle) -> StyleConfig {
    StyleConfig {
        id: Uuid::new_v4(),
        style,
    }
}

fn fill() -> StyleConfig {
    style(DrawStyle::Fill {
        color: Color {
            r: 232,
            g: 171,
            b: 61,
            a: 213,
        },
        offset: 0.0,
    })
}

fn neutral_ensemble() -> EnsembleData {
    EnsembleData {
        enabled: true,
        effector_configs: Vec::new(),
        decorator_configs: Vec::new(),
        patches: HashMap::new(),
    }
}

fn render_raster(
    text: &str,
    styles: &[StyleConfig],
    ensemble: Option<&EnsembleData>,
    transform: Affine2D,
) -> Vec<[f32; 4]> {
    let mut renderer = renderer("neutral-ensemble-shaped-text", WIDTH, HEIGHT);
    let output = renderer
        .rasterize_text_layer(TextRasterRequest {
            text,
            size: 42.0,
            font_name: "Arial",
            styles,
            ensemble,
            transform,
            current_time: 0.0,
        })
        .expect("rasterize shaped text");
    let RenderOutput::Working(output) = output else {
        panic!("Project-linear Text must retain working pixels");
    };
    output.pixels().pixels().to_vec()
}

fn render_paragraph_reference(
    text: &str,
    styles: &[StyleConfig],
    transform: Affine2D,
) -> Vec<[f32; 4]> {
    let mut renderer = renderer("original-paragraph-shaped-text", WIDTH, HEIGHT);
    let mut surface = renderer
        .create_layer_surface(WIDTH, HEIGHT)
        .expect("Paragraph reference surface");
    surface.canvas().clear(skia_safe::Color::TRANSPARENT);
    let canvas = surface.canvas();
    canvas.save();
    canvas.concat(
        &crate::core::rendering::skia_renderer::output_compositing::build_transform_matrix(
            &transform,
        ),
    );
    for config in styles {
        if config.style.composite_phase()
            != crate::core::rendering::skia_renderer::layer_styles::CompositePhase::Body
        {
            continue;
        }
        let paint = crate::core::rendering::skia_renderer::paint::PaintFactory::new(
            &renderer.surface_contract,
        )
        .text_paint(&config.style, 1.0, None)
        .expect("Paragraph reference paint");
        let paragraph =
            crate::rendering::text_layout::build_text_paragraph(text, "Arial", 42.0, Some(&paint));
        paragraph.paint(canvas, (0.0, 0.0));
    }
    canvas.restore();
    let output = renderer
        .snapshot_surface(&mut surface, WIDTH, HEIGHT)
        .expect("snapshot Paragraph reference");
    let RenderOutput::Working(output) = output else {
        panic!("Paragraph reference must retain working pixels");
    };
    output.pixels().pixels().to_vec()
}

fn assert_pixels_exact(context: &str, plain: &[[f32; 4]], ensemble: &[[f32; 4]]) {
    assert_eq!(plain.len(), ensemble.len());
    if let Some((index, (plain, ensemble))) = plain
        .iter()
        .zip(ensemble)
        .enumerate()
        .find(|(_, (plain, ensemble))| plain != ensemble)
    {
        let x = index as u32 % WIDTH;
        let y = index as u32 / WIDTH;
        panic!(
            "{context}: neutral Ensemble changed pixel ({x}, {y}): plain={plain:?}, ensemble={ensemble:?}"
        );
    }
}

fn alpha_bounds(pixels: &[[f32; 4]]) -> (u32, u32, u32, u32) {
    let mut left = WIDTH;
    let mut top = HEIGHT;
    let mut right = 0;
    let mut bottom = 0;
    for (index, pixel) in pixels.iter().enumerate() {
        if pixel[3] <= 0.001 {
            continue;
        }
        let x = index as u32 % WIDTH;
        let y = index as u32 / WIDTH;
        left = left.min(x);
        top = top.min(y);
        right = right.max(x + 1);
        bottom = bottom.max(y + 1);
    }
    assert!(
        left < right && top < bottom,
        "expected non-empty alpha bounds"
    );
    (left, top, right, bottom)
}

fn assert_neutral_raster(text: &str, context: &str) {
    let styles = [fill()];
    let neutral = neutral_ensemble();
    let transform = Affine2D::translate(34.0, 48.0);
    let reference = render_paragraph_reference(text, &styles, transform);
    let plain = render_raster(text, &styles, None, transform);
    let ensemble = render_raster(text, &styles, Some(&neutral), transform);
    assert_pixels_exact(&format!("{context} Paragraph oracle"), &reference, &plain);
    assert_pixels_exact(context, &plain, &ensemble);
}

#[test]
fn neutral_ensemble_preserves_latin_kerning_and_ligatures() {
    assert_neutral_raster("AVATAR office affine ffi", "Latin kerning/ligatures");
}

#[test]
fn neutral_ensemble_preserves_japanese_and_font_fallback() {
    assert_neutral_raster("日本語 かな カナ ABC", "Japanese/fallback shaping");
}

#[test]
fn neutral_ensemble_preserves_combining_marks_and_emoji_clusters() {
    assert_neutral_raster(
        "Cafe\u{301} nai\u{308}ve 👩‍💻 👍🏽",
        "combining marks/emoji clusters",
    );
}

#[test]
fn neutral_ensemble_preserves_rtl_and_multiline_layout() {
    assert_neutral_raster("مرحبا بالعالم\nשלום עולם", "RTL/multiline shaping");
}

#[test]
fn disabled_ensemble_is_exactly_the_plain_text_path() {
    let styles = [fill()];
    let disabled = EnsembleData {
        enabled: false,
        effector_configs: vec![EffectorConfig::Transform {
            translate: (87.0, -43.0),
            rotate: 21.0,
            scale: (1.4, 0.6),
            target: EffectorTarget::Block,
        }],
        decorator_configs: Vec::new(),
        patches: HashMap::from([(
            0,
            TransformData {
                translate: (100.0, 100.0),
                ..TransformData::identity()
            },
        )]),
    };
    let transform = Affine2D::translate(34.0, 48.0);
    let plain = render_raster("AV office", &styles, None, transform);
    let ensemble = render_raster("AV office", &styles, Some(&disabled), transform);
    assert_pixels_exact("disabled Ensemble", &plain, &ensemble);
}

#[test]
fn explicit_identity_effector_is_exactly_neutral() {
    let styles = [fill()];
    let identity = EnsembleData {
        enabled: true,
        effector_configs: vec![EffectorConfig::Transform {
            translate: (0.0, 0.0),
            rotate: 0.0,
            scale: (1.0, 1.0),
            target: EffectorTarget::Block,
        }],
        decorator_configs: Vec::new(),
        patches: HashMap::new(),
    };
    let transform = Affine2D::translate(34.0, 48.0);
    let plain = render_raster("AV office", &styles, None, transform);
    let ensemble = render_raster("AV office", &styles, Some(&identity), transform);
    assert_pixels_exact("identity Transform effector", &plain, &ensemble);
}

#[test]
fn plain_fill_and_stroke_retain_the_original_paragraph_paint_oracle() {
    let styles = [
        fill(),
        style(DrawStyle::Stroke {
            color: Color {
                r: 35,
                g: 211,
                b: 184,
                a: 197,
            },
            width: 2.5,
            offset: 0.0,
            cap: CapType::Round,
            join: JoinType::Round,
            miter: 4.0,
            dash_array: Vec::new(),
            dash_offset: 0.0,
        }),
    ];
    let transform = Affine2D::translate(34.0, 48.0);
    let reference = render_paragraph_reference("AVATAR office ffi", &styles, transform);
    let plain = render_raster("AVATAR office ffi", &styles, None, transform);
    assert_pixels_exact("plain Fill/Stroke Paragraph oracle", &reference, &plain);
}

#[test]
fn plain_fill_and_stroke_match_paragraph_under_fractional_nonuniform_transform() {
    let styles = [
        fill(),
        style(DrawStyle::Stroke {
            color: Color {
                r: 35,
                g: 211,
                b: 184,
                a: 197,
            },
            width: 2.5,
            offset: 0.0,
            cap: CapType::Round,
            join: JoinType::Round,
            miter: 4.0,
            dash_array: Vec::new(),
            dash_offset: 0.0,
        }),
    ];
    let transform = Affine2D {
        scale_x: 1.23,
        skew_x: -0.17,
        translate_x: 47.375,
        skew_y: 0.11,
        scale_y: 0.68,
        translate_y: 63.625,
    };
    let text = "AVATAR office ffi";
    let reference = render_paragraph_reference(text, &styles, transform);
    let plain = render_raster(text, &styles, None, transform);
    assert_pixels_exact(
        "fractional nonuniform Fill/Stroke Paragraph oracle",
        &reference,
        &plain,
    );
}

#[test]
fn neutral_ensemble_preserves_fill_stroke_shadow_opacity_and_outer_transform() {
    let styles = [
        fill(),
        style(DrawStyle::Stroke {
            color: Color {
                r: 35,
                g: 211,
                b: 184,
                a: 197,
            },
            width: 2.5,
            offset: 0.0,
            cap: CapType::Round,
            join: JoinType::Round,
            miter: 4.0,
            dash_array: Vec::new(),
            dash_offset: 0.0,
        }),
        style(DrawStyle::DropShadow {
            color: Color {
                r: 195,
                g: 34,
                b: 91,
                a: 181,
            },
            opacity: 0.72,
            blend_mode: BlendMode::Multiply,
            angle: 23.0,
            distance: 8.0,
            spread: 0.1,
            size: 3.0,
        }),
    ];
    let neutral = neutral_ensemble();
    let transform = Affine2D {
        scale_x: 1.17,
        skew_x: -0.23,
        translate_x: 71.5,
        skew_y: 0.19,
        scale_y: 0.76,
        translate_y: 52.25,
    };
    let render = |ensemble| {
        let mut renderer = renderer("neutral-ensemble-styled-composition", WIDTH, HEIGHT);
        renderer
            .draw_text_layer(
                TextRasterRequest {
                    text: "AV office",
                    size: 42.0,
                    font_name: "Arial",
                    styles: &styles,
                    ensemble,
                    transform,
                    current_time: 0.0,
                },
                0.61,
                BlendMode::Screen,
            )
            .expect("draw styled Text");
        working_pixels(&mut renderer)
    };
    let plain = render(None);
    let ensemble = render(Some(&neutral));
    assert_pixels_exact("styled/composited neutral Ensemble", &plain, &ensemble);
}

#[test]
fn animated_patch_moves_only_its_target_without_repainting_untouched_glyphs() {
    let styles = [fill()];
    let neutral = neutral_ensemble();
    let animated = EnsembleData {
        patches: HashMap::from([(
            1,
            TransformData {
                translate: (70.0, 18.0),
                ..TransformData::identity()
            },
        )]),
        ..neutral.clone()
    };
    let transform = Affine2D::translate(80.0, 62.0);
    let baseline = render_raster("AV", &styles, Some(&neutral), transform);
    let moved = render_raster("AV", &styles, Some(&animated), transform);
    assert!(
        baseline != moved,
        "animated target must change rendered pixels"
    );

    // The first glyph lies completely before x=105 for this fixture. Its
    // shaped baseline pixels must remain byte-for-byte unchanged while only
    // the second source element is patched.
    for y in 0..HEIGHT as usize {
        for x in 0..105_usize {
            let index = y * WIDTH as usize + x;
            assert_eq!(
                baseline[index], moved[index],
                "patch for element 1 changed untouched element 0 at ({x}, {y})"
            );
        }
    }
}

#[test]
fn one_patch_moves_an_entire_authoritative_ligature_cluster() {
    // Font feature availability is platform-dependent. Require one of these
    // multi-grapheme candidates to resolve as a single Paragraph cluster,
    // then animate the cluster actually reported by the production shaper.
    let (text, _) = ["ffi", "ff", "\u{644}\u{627}"]
        .into_iter()
        .map(|text| {
            (
                text,
                crate::rendering::text_layout::layout_runtime_text_shape(text, "Arial", 42.0),
            )
        })
        .find(|(text, shape)| shape.elements.len() == 1 && shape.elements[0].source == *text)
        .expect("fixture font must provide at least one multi-grapheme ligature cluster");

    let styles = [fill()];
    let neutral = neutral_ensemble();
    let moved_ensemble = EnsembleData {
        patches: HashMap::from([(
            0,
            TransformData {
                translate: (64.0, 19.0),
                ..TransformData::identity()
            },
        )]),
        ..neutral.clone()
    };
    let transform = Affine2D::translate(80.0, 62.0);
    let baseline = render_raster(text, &styles, Some(&neutral), transform);
    let moved = render_raster(text, &styles, Some(&moved_ensemble), transform);
    let baseline_bounds = alpha_bounds(&baseline);
    let moved_bounds = alpha_bounds(&moved);
    assert_eq!(moved_bounds.0 as i64 - baseline_bounds.0 as i64, 64);
    assert_eq!(moved_bounds.1 as i64 - baseline_bounds.1 as i64, 19);
    assert_eq!(
        moved_bounds.2 - moved_bounds.0,
        baseline_bounds.2 - baseline_bounds.0
    );
    assert_eq!(
        moved_bounds.3 - moved_bounds.1,
        baseline_bounds.3 - baseline_bounds.1
    );
}

#[test]
fn empty_text_still_rejects_parts_target_before_drawing() {
    let styles = [fill()];
    let ensemble = EnsembleData {
        enabled: true,
        effector_configs: vec![EffectorConfig::Transform {
            translate: (0.0, 0.0),
            rotate: 0.0,
            scale: (1.0, 1.0),
            target: EffectorTarget::Parts,
        }],
        decorator_configs: Vec::new(),
        patches: HashMap::new(),
    };
    let mut renderer = renderer("empty-text-parts-target", WIDTH, HEIGHT);
    let error = renderer
        .rasterize_text_layer(TextRasterRequest {
            text: "",
            size: 42.0,
            font_name: "Arial",
            styles: &styles,
            ensemble: Some(&ensemble),
            transform: Affine2D::IDENTITY,
            current_time: 0.0,
        })
        .expect_err("Parts target must be rejected even when Text has no elements");
    assert!(
        matches!(error, LibraryError::Render(message) if message.contains("EffectorTarget::Parts"))
    );
}

#[test]
fn identity_opacity_effector_does_not_force_a_different_shaping_path() {
    let styles = [fill()];
    let ensemble = EnsembleData {
        enabled: true,
        effector_configs: vec![EffectorConfig::Opacity {
            target_opacity: 100.0,
            mode: OpacityMode::Multiply,
            target: EffectorTarget::Block,
        }],
        decorator_configs: Vec::new(),
        patches: HashMap::new(),
    };
    let transform = Affine2D::translate(34.0, 48.0);
    let plain = render_raster("AV office", &styles, None, transform);
    let ensemble = render_raster("AV office", &styles, Some(&ensemble), transform);
    assert_pixels_exact("identity Opacity effector", &plain, &ensemble);
}
