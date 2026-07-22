use super::{RenderOutput, Renderer, SkiaRenderer};
use crate::error::LibraryError;
use crate::model::BlendMode;
use crate::model::frame::Image;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::DrawStyle;
use crate::model::frame::entity::{SkSLColorDomain, StyleConfig};
use crate::rendering::renderer::{
    Affine2D, ShapeRasterRequest, SkSLRasterRequest, TextRasterRequest, TextureInfo,
    WorkingSurfaceContract,
};
use ruvie_color_management::{
    BuiltinColorTransform, ColorContext, ColorTransformBackend, ColorTransformRequest,
    LINEAR_SRGB_SPACE_ID, LinearWorkingImage, ManagedLinearWorkingImage, SRGB_SPACE_ID,
    VerifiedSourceSpace, WorkingColorIdentity,
};
use uuid::Uuid;

fn working_identity(config: &str) -> WorkingColorIdentity {
    let verified = BuiltinColorTransform
        .verify_working_space(LINEAR_SRGB_SPACE_ID, &ColorContext::default())
        .unwrap();
    WorkingColorIdentity::from_verified(config, verified).unwrap()
}

fn source_space(space: &str) -> VerifiedSourceSpace {
    BuiltinColorTransform
        .verify_source_space(space, &ColorContext::default())
        .unwrap()
}

fn source_processor(source: &str) -> Box<dyn ruvie_color_management::CpuColorProcessor> {
    BuiltinColorTransform
        .create_cpu_processor(&ColorTransformRequest::source_to_working(
            source,
            LINEAR_SRGB_SPACE_ID,
        ))
        .unwrap()
}

fn working_contract(config: &str) -> WorkingSurfaceContract {
    WorkingSurfaceContract::new(
        working_identity(config),
        source_space(SRGB_SPACE_ID),
        source_processor(SRGB_SPACE_ID),
    )
    .unwrap()
}

fn encoded_pixel(config: &str, rgba: [u8; 4]) -> ManagedLinearWorkingImage {
    ManagedLinearWorkingImage::solid_from_straight_rgba8(
        working_identity(config),
        &source_space(SRGB_SPACE_ID),
        1,
        1,
        rgba,
        source_processor(SRGB_SPACE_ID).as_ref(),
    )
    .unwrap()
}

fn working_pixel(config: &str, rgba: [f32; 4]) -> ManagedLinearWorkingImage {
    let pixels = LinearWorkingImage::from_premultiplied_rgba_f32(1, 1, vec![rgba]).unwrap();
    // SAFETY: This test fixture explicitly supplies premultiplied samples
    // in the same verified linear-sRGB identity installed on the renderer.
    unsafe {
        ManagedLinearWorkingImage::from_working_pixels_unchecked(working_identity(config), pixels)
    }
}

fn assert_pixel_near(actual: [f32; 4], expected: [f32; 4]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            (actual - expected).abs() <= 0.002,
            "actual {actual}, expected {expected} for {actual:?}"
        );
    }
}

#[test]
fn construction_returns_an_error_for_invalid_dimensions() {
    let result = SkiaRenderer::new(0, 0, Color::black(), false, None, None);
    assert!(matches!(result, Err(LibraryError::Render(_))));
}

#[test]
fn failed_render_target_replacement_preserves_the_current_surface() {
    let mut renderer = SkiaRenderer::new(2, 2, Color::black(), false, None, None).unwrap();
    let result = renderer.replace_render_target(None, Some(99), Some(77), |_| {
        Err(LibraryError::Render(
            "injected surface creation failure".to_string(),
        ))
    });

    assert!(matches!(result, Err(LibraryError::Render(_))));
    assert_eq!(renderer.sharing_handle, None);
    assert_eq!(renderer.sharing_hwnd, None);
    renderer.clear().unwrap();
    let RenderOutput::Image(image) = renderer.finalize().unwrap() else {
        panic!("CPU renderer must retain its image surface");
    };
    assert_eq!((image.width, image.height), (2, 2));
}

