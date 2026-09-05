use super::*;
use crate::model::frame::draw_type::{
    BevelDirection, BevelStyle, BevelTechnique, GradientStyle, GradientStyleStop, PatternStyle,
};
use crate::model::frame::entity::FramePathPart;
use crate::model::property::{GradientGeometry, GradientSpread, PatternKind, Vec2};
use ordered_float::OrderedFloat;

const WIDTH: usize = 72;
const HEIGHT: usize = 72;

fn render_shape(styles: Vec<DrawStyle>) -> Vec<[f32; 4]> {
    render_shape_with_transform(styles, Affine2D::IDENTITY)
}

fn render_shape_with_transform(styles: Vec<DrawStyle>, transform: Affine2D) -> Vec<[f32; 4]> {
    render_shape_with_parts(styles, transform, &[])
}

fn render_shape_with_parts(
    styles: Vec<DrawStyle>,
    transform: Affine2D,
    parts: &[FramePathPart],
) -> Vec<[f32; 4]> {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer =
        SkiaRenderer::new(WIDTH as u32, HEIGHT as u32, transparent, false, None, None)
            .expect("CPU Skia renderer");
    renderer
        .use_project_linear_surface(working_contract("alpha-mask-layer-style"))
        .expect("Project working surface");
    let styles = styles
        .into_iter()
        .map(|style| StyleConfig {
            id: Uuid::new_v4(),
            style,
        })
        .collect::<Vec<_>>();
    let output = renderer
        .rasterize_shape_layer(ShapeRasterRequest {
            path_data: "M 20 20 L 44 20 L 44 44 L 20 44 Z",
            canonical_path: None,
            parts,
            styles: &styles,
            path_effects: &[],
            ensemble: None,
            transform,
        })
        .expect("render layer style");
    let RenderOutput::Working(output) = output else {
        panic!("Project-linear rasterizer must return working pixels");
    };
    output.pixels().pixels().to_vec()
}

fn path_part(path: &str, opacity: f32) -> FramePathPart {
    FramePathPart {
        path: path.to_string(),
        canonical_path: None,
        opacity: OrderedFloat(opacity),
    }
}

fn render_text(styles: Vec<DrawStyle>) -> Vec<[f32; 4]> {
    render_text_content(styles, "T", None)
}

fn render_text_with_ensemble(styles: Vec<DrawStyle>, ensemble_enabled: bool) -> Vec<[f32; 4]> {
    let ensemble = ensemble_enabled.then(|| crate::core::ensemble::EnsembleData {
        enabled: true,
        effector_configs: Vec::new(),
        decorator_configs: Vec::new(),
        patches: std::collections::HashMap::new(),
    });
    render_text_content(styles, "T", ensemble)
}

fn render_text_content(
    styles: Vec<DrawStyle>,
    text: &str,
    ensemble: Option<crate::core::ensemble::EnsembleData>,
) -> Vec<[f32; 4]> {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer =
        SkiaRenderer::new(WIDTH as u32, HEIGHT as u32, transparent, false, None, None)
            .expect("CPU Skia renderer");
    renderer
        .use_project_linear_surface(working_contract("alpha-mask-text-style"))
        .expect("Project working surface");
    let styles = styles
        .into_iter()
        .map(|style| StyleConfig {
            id: Uuid::new_v4(),
            style,
        })
        .collect::<Vec<_>>();
    let output = renderer
        .rasterize_text_layer(TextRasterRequest {
            text,
            size: 36.0,
            font_name: "Arial",
            styles: &styles,
            ensemble: ensemble.as_ref(),
            transform: Affine2D::translate(20.0, 16.0),
            current_time: 0.0,
        })
        .expect("render text layer style");
    let RenderOutput::Working(output) = output else {
        panic!("Project-linear rasterizer must return working pixels");
    };
    output.pixels().pixels().to_vec()
}

fn pixel(pixels: &[[f32; 4]], x: usize, y: usize) -> [f32; 4] {
    pixels[y * WIDTH + x]
}

