use super::{RenderOutput, Renderer, SkiaRenderer};
use crate::error::LibraryError;
use crate::model::BlendMode;
#[cfg(all(feature = "gl", target_os = "windows"))]
use crate::model::authoring::{InstancePath, ModuleInstanceId, ModuleOutputId, TimelineId};
use crate::model::frame::Image;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::DrawStyle;
use crate::model::frame::entity::{SkSLColorDomain, StyleConfig};
#[cfg(all(feature = "gl", target_os = "windows"))]
use crate::model::frame::particle::{
    ParticleSceneFrame, ParticleSceneParameters, SceneInvocationKey,
};
#[cfg(all(feature = "gl", target_os = "windows"))]
use crate::model::property::Vec3;
#[cfg(all(feature = "gl", target_os = "windows"))]
use crate::rendering::renderer::ParticleRasterRequest;
use crate::rendering::renderer::{
    Affine2D, ShapeRasterRequest, SkSLRasterRequest, TextRasterRequest, TextureInfo,
    WorkingSurfaceContract,
};
#[cfg(all(feature = "gl", target_os = "windows"))]
use crate::rendering::skia_utils::{create_gpu_context, get_current_context_handle};
#[cfg(all(feature = "gl", target_os = "windows"))]
use ordered_float::OrderedFloat;
use ruvie_color_management::{
    BuiltinColorTransform, ColorContext, ColorTransformBackend, ColorTransformRequest,
    LINEAR_SRGB_SPACE_ID, LinearWorkingImage, ManagedLinearWorkingImage, SRGB_SPACE_ID,
    VerifiedSourceSpace, WorkingColorIdentity,
};
use uuid::Uuid;

#[path = "tests/render_target.rs"]
mod render_target;
#[path = "tests/vector_layer_native.rs"]
mod vector_layer_native;

const CUSTOM_BLEND_MODES: [BlendMode; 10] = [
    BlendMode::LinearBurn,
    BlendMode::DarkerColor,
    BlendMode::LinearDodge,
    BlendMode::LighterColor,
    BlendMode::VividLight,
    BlendMode::LinearLight,
    BlendMode::PinLight,
    BlendMode::HardMix,
    BlendMode::Subtract,
    BlendMode::Divide,
];

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
fn cross_dissolve_mixes_premultiplied_sources_in_working_linear_space() {
    let config = "cross-dissolve-linear";
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer = SkiaRenderer::new(1, 1, transparent, false, None, None).unwrap();
    renderer
        .use_project_linear_surface(working_contract(config))
        .unwrap();
    let from = RenderOutput::Working(working_pixel(config, [0.5, 0.0, 0.0, 0.5]));
    let to = RenderOutput::Working(working_pixel(config, [0.0, 0.0, 1.0, 1.0]));
    renderer
        .draw_cross_dissolve(&from, &to, 0.25, BlendMode::Normal)
        .unwrap();
    let RenderOutput::Working(output) = renderer.finalize().unwrap() else {
        panic!("Cross Dissolve must remain in the Project working domain");
    };
    assert_pixel_near(output.pixels().pixels()[0], [0.375, 0.0, 0.25, 0.625]);
}

#[test]
fn retained_cross_dissolve_consumes_native_layers_without_render_output_round_trip() {
    let config = "retained-cross-dissolve-linear";
    let mut renderer = SkiaRenderer::new(1, 1, Color::black(), false, None, None).unwrap();
    renderer
        .use_project_linear_surface(working_contract(config))
        .unwrap();
    renderer
        .begin_group(
            1,
            1,
            &Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
        )
        .unwrap();
    let from = renderer.end_group_retained().unwrap();
    renderer
        .begin_group(
            1,
            1,
            &Color {
                r: 0,
                g: 0,
                b: 255,
                a: 255,
            },
        )
        .unwrap();
    let to = renderer.end_group_retained().unwrap();
    assert_eq!(renderer.retained_group_surfaces.len(), 2);

    renderer
        .draw_cross_dissolve_retained(from, to, 0.25, BlendMode::Normal)
        .unwrap();
    assert!(renderer.retained_group_surfaces.is_empty());
    let RenderOutput::Working(output) = renderer.finalize().unwrap() else {
        panic!("retained Cross Dissolve must remain in Project working space");
    };
    assert_pixel_near(output.pixels().pixels()[0], [0.75, 0.0, 0.25, 1.0]);
}

