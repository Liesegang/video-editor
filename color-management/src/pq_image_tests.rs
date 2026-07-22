use crate::{
    BuiltinColorTransform, ColorContext, ColorTransformBackend, ColorTransformRequest,
    LINEAR_REC2020_SPACE_ID, ManagedLinearWorkingImage, PQ_LINEARIZATION_POLICY_CONTEXT_KEY,
    REC2100_PQ_SPACE_ID, REFERENCE_WHITE_NITS_CONTEXT_KEY, RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY,
    WorkingColorIdentity,
};

#[test]
fn managed_pq_ingress_retains_the_explicit_display_luminance_policy_in_identity() {
    let backend = BuiltinColorTransform;
    let context = ColorContext::default()
        .with_variable(
            PQ_LINEARIZATION_POLICY_CONTEXT_KEY,
            RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY,
        )
        .with_variable(REFERENCE_WHITE_NITS_CONTEXT_KEY, "100");
    let source = backend
        .verify_source_space(REC2100_PQ_SPACE_ID, &context)
        .unwrap();
    let working = backend
        .verify_working_space(LINEAR_REC2020_SPACE_ID, &context)
        .unwrap();
    let identity = WorkingColorIdentity::from_verified("pq-project", working).unwrap();
    let processor = backend
        .create_cpu_processor(
            &ColorTransformRequest::source_to_working(REC2100_PQ_SPACE_ID, LINEAR_REC2020_SPACE_ID)
                .with_context(context.clone()),
        )
        .unwrap();

    let image = ManagedLinearWorkingImage::from_straight_rgba_f32(
        identity,
        &source,
        1,
        1,
        &[[0.508_078_4, 0.508_078_4, 0.508_078_4, 0.5]],
        processor.as_ref(),
    )
    .unwrap();

    assert_eq!(image.identity().context(), &context);
    let pixel = image.pixels().pixels()[0];
    assert!((pixel[0] - 0.5).abs() < 2.0e-6);
    assert!((pixel[1] - 0.5).abs() < 2.0e-6);
    assert!((pixel[2] - 0.5).abs() < 2.0e-6);
    assert_eq!(pixel[3], 0.5);
}
