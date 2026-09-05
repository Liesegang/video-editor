use super::*;
use ruvie_color_management::{GpuShaderLanguage, GpuTerminalChain, StandardColorSpaceId};

fn gpu_renderer(config: &str, width: u32, height: u32) -> SkiaRenderer {
    let context = create_gpu_context(None, None).expect("actual OpenGL context required");
    let mut renderer = SkiaRenderer::new(
        width,
        height,
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        },
        true,
        Some(context),
        None,
    )
    .unwrap();
    renderer
        .use_project_linear_surface(working_contract(config))
        .unwrap();
    assert!(
        renderer.is_gpu_backed().unwrap(),
        "test must not silently use raster"
    );
    renderer
}

fn chain(config: &str, requests: &[ColorTransformRequest]) -> GpuTerminalChain {
    GpuTerminalChain::new(
        working_identity(config),
        requests
            .iter()
            .map(|request| {
                BuiltinColorTransform
                    .extract_gpu_transform(request, GpuShaderLanguage::Glsl)
                    .unwrap()
            })
            .collect(),
    )
    .unwrap()
}

fn pattern(renderer: &mut SkiaRenderer, config: &str, width: u32, height: u32) {
    let mut pixels = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let alpha = [0.0, 0.2, 0.7, 1.0][((x + y) % 4) as usize];
            pixels.push([
                (x as f32 / width as f32) * alpha,
                (y as f32 / height as f32) * alpha,
                (0.25 + x as f32 / width as f32) * alpha,
                alpha,
            ]);
        }
    }
    let pixels = LinearWorkingImage::from_premultiplied_rgba_f32(width, height, pixels).unwrap();
    // SAFETY: synthetic pixels are explicitly premultiplied linear sRGB with
    // the exact identity installed by this test's renderer factory.
    let image = unsafe {
        ManagedLinearWorkingImage::from_working_pixels_unchecked(working_identity(config), pixels)
    };
    renderer.clear().unwrap();
    renderer
        .draw_layer_affine_with_blend(
            &RenderOutput::Working(image),
            &Affine2D::IDENTITY,
            1.0,
            BlendMode::Normal,
        )
        .unwrap();
}

fn assert_near(actual: &Image, expected: &[u8]) {
    assert_eq!(actual.data.len(), expected.len());
    for (i, (&left, &right)) in actual.data.iter().zip(expected).enumerate() {
        assert!(
            left.abs_diff(right) <= 1,
            "terminal byte {i}: GPU={left}, CPU={right}"
        );
    }
}

#[test]
#[ignore = "requires a real desktop OpenGL 4.3 GPU"]
fn gpu_terminal_preserves_extended_negative_working_color_until_terminal_packing() {
    let config = "negative-terminal";
    let mut renderer = gpu_renderer(config, 1, 1);
    renderer.clear().unwrap();
    renderer
        .draw_layer_affine_with_blend(
            &RenderOutput::Working(working_pixel(config, [-0.2, 0.75, 1.3, 1.0])),
            &Affine2D::IDENTITY,
            1.0,
            BlendMode::Normal,
        )
        .unwrap();
    let RenderOutput::Working(working) = renderer.finalize().unwrap() else {
        panic!("working")
    };
    assert!(working.pixels().pixels()[0][0] < 0.0);
    assert!(working.pixels().pixels()[0][2] > 1.0);
    for space in [
        SRGB_SPACE_ID,
        "display-p3",
        "rec2020-sdr-exact",
        "linear-rec2020",
    ] {
        let request = ColorTransformRequest::working_to_display(LINEAR_SRGB_SPACE_ID, space);
        let cpu = BuiltinColorTransform
            .create_cpu_processor(&request)
            .unwrap();
        let terminal = chain(config, &[request]);
        let expected = working.to_straight_rgba8(cpu.as_ref()).unwrap();
        assert_near(
            &renderer.finalize_gpu_terminal(&terminal).unwrap().unwrap(),
            &expected,
        );
    }
}

