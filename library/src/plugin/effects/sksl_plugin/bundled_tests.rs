use super::*;
use crate::model::property::{ColorSpaceRef, ColorValue};
use crate::plugin::PluginManager;
use ordered_float::OrderedFloat;
use ruvie_color_management::{
    BuiltinColorTransform, ColorContext, ColorTransformBackend, LINEAR_SRGB_SPACE_ID,
    LinearWorkingImage, ManagedLinearWorkingImage, WorkingColorIdentity,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const BUNDLED_EFFECT_IDS: [&str; 25] = [
    "brightness_contrast",
    "chroma_key",
    "chromatic_aberration",
    "color_balance",
    "diagonal_clip",
    "edge_detection",
    "flash",
    "four_color_gradient",
    "halftone",
    "hsv_adjust",
    "lens_distortion",
    "levels",
    "mirror",
    "mosaic",
    "noise",
    "pixelate",
    "polar_coordinates",
    "raster_wave",
    "ripple",
    "silhouette",
    "speed_lines",
    "split_screen",
    "tiling",
    "vignette",
    "zoom_blur",
];

fn bundled_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/plugins/sksl")
}

fn bundled_sources(id: &str) -> (String, String) {
    let directory = bundled_root().join(id);
    let read = |name: &str| {
        let path = directory.join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
    };
    (read("config.toml"), read("shader.sksl"))
}

fn managed_pixel(pixel: [f32; 4]) -> RenderOutput {
    let backend = BuiltinColorTransform;
    let working = backend
        .verify_working_space(LINEAR_SRGB_SPACE_ID, &ColorContext::default())
        .expect("verify test working space");
    let identity = WorkingColorIdentity::from_verified("bundled-sksl-linear-test", working)
        .expect("working identity");
    let pixels = LinearWorkingImage::from_premultiplied_rgba_f32(1, 1, vec![pixel])
        .expect("valid RGBAF32 test image");
    // SAFETY: The fixture is authored directly in the verified linear-sRGB
    // identity and all samples use premultiplied alpha.
    RenderOutput::Working(unsafe {
        ManagedLinearWorkingImage::from_working_pixels_unchecked(identity, pixels)
    })
}

fn working_color(rgba: [f64; 4]) -> PropertyValue {
    PropertyValue::ColorValue(
        ColorValue::new(ColorSpaceRef::linear_srgb(), rgba).expect("valid working test color"),
    )
}

fn vec2(x: f64, y: f64) -> PropertyValue {
    PropertyValue::Vec2(crate::model::property::Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    })
}

fn default_params(plugin: &SkslEffectPlugin) -> HashMap<String, PropertyValue> {
    plugin
        .properties()
        .into_iter()
        .map(|definition| {
            let value = match definition.default_value() {
                // Descriptor defaults are authored sRGB values. RenderService
                // normally performs this conversion before invoking a
                // project-linear effect. All bundled defaults use exact 0/1
                // primaries, so retagging is numerically exact in this test.
                PropertyValue::ColorValue(color) => working_color(color.rgba()),
                value => value.clone(),
            };
            (definition.name().to_string(), value)
        })
        .collect()
}