fn non_transparent_count(pixels: &[[f32; 4]]) -> usize {
    pixels.iter().filter(|pixel| pixel[3] > 0.002).count()
}

fn alpha_bounds(pixels: &[[f32; 4]]) -> (usize, usize, usize, usize) {
    let mut left = WIDTH;
    let mut top = HEIGHT;
    let mut right = 0;
    let mut bottom = 0;
    for (index, pixel) in pixels.iter().enumerate() {
        if pixel[3] > 0.002 {
            let x = index % WIDTH;
            let y = index / WIDTH;
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }
    assert!(
        left <= right && top <= bottom,
        "render has no visible alpha"
    );
    (left, top, right, bottom)
}

fn white_fill() -> DrawStyle {
    DrawStyle::Fill {
        color: Color::white(),
        offset: 0.0,
    }
}

fn stroke(color: Color, width: f64) -> DrawStyle {
    DrawStyle::Stroke {
        color,
        width,
        offset: 0.0,
        cap: Default::default(),
        join: Default::default(),
        miter: 4.0,
        dash_array: Vec::new(),
        dash_offset: 0.0,
    }
}

fn point(x: f64, y: f64) -> Vec2 {
    Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    }
}

fn opaque(r: u8, g: u8, b: u8) -> Color {
    Color { r, g, b, a: 255 }
}

#[test]
fn color_gradient_and_pattern_overlays_render_real_masked_pixels() {
    let solid = render_shape(vec![DrawStyle::ColorOverlay {
        color: opaque(255, 0, 0),
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
    }]);
    let solid_center = pixel(&solid, 32, 32);
    assert!(solid_center[0] > 0.95 && solid_center[1] < 0.01);
    assert_eq!(pixel(&solid, 10, 10), [0.0; 4]);

    let gradient = render_shape(vec![DrawStyle::GradientOverlay {
        gradient: GradientStyle {
            geometry: GradientGeometry::Linear {
                start: point(20.0 / WIDTH as f64, 0.5),
                end: point(44.0 / WIDTH as f64, 0.5),
            },
            spread: GradientSpread::Pad,
            stops: vec![
                GradientStyleStop {
                    offset: OrderedFloat(0.0),
                    color: opaque(255, 0, 0),
                },
                GradientStyleStop {
                    offset: OrderedFloat(1.0),
                    color: opaque(0, 0, 255),
                },
            ],
        },
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
    }]);
    let left = pixel(&gradient, 22, 32);
    let right = pixel(&gradient, 42, 32);
    assert!(left[0] > left[2], "gradient left stop missing: {left:?}");
    assert!(
        right[2] > right[0],
        "gradient right stop missing: {right:?}"
    );
    assert_eq!(pixel(&gradient, 10, 10), [0.0; 4]);

    let pattern = render_shape(vec![DrawStyle::PatternOverlay {
        pattern: PatternStyle {
            kind: PatternKind::Stripes,
            foreground: opaque(255, 255, 255),
            background: opaque(0, 0, 0),
            scale: point(8.0, 8.0),
            phase: point(0.0, 0.0),
            angle: OrderedFloat(0.0),
            duty: OrderedFloat(0.5),
        },
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
    }]);
    let foreground = pixel(&pattern, 24, 32);
    let background = pixel(&pattern, 30, 32);
    assert!(
        foreground[0] > 0.95,
        "pattern foreground missing: {foreground:?}"
    );
    assert!(
        background[0] < 0.01,
        "pattern background missing: {background:?}"
    );
    assert_eq!(pixel(&pattern, 10, 10), [0.0; 4]);
}

fn red_shadow(distance: f64, size: f64) -> DrawStyle {
    DrawStyle::DropShadow {
        color: Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        },
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        angle: 180.0,
        distance,
        spread: 0.0,
        size,
    }
}

