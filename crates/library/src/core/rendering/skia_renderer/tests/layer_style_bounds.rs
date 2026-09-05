use super::*;
use crate::model::frame::draw_type::{GradientStyle, GradientStyleStop};
use crate::model::property::{GradientGeometry, GradientSpread, Vec2};
use ordered_float::OrderedFloat;

const SMALL: (u32, u32) = (180, 96);
const LARGE: (u32, u32) = (300, 160);

fn point(x: f64, y: f64) -> Vec2 {
    Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    }
}

fn normalized_gradient() -> DrawStyle {
    gradient_overlay(GradientGeometry::Linear {
        start: point(0.0, 0.5),
        end: point(1.0, 0.5),
    })
}

fn gradient_overlay(geometry: GradientGeometry) -> DrawStyle {
    DrawStyle::GradientOverlay {
        gradient: GradientStyle {
            geometry,
            spread: GradientSpread::Pad,
            stops: vec![
                GradientStyleStop {
                    offset: OrderedFloat(0.0),
                    color: Color {
                        r: 255,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                },
                GradientStyleStop {
                    offset: OrderedFloat(1.0),
                    color: Color {
                        r: 0,
                        g: 0,
                        b: 255,
                        a: 255,
                    },
                },
            ],
        },
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
    }
}

fn stroke() -> DrawStyle {
    DrawStyle::Stroke {
        color: Color {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        width: 8.0,
        offset: 0.0,
        cap: Default::default(),
        join: Default::default(),
        miter: 4.0,
        dash_array: Vec::new(),
        dash_offset: 0.0,
    }
}

fn render_text_gradient(
    dimensions: (u32, u32),
    ensemble: Option<&crate::core::ensemble::EnsembleData>,
) -> Vec<[f32; 4]> {
    render_text_styles(dimensions, "MMMM", &[normalized_gradient()], ensemble)
}

fn render_text_styles(
    dimensions: (u32, u32),
    text: &str,
    draw_styles: &[DrawStyle],
    ensemble: Option<&crate::core::ensemble::EnsembleData>,
) -> Vec<[f32; 4]> {
    let mut renderer = SkiaRenderer::new(
        dimensions.0,
        dimensions.1,
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        },
        false,
        None,
        None,
    )
    .expect("CPU Skia renderer");
    renderer
        .use_project_linear_surface(working_contract("gradient-object-bounds"))
        .expect("Project working surface");
    let styles = draw_styles
        .iter()
        .cloned()
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
            ensemble,
            transform: Affine2D::translate(31.0, 23.0),
            current_time: 0.0,
        })
        .expect("render normalized Text Gradient Overlay");
    let RenderOutput::Working(output) = output else {
        panic!("Project-linear rasterizer must return working pixels");
    };
    output.pixels().pixels().to_vec()
}

fn render_shape_gradient(dimensions: (u32, u32)) -> Vec<[f32; 4]> {
    render_shape_styles(
        dimensions,
        "M 0 0 L 72 0 L 72 38 L 0 38 Z",
        &[normalized_gradient()],
    )
    .expect("render normalized Shape Gradient Overlay")
}

fn render_shape_styles(
    dimensions: (u32, u32),
    path_data: &str,
    draw_styles: &[DrawStyle],
) -> Result<Vec<[f32; 4]>, LibraryError> {
    let mut renderer = SkiaRenderer::new(
        dimensions.0,
        dimensions.1,
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        },
        false,
        None,
        None,
    )?;
    renderer.use_project_linear_surface(working_contract("shape-gradient-object-bounds"))?;
    let styles = draw_styles
        .iter()
        .cloned()
        .map(|style| StyleConfig {
            id: Uuid::new_v4(),
            style,
        })
        .collect::<Vec<_>>();
    let output = renderer.rasterize_shape_layer(ShapeRasterRequest {
        path_data,
        canonical_path: None,
        parts: &[],
        styles: &styles,
        path_effects: &[],
        ensemble: None,
        transform: Affine2D::translate(31.0, 23.0),
    })?;
    let RenderOutput::Working(output) = output else {
        return Err(LibraryError::Render(
            "Project-linear rasterizer must return working pixels".to_string(),
        ));
    };
    Ok(output.pixels().pixels().to_vec())
}