#[test]
fn cross_dissolve_rejects_encoded_input_at_the_project_linear_boundary() {
    let config = "cross-dissolve-boundary";
    let mut renderer = SkiaRenderer::new(1, 1, Color::black(), false, None, None).unwrap();
    renderer
        .use_project_linear_surface(working_contract(config))
        .unwrap();
    let encoded = RenderOutput::Image(Image::new(1, 1, vec![255, 0, 0, 255]));
    let working = RenderOutput::Working(working_pixel(config, [0.0, 0.0, 1.0, 1.0]));
    let error = renderer
        .draw_cross_dissolve(&encoded, &working, 0.5, BlendMode::Normal)
        .unwrap_err();
    assert!(error.to_string().contains("encoded RGBA8"));
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
fn custom_blends_pass_extended_source_through_a_transparent_working_backdrop() {
    let config = "custom-blend-transparent-extended";
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let source = RenderOutput::Working(working_pixel(config, [-0.25, 2.0, 0.5, 1.0]));
    let mut renderer = SkiaRenderer::new(1, 1, transparent, false, None, None).unwrap();
    renderer
        .use_project_linear_surface(working_contract(config))
        .unwrap();

    for mode in CUSTOM_BLEND_MODES {
        renderer.clear().unwrap();
        renderer
            .draw_layer_affine_with_blend(&source, &Affine2D::IDENTITY, 1.0, mode)
            .unwrap();
        let RenderOutput::Working(output) = renderer.finalize().unwrap() else {
            panic!("Project root must remain working-linear");
        };
        assert_pixel_near(output.pixels().pixels()[0], [-0.25, 2.0, 0.5, 1.0]);
    }
}

#[test]
fn custom_blends_use_unclamped_premultiplied_source_over_on_partial_backdrops() {
    let config = "custom-blend-partial-extended";
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let base = RenderOutput::Working(working_pixel(config, [-0.12, 0.9, 0.18, 0.6]));
    let source = RenderOutput::Working(working_pixel(config, [-0.2, 1.2, 0.3, 0.5]));
    let mut renderer = SkiaRenderer::new(1, 1, transparent, false, None, None).unwrap();
    renderer
        .use_project_linear_surface(working_contract(config))
        .unwrap();

    // The straight base/source values are [-0.2, 1.5, 0.3] and
    // [-0.4, 2.4, 0.6]. With Ab = 0.6 and As = 0.5, W3C source-over is
    // Co = 0.2 * Cs + 0.3 * B(Cb, Cs) + 0.3 * Cb, Ao = 0.8.
    let cases = [
        (BlendMode::LinearBurn, [-0.14, 1.8, 0.21, 0.8]),
        (BlendMode::DarkerColor, [-0.2, 1.38, 0.3, 0.8]),
        (BlendMode::LinearDodge, [-0.32, 1.23, 0.48, 0.8]),
        (BlendMode::LighterColor, [-0.26, 1.65, 0.39, 0.8]),
        (BlendMode::VividLight, [-0.14, 1.23, 0.3225, 0.8]),
        (BlendMode::LinearLight, [-0.14, 1.23, 0.36, 0.8]),
        (BlendMode::PinLight, [-0.38, 2.07, 0.3, 0.8]),
        (BlendMode::HardMix, [-0.14, 1.23, 0.21, 0.8]),
        (BlendMode::Subtract, [-0.08, 0.93, 0.21, 0.8]),
        (BlendMode::Divide, [0.16, 1.1175, 0.36, 0.8]),
    ];
    for (mode, expected) in cases {
        renderer.clear().unwrap();
        renderer
            .draw_layer_affine_with_blend(&base, &Affine2D::IDENTITY, 1.0, BlendMode::Normal)
            .unwrap();
        renderer
            .draw_layer_affine_with_blend(&source, &Affine2D::IDENTITY, 1.0, mode)
            .unwrap();
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

#[cfg(all(feature = "gl", target_os = "windows"))]
fn particle_vec3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
        z: OrderedFloat(z),
    }
}

#[cfg(all(feature = "gl", target_os = "windows"))]
fn particle_scene(target_step: u64) -> ParticleSceneFrame {
    ParticleSceneFrame {
        invocation: SceneInvocationKey {
            instance_path: InstancePath::root(TimelineId::from_uuid(Uuid::from_u128(1))),
            module_instance_id: ModuleInstanceId::from_uuid(Uuid::from_u128(2)),
            state_slot_id: Uuid::from_u128(3),
            output_id: ModuleOutputId::from_uuid(Uuid::from_u128(4)),
        },
        random_stream_id: Uuid::from_u128(5),
        executable_hash: [17; 32],
        target_step,
        logical_width: 256,
        logical_height: 144,
        parameters: ParticleSceneParameters {
            capacity: 1_024,
            emission_rate: OrderedFloat(120.0),
            lifetime_seconds: OrderedFloat(4.0),
            seed: 42,
            velocity_min: particle_vec3(-40.0, -120.0, -20.0),
            velocity_max: particle_vec3(40.0, -80.0, 20.0),
            gravity: particle_vec3(0.0, 100.0, 0.0),
            drag: OrderedFloat(0.1),
            size_min: OrderedFloat(4.0),
            size_max: OrderedFloat(10.0),
            color: Color {
                r: 100,
                g: 190,
                b: 255,
                a: 230,
            },
        },
    }
}

#[cfg(all(feature = "gl", target_os = "windows"))]
fn render_particle_test_scene(
    renderer: &mut SkiaRenderer,
    scene: &ParticleSceneFrame,
) -> Result<Image, String> {
    render_particle_test_scene_with_transform(renderer, scene, &Affine2D::IDENTITY)
}

#[cfg(all(feature = "gl", target_os = "windows"))]
fn render_particle_test_scene_with_transform(
    renderer: &mut SkiaRenderer,
    scene: &ParticleSceneFrame,
    transform: &Affine2D,
) -> Result<Image, String> {
    let output = renderer
        .rasterize_particle_layer(ParticleRasterRequest { scene, transform })
        .map_err(|error| error.to_string())?;
    match output {
        RenderOutput::Image(image) => Ok(image),
        other => Err(format!("unexpected Particle output {other:?}")),
    }
}

#[cfg(all(feature = "gl", target_os = "windows"))]
fn nontransparent_bounds(image: &Image) -> Option<(u32, u32)> {
    let mut min_x = image.width;
    let mut min_y = image.height;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found = false;
    for (index, pixel) in image.data.chunks_exact(4).enumerate() {
        if pixel[3] == 0 {
            continue;
        }
        let index = index as u32;
        let (x, y) = (index % image.width, index / image.width);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
        found = true;
    }
    found.then_some((max_x - min_x + 1, max_y - min_y + 1))
}

#[cfg(all(feature = "gl", target_os = "windows"))]
#[test]
fn cpu_renderer_fails_closed_for_gpu_particle_scene() {
    let mut renderer = SkiaRenderer::new(256, 144, Color::black(), false, None, None).unwrap();
    let error = render_particle_test_scene(&mut renderer, &particle_scene(1)).unwrap_err();
    assert!(error.contains("no active GPU context"));
}

/// This exercises a real OpenGL compute/SSBO/FBO path. Keep it opt-in so CI
/// without a GPU and interactive sessions sharing the user's GPU stay safe.
#[cfg(all(feature = "gl", target_os = "windows"))]
#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_particle_seek_and_independent_renderer_are_deterministic() {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut preview = SkiaRenderer::new(256, 144, transparent.clone(), true, None, None).unwrap();
    let at_checkpoint = particle_scene(240);
    let first = match render_particle_test_scene(&mut preview, &at_checkpoint) {
        Ok(image) => image,
        Err(diagnostic) if diagnostic.contains("GPU Particle unavailable") => {
            eprintln!("skipping unsupported device: {diagnostic}");
            return;
        }
        Err(error) => panic!("GPU Particle render failed: {error}"),
    };
    assert!(first.data.iter().any(|component| *component != 0));
    render_particle_test_scene(&mut preview, &particle_scene(480)).expect("forward simulation");
    let replayed =
        render_particle_test_scene(&mut preview, &at_checkpoint).expect("checkpoint restore");
    assert_eq!(first.data, replayed.data);

    let mut export = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    let preview_handle = preview
        .gpu_context
        .as_ref()
        .and_then(|context| {
            context.ensure_current().ok()?;
            get_current_context_handle()
        })
        .expect("preview renderer must own a WGL context");
    let export_handle = export
        .gpu_context
        .as_ref()
        .and_then(|context| {
            context.ensure_current().ok()?;
            get_current_context_handle()
        })
        .expect("export renderer must own a WGL context");
    assert_ne!(preview_handle, export_handle);

    // Export construction made a second context current. Preview must reclaim
    // its own context at the Renderer boundary without the caller knowing
    // about GL ownership, then export must be able to do the same in reverse.
    let preview_after_export_creation = render_particle_test_scene(&mut preview, &at_checkpoint)
        .expect("preview must reactivate its context after export construction");
    assert_eq!(get_current_context_handle(), Some(preview_handle));
    assert_eq!(first.data, preview_after_export_creation.data);
    let independent = render_particle_test_scene(&mut export, &at_checkpoint)
        .expect("independent export session");
    assert_eq!(get_current_context_handle(), Some(export_handle));
    assert_eq!(first.data, independent.data);

    let singular = render_particle_test_scene_with_transform(
        &mut preview,
        &at_checkpoint,
        &Affine2D::scale(0.0, 0.0),
    )
    .expect("singular Particle transform");
    assert!(
        singular.data.iter().all(|component| *component == 0),
        "a zero-area Particle transform must produce exact transparent pixels"
    );

    let mut translucent_overlap = particle_scene(4);
    translucent_overlap.parameters.capacity = 4;
    translucent_overlap.parameters.emission_rate = OrderedFloat(120.0);
    translucent_overlap.parameters.velocity_min = particle_vec3(0.0, 0.0, -120.0);
    translucent_overlap.parameters.velocity_max = particle_vec3(0.0, 0.0, -120.0);
    translucent_overlap.parameters.gravity = particle_vec3(0.0, 0.0, 0.0);
    translucent_overlap.parameters.drag = OrderedFloat(0.0);
    translucent_overlap.parameters.size_min = OrderedFloat(32.0);
    translucent_overlap.parameters.size_max = OrderedFloat(32.0);
    translucent_overlap.parameters.color = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 64,
    };
    let soft_particles = render_particle_test_scene(&mut preview, &translucent_overlap)
        .expect("overlapping translucent Particles");
    let center_alpha = soft_particles.data[((72 * 256 + 128) * 4 + 3) as usize];
    assert!(
        center_alpha > 150,
        "all four translucent sprites must composite at center; alpha was {center_alpha}"
    );

    let mut stretched_scene = particle_scene(1);
    stretched_scene.parameters.capacity = 1;
    stretched_scene.parameters.emission_rate = OrderedFloat(120.0);
    stretched_scene.parameters.velocity_min = particle_vec3(0.0, 0.0, 0.0);
    stretched_scene.parameters.velocity_max = particle_vec3(0.0, 0.0, 0.0);
    stretched_scene.parameters.gravity = particle_vec3(0.0, 0.0, 0.0);
    stretched_scene.parameters.drag = OrderedFloat(0.0);
    stretched_scene.parameters.size_min = OrderedFloat(32.0);
    stretched_scene.parameters.size_max = OrderedFloat(32.0);
    stretched_scene.parameters.color = Color::white();
    let centered_non_uniform = Affine2D::translate(128.0, 72.0)
        .compose(Affine2D::scale(4.0, 0.25))
        .compose(Affine2D::translate(-128.0, -72.0));
    let stretched = render_particle_test_scene_with_transform(
        &mut preview,
        &stretched_scene,
        &centered_non_uniform,
    )
    .expect("non-uniform Particle transform");
    let (stretched_width, stretched_height) =
        nontransparent_bounds(&stretched).expect("stretched sprite pixels");
    assert!(
        stretched_width > 80 && stretched_height < 20 && stretched_width > stretched_height * 8,
        "Particle quad must follow the full affine; bounds were {stretched_width}x{stretched_height}"
    );

    // Perspective is authored in logical Composition space. Preview quality
    // scaling must only change raster resolution, never the apparent logical
    // size of a particle with non-zero Z.
    let mut perspective_scene = particle_scene(180);
    perspective_scene.parameters.capacity = 1;
    perspective_scene.parameters.emission_rate = OrderedFloat(1.0);
    perspective_scene.parameters.velocity_min = particle_vec3(0.0, 0.0, 144.0);
    perspective_scene.parameters.velocity_max = particle_vec3(0.0, 0.0, 144.0);
    perspective_scene.parameters.gravity = particle_vec3(0.0, 0.0, 0.0);
    perspective_scene.parameters.drag = OrderedFloat(0.0);
    perspective_scene.parameters.size_min = OrderedFloat(48.0);
    perspective_scene.parameters.size_max = OrderedFloat(48.0);
    perspective_scene.parameters.color = Color::white();
    let full_resolution = render_particle_test_scene(&mut preview, &perspective_scene)
        .expect("full-resolution perspective Particle");
    let mut half_resolution_renderer = SkiaRenderer::new(
        128,
        72,
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        },
        true,
        None,
        None,
    )
    .unwrap();
    let half_resolution = render_particle_test_scene_with_transform(
        &mut half_resolution_renderer,
        &perspective_scene,
        &Affine2D::scale(0.5, 0.5),
    )
    .expect("half-resolution perspective Particle");
    let (full_width, full_height) =
        nontransparent_bounds(&full_resolution).expect("full-resolution perspective pixels");
    let (half_width, half_height) =
        nontransparent_bounds(&half_resolution).expect("half-resolution perspective pixels");
    assert!(
        full_width.abs_diff(half_width * 2) <= 4 && full_height.abs_diff(half_height * 2) <= 4,
        "logical-space perspective must be resolution invariant; full={full_width}x{full_height}, half={half_width}x{half_height}"
    );

    // Three minutes is beyond the per-request replay budget and the retained
    // checkpoint window. A cold/direct seek reconstructs only the live
    // lifetime suffix and must equal ordinary sequential playback exactly.
    render_particle_test_scene(&mut preview, &particle_scene(7_200)).expect("first minute");
    render_particle_test_scene(&mut preview, &particle_scene(14_400)).expect("second minute");
    let sequential_far = render_particle_test_scene(&mut preview, &particle_scene(21_600))
        .expect("sequential third minute");
    let direct_far = render_particle_test_scene(&mut export, &particle_scene(21_600))
        .expect("bounded direct seek");
    assert_eq!(sequential_far.data, direct_far.data);
    let distant_rewind =
        render_particle_test_scene(&mut preview, &at_checkpoint).expect("distant rewind");
    assert_eq!(first.data, distant_rewind.data);

    // Ganesh may leave the borrowed SceneRuntime texture bound after a draw.
    // Replacing that target on resize must never restore its deleted GL name.
    preview
        .resize_render_target(
            320,
            180,
            Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
        )
        .expect("resize Particle renderer");
    let resized = render_particle_test_scene(&mut preview, &at_checkpoint)
        .expect("Particle render after target growth");
    assert_eq!((resized.width, resized.height), (320, 180));
    preview
        .resize_render_target(
            256,
            144,
            Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
        )
        .expect("restore Particle renderer size");
    let after_resize = render_particle_test_scene(&mut preview, &at_checkpoint)
        .expect("Particle render after target restoration");
    assert_eq!(after_resize.data, first.data);
}