#[test]
fn drop_shadow_and_outer_glow_render_real_pixels_beyond_the_source_alpha() {
    let shadow = render_shape(vec![DrawStyle::DropShadow {
        color: Color::white(),
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        angle: 180.0,
        distance: 10.0,
        spread: 0.0,
        size: 6.0,
    }]);
    assert!(
        pixel(&shadow, 50, 32)[3] > 0.01,
        "translated shadow missing"
    );
    assert_eq!(pixel(&shadow, 8, 8), [0.0; 4]);

    let glow = render_shape(vec![DrawStyle::OuterGlow {
        color: Color::white(),
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        spread: 0.25,
        size: 8.0,
    }]);
    assert!(pixel(&glow, 16, 32)[3] > 0.01, "outer glow missing");
    assert!(pixel(&glow, 32, 32)[3] < 0.01, "outer glow leaked inside");
}

#[test]
fn inner_shadow_glow_and_satin_remain_clipped_to_the_source_alpha() {
    let styles = [
        (
            "inner shadow",
            DrawStyle::InnerShadow {
                color: Color::white(),
                opacity: 1.0,
                blend_mode: BlendMode::Normal,
                angle: 180.0,
                distance: 5.0,
                spread: 0.0,
                size: 6.0,
            },
        ),
        (
            "inner glow",
            DrawStyle::InnerGlow {
                color: Color::white(),
                opacity: 1.0,
                blend_mode: BlendMode::Normal,
                spread: 0.0,
                size: 8.0,
            },
        ),
        (
            "satin",
            DrawStyle::Satin {
                color: Color::white(),
                opacity: 1.0,
                blend_mode: BlendMode::Normal,
                angle: 45.0,
                distance: 5.0,
                size: 5.0,
                invert: false,
            },
        ),
    ];
    for (label, style) in styles {
        let pixels = render_shape(vec![style]);
        assert!(
            non_transparent_count(&pixels) > 0,
            "{label} rendered no pixels"
        );
        assert_eq!(pixel(&pixels, 16, 32), [0.0; 4], "{label} escaped mask");
        assert_eq!(pixel(&pixels, 48, 32), [0.0; 4], "{label} escaped mask");
    }
}

#[test]
fn bevel_emboss_produces_opposing_highlight_and_shadow_edges() {
    let pixels = render_shape(vec![DrawStyle::BevelEmboss {
        style: BevelStyle::InnerBevel,
        technique: BevelTechnique::Smooth,
        depth: 0.8,
        direction: BevelDirection::Up,
        size: 8.0,
        soften: 2.0,
        angle: 135.0,
        altitude: 45.0,
        highlight_color: Color::white(),
        highlight_opacity: 1.0,
        highlight_blend_mode: BlendMode::Normal,
        shadow_color: Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        },
        shadow_opacity: 1.0,
        shadow_blend_mode: BlendMode::Normal,
    }]);
    let colored = pixels.iter().filter(|pixel| pixel[3] > 0.01).count();
    assert!(colored > 0, "bevel rendered no alpha-mask lighting");
    assert_eq!(pixel(&pixels, 10, 10), [0.0; 4]);
}

#[test]
fn derived_bevel_kernel_stays_within_its_reported_outset_for_every_technique() {
    let body_bounds = alpha_bounds(&render_shape(vec![white_fill()]));
    for technique in [
        BevelTechnique::Smooth,
        BevelTechnique::ChiselSoft,
        BevelTechnique::ChiselHard,
    ] {
        let bevel = DrawStyle::BevelEmboss {
            style: BevelStyle::OuterBevel,
            technique,
            depth: 0.0,
            direction: BevelDirection::Up,
            size: 12.0,
            soften: 0.0,
            angle: 135.0,
            altitude: 90.0,
            highlight_color: Color::white(),
            highlight_opacity: 1.0,
            highlight_blend_mode: BlendMode::Normal,
            shadow_color: opaque(255, 0, 0),
            shadow_opacity: 1.0,
            shadow_blend_mode: BlendMode::Normal,
        };
        let outset = bevel.visual_outset().ceil() as usize;
        assert!(outset > 0, "{technique:?} lost its derived kernel outset");
        let styled_bounds = alpha_bounds(&render_shape(vec![white_fill(), bevel]));
        assert!(
            styled_bounds.0 >= body_bounds.0.saturating_sub(outset)
                && styled_bounds.1 >= body_bounds.1.saturating_sub(outset)
                && styled_bounds.2 <= (body_bounds.2 + outset).min(WIDTH - 1)
                && styled_bounds.3 <= (body_bounds.3 + outset).min(HEIGHT - 1),
            "{technique:?} rendered outside reported outset: body={body_bounds:?}, styled={styled_bounds:?}, outset={outset}"
        );
    }
}