#[test]
#[ignore = "requires a real desktop OpenGL 4.3 GPU"]
fn gpu_working_upload_preserves_extended_samples_at_the_skia_boundary() {
    let config = "extended-upload-boundary";
    let mut renderer = gpu_renderer(config, 1, 1);
    let source = working_pixel(config, [-0.2, 0.75, 1.3, 1.0]);
    let image = crate::rendering::skia_working_surface::managed_working_to_skia_image(
        &source,
        renderer
            .surface_contract
            .working()
            .expect("working surface contract"),
        renderer.gpu_context.as_mut(),
    )
    .unwrap();
    let shader = image
        .to_raw_shader(None, skia_safe::SamplingOptions::default(), None)
        .expect("raw working shader");
    let mut paint = skia_safe::Paint::default();
    paint.set_shader(shader);
    paint.set_blend_mode(skia_safe::BlendMode::Src);
    renderer
        .surface
        .canvas()
        .draw_rect(skia_safe::Rect::from_wh(1.0, 1.0), &paint);
    let RenderOutput::Working(working) = renderer.finalize().unwrap() else {
        panic!("working output")
    };
    assert_pixel_near(working.pixels().pixels()[0], [-0.2, 0.75, 1.3, 1.0]);
}

#[test]
#[ignore = "requires a real desktop OpenGL 4.3 GPU"]
fn gpu_working_upload_matches_cpu_for_opacity_transform_and_blend() {
    let cases = [
        (Affine2D::IDENTITY, 0.4, BlendMode::Normal),
        (Affine2D::translate(1.0, 0.0), 0.65, BlendMode::Normal),
        (Affine2D::IDENTITY, 0.7, BlendMode::Multiply),
    ];
    for (index, (transform, opacity, blend)) in cases.into_iter().enumerate() {
        let config = format!("extended-composite-{index}");
        let render = |use_gpu| {
            let mut renderer = if use_gpu {
                gpu_renderer(&config, 3, 1)
            } else {
                let mut renderer = SkiaRenderer::new(
                    3,
                    1,
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
                .unwrap();
                renderer
                    .use_project_linear_surface(working_contract(&config))
                    .unwrap();
                renderer
            };
            renderer.clear().unwrap();
            renderer
                .draw_layer_affine_with_blend(
                    &RenderOutput::Working(working_pixel(&config, [0.4, 0.25, 0.7, 1.0])),
                    &Affine2D::IDENTITY,
                    1.0,
                    BlendMode::Normal,
                )
                .unwrap();
            renderer
                .draw_layer_affine_with_blend(
                    &RenderOutput::Working(working_pixel(&config, [-0.2, 0.75, 1.3, 1.0])),
                    &transform,
                    opacity,
                    blend,
                )
                .unwrap();
            let RenderOutput::Working(output) = renderer.finalize().unwrap() else {
                panic!("working output")
            };
            output
        };
        let cpu = render(false);
        let gpu = render(true);
        assert_eq!(gpu.pixels().pixels().len(), cpu.pixels().pixels().len());
        for (actual, expected) in gpu.pixels().pixels().iter().zip(cpu.pixels().pixels()) {
            assert_pixel_near(*actual, *expected);
        }
    }
}

#[test]
#[ignore = "requires a real desktop OpenGL 4.3 GPU"]
fn gpu_terminal_matches_cpu_color_alpha_orientation_and_reuses_program() {
    let config = "terminal-parity";
    let mut renderer = gpu_renderer(config, 19, 13);
    pattern(&mut renderer, config, 19, 13);
    let RenderOutput::Working(working) = renderer.finalize().unwrap() else {
        panic!("typed working image")
    };
    for space in StandardColorSpaceId::ALL {
        let request =
            ColorTransformRequest::working_to_display(LINEAR_SRGB_SPACE_ID, space.as_str());
        // PQ requires Project luminance metadata; its explicit HDR contract is
        // exercised separately rather than inventing missing configuration.
        let Ok(cpu) = BuiltinColorTransform.create_cpu_processor(&request) else {
            continue;
        };
        let chain = chain(config, std::slice::from_ref(&request));
        let expected = working.to_straight_rgba8(cpu.as_ref()).unwrap();
        let actual = renderer
            .finalize_gpu_terminal(&chain)
            .unwrap()
            .expect("GPU terminal available");
        assert_near(&actual, &expected);
        let compiled = renderer.terminal_compute.as_ref().unwrap().compilations;
        assert_near(
            &renderer.finalize_gpu_terminal(&chain).unwrap().unwrap(),
            &expected,
        );
        assert_eq!(
            renderer.terminal_compute.as_ref().unwrap().compilations,
            compiled
        );
        // Same program supports odd dimensions and does not retain a stale
        // allocation or orientation from the previous frame.
        renderer
            .resize_render_target(
                7,
                11,
                Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 0,
                },
            )
            .unwrap();
        pattern(&mut renderer, config, 7, 11);
        let RenderOutput::Working(resized) = renderer.finalize().unwrap() else {
            panic!("working")
        };
        assert_near(
            &renderer.finalize_gpu_terminal(&chain).unwrap().unwrap(),
            &resized.to_straight_rgba8(cpu.as_ref()).unwrap(),
        );
        assert_eq!(
            renderer.terminal_compute.as_ref().unwrap().compilations,
            compiled
        );
        renderer
            .resize_render_target(
                19,
                13,
                Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 0,
                },
            )
            .unwrap();
        pattern(&mut renderer, config, 19, 13);
    }
    let other = chain(
        "other-project",
        &[ColorTransformRequest::working_to_display(
            LINEAR_SRGB_SPACE_ID,
            SRGB_SPACE_ID,
        )],
    );
    assert!(renderer.finalize_gpu_terminal(&other).is_err());
}

