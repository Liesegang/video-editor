//! CPU differential contracts for full-boundary and backend-native vectors.
//!
//! Native layers may use a physically cropped transient target, but their
//! target-space pixels must remain identical to the full-size raster boundary
//! retained for external Image Effects.

use super::vector_layer_native::{assert_pixels_near, renderer, working_pixels};
use super::*;
use crate::core::rendering::skia_renderer::vector_surface::VectorSurfaceMode;

const WIDTH: u32 = 96;
const HEIGHT: u32 = 72;

fn style(style: DrawStyle) -> StyleConfig {
    StyleConfig {
        id: Uuid::new_v4(),
        style,
    }
}

fn fill(color: Color) -> StyleConfig {
    style(DrawStyle::Fill { color, offset: 0.0 })
}

fn shadow() -> StyleConfig {
    style(DrawStyle::DropShadow {
        color: Color {
            r: 238,
            g: 42,
            b: 76,
            a: 191,
        },
        opacity: 0.68,
        blend_mode: BlendMode::Normal,
        angle: 17.0,
        distance: 7.0,
        spread: 0.15,
        size: 3.0,
    })
}

fn transparent() -> Color {
    Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    }
}

fn draw_boundary_shape(
    renderer: &mut SkiaRenderer,
    request: ShapeRasterRequest<'_>,
    opacity: f64,
    blend_mode: BlendMode,
) {
    let output = renderer
        .rasterize_shape_layer(request)
        .expect("full Shape raster boundary");
    renderer
        .draw_layer_affine_with_blend(&output, &Affine2D::IDENTITY, opacity, blend_mode)
        .expect("composite full Shape boundary");
}

fn draw_boundary_text(
    renderer: &mut SkiaRenderer,
    request: TextRasterRequest<'_>,
    opacity: f64,
    blend_mode: BlendMode,
) {
    let output = renderer
        .rasterize_text_layer(request)
        .expect("full Text raster boundary");
    renderer
        .draw_layer_affine_with_blend(&output, &Affine2D::IDENTITY, opacity, blend_mode)
        .expect("composite full Text boundary");
}

fn draw_fixed_backdrop(renderer: &mut SkiaRenderer) {
    let styles = [fill(Color {
        r: 47,
        g: 103,
        b: 211,
        a: 255,
    })];
    draw_boundary_shape(
        renderer,
        ShapeRasterRequest {
            path_data: "M 0 0 L 96 0 L 96 72 L 0 72 Z",
            canonical_path: None,
            parts: &[],
            styles: &styles,
            path_effects: &[],
            ensemble: None,
            transform: Affine2D::IDENTITY,
        },
        1.0,
        BlendMode::Normal,
    );
}

fn render_blend_scene(native: bool, blend_mode: BlendMode) -> Vec<[f32; 4]> {
    let mut renderer = renderer("native-crop-all-blends", WIDTH, HEIGHT);
    draw_fixed_backdrop(&mut renderer);

    let shape_styles = [
        fill(Color {
            r: 246,
            g: 181,
            b: 31,
            a: 173,
        }),
        shadow(),
    ];
    let shape = ShapeRasterRequest {
        path_data: "M -5 2 L 23 2 L 23 27 L -5 27 Z",
        canonical_path: None,
        parts: &[],
        styles: &shape_styles,
        path_effects: &[],
        ensemble: None,
        transform: Affine2D {
            scale_x: 1.1,
            skew_x: -0.3,
            translate_x: 17.25,
            skew_y: 0.42,
            scale_y: 0.7,
            translate_y: 7.75,
        },
    };
    if native {
        renderer
            .draw_shape_layer(shape, 0.63, blend_mode)
            .expect("native transformed Shape");
    } else {
        draw_boundary_shape(&mut renderer, shape, 0.63, blend_mode);
    }

    let text_styles = [fill(Color {
        r: 86,
        g: 236,
        b: 143,
        a: 187,
    })];
    let text = TextRasterRequest {
        text: "Crop",
        size: 19.0,
        font_name: "Arial",
        styles: &text_styles,
        ensemble: None,
        transform: Affine2D {
            scale_x: 0.82,
            skew_x: 0.17,
            translate_x: 44.5,
            skew_y: -0.11,
            scale_y: 1.24,
            translate_y: 29.25,
        },
        current_time: 0.0,
    };
    if native {
        renderer
            .draw_text_layer(text, 0.71, blend_mode)
            .expect("native transformed Text");
    } else {
        draw_boundary_text(&mut renderer, text, 0.71, blend_mode);
    }

    working_pixels(&mut renderer)
}