#[test]
fn text_uses_the_same_alpha_mask_style_renderer_as_shapes() {
    let fill = render_text(vec![white_fill()]);
    let styled = render_text(vec![
        DrawStyle::OuterGlow {
            color: Color::white(),
            opacity: 1.0,
            blend_mode: BlendMode::LinearDodge,
            spread: 0.2,
            size: 8.0,
        },
        white_fill(),
    ]);
    assert!(
        non_transparent_count(&styled) > non_transparent_count(&fill),
        "text outer glow did not expand the glyph alpha"
    );
}

#[test]
fn drop_shadow_is_always_composited_below_shape_content() {
    let fill_then_shadow = render_shape(vec![white_fill(), red_shadow(0.0, 0.0)]);
    let shadow_then_fill = render_shape(vec![red_shadow(0.0, 0.0), white_fill()]);
    assert_eq!(fill_then_shadow, shadow_then_fill);
    let center = pixel(&fill_then_shadow, 32, 32);
    assert!(center[0] > 0.95 && center[1] > 0.95 && center[2] > 0.95);

    let offset = render_shape(vec![white_fill(), red_shadow(10.0, 0.0)]);
    let cast_pixel = pixel(&offset, 50, 32);
    assert!(
        cast_pixel[0] > 0.8 && cast_pixel[1] < 0.05 && cast_pixel[2] < 0.05,
        "positive-distance shadow must remain visible beyond the shape body: {cast_pixel:?}"
    );
}

#[test]
fn mask_comes_from_composed_fill_and_stroke_alpha() {
    let transparent_fill = DrawStyle::Fill {
        color: Color {
            r: 255,
            g: 255,
            b: 255,
            a: 0,
        },
        offset: 0.0,
    };
    let transparent = render_shape(vec![transparent_fill, red_shadow(8.0, 0.0)]);
    assert_eq!(non_transparent_count(&transparent), 0);

    let hollow = render_shape(vec![stroke(Color::white(), 4.0), red_shadow(0.0, 0.0)]);
    assert_eq!(pixel(&hollow, 32, 32), [0.0; 4]);

    let offset_fill = DrawStyle::Fill {
        color: Color::white(),
        offset: 5.0,
    };
    let expanded = render_shape(vec![offset_fill, red_shadow(10.0, 0.0)]);
    assert!(
        pixel(&expanded, 57, 32)[3] > 0.9,
        "positive Fill offset did not expand the cast alpha"
    );

    let half_fill = DrawStyle::Fill {
        color: Color {
            r: 255,
            g: 255,
            b: 255,
            a: 128,
        },
        offset: 0.0,
    };
    let partial = render_shape(vec![half_fill, red_shadow(10.0, 0.0)]);
    assert!(
        (pixel(&partial, 50, 32)[3] - 128.0 / 255.0).abs() < 0.02,
        "shadow alpha did not preserve composed body opacity"
    );
}

#[test]
fn transformed_stroke_mask_preserves_its_hollow_center() {
    let transform = Affine2D::translate(-10.0, 4.0).compose(Affine2D::scale(1.5, 0.75));
    let pixels = render_shape_with_transform(
        vec![stroke(Color::white(), 4.0), red_shadow(0.0, 0.0)],
        transform,
    );
    let (x, y) = transform.map_point(32.0, 32.0);
    assert_eq!(pixel(&pixels, x as usize, y as usize), [0.0; 4]);
}