fn assert_composition_independent_body_gradient(
    small: &[[f32; 4]],
    large: &[[f32; 4]],
    label: &str,
) {
    let mut body_pixels = 0;
    let mut max_difference = 0.0_f32;
    let mut max_position = (0, 0);
    let mut body_left = SMALL.0 as usize;
    let mut body_right = 0;
    for y in 0..SMALL.1 as usize {
        for x in 0..SMALL.0 as usize {
            let left = small[y * SMALL.0 as usize + x];
            let right = large[y * LARGE.0 as usize + x];
            if left[3] <= 0.25 && right[3] <= 0.25 {
                continue;
            }
            body_pixels += 1;
            body_left = body_left.min(x);
            body_right = body_right.max(x);
            for channel in 0..4 {
                let difference = (left[channel] - right[channel]).abs();
                if difference > max_difference {
                    max_difference = difference;
                    max_position = (x, y);
                }
            }
        }
    }
    assert!(
        body_pixels > 100,
        "{label} did not render enough off-origin glyph body pixels: {body_pixels}"
    );
    assert!(
        body_left > 0 && body_right + 1 < SMALL.0 as usize,
        "{label} body must be off-origin and unclipped: x={body_left}..={body_right}"
    );
    assert!(
        max_difference <= 2.0e-3,
        "{label} normalized Gradient Overlay changed with Composition dimensions; \
         max difference {max_difference} at {max_position:?}"
    );
}

#[test]
fn plain_text_gradient_overlay_uses_object_body_bounds() {
    let small = render_text_gradient(SMALL, None);
    let large = render_text_gradient(LARGE, None);
    assert_composition_independent_body_gradient(&small, &large, "plain Text");
}

#[test]
fn ensemble_text_gradient_overlay_uses_object_body_bounds() {
    let mut patches = std::collections::HashMap::new();
    patches.insert(
        0,
        crate::core::ensemble::TransformData {
            translate: (7.0, 3.0),
            rotate: 13.0,
            scale: (0.55, 1.2),
            opacity: 0.8,
            color_override: None,
        },
    );
    let ensemble = crate::core::ensemble::EnsembleData {
        enabled: true,
        effector_configs: Vec::new(),
        decorator_configs: Vec::new(),
        patches,
    };
    let small = render_text_gradient(SMALL, Some(&ensemble));
    let large = render_text_gradient(LARGE, Some(&ensemble));
    assert_composition_independent_body_gradient(&small, &large, "Ensemble Text");
}

#[test]
fn shape_gradient_overlay_uses_object_body_bounds() {
    let small = render_shape_gradient(SMALL);
    let large = render_shape_gradient(LARGE);
    assert_composition_independent_body_gradient(&small, &large, "Shape");
}

#[test]
fn empty_and_whitespace_text_with_mask_styles_remain_transparent() {
    let shadow = DrawStyle::DropShadow {
        color: Color {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        },
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        angle: 45.0,
        distance: 20.0,
        spread: 0.5,
        size: 12.0,
    };
    let styles = [normalized_gradient(), shadow];

    for text in ["", "   "] {
        let pixels = render_text_styles(SMALL, text, &styles, None);
        assert!(
            pixels.iter().all(|pixel| pixel[3] == 0.0),
            "empty Text {text:?} with Gradient/Shadow must remain transparent"
        );
    }
}

#[test]
fn zero_axis_stroked_paths_accept_normalized_linear_and_radial_gradients() {
    let gradients = [
        (
            "linear",
            GradientGeometry::Linear {
                start: point(0.0, 0.5),
                end: point(1.0, 0.5),
            },
        ),
        (
            "radial",
            GradientGeometry::Radial {
                center: point(0.5, 0.5),
                radius: OrderedFloat(0.5),
            },
        ),
    ];
    let paths = [
        ("horizontal", "M 10 30 L 90 30"),
        ("vertical", "M 50 5 L 50 65"),
    ];

    for (axis, path) in paths {
        for (kind, geometry) in gradients {
            let pixels = render_shape_styles(SMALL, path, &[stroke(), gradient_overlay(geometry)])
                .unwrap_or_else(|error| {
                    panic!("{axis} Stroke with normalized {kind} Gradient failed: {error}")
                });
            assert!(
                pixels.iter().any(|pixel| pixel[3] > 0.25),
                "{axis} Stroke with normalized {kind} Gradient rendered no body pixels"
            );
            assert!(
                pixels.iter().any(|pixel| {
                    pixel[3] > 0.25
                        && ((pixel[0] - pixel[1]).abs() > 0.05
                            || (pixel[2] - pixel[1]).abs() > 0.05)
                }),
                "{axis} Stroke with normalized {kind} Gradient did not apply its colored overlay"
            );
        }
    }
}
