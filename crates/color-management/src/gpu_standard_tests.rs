use crate::{
    AlphaRepresentation, BuiltinColorTransform, ColorContext, ColorTransformBackend,
    ColorTransformRequest, ComponentStorage, GpuInvalidPixelPolicy, GpuShaderLanguage,
    GpuTerminalChain, PQ_LINEARIZATION_POLICY_CONTEXT_KEY, REFERENCE_WHITE_NITS_CONTEXT_KEY,
    RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY,
};

fn pq_context(reference_white_nits: &str) -> ColorContext {
    ColorContext::default()
        .with_variable(REFERENCE_WHITE_NITS_CONTEXT_KEY, reference_white_nits)
        .with_variable(
            PQ_LINEARIZATION_POLICY_CONTEXT_KEY,
            RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY,
        )
}

fn assert_gpu_matches_cpu_identity(request: &ColorTransformRequest) {
    let backend = BuiltinColorTransform;
    let cpu = backend.create_cpu_processor(request).unwrap();
    let gpu = backend
        .extract_gpu_transform(request, GpuShaderLanguage::Glsl)
        .unwrap();
    assert_eq!(
        gpu.compiled_transform_identity(),
        cpu.compiled_transform_identity()
    );
    assert_eq!(gpu.language(), GpuShaderLanguage::Glsl);
    assert!(gpu.source().contains(gpu.entry_point()));
    assert!(gpu.source().contains(gpu.domain_entry_point()));
    assert!(gpu.luts().is_empty());
    let contract = gpu.pixel_contract();
    assert_eq!(contract.input_alpha, AlphaRepresentation::Straight);
    assert_eq!(contract.output_alpha, AlphaRepresentation::Straight);
    assert_eq!(contract.component_storage, ComponentStorage::Float32);
    assert_eq!(
        contract.invalid_pixel_policy,
        GpuInvalidPixelPolicy::RejectFrame
    );
}

#[test]
fn glsl_programs_retain_the_exact_cpu_processor_identity_for_standard_boundaries() {
    let requests = [
        ColorTransformRequest::explicit("srgb", "linear-srgb"),
        ColorTransformRequest::explicit("linear-display-p3", "display-p3"),
        ColorTransformRequest::source_to_working("rec2020-sdr-10", "linear-srgb"),
        ColorTransformRequest::working_to_display("linear-srgb", "srgb"),
        ColorTransformRequest::working_to_output("linear-srgb", "rec2020-sdr-12"),
        ColorTransformRequest::source_to_working("rec2100-hlg", "linear-rec2020"),
        ColorTransformRequest::working_to_output("linear-rec2020", "rec2100-hlg"),
        ColorTransformRequest::source_to_working("rec2100-pq", "linear-rec2020")
            .with_context(pq_context("100")),
        ColorTransformRequest::working_to_output("linear-rec2020", "rec2100-pq")
            .with_context(pq_context("203")),
    ];
    for request in requests {
        assert_gpu_matches_cpu_identity(&request);
    }
}

#[test]
fn every_cpu_compilable_standard_program_has_the_same_gpu_identity() {
    let backend = BuiltinColorTransform;
    for source in crate::StandardColorSpaceId::ALL {
        for destination in crate::StandardColorSpaceId::ALL {
            let request = ColorTransformRequest::explicit(source.as_str(), destination.as_str())
                .with_context(pq_context("100"));
            if backend.create_cpu_processor(&request).is_ok() {
                assert_gpu_matches_cpu_identity(&request);
            }
        }
    }

    let working_spaces = [
        "linear-srgb",
        "linear-bt709",
        "linear-display-p3",
        "linear-rec2020",
    ];
    for working in working_spaces {
        for output in crate::StandardColorSpaceId::ALL {
            let request = ColorTransformRequest::working_to_output(working, output.as_str())
                .with_context(pq_context("203"));
            if backend.create_cpu_processor(&request).is_ok() {
                assert_gpu_matches_cpu_identity(&request);
            }
        }
    }
}

#[test]
fn generated_names_are_stable_and_separate_context_dependent_programs() {
    let backend = BuiltinColorTransform;
    let request = ColorTransformRequest::working_to_output("linear-rec2020", "rec2100-pq")
        .with_context(pq_context("100"));
    let same = backend
        .extract_gpu_transform(&request, GpuShaderLanguage::Glsl)
        .unwrap();
    let again = backend
        .extract_gpu_transform(&request, GpuShaderLanguage::Glsl)
        .unwrap();
    let other = backend
        .extract_gpu_transform(
            &ColorTransformRequest::working_to_output("linear-rec2020", "rec2100-pq")
                .with_context(pq_context("203")),
            GpuShaderLanguage::Glsl,
        )
        .unwrap();

    assert_eq!(same.entry_point(), again.entry_point());
    assert_eq!(same.source(), again.source());
    assert_ne!(same.entry_point(), other.entry_point());
    assert_ne!(same.source(), other.source());
}

#[test]
fn unsupported_shader_languages_are_reported_instead_of_approximated() {
    let backend = BuiltinColorTransform;
    let request = ColorTransformRequest::working_to_display("linear-srgb", "srgb");
    for language in [GpuShaderLanguage::SkSl, GpuShaderLanguage::Wgsl] {
        assert!(matches!(
            backend.extract_gpu_transform(&request, language),
            Err(crate::ColorManagementError::GpuTransformUnavailable { .. })
        ));
    }
}

#[test]
fn terminal_chain_is_rooted_in_the_exact_working_owner_and_is_continuous() {
    let backend = BuiltinColorTransform;
    let context = ColorContext::default();
    let verified = backend
        .verify_working_space("linear-srgb", &context)
        .unwrap();
    let working = crate::WorkingColorIdentity::from_verified("project-config", verified).unwrap();
    let first = backend
        .extract_gpu_transform(
            &ColorTransformRequest::working_to_display("linear-srgb", "display-p3"),
            GpuShaderLanguage::Glsl,
        )
        .unwrap();
    let second = backend
        .extract_gpu_transform(
            &ColorTransformRequest::explicit("display-p3", "srgb"),
            GpuShaderLanguage::Glsl,
        )
        .unwrap();
    let chain = GpuTerminalChain::new(working.clone(), vec![first.clone(), second]).unwrap();
    assert_eq!(chain.working_identity(), &working);
    assert_eq!(chain.language(), GpuShaderLanguage::Glsl);
    assert_eq!(chain.stages().len(), 2);

    let discontinuous = backend
        .extract_gpu_transform(
            &ColorTransformRequest::explicit("linear-srgb", "srgb"),
            GpuShaderLanguage::Glsl,
        )
        .unwrap();
    assert!(GpuTerminalChain::new(working, vec![first, discontinuous]).is_err());
}