#[test]
fn off_origin_nonuniform_shape_shadow_preserves_unmasked_fill_and_stroke_silhouettes() {
    let transform = Affine2D::translate(-6.0, 18.0).compose(Affine2D::scale(1.35, 0.55));
    for (label, body) in [
        ("Fill", white_fill()),
        ("Stroke", stroke(Color::white(), 6.0)),
    ] {
        let baseline = render_shape_with_transform(vec![body.clone()], transform);
        let styled = render_shape_with_transform(vec![body, red_shadow(10.0, 2.0)], transform);
        let baseline_bounds = alpha_bounds(&baseline);
        let styled_bounds = alpha_bounds(&styled);
        assert!(
            styled_bounds.2 > baseline_bounds.2,
            "{label} shadow was clipped instead of extending its off-origin silhouette: baseline={baseline_bounds:?}, styled={styled_bounds:?}"
        );
        for (index, pixel) in baseline
            .iter()
            .enumerate()
            .filter(|(_, pixel)| pixel[3] > 0.99)
        {
            assert_eq!(
                styled[index], *pixel,
                "{label} shadow mask clipped or changed opaque body pixel {index}"
            );
        }
    }
}

#[test]
fn drop_shadow_is_always_composited_below_plain_and_ensemble_text() {
    for ensemble_enabled in [false, true] {
        let fill = render_text_with_ensemble(vec![white_fill()], ensemble_enabled);
        let styled =
            render_text_with_ensemble(vec![white_fill(), red_shadow(0.0, 0.0)], ensemble_enabled);
        let interior_index = fill
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| left[3].total_cmp(&right[3]))
            .map(|(index, _)| index)
            .expect("text render has pixels");
        assert!(fill[interior_index][3] > 0.9, "text has no opaque body");
        assert_eq!(
            styled[interior_index], fill[interior_index],
            "drop shadow covered the text body (ensemble={ensemble_enabled})"
        );
    }
}

#[test]
fn ensemble_character_shadow_cannot_cover_an_earlier_character() {
    use crate::core::ensemble::target::EffectorTarget;
    use crate::core::ensemble::types::EffectorConfig;

    let ensemble = || crate::core::ensemble::EnsembleData {
        enabled: true,
        effector_configs: vec![EffectorConfig::Tracking {
            amount: -4.0,
            target: EffectorTarget::Block,
        }],
        decorator_configs: Vec::new(),
        patches: std::collections::HashMap::new(),
    };
    let fill = render_text_content(vec![white_fill()], "TT", Some(ensemble()));
    let styled = render_text_content(
        vec![white_fill(), red_shadow(18.0, 0.0)],
        "TT",
        Some(ensemble()),
    );
    let opaque_body_indices = fill
        .iter()
        .enumerate()
        .filter_map(|(index, pixel)| (pixel[3] > 0.98).then_some(index))
        .collect::<Vec<_>>();
    assert!(!opaque_body_indices.is_empty(), "ensemble text has no body");
    for index in opaque_body_indices {
        assert_eq!(
            styled[index], fill[index],
            "a later character's offset shadow covered opaque text at pixel {index}"
        );
    }
}

#[test]
fn grouped_path_opacity_is_applied_once_before_one_shared_shadow_phase() {
    let parts = vec![
        path_part("M 12 20 L 36 20 L 36 44 L 12 44 Z", 1.0),
        path_part("M 38 20 L 62 20 L 62 44 L 38 44 Z", 0.5),
    ];
    let fill = render_shape_with_parts(vec![white_fill()], Affine2D::IDENTITY, &parts);
    let left_cast_shadow = DrawStyle::DropShadow {
        color: Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        },
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        angle: 180.0,
        distance: 12.0,
        spread: 0.0,
        size: 0.0,
    };
    let styled = render_shape_with_parts(
        vec![white_fill(), left_cast_shadow],
        Affine2D::IDENTITY,
        &parts,
    );
    for (index, body) in fill.iter().enumerate().filter(|(_, pixel)| pixel[3] > 0.99) {
        assert_eq!(
            styled[index], *body,
            "the second part's shadow covered the first part body at pixel {index}"
        );
    }

    let partial = vec![path_part("M 20 20 L 44 20 L 44 44 L 20 44 Z", 0.5)];
    let cast = render_shape_with_parts(
        vec![white_fill(), red_shadow(10.0, 0.0)],
        Affine2D::IDENTITY,
        &partial,
    );
    assert!(
        (pixel(&cast, 50, 32)[3] - 0.5).abs() < 0.02,
        "part opacity was multiplied into both content alpha and shadow opacity"
    );
}