fn apply_bundled(
    id: &str,
    input: &RenderOutput,
    overrides: impl IntoIterator<Item = (&'static str, PropertyValue)>,
) -> RenderOutput {
    let (config, shader) = bundled_sources(id);
    let plugin = SkslEffectPlugin::new(&config, &shader)
        .unwrap_or_else(|error| panic!("bundled effect {id} does not compile: {error}"));
    assert_eq!(
        plugin.color_domain(),
        EffectColorDomain::ProjectLinearPreserving,
        "bundled effect {id} must explicitly accept Project-linear RGBAF32"
    );
    let mut params = default_params(&plugin);
    params.extend(
        overrides
            .into_iter()
            .map(|(name, value)| (name.to_string(), value)),
    );
    let manager = PluginManager::new();
    manager.register_effect(Arc::new(plugin));
    manager
        .apply_effect(id, input, &params, None)
        .unwrap_or_else(|error| panic!("bundled effect {id} rejected RGBAF32: {error}"))
}

fn output_pixel(output: &RenderOutput) -> [f32; 4] {
    let RenderOutput::Working(output) = output else {
        panic!("bundled effect dropped the managed working frame")
    };
    output.pixels().pixels()[0]
}

fn assert_pixel_near(actual: [f32; 4], expected: [f32; 4]) {
    for (component, (actual, expected)) in actual.into_iter().zip(expected).enumerate() {
        assert!(
            (actual - expected).abs() <= 3.0e-3,
            "component {component}: expected {expected}, got {actual}"
        );
    }
}

#[test]
fn every_bundled_effect_explicitly_declares_and_executes_project_linear() {
    let discovered = std::fs::read_dir(bundled_root())
        .expect("bundled SkSL directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<HashSet<_>>();
    assert_eq!(
        discovered,
        BUNDLED_EFFECT_IDS.into_iter().map(str::to_string).collect(),
        "the contract table must be updated when a bundled effect is added or removed"
    );

    let input = managed_pixel([0.1, 0.2, 0.3, 0.5]);
    let RenderOutput::Working(input_working) = &input else {
        panic!("managed_pixel must create a Project working-space frame")
    };
    for id in BUNDLED_EFFECT_IDS {
        let (config_source, _) = bundled_sources(id);
        assert!(
            config_source
                .lines()
                .any(|line| line.trim() == "color_domain = \"project_linear\""),
            "bundled effect {id} must state its domain; relying on the external-plugin default is forbidden"
        );
        let config: SkslPluginConfig = toml::from_str(&config_source).unwrap();
        assert_eq!(config.color_domain, SkslEffectColorDomain::ProjectLinear);

        let output = apply_bundled(id, &input, []);
        let RenderOutput::Working(output) = output else {
            panic!("bundled effect {id} must preserve the Project working-space boundary")
        };
        assert_eq!(
            output.identity(),
            input_working.identity(),
            "bundled effect {id} changed working identity"
        );
        let pixel = output.pixels().pixels()[0];
        assert!(
            pixel.into_iter().all(f32::is_finite),
            "bundled effect {id} produced a non-finite pixel: {pixel:?}"
        );
        assert!(
            (-1.0e-6..=1.0 + 1.0e-6).contains(&pixel[3]),
            "bundled effect {id} produced invalid straight alpha: {pixel:?}"
        );
        if pixel[3].abs() <= 1.0e-6 {
            assert!(
                pixel[..3].iter().all(|component| component.abs() <= 1.0e-6),
                "bundled effect {id} left hidden RGB in a transparent pixel: {pixel:?}"
            );
        }

        let transparent = output_pixel(&apply_bundled(id, &managed_pixel([0.0; 4]), []));
        assert!(transparent.into_iter().all(f32::is_finite));
        if transparent[3].abs() <= 1.0e-6 {
            assert!(
                transparent[..3]
                    .iter()
                    .all(|component| component.abs() <= 1.0e-6),
                "bundled effect {id} manufactured hidden RGB from transparent input: {transparent:?}"
            );
        }
    }
}

#[test]
fn diagonal_clip_runs_on_rgba_f32_and_preserves_visible_premultiplied_input() {
    let input = managed_pixel([0.1, 0.2, 0.3, 0.5]);
    let visible = apply_bundled(
        "diagonal_clip",
        &input,
        [
            ("position", PropertyValue::from(-2.0)),
            ("softness", PropertyValue::from(0.0)),
            ("resolution", vec2(1.0, 1.0)),
        ],
    );
    assert_pixel_near(output_pixel(&visible), [0.1, 0.2, 0.3, 0.5]);

    let inverted = apply_bundled(
        "diagonal_clip",
        &input,
        [
            ("position", PropertyValue::from(-2.0)),
            ("softness", PropertyValue::from(0.0)),
            ("invert", PropertyValue::Boolean(true)),
            ("resolution", vec2(1.0, 1.0)),
        ],
    );
    assert_pixel_near(output_pixel(&inverted), [0.0; 4]);
}

#[test]
fn straight_rgb_effects_premultiply_once_and_canonicalize_transparency() {
    let input = managed_pixel([0.1, 0.2, 0.3, 0.5]);
    let bright = apply_bundled(
        "brightness_contrast",
        &input,
        [
            ("brightness", PropertyValue::from(0.2)),
            ("contrast", PropertyValue::from(1.0)),
        ],
    );
    assert_pixel_near(output_pixel(&bright), [0.2, 0.3, 0.4, 0.5]);

    let flash = apply_bundled(
        "flash",
        &input,
        [
            ("intensity", PropertyValue::from(0.25)),
            ("color", working_color([1.0, 1.0, 1.0, 1.0])),
        ],
    );
    assert_pixel_near(output_pixel(&flash), [0.225, 0.325, 0.425, 0.5]);

    for id in [
        "brightness_contrast",
        "chroma_key",
        "chromatic_aberration",
        "color_balance",
        "flash",
        "hsv_adjust",
        "levels",
        "noise",
    ] {
        let output = apply_bundled(id, &managed_pixel([0.0; 4]), []);
        assert_eq!(
            output_pixel(&output),
            [0.0; 4],
            "{id} manufactured hidden RGB from transparent input"
        );
    }
}

#[test]
fn generated_working_color_is_premultiplied_once_without_clipping_hdr_rgb() {
    let input = managed_pixel([0.0; 4]);
    let color = working_color([2.0, 0.5, -0.25, 0.25]);
    let output = apply_bundled(
        "four_color_gradient",
        &input,
        [
            ("colTL", color.clone()),
            ("colTR", color.clone()),
            ("colBL", color.clone()),
            ("colBR", color),
            ("resolution", vec2(1.0, 1.0)),
        ],
    );
    assert_pixel_near(output_pixel(&output), [0.5, 0.125, -0.0625, 0.25]);
}

#[test]
fn external_plugin_without_domain_declaration_stays_on_legacy_boundary() {
    let config = r#"
id = "external-default"
name = "External default"
category = "Test"
properties = []
"#;
    let plugin = SkslEffectPlugin::new(config, "half4 main(float2 p) { return half4(0); }")
        .expect("valid external shader");
    assert_eq!(
        plugin.color_domain(),
        EffectColorDomain::UnmanagedSrgba8Only
    );
}

#[test]
fn bundled_source_files_are_regular_files_below_repository_limit() {
    for id in BUNDLED_EFFECT_IDS {
        for name in ["config.toml", "shader.sksl"] {
            let path = bundled_root().join(id).join(name);
            assert!(Path::new(&path).is_file(), "missing {}", path.display());
            let lines = std::fs::read_to_string(&path).unwrap().lines().count();
            assert!(lines < 1_000, "{} has {lines} lines", path.display());
        }
    }
}