#[test]
#[ignore = "requires a real desktop OpenGL 4.3 GPU"]
fn gpu_terminal_executes_every_stage_and_rejects_nonfinite_pixels() {
    let config = "terminal-chain";
    let mut renderer = gpu_renderer(config, 9, 7);
    pattern(&mut renderer, config, 9, 7);
    let requests = [
        ColorTransformRequest::working_to_display(LINEAR_SRGB_SPACE_ID, "display-p3"),
        ColorTransformRequest::explicit("display-p3", SRGB_SPACE_ID),
    ];
    let chain = chain(config, &requests);
    let RenderOutput::Working(working) = renderer.finalize().unwrap() else {
        panic!("working")
    };
    let processors = requests
        .iter()
        .map(|request| BuiltinColorTransform.create_cpu_processor(request).unwrap())
        .collect::<Vec<_>>();
    let mut expected = Vec::new();
    for pixel in working.pixels().pixels() {
        let alpha = pixel[3];
        let mut rgb = if alpha == 0.0 {
            [0.0; 3]
        } else {
            [pixel[0], pixel[1], pixel[2]].map(|v| f64::from(v / alpha))
        };
        for processor in &processors {
            rgb = processor
                .transform_rgb(rgb)
                .unwrap()
                .map(|v| f64::from(v as f32));
        }
        if alpha == 0.0 {
            rgb = [0.0; 3];
        }
        expected.extend(
            [rgb[0], rgb[1], rgb[2], f64::from(alpha)]
                .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8),
        );
    }
    assert_near(
        &renderer.finalize_gpu_terminal(&chain).unwrap().unwrap(),
        &expected,
    );
    // Inject invalid storage directly at the renderer boundary. Authored color
    // APIs properly reject such values before they reach a draw operation.
    renderer
        .surface
        .canvas()
        .clear(skia_safe::Color4f::new(f32::NAN, 0.0, 0.0, 1.0));
    let error = renderer
        .finalize_gpu_terminal(&chain)
        .expect_err("nonfinite frame must fail")
        .to_string();
    assert!(error.contains("invalid"), "{error}");
    pattern(&mut renderer, config, 9, 7);
    assert_near(
        &renderer.finalize_gpu_terminal(&chain).unwrap().unwrap(),
        &expected,
    );
}