#[test]
fn overlapping_partial_parts_share_body_alpha_without_reapplying_paint_opacity() {
    let parts = [
        path_part("M 12 20 L 32 20 L 32 44 L 12 44 Z", 0.5),
        path_part("M 24 20 L 44 20 L 44 44 L 24 44 Z", 0.5),
    ];
    // Each part contains two opaque paints. Its 0.5 opacity must wrap their
    // complete body, rather than turn each paint into a separate 0.5 layer.
    let pixels = render_shape_with_parts(
        vec![white_fill(), white_fill(), red_shadow(36.0, 0.0)],
        Affine2D::IDENTITY,
        &parts,
    );
    for (x, expected) in [(18, 0.5), (28, 0.75), (54, 0.5), (64, 0.75)] {
        let actual = pixel(&pixels, x, 32)[3];
        assert!(
            (actual - expected).abs() < 2.0e-3,
            "grouped body/shadow opacity at x={x}: actual={actual}, expected={expected}"
        );
    }
    assert!(pixel(&pixels, 28, 32)[1] > 0.7, "the body remains white");
    assert!(
        pixel(&pixels, 64, 32)[1] < 1.0e-6,
        "the cast shadow remains red"
    );
}

#[test]
fn grouped_open_strokes_keep_zero_height_geometry_in_the_shared_mask_bounds() {
    let parts = vec![
        path_part("M 12 20 L 60 20", 1.0),
        path_part("M 12 44 L 60 44", 0.5),
    ];
    let rendered = render_shape_with_parts(
        vec![stroke(Color::white(), 4.0), red_shadow(0.0, 0.0)],
        Affine2D::IDENTITY,
        &parts,
    );

    assert!(
        pixel(&rendered, 32, 20)[3] > 0.98,
        "first zero-height path was dropped from grouped mask bounds"
    );
    assert!(
        (pixel(&rendered, 32, 44)[3] - 0.75).abs() < 0.03,
        "second zero-height path was dropped or lost its child opacity before shadow composition"
    );
}

#[test]
fn authored_order_remains_stable_within_the_same_compositing_phase() {
    let blue_shadow = DrawStyle::DropShadow {
        color: Color {
            r: 0,
            g: 0,
            b: 255,
            a: 255,
        },
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        angle: 0.0,
        distance: 0.0,
        spread: 0.0,
        size: 0.0,
    };
    let red_then_blue = render_shape(vec![red_shadow(0.0, 0.0), blue_shadow.clone()]);
    let blue_then_red = render_shape(vec![blue_shadow, red_shadow(0.0, 0.0)]);
    let first = pixel(&red_then_blue, 32, 32);
    let second = pixel(&blue_then_red, 32, 32);
    assert!(first[2] > 0.95 && first[0] < 0.05);
    assert!(second[0] > 0.95 && second[2] < 0.05);
}

#[test]
fn visual_outset_expands_only_styles_that_can_render_outside_the_silhouette() {
    let outer = DrawStyle::OuterGlow {
        color: Color::white(),
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        spread: 0.5,
        size: 8.0,
    };
    let inner = DrawStyle::InnerGlow {
        color: Color::white(),
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        spread: 0.5,
        size: 8.0,
    };
    assert_eq!(outer.visual_outset(), 12.0);
    assert_eq!(inner.visual_outset(), 0.0);
}
