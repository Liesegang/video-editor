use super::{LinearWorkingImage, pack_straight_rgba8};
use crate::{
    BuiltinColorTransform, ColorContext, ColorTransformBackend, ColorTransformRequest,
    CpuColorProcessor, LINEAR_SRGB_SPACE_ID, PQ_LINEARIZATION_POLICY_CONTEXT_KEY,
    REFERENCE_WHITE_NITS_CONTEXT_KEY, RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY, StandardColorSpaceId,
};

fn output_processor(destination: StandardColorSpaceId) -> Box<dyn CpuColorProcessor> {
    let context = ColorContext::default()
        .with_variable(
            PQ_LINEARIZATION_POLICY_CONTEXT_KEY,
            RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY,
        )
        .with_variable(REFERENCE_WHITE_NITS_CONTEXT_KEY, "203");
    BuiltinColorTransform
        .create_cpu_processor(
            &ColorTransformRequest::working_to_output(LINEAR_SRGB_SPACE_ID, destination.as_str())
                .with_context(context),
        )
        .unwrap()
}

#[test]
fn fused_terminal_pack_matches_rgba_f32_path_for_all_standard_outputs_and_alpha() {
    let pixel_count = crate::terminal_pack::TERMINAL_PARALLEL_PIXEL_THRESHOLD + 37;
    let pixels = (0..pixel_count)
        .map(|index| {
            let alpha = match index % 5 {
                0 => 0.0,
                1 => 1.0 / 255.0,
                2 => 0.25,
                3 => 0.75,
                _ => 1.0,
            };
            let level = match index % 4 {
                0 => 0.0,
                1 => 0.018,
                2 => 0.18,
                _ => 1.0,
            };
            [level * alpha, level * alpha, level * alpha, alpha]
        })
        .collect();
    let image = LinearWorkingImage::from_premultiplied_rgba_f32(
        u32::try_from(pixel_count).unwrap(),
        1,
        pixels,
    )
    .unwrap();

    for destination in StandardColorSpaceId::ALL {
        let processor = output_processor(destination);
        let straight = image.to_straight_rgba_f32(processor.as_ref()).unwrap();
        let expected = pack_straight_rgba8(image.width(), image.height(), &straight).unwrap();
        let actual = image.to_straight_rgba8(processor.as_ref()).unwrap();
        assert_eq!(actual, expected, "output {}", destination.as_str());
    }
}