#[test]
#[ignore = "requires a real desktop OpenGL 4.3 GPU"]
fn gpu_terminal_resources_follow_their_owner_across_replacement_and_drop() {
    let config = "terminal-context-ownership";
    let width = 17;
    let height = 11;
    let request = ColorTransformRequest::working_to_display(LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID);
    let terminal_chain = chain(config, std::slice::from_ref(&request));
    let cpu = BuiltinColorTransform
        .create_cpu_processor(&request)
        .unwrap();

    let mut first = gpu_renderer(config, width, height);
    let first_owner = get_current_context_handle().expect("first renderer owner must be current");
    pattern(&mut first, config, width, height);
    let RenderOutput::Working(first_working) = first.finalize().unwrap() else {
        panic!("first renderer must retain typed working pixels");
    };
    let expected = first_working.to_straight_rgba8(cpu.as_ref()).unwrap();
    assert_near(
        &first
            .finalize_gpu_terminal(&terminal_chain)
            .unwrap()
            .expect("first GPU terminal available"),
        &expected,
    );
    assert_eq!(first.terminal_compute.as_ref().unwrap().compilations, 1);
    assert_eq!(get_current_context_handle(), Some(first_owner));

    let mut second = gpu_renderer(config, width, height);
    let second_owner = get_current_context_handle().expect("second renderer owner must be current");
    assert_ne!(second_owner, first_owner);
    pattern(&mut second, config, width, height);
    assert_near(
        &second
            .finalize_gpu_terminal(&terminal_chain)
            .unwrap()
            .expect("second GPU terminal available"),
        &expected,
    );
    assert_eq!(second.terminal_compute.as_ref().unwrap().compilations, 1);
    assert_eq!(get_current_context_handle(), Some(second_owner));

    // Re-enter the first owner with its already-linked program, then return
    // to the second. Program and buffer names may overlap across unshared GL
    // contexts, so this alternation catches accidental use of thread-current
    // state instead of the renderer's actual owner.
    assert_near(
        &first
            .finalize_gpu_terminal(&terminal_chain)
            .unwrap()
            .expect("first terminal remains available after alternation"),
        &expected,
    );
    assert_eq!(get_current_context_handle(), Some(first_owner));
    assert_near(
        &second
            .finalize_gpu_terminal(&terminal_chain)
            .unwrap()
            .expect("second terminal remains available after alternation"),
        &expected,
    );
    assert_eq!(get_current_context_handle(), Some(second_owner));

    let Some(mut replacement) = create_gpu_context(None, None) else {
        panic!("test requires a replacement OpenGL context");
    };
    replacement.resize(width, height);
    let replacement_owner =
        get_current_context_handle().expect("replacement owner must be current");
    assert_ne!(replacement_owner, first_owner);
    assert_ne!(replacement_owner, second_owner);
    let surface_contract = first.surface_contract.clone();
    first
        .replace_render_target(
            Some(replacement),
            Some(replacement_owner),
            None,
            move |direct_context| {
                crate::rendering::skia_working_surface::create_surface(
                    width,
                    height,
                    direct_context,
                    &surface_contract,
                    true,
                )
            },
        )
        .expect("replace first renderer's actual GL owner");
    assert_eq!(get_current_context_handle(), Some(replacement_owner));
    assert_eq!(
        first.terminal_compute.as_ref().unwrap().compilations,
        0,
        "replacement must not retain the old context's linked program"
    );
    pattern(&mut first, config, width, height);
    assert_near(
        &first
            .finalize_gpu_terminal(&terminal_chain)
            .unwrap()
            .expect("replacement terminal available"),
        &expected,
    );
    assert_eq!(first.terminal_compute.as_ref().unwrap().compilations, 1);

    // Leave the second owner current, then destroy the first renderer. Its
    // Drop must reactivate the replacement owner before deleting raw terminal
    // resources; otherwise overlapping GL names can corrupt `second`.
    assert_near(
        &second
            .finalize_gpu_terminal(&terminal_chain)
            .unwrap()
            .expect("second terminal before first drop"),
        &expected,
    );
    assert_eq!(get_current_context_handle(), Some(second_owner));
    drop(first);

    pattern(&mut second, config, width, height);
    assert_near(
        &second
            .finalize_gpu_terminal(&terminal_chain)
            .unwrap()
            .expect("second terminal after first drop"),
        &expected,
    );
    assert_eq!(second.terminal_compute.as_ref().unwrap().compilations, 1);
    assert_eq!(get_current_context_handle(), Some(second_owner));
}
