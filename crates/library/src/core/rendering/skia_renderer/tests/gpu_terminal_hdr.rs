use super::*;
use ruvie_color_management::{
    GpuShaderLanguage, GpuTerminalChain, PQ_LINEARIZATION_POLICY_CONTEXT_KEY, REC2100_HLG_SPACE_ID,
    REC2100_PQ_SPACE_ID, REFERENCE_WHITE_NITS_CONTEXT_KEY, RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY,
};

fn pq_context(reference_white_nits: &str) -> ColorContext {
    ColorContext::default()
        .with_variable(REFERENCE_WHITE_NITS_CONTEXT_KEY, reference_white_nits)
        .with_variable(
            PQ_LINEARIZATION_POLICY_CONTEXT_KEY,
            RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY,
        )
}

fn identity(config: &str, context: &ColorContext) -> WorkingColorIdentity {
    let verified = BuiltinColorTransform
        .verify_working_space(LINEAR_SRGB_SPACE_ID, context)
        .unwrap();
    WorkingColorIdentity::from_verified(config, verified).unwrap()
}

fn renderer(config: &str, context: &ColorContext) -> SkiaRenderer {
    let gpu = create_gpu_context(None, None).expect("actual OpenGL context required");
    let mut renderer = SkiaRenderer::new(
        5,
        3,
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        },
        true,
        Some(gpu),
        None,
    )
    .unwrap();
    let source = BuiltinColorTransform
        .verify_source_space(SRGB_SPACE_ID, context)
        .unwrap();
    let ingress = BuiltinColorTransform
        .create_cpu_processor(
            &ColorTransformRequest::source_to_working(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID)
                .with_context(context.clone()),
        )
        .unwrap();
    renderer
        .use_project_linear_surface(
            WorkingSurfaceContract::new(identity(config, context), source, ingress).unwrap(),
        )
        .unwrap();
    assert!(renderer.is_gpu_backed().unwrap());
    renderer
}

fn draw_hdr_pattern(renderer: &mut SkiaRenderer, config: &str, context: &ColorContext) {
    let samples = [
        [0.0, 0.0, 0.0, 0.0],
        [0.025, 0.0125, 0.05, 0.25],
        [0.25, 0.125, 0.5, 0.5],
        [0.75, 0.3, 0.1, 1.0],
        [1.5, 0.8, 0.2, 1.0],
    ];
    let pixels = (0..15)
        .map(|index| samples[index % samples.len()])
        .collect();
    let pixels = LinearWorkingImage::from_premultiplied_rgba_f32(5, 3, pixels).unwrap();
    // SAFETY: samples are finite premultiplied linear sRGB and carry the exact
    // context-bound identity installed on this renderer.
    let image = unsafe {
        ManagedLinearWorkingImage::from_working_pixels_unchecked(identity(config, context), pixels)
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

fn terminal_chain(config: &str, context: &ColorContext, output: &str) -> GpuTerminalChain {
    let request = ColorTransformRequest::working_to_output(LINEAR_SRGB_SPACE_ID, output)
        .with_context(context.clone());
    let stage = BuiltinColorTransform
        .extract_gpu_transform(&request, GpuShaderLanguage::Glsl)
        .unwrap();
    GpuTerminalChain::new(identity(config, context), vec![stage]).unwrap()
}

fn assert_near(actual: &Image, expected: &[u8]) {
    assert_eq!(actual.data.len(), expected.len());
    for (index, (&gpu, &cpu)) in actual.data.iter().zip(expected).enumerate() {
        assert!(
            gpu.abs_diff(cpu) <= 1,
            "HDR terminal byte {index}: GPU={gpu}, CPU={cpu}"
        );
    }
}

#[test]
#[ignore = "requires a real desktop OpenGL 4.3 GPU"]
fn gpu_hdr_terminal_matches_hlg_and_context_bound_pq_cpu_results() {
    let cases = [
        (ColorContext::default(), REC2100_HLG_SPACE_ID),
        (pq_context("100"), REC2100_PQ_SPACE_ID),
        (pq_context("203"), REC2100_PQ_SPACE_ID),
    ];
    let mut pq_outputs = Vec::new();
    for (index, (context, output)) in cases.into_iter().enumerate() {
        let config = format!("hdr-terminal-{index}");
        let mut renderer = renderer(&config, &context);
        draw_hdr_pattern(&mut renderer, &config, &context);
        let RenderOutput::Working(working) = renderer.finalize().unwrap() else {
            panic!("typed working image")
        };
        let request = ColorTransformRequest::working_to_output(LINEAR_SRGB_SPACE_ID, output)
            .with_context(context.clone());
        let cpu = BuiltinColorTransform
            .create_cpu_processor(&request)
            .unwrap();
        let expected = working.to_straight_rgba8(cpu.as_ref()).unwrap();
        let actual = renderer
            .finalize_gpu_terminal(&terminal_chain(&config, &context, output))
            .unwrap()
            .expect("GPU HDR terminal available");
        assert_near(&actual, &expected);
        if output == REC2100_PQ_SPACE_ID {
            pq_outputs.push(actual.data);
        }
    }
    assert_ne!(pq_outputs[0], pq_outputs[1]);
}

#[test]
#[ignore = "requires a real desktop OpenGL 4.3 GPU"]
fn gpu_pq_terminal_rejects_domain_errors_and_foreign_context_identity() {
    let context_100 = pq_context("100");
    let config = "pq-domain";
    let mut renderer = renderer(config, &context_100);
    renderer
        .surface
        .canvas()
        .clear(skia_safe::Color4f::new(-0.1, 0.0, 0.0, 1.0));
    let chain_100 = terminal_chain(config, &context_100, REC2100_PQ_SPACE_ID);
    let error = renderer
        .finalize_gpu_terminal(&chain_100)
        .expect_err("negative absolute luminance must fail")
        .to_string();
    assert!(error.contains("invalid"), "{error}");

    renderer
        .surface
        .canvas()
        .clear(skia_safe::Color4f::new(0.25, 0.2, 0.1, 1.0));
    assert!(
        renderer
            .finalize_gpu_terminal(&chain_100)
            .unwrap()
            .is_some()
    );

    let context_203 = pq_context("203");
    let foreign = terminal_chain(config, &context_203, REC2100_PQ_SPACE_ID);
    let error = renderer
        .finalize_gpu_terminal(&foreign)
        .expect_err("different Project context must fail")
        .to_string();
    assert!(error.contains("does not belong"), "{error}");
}