/// Exercises the transaction used when Preview adopts a newly shared WGL
/// context. Both failure rollback and successful replacement must leave the
/// renderer's owning context current before the next SceneRuntime operation.
#[cfg(all(feature = "gl", target_os = "windows"))]
#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_render_target_replacement_restores_and_activates_the_owner_context() {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    if renderer.gpu_context.is_none() {
        eprintln!("skipping unsupported device: renderer has no GPU context");
        return;
    }
    let Some(previous_handle) = get_current_context_handle() else {
        panic!("GPU renderer did not leave its WGL context current");
    };
    let scene = particle_scene(240);
    let first = match render_particle_test_scene(&mut renderer, &scene) {
        Ok(image) => image,
        Err(diagnostic) if diagnostic.contains("GPU Particle unavailable") => {
            eprintln!("skipping unsupported device: {diagnostic}");
            return;
        }
        Err(error) => panic!("GPU Particle render failed before replacement: {error}"),
    };

    let Some(rejected_context) = create_gpu_context(None, None) else {
        eprintln!("skipping device unable to create a second GPU context");
        return;
    };
    let rejected = renderer.replace_render_target(Some(rejected_context), Some(91), None, |_| {
        Err(LibraryError::Render(
            "injected GPU replacement failure".to_string(),
        ))
    });
    assert!(rejected.is_err());
    assert_eq!(get_current_context_handle(), Some(previous_handle));
    let restored = render_particle_test_scene(&mut renderer, &scene)
        .expect("old SceneRuntime must remain usable after replacement rollback");
    assert_eq!(restored.data, first.data);

    let Some(mut incoming_context) = create_gpu_context(None, None) else {
        eprintln!("skipping device unable to create a replacement GPU context");
        return;
    };
    incoming_context.resize(256, 144);
    let Some(incoming_handle) = get_current_context_handle() else {
        panic!("replacement WGL context was not current after construction");
    };
    assert_ne!(incoming_handle, previous_handle);
    let contract = renderer.surface_contract.clone();
    renderer
        .replace_render_target(
            Some(incoming_context),
            Some(incoming_handle),
            None,
            move |direct_context| {
                crate::rendering::skia_working_surface::create_surface(
                    256,
                    144,
                    direct_context,
                    &contract,
                    false,
                )
            },
        )
        .expect("GPU target replacement");
    assert_eq!(get_current_context_handle(), Some(incoming_handle));
    let replaced = render_particle_test_scene(&mut renderer, &scene)
        .expect("new SceneRuntime must use the replacement context");
    assert_eq!(replaced.data, first.data);
}