#[test]
fn cropped_native_vectors_match_full_boundary_for_the_complete_blend_catalog() {
    for blend_mode in BlendMode::ALL {
        let native = render_blend_scene(true, blend_mode);
        let boundary = render_blend_scene(false, blend_mode);
        if native.iter().zip(&boundary).any(|(native, boundary)| {
            native
                .iter()
                .zip(boundary)
                .any(|(native, boundary)| (native - boundary).abs() > 1.0e-6)
        }) {
            panic!("native vector pixels differ for {blend_mode:?}");
        }
        assert_pixels_near(&native, &boundary);

        if blend_mode == BlendMode::Dissolve {
            let backdrop = {
                let mut renderer = renderer("native-crop-dissolve-backdrop", WIDTH, HEIGHT);
                draw_fixed_backdrop(&mut renderer);
                working_pixels(&mut renderer)
            };
            assert!(
                native != backdrop,
                "Dissolve fixture must exercise source-coordinate noise"
            );
        }
    }
}

fn render_nested_group(native: bool) -> Vec<[f32; 4]> {
    let mut renderer = renderer("native-crop-nested-group", 80, 60);
    draw_fixed_backdrop(&mut renderer);
    renderer
        .begin_group(38, 30, &transparent())
        .expect("begin clipped nested target");
    let styles = [
        fill(Color {
            r: 228,
            g: 69,
            b: 188,
            a: 149,
        }),
        shadow(),
    ];
    let request = ShapeRasterRequest {
        path_data: "M 0 0 L 34 0 L 34 22 L 0 22 Z",
        canonical_path: None,
        parts: &[],
        styles: &styles,
        path_effects: &[],
        ensemble: None,
        transform: Affine2D {
            scale_x: 1.25,
            skew_x: -0.2,
            translate_x: -11.5,
            skew_y: 0.15,
            scale_y: 0.78,
            translate_y: 7.25,
        },
    };
    if native {
        renderer
            .draw_shape_layer(request, 0.77, BlendMode::Screen)
            .expect("native Shape in nested target");
    } else {
        draw_boundary_shape(&mut renderer, request, 0.77, BlendMode::Screen);
    }
    renderer
        .end_group_and_draw(&Affine2D::translate(22.0, 15.0), 0.74, BlendMode::Multiply)
        .expect("composite clipped nested target");
    working_pixels(&mut renderer)
}

#[test]
fn cropped_native_bounds_are_clipped_to_the_current_nested_group() {
    let native = render_nested_group(true);
    let boundary = render_nested_group(false);
    assert_pixels_near(&native, &boundary);
}

fn render_empty_and_offscreen(native: bool) -> Vec<[f32; 4]> {
    let mut renderer = renderer("native-crop-empty-offscreen", WIDTH, HEIGHT);
    draw_fixed_backdrop(&mut renderer);
    let styles = [fill(Color::white()), shadow()];
    let empty_text = TextRasterRequest {
        text: "",
        size: 24.0,
        font_name: "Arial",
        styles: &styles,
        ensemble: None,
        transform: Affine2D::translate(13.0, 9.0),
        current_time: 0.0,
    };
    if native {
        renderer
            .draw_text_layer(empty_text, 1.0, BlendMode::Normal)
            .expect("empty native Text");
    } else {
        draw_boundary_text(&mut renderer, empty_text, 1.0, BlendMode::Normal);
    }

    let offscreen_shape = ShapeRasterRequest {
        path_data: "M 0 0 L 12 0 L 12 12 L 0 12 Z",
        canonical_path: None,
        parts: &[],
        styles: &styles,
        path_effects: &[],
        ensemble: None,
        transform: Affine2D::translate(500.0, -400.0),
    };
    if native {
        renderer
            .draw_shape_layer(offscreen_shape, 1.0, BlendMode::Normal)
            .expect("offscreen native Shape");
    } else {
        draw_boundary_shape(&mut renderer, offscreen_shape, 1.0, BlendMode::Normal);
    }
    working_pixels(&mut renderer)
}

