#![cfg(feature = "opencolorio")]

use std::error::Error;

use ruvie_color_management::{
    BackendBuild, ColorContext, ColorTransformBackend, ColorTransformRequest,
    ManagedLinearWorkingImage, OcioColorTransformBackend, WorkingColorIdentity,
};

const REQUIRE_REAL_OCIO_ENV: &str = "RUVIE_REQUIRE_REAL_OCIO";
const SOURCE_SPACE: &str = "fixture-source";
const WORKING_SPACE: &str = "fixture-linear-working";
// OCIO's optimized f32 exponent renderer is allowed a small approximation
// error relative to the direct f32 `powi` oracle.
const PROCESSOR_TOLERANCE: f32 = 2.0e-5;

// This config is deliberately self-contained: a nonlinear exponent followed
// by a channel-mixing matrix exercises real OCIO processor evaluation without
// relying on a built-in config, environment substitution, or an external LUT.
const NON_IDENTITY_CONFIG: &[u8] = br#"ocio_profile_version: 2

search_path: ""
strictparsing: true

roles:
  default: fixture-source
  scene_linear: fixture-linear-working

displays:
  fixture-display:
    - !<View> {name: fixture-view, colorspace: fixture-linear-working}

active_displays: [fixture-display]
active_views: [fixture-view]

colorspaces:
  - !<ColorSpace>
    name: fixture-linear-working
    family: fixture
    bitdepth: 32f
    isdata: false
    allocation: uniform

  - !<ColorSpace>
    name: fixture-source
    family: fixture
    bitdepth: 32f
    isdata: false
    allocation: uniform
    to_scene_reference: !<GroupTransform>
      children:
        - !<ExponentTransform> {value: 2.0, style: pass_thru}
        - !<MatrixTransform> {matrix: [1.25, 0.10, 0.00, 0.00, 0.00, 0.75, 0.20, 0.00, 0.05, 0.00, 1.50, 0.00, 0.00, 0.00, 0.00, 1.00], offset: [0.01, 0.02, -0.03, 0.00]}
"#;

#[test]
fn real_ocio_executes_non_identity_numeric_oracle() -> Result<(), Box<dyn Error>> {
    if ocio_rs::is_stub_build() {
        assert_ne!(
            std::env::var(REQUIRE_REAL_OCIO_ENV).as_deref(),
            Ok("1"),
            "the mandatory real-OpenColorIO numeric gate linked the ocio-rs stub"
        );
        eprintln!("skipped: ordinary test build uses the ocio-rs stub");
        return Ok(());
    }

    let runtime_version = ocio_rs::version().ok_or("real OCIO did not report its version")?;
    let backend =
        OcioColorTransformBackend::from_exact_bytes(NON_IDENTITY_CONFIG, runtime_version.as_str())?;
    let context = ColorContext::default();
    let source = backend.verify_source_space(SOURCE_SPACE, &context)?;
    let working = backend.verify_working_space(WORKING_SPACE, &context)?;
    let identity = WorkingColorIdentity::from_verified("real-ocio-numeric-fixture", working)?;
    let request =
        ColorTransformRequest::source_to_working(SOURCE_SPACE, WORKING_SPACE).with_context(context);
    let processor = backend.create_cpu_processor(&request)?;

    assert_eq!(
        processor.compiled_transform_identity().backend_build(),
        BackendBuild::Real
    );

    let source_rgba = [
        [0.20, 0.40, 0.60, 0.25],
        [0.75, 0.25, 0.50, 1.00],
        [0.90, 0.80, 0.70, 0.00],
    ];
    let expected_straight =
        source_rgba.map(|rgba| oracle_source_to_working([rgba[0], rgba[1], rgba[2]]));

    let mut actual_rgb = source_rgba.map(|rgba| [rgba[0], rgba[1], rgba[2]]);
    processor.transform_rgb_f32_in_place(&mut actual_rgb)?;
    for (actual, expected) in actual_rgb.iter().zip(expected_straight) {
        assert_rgb_close(*actual, expected);
    }
    assert_ne!(
        actual_rgb[0],
        [source_rgba[0][0], source_rgba[0][1], source_rgba[0][2]]
    );

    let image = ManagedLinearWorkingImage::from_straight_rgba_f32(
        identity,
        &source,
        3,
        1,
        &source_rgba,
        processor.as_ref(),
    )?;
    let pixels = image.pixels().pixels();
    assert_rgba_close(
        pixels[0],
        [
            expected_straight[0][0] * 0.25,
            expected_straight[0][1] * 0.25,
            expected_straight[0][2] * 0.25,
            0.25,
        ],
    );
    assert_rgba_close(
        pixels[1],
        [
            expected_straight[1][0],
            expected_straight[1][1],
            expected_straight[1][2],
            1.0,
        ],
    );
    assert_eq!(pixels[2], [0.0; 4]);

    Ok(())
}

fn oracle_source_to_working(rgb: [f32; 3]) -> [f32; 3] {
    // Independent expression of the authored OCIO GroupTransform: exponent
    // first, then the row-major matrix and offset.
    let [r, g, b] = rgb.map(|value| value.powi(2));
    [
        1.25 * r + 0.10 * g + 0.01,
        0.75 * g + 0.20 * b + 0.02,
        0.05 * r + 1.50 * b - 0.03,
    ]
}

fn assert_rgb_close(actual: [f32; 3], expected: [f32; 3]) {
    for (channel, (actual, expected)) in ["r", "g", "b"]
        .into_iter()
        .zip(actual.into_iter().zip(expected))
    {
        assert!(
            (actual - expected).abs() <= PROCESSOR_TOLERANCE,
            "{channel}: expected {expected:.9}, got {actual:.9}"
        );
    }
}

fn assert_rgba_close(actual: [f32; 4], expected: [f32; 4]) {
    for (channel, (actual, expected)) in ["r", "g", "b", "a"]
        .into_iter()
        .zip(actual.into_iter().zip(expected))
    {
        assert!(
            (actual - expected).abs() <= PROCESSOR_TOLERANCE,
            "{channel}: expected {expected:.9}, got {actual:.9}"
        );
    }
}