#[test]
fn project_surface_refuses_untyped_texture_and_rgba8_readback_boundaries() {
    let mut renderer = SkiaRenderer::new(1, 1, Color::black(), false, None, None).unwrap();
    renderer
        .use_project_linear_surface(working_contract("typed-boundary"))
        .unwrap();

    let texture_error = renderer.render_to_texture().unwrap_err().to_string();
    assert!(texture_error.contains("untyped GPU TextureInfo"));

    let output = renderer.finalize().unwrap();
    assert!(matches!(output, RenderOutput::Working(_)));
    let readback_error = renderer.read_surface(&output).unwrap_err().to_string();
    assert!(readback_error.contains("apply the Project terminal processor"));
}

#[test]
fn project_surface_rejects_every_untyped_or_foreign_color_bearing_input() {
    let mut renderer = SkiaRenderer::new(1, 1, Color::black(), false, None, None).unwrap();
    renderer
        .use_project_linear_surface(working_contract("destination"))
        .unwrap();

    let encoded = RenderOutput::Image(Image::new(1, 1, vec![255, 0, 0, 255]));
    assert!(
        renderer
            .draw_layer_affine_with_blend(&encoded, &Affine2D::IDENTITY, 1.0, BlendMode::Normal,)
            .unwrap_err()
            .to_string()
            .contains("encoded RGBA8")
    );

    let texture = RenderOutput::Texture(TextureInfo {
        texture_id: 7,
        width: 1,
        height: 1,
    });
    assert!(
        renderer
            .draw_layer_affine_with_blend(&texture, &Affine2D::IDENTITY, 1.0, BlendMode::Normal,)
            .unwrap_err()
            .to_string()
            .contains("untyped GPU texture")
    );

    let foreign = RenderOutput::Working(working_pixel("foreign", [1.0, 0.0, 0.0, 1.0]));
    assert!(
        renderer
            .draw_layer_affine_with_blend(&foreign, &Affine2D::IDENTITY, 1.0, BlendMode::Normal,)
            .unwrap_err()
            .to_string()
            .contains("cannot draw working image")
    );
}

#[test]
fn project_surface_composites_half_white_in_linear_light_and_terminals_once() {
    let config = "linear-source-over";
    let mut renderer = SkiaRenderer::new(1, 1, Color::black(), false, None, None).unwrap();
    renderer
        .use_project_linear_surface(working_contract(config))
        .unwrap();
    renderer
        .draw_layer_affine_with_blend(
            &RenderOutput::Working(encoded_pixel(config, [255, 255, 255, 128])),
            &Affine2D::IDENTITY,
            1.0,
            BlendMode::Normal,
        )
        .unwrap();

    let RenderOutput::Working(working) = renderer.finalize().unwrap() else {
        panic!("Project root must retain its working identity");
    };
    assert_pixel_near(
        working.pixels().pixels()[0],
        [128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 1.0],
    );
    let terminal = BuiltinColorTransform
        .create_cpu_processor(&ColorTransformRequest::working_to_output(
            LINEAR_SRGB_SPACE_ID,
            SRGB_SPACE_ID,
        ))
        .unwrap();
    assert_eq!(
        working.to_straight_rgba8(terminal.as_ref()).unwrap(),
        [188, 188, 188, 255]
    );
}

#[test]
fn project_surface_converts_authored_background_once_when_contract_changes() {
    let gray = Color {
        r: 128,
        g: 128,
        b: 128,
        a: 255,
    };
    let mut renderer = SkiaRenderer::new(1, 1, gray, false, None, None).unwrap();
    renderer
        .use_project_linear_surface(working_contract("single-authoring-transform"))
        .unwrap();
    let RenderOutput::Working(working) = renderer.finalize().unwrap() else {
        panic!("Project root must retain its working identity");
    };
    assert_pixel_near(
        working.pixels().pixels()[0],
        [0.215_86, 0.215_86, 0.215_86, 1.0],
    );
}