#[test]
fn empty_and_fully_offscreen_native_vectors_are_exact_no_ops() {
    let native = render_empty_and_offscreen(true);
    let boundary = render_empty_and_offscreen(false);
    assert_pixels_near(&native, &boundary);

    let mut backdrop = renderer("native-crop-no-op-backdrop", WIDTH, HEIGHT);
    draw_fixed_backdrop(&mut backdrop);
    assert_pixels_near(&native, &working_pixels(&mut backdrop));
}

#[test]
fn raster_boundaries_remain_full_target_images_for_external_effects() {
    let mut renderer = renderer("native-crop-full-effect-boundary", 73, 41);
    let styles = [fill(Color::white())];
    let shape = renderer
        .rasterize_shape_layer(ShapeRasterRequest {
            path_data: "M 1 1 L 5 1 L 5 4 L 1 4 Z",
            canonical_path: None,
            parts: &[],
            styles: &styles,
            path_effects: &[],
            ensemble: None,
            transform: Affine2D::translate(9.0, 7.0),
        })
        .expect("Shape Image Effect boundary");
    let text = renderer
        .rasterize_text_layer(TextRasterRequest {
            text: "Fx",
            size: 9.0,
            font_name: "Arial",
            styles: &styles,
            ensemble: None,
            transform: Affine2D::translate(2.0, 3.0),
            current_time: 0.0,
        })
        .expect("Text Image Effect boundary");
    for output in [shape, text] {
        let RenderOutput::Working(output) = output else {
            panic!("Project-linear raster boundary must retain working identity");
        };
        assert_eq!(
            (output.pixels().width(), output.pixels().height()),
            (73, 41)
        );
    }
}

fn render_grouped_path_backplate(native: bool) -> Vec<[f32; 4]> {
    use crate::core::ensemble::decorators::{BackplateShape, BackplateTarget};
    use crate::core::ensemble::types::DecoratorConfig;
    use crate::model::frame::entity::FramePathPart;
    use ordered_float::OrderedFloat;

    let mut renderer = renderer("native-crop-grouped-backplate", WIDTH, HEIGHT);
    let styles = [fill(Color::white())];
    let parts = [FramePathPart {
        path: "M 12 14 L 28 14 L 28 26 L 12 26 Z".to_string(),
        canonical_path: None,
        opacity: OrderedFloat(1.0),
    }];
    let ensemble = crate::core::ensemble::EnsembleData {
        enabled: true,
        effector_configs: Vec::new(),
        decorator_configs: vec![DecoratorConfig::LegacyBackplate {
            target: BackplateTarget::Block,
            shape: BackplateShape::Rect,
            color: Color {
                r: 224,
                g: 32,
                b: 48,
                a: 255,
            },
            padding: (4.0, 4.0, 4.0, 4.0),
            corner_radius: 0.0,
        }],
        patches: std::collections::HashMap::new(),
    };
    let request = ShapeRasterRequest {
        // This presentation fallback is deliberately far from the
        // authoritative grouped part and must not place its Backplate.
        path_data: "M 65 48 L 80 48 L 80 62 L 65 62 Z",
        canonical_path: None,
        parts: &parts,
        styles: &styles,
        path_effects: &[],
        ensemble: Some(&ensemble),
        transform: Affine2D::translate(15.0, 8.0),
    };
    if native {
        renderer
            .draw_shape_layer(request, 1.0, BlendMode::Normal)
            .expect("cropped grouped Shape");
    } else {
        draw_boundary_shape(&mut renderer, request, 1.0, BlendMode::Normal);
    }
    working_pixels(&mut renderer)
}

