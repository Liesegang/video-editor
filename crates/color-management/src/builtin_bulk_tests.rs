use super::{
    BUILTIN_PARALLEL_PIXEL_THRESHOLD, BuiltinColorTransform, ColorManagementError,
    ColorTransformBackend, ColorTransformRequest, CpuColorProcessor, StandardColorSpaceId,
};
use crate::{
    ColorContext, LINEAR_BT709_SPACE_ID, LINEAR_DISPLAY_P3_SPACE_ID, LINEAR_REC2020_SPACE_ID,
    PQ_LINEARIZATION_POLICY_CONTEXT_KEY, REC2100_HLG_SPACE_ID, REC2100_PQ_SPACE_ID,
    REFERENCE_WHITE_NITS_CONTEXT_KEY, RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY,
};

fn hdr_context() -> ColorContext {
    ColorContext::default()
        .with_variable(
            PQ_LINEARIZATION_POLICY_CONTEXT_KEY,
            RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY,
        )
        .with_variable(REFERENCE_WHITE_NITS_CONTEXT_KEY, "203")
}

fn assert_scalar_bulk_exact(processor: &dyn CpuColorProcessor, source_samples: &[[f32; 3]]) {
    let pixel_count = BUILTIN_PARALLEL_PIXEL_THRESHOLD + 37;
    let mut bulk = (0..pixel_count)
        .map(|index| source_samples[index % source_samples.len()])
        .collect::<Vec<_>>();
    let scalar = bulk
        .iter()
        .map(|pixel| {
            processor
                .transform_rgb(pixel.map(f64::from))
                .map(|rgb| rgb.map(|component| component as f32))
        })
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    processor.transform_rgb_f32_in_place(&mut bulk).unwrap();
    assert_eq!(bulk, scalar);
}

#[test]
fn builtin_bulk_is_bit_exact_with_scalar_for_every_compilable_standard_pair() {
    let backend = BuiltinColorTransform;
    let samples = [
        [0.0, 0.0, 0.0],
        [0.001, 0.018, 0.04],
        [0.18, 0.5, 0.75],
        [1.0, 1.0, 1.0],
    ];
    let mut compiled_pairs = 0;
    for source in StandardColorSpaceId::ALL {
        for destination in StandardColorSpaceId::ALL {
            let request = ColorTransformRequest::explicit(source.as_str(), destination.as_str())
                .with_context(hdr_context());
            let Ok(processor) = backend.create_cpu_processor(&request) else {
                continue;
            };
            assert_scalar_bulk_exact(processor.as_ref(), &samples);
            compiled_pairs += 1;
        }
    }
    assert!(
        compiled_pairs >= 100,
        "unexpectedly narrow standard-space coverage"
    );
}

#[test]
fn builtin_bulk_is_bit_exact_for_supported_pq_and_hlg_boundaries() {
    let backend = BuiltinColorTransform;
    let encoded_samples = [[0.0; 3], [0.18; 3], [0.5; 3], [1.0; 3]];
    let linear_samples = [[0.0; 3], [0.01; 3], [0.18; 3], [1.0; 3]];

    for encoded in [REC2100_PQ_SPACE_ID, REC2100_HLG_SPACE_ID] {
        for working in [
            LINEAR_BT709_SPACE_ID,
            LINEAR_DISPLAY_P3_SPACE_ID,
            LINEAR_REC2020_SPACE_ID,
        ] {
            let processor = backend
                .create_cpu_processor(
                    &ColorTransformRequest::source_to_working(encoded, working)
                        .with_context(hdr_context()),
                )
                .unwrap();
            assert_scalar_bulk_exact(processor.as_ref(), &encoded_samples);
        }
    }

    for working in [
        LINEAR_BT709_SPACE_ID,
        LINEAR_DISPLAY_P3_SPACE_ID,
        LINEAR_REC2020_SPACE_ID,
    ] {
        for encoded in [REC2100_PQ_SPACE_ID, REC2100_HLG_SPACE_ID] {
            let processor = backend
                .create_cpu_processor(
                    &ColorTransformRequest::working_to_output(working, encoded)
                        .with_context(hdr_context()),
                )
                .unwrap();
            assert_scalar_bulk_exact(processor.as_ref(), &linear_samples);
        }
    }
}

#[test]
fn parallel_bulk_reports_the_same_first_error_as_scalar_order() {
    let processor = BuiltinColorTransform
        .create_cpu_processor(
            &ColorTransformRequest::working_to_output(LINEAR_REC2020_SPACE_ID, REC2100_PQ_SPACE_ID)
                .with_context(hdr_context()),
        )
        .unwrap();
    let mut pixels = vec![[0.18; 3]; BUILTIN_PARALLEL_PIXEL_THRESHOLD * 2 + 1];
    pixels[3] = [-0.01, 0.18, 0.18];
    pixels[BUILTIN_PARALLEL_PIXEL_THRESHOLD + 1] = [f32::NAN, 0.0, 0.0];

    let scalar_error = processor
        .transform_rgb(pixels[3].map(f64::from))
        .unwrap_err();
    assert!(matches!(
        scalar_error,
        ColorManagementError::InvalidTransferDomain { .. }
    ));
    for _ in 0..8 {
        let mut bulk = pixels.clone();
        assert_eq!(
            processor.transform_rgb_f32_in_place(&mut bulk),
            Err(scalar_error.clone())
        );
    }
}