#[test]
fn project_working_sksl_premultiplies_once_and_terminals_from_straight_values() {
    let config = "working-sksl";
    let mut renderer = SkiaRenderer::new(1, 1, Color::black(), false, None, None).unwrap();
    renderer
        .use_project_linear_surface(working_contract(config))
        .unwrap();

    let output = renderer
        .rasterize_sksl_layer(SkSLRasterRequest {
            shader_code: "half4 main(float2 p) { return half4(0.5, 0.25, 0.75, 0.5); }",
            resolution: (1.0, 1.0),
            time: 0.0,
            transform: &Affine2D::IDENTITY,
            color_domain: SkSLColorDomain::ProjectWorkingLinear,
        })
        .unwrap();
    let RenderOutput::Working(output) = output else {
        panic!("declared Project-working SkSL must retain its typed working output");
    };
    assert_eq!(output.identity(), &working_identity(config));
    assert_pixel_near(output.pixels().pixels()[0], [0.25, 0.125, 0.375, 0.5]);

    let terminal = BuiltinColorTransform
        .create_cpu_processor(&ColorTransformRequest::working_to_output(
            LINEAR_SRGB_SPACE_ID,
            SRGB_SPACE_ID,
        ))
        .unwrap();
    assert_eq!(
        output.to_straight_rgba8(terminal.as_ref()).unwrap(),
        [188, 137, 225, 128]
    );
}

#[test]
fn project_working_sksl_premultiplication_preserves_extended_and_negative_rgb() {
    let config = "working-sksl-extended";
    let mut renderer = SkiaRenderer::new(1, 1, Color::black(), false, None, None).unwrap();
    renderer
        .use_project_linear_surface(working_contract(config))
        .unwrap();

    let output = renderer
        .rasterize_sksl_layer(SkSLRasterRequest {
            shader_code: "half4 main(float2 p) { return half4(-0.5, 2.0, 0.25, 0.5); }",
            resolution: (1.0, 1.0),
            time: 0.0,
            transform: &Affine2D::IDENTITY,
            color_domain: SkSLColorDomain::ProjectWorkingLinear,
        })
        .unwrap();
    let RenderOutput::Working(output) = output else {
        panic!("declared Project-working SkSL must retain its typed working output");
    };
    assert_pixel_near(output.pixels().pixels()[0], [-0.25, 1.0, 0.125, 0.5]);
}

#[test]
fn sksl_domain_mismatch_and_compilation_error_fail_closed() {
    let request = SkSLRasterRequest {
        shader_code: "half4 main(float2 p) { return half4(1); }",
        resolution: (1.0, 1.0),
        time: 0.0,
        transform: &Affine2D::IDENTITY,
        color_domain: SkSLColorDomain::ProjectWorkingLinear,
    };
    let mut unmanaged = SkiaRenderer::new(1, 1, Color::black(), false, None, None).unwrap();
    assert!(
        unmanaged
            .rasterize_sksl_layer(request)
            .unwrap_err()
            .to_string()
            .contains("cannot render into an unmanaged sRGBA8 surface")
    );

    let mut managed = SkiaRenderer::new(1, 1, Color::black(), false, None, None).unwrap();
    managed
        .use_project_linear_surface(working_contract("invalid-sksl"))
        .unwrap();
    let error = managed
        .rasterize_sksl_layer(SkSLRasterRequest {
            shader_code: "this is not valid SkSL",
            ..request
        })
        .unwrap_err()
        .to_string();
    assert!(error.contains("SkSL compilation failed"));
}