#[test]
fn grouped_path_backplate_uses_authoritative_parts_bounds_when_cropped() {
    let native = render_grouped_path_backplate(true);
    let boundary = render_grouped_path_backplate(false);
    assert_pixels_near(&native, &boundary);

    let expected_backplate = 28 * WIDTH as usize + 25;
    assert!(
        native[expected_backplate][0] > 0.5 && native[expected_backplate][1] < 0.1,
        "Backplate must surround the authoritative part"
    );
    let stale_aggregate = 63 * WIDTH as usize + 78;
    assert_eq!(
        native[stale_aggregate],
        [0.0, 0.0, 0.0, 1.0],
        "stale aggregate fallback must not place the grouped Shape Backplate"
    );
}

#[test]
fn real_vector_builders_allocate_tight_native_surfaces_and_full_raster_targets() {
    let mut renderer = renderer("native-crop-physical-allocation", 320, 180);
    let styles = [fill(Color::white()), shadow()];

    let shape_request = ShapeRasterRequest {
        path_data: "M 0 0 L 14 0 L 14 9 L 0 9 Z",
        canonical_path: None,
        parts: &[],
        styles: &styles,
        path_effects: &[],
        ensemble: None,
        transform: Affine2D::translate(91.0, 37.0),
    };
    let native_shape = renderer
        .create_shape_layer_surface(shape_request, VectorSurfaceMode::Content)
        .expect("tight Shape surface");
    let target_shape = renderer
        .create_shape_layer_surface(shape_request, VectorSurfaceMode::Target)
        .expect("full Shape surface");
    assert!(native_shape.origin.x > 0.0 && native_shape.origin.y > 0.0);
    assert!(native_shape.surface.width() < 100 && native_shape.surface.height() < 100);
    assert_eq!(
        (target_shape.surface.width(), target_shape.surface.height()),
        (320, 180)
    );

    let text_request = TextRasterRequest {
        text: "Tight",
        size: 16.0,
        font_name: "Arial",
        styles: &styles,
        ensemble: None,
        transform: Affine2D::translate(137.0, 69.0),
        current_time: 0.0,
    };
    let native_text = renderer
        .create_text_layer_surface(text_request, VectorSurfaceMode::Content)
        .expect("tight Text surface");
    let target_text = renderer
        .create_text_layer_surface(text_request, VectorSurfaceMode::Target)
        .expect("full Text surface");
    assert!(native_text.origin.x > 0.0 && native_text.origin.y > 0.0);
    assert!(native_text.surface.width() < 150 && native_text.surface.height() < 100);
    assert_eq!(
        (target_text.surface.width(), target_text.surface.height()),
        (320, 180)
    );

    let sksl_transform = Affine2D::translate(181.0, 83.0);
    let sksl_request = SkSLRasterRequest {
        shader_code: "half4 main(float2 position) { return half4(0.4, 0.2, 0.1, 1.0); }",
        resolution: (11.0, 7.0),
        time: 0.0,
        transform: &sksl_transform,
        color_domain: SkSLColorDomain::ProjectWorkingLinear,
    };
    let native_sksl = renderer
        .create_sksl_layer_surface(sksl_request, VectorSurfaceMode::Content)
        .expect("tight SkSL surface");
    let target_sksl = renderer
        .create_sksl_layer_surface(sksl_request, VectorSurfaceMode::Target)
        .expect("full SkSL surface");
    assert!(native_sksl.origin.x > 0.0 && native_sksl.origin.y > 0.0);
    assert!(native_sksl.surface.width() < 20 && native_sksl.surface.height() < 20);
    assert_eq!(
        (target_sksl.surface.width(), target_sksl.surface.height()),
        (320, 180)
    );
}