#[test]
fn project_groups_keep_extended_and_negative_float_samples() {
    let config = "extended-group";
    let mut renderer = SkiaRenderer::new(1, 1, Color::black(), false, None, None).unwrap();
    renderer
        .use_project_linear_surface(working_contract(config))
        .unwrap();
    renderer
        .begin_group(
            1,
            1,
            &Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
        )
        .unwrap();
    renderer
        .draw_layer_affine_with_blend(
            &RenderOutput::Working(working_pixel(config, [-0.25, 2.0, 0.5, 1.0])),
            &Affine2D::IDENTITY,
            1.0,
            BlendMode::Normal,
        )
        .unwrap();
    let group = renderer.end_group().unwrap();
    let RenderOutput::Working(group_working) = &group else {
        panic!("isolated Project group must remain working-linear");
    };
    assert_pixel_near(group_working.pixels().pixels()[0], [-0.25, 2.0, 0.5, 1.0]);

    renderer
        .draw_layer_affine_with_blend(&group, &Affine2D::IDENTITY, 1.0, BlendMode::Normal)
        .unwrap();
    let RenderOutput::Working(root) = renderer.finalize().unwrap() else {
        panic!("Project root must remain working-linear");
    };
    assert_pixel_near(root.pixels().pixels()[0], [-0.25, 2.0, 0.5, 1.0]);
}

#[test]
fn project_surface_keeps_normal_multiply_and_add_in_linear_working_space() {
    let cases = [
        (BlendMode::Normal, [0.5, 0.25, 0.5, 1.0]),
        (BlendMode::Multiply, [0.125, 0.125, 0.375, 1.0]),
        (BlendMode::LinearDodge, [0.75, 0.75, 1.0, 1.0]),
    ];
    for (mode, expected) in cases {
        let config = format!("linear-blend-{mode:?}");
        let mut renderer = SkiaRenderer::new(1, 1, Color::black(), false, None, None).unwrap();
        renderer
            .use_project_linear_surface(working_contract(&config))
            .unwrap();
        for (pixel, blend) in [
            ([0.25, 0.5, 0.75, 1.0], BlendMode::Normal),
            ([0.5, 0.25, 0.5, 1.0], mode),
        ] {
            renderer
                .draw_layer_affine_with_blend(
                    &RenderOutput::Working(working_pixel(&config, pixel)),
                    &Affine2D::IDENTITY,
                    1.0,
                    blend,
                )
                .unwrap();
        }
        let RenderOutput::Working(output) = renderer.finalize().unwrap() else {
            panic!("Project root must remain working-linear");
        };
        assert_pixel_near(output.pixels().pixels()[0], expected);
    }
}

#[test]
fn project_text_and_transformed_shape_rasterizers_keep_the_working_contract() {
    const ROW_STRIDE: usize = 32;
    let config = "vector-rasterizers";
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer = SkiaRenderer::new(32, 32, transparent, false, None, None).unwrap();
    renderer
        .use_project_linear_surface(working_contract(config))
        .unwrap();
    let styles = [StyleConfig {
        id: Uuid::new_v4(),
        style: DrawStyle::Fill {
            color: Color {
                r: 0,
                g: 255,
                b: 0,
                a: 255,
            },
            offset: 0.0,
        },
    }];

    let shape = renderer
        .rasterize_shape_layer(ShapeRasterRequest {
            path_data: "M 0 0 L 4 0 L 4 4 L 0 4 Z",
            canonical_path: None,
            styles: &styles,
            path_effects: &[],
            ensemble: None,
            transform: Affine2D::translate(8.0, 6.0),
        })
        .unwrap();
    let RenderOutput::Working(shape) = &shape else {
        assert!(matches!(shape, RenderOutput::Working(_)));
        return;
    };
    assert_eq!(shape.identity(), &working_identity(config));
    let inside = shape.pixels().pixels()[7 * ROW_STRIDE + 9];
    assert_pixel_near(inside, [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(shape.pixels().pixels()[ROW_STRIDE + 1], [0.0; 4]);

    let text = renderer
        .rasterize_text_layer(TextRasterRequest {
            text: "M",
            size: 12.0,
            font_name: "Arial",
            styles: &styles,
            ensemble: None,
            transform: Affine2D::translate(2.0, 2.0),
            current_time: 0.0,
        })
        .unwrap();
    let RenderOutput::Working(text) = &text else {
        assert!(matches!(&text, RenderOutput::Working(_)));
        return;
    };
    assert_eq!(text.identity(), &working_identity(config));
}
