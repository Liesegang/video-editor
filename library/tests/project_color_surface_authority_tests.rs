#![allow(
    clippy::expect_used,
    reason = "integration tests use expect messages as assertion diagnostics"
)]

use anyhow::{Context, Result};
use library::model::authoring::AuthoringProject;
use library::model::project::{
    ColorConfigIdentity, ColorManagementConfig, ColorManagementIssue, ExportColorConfig,
    PreviewColorConfig, PreviewSurfaceEncoding, ResolvedColorManagementConfig,
};
use serde_json::{Value, json};

const EXACT_OCIO_CONFIG: &str = "ocio://studio-config-v4.0.0_aces-v2.0_ocio-v2.5";
const EXACT_OCIO_VIEW_OUTPUT: &str = "sRGB Encoded Rec.709 (sRGB)";

fn test_project(name: &str) -> AuthoringProject {
    AuthoringProject::new(name, 1920, 1080, 30.0, 60.0).expect("valid test Timeline")
}

fn load_project(source: &str) -> Result<AuthoringProject> {
    Ok(serde_json::from_str(source)?)
}

trait TestProjectJson {
    fn save(&self) -> Result<String>;
}

impl TestProjectJson for AuthoringProject {
    fn save(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }
}

fn bound_config(identity: ColorConfigIdentity) -> ColorManagementConfig {
    ColorManagementConfig::new(
        identity,
        "ACEScg",
        PreviewColorConfig::named_view(
            "sRGB - Display",
            "ACES 2.0 - SDR 100 nits",
            EXACT_OCIO_VIEW_OUTPUT,
            PreviewSurfaceEncoding::Srgb,
        ),
        ExportColorConfig::new("Output - Rec.2020"),
    )
    .with_srgb_surface_space(EXACT_OCIO_VIEW_OUTPUT)
}

#[test]
fn custom_config_without_srgb_surface_binding_is_losslessly_repairable() -> Result<()> {
    let base = test_project("repair custom surface binding");
    let mut value = serde_json::to_value(&base)?;
    let raw = json!({
        "config": {
            "kind": "ocio_builtin",
            "uri": EXACT_OCIO_CONFIG,
            "ocio_version": "2.5.2"
        },
        "working_space": "ACEScg",
        "preview": {
            "display": "sRGB - Display",
            "view": "ACES 2.0 - SDR 100 nits",
            "surface_encoding": "srgb",
            "view_output_color_space": EXACT_OCIO_VIEW_OUTPUT
        },
        "export": { "output_space": "Output - Rec.2020" }
    });
    value["color_management"] = raw.clone();

    let loaded = load_project(&serde_json::to_string(&value)?)?;
    let requested = loaded
        .requested_color_management_config()
        .context("structurally valid custom config")?;
    assert_eq!(requested.srgb_surface_space(), None);
    assert!(
        loaded
            .color_management_diagnostics()
            .contains(&ColorManagementIssue::MissingSrgbSurfaceColorSpaceBinding)
    );
    assert!(matches!(
        loaded.resolved_color_management(),
        ResolvedColorManagementConfig::Unavailable { .. }
    ));
    let saved: Value = serde_json::from_str(&loaded.save()?)?;
    assert_eq!(saved["color_management"], raw);
    Ok(())
}

#[test]
fn custom_srgb_surface_binding_persists_its_exact_config_authority() -> Result<()> {
    let identity = ColorConfigIdentity::OcioBuiltin {
        uri: EXACT_OCIO_CONFIG.to_string(),
        ocio_version: "2.5.2".to_string(),
    };
    let mut project = test_project("bound custom surface");
    project
        .set_color_management(bound_config(identity.clone()))
        .map_err(|issues| anyhow::anyhow!("valid binding rejected: {issues:?}"))?;

    let saved: Value = serde_json::from_str(&project.save()?)?;
    let binding = &saved["color_management"]["srgb_surface_space"];
    assert_eq!(binding["color_space"], EXACT_OCIO_VIEW_OUTPUT);
    assert_eq!(binding["config"], serde_json::to_value(identity)?);
    let loaded = load_project(&serde_json::to_string(&saved)?)?;
    let resolution = loaded.resolved_color_management();
    let exact = resolution
        .model_validated_intent()
        .context("bound config remains ready")?
        .srgb_surface_space()?;
    assert_eq!(exact.color_space(), EXACT_OCIO_VIEW_OUTPUT);
    Ok(())
}

#[test]
fn surface_binding_changes_cache_identity_and_rejects_foreign_config_authority() -> Result<()> {
    let identity = ColorConfigIdentity::OcioBuiltin {
        uri: EXACT_OCIO_CONFIG.to_string(),
        ocio_version: "2.5.2".to_string(),
    };
    let mut first = test_project("first surface binding");
    first
        .set_color_management(bound_config(identity.clone()))
        .map_err(|issues| anyhow::anyhow!("first binding rejected: {issues:?}"))?;
    let first_resolution = first.resolved_color_management();
    let first_cache = first_resolution
        .model_validated_intent()
        .context("first binding ready")?
        .cache_identity()
        .to_string();

    let mut second = test_project("second surface binding");
    second
        .set_color_management(bound_config(identity).with_srgb_surface_space("show-ui-srgb-v2"))
        .map_err(|issues| anyhow::anyhow!("second binding rejected: {issues:?}"))?;
    let second_resolution = second.resolved_color_management();
    let second_cache = second_resolution
        .model_validated_intent()
        .context("second binding ready")?
        .cache_identity();
    assert_ne!(first_cache, second_cache);

    let mut foreign = serde_json::to_value(&first)?;
    foreign["color_management"]["srgb_surface_space"]["config"] = json!({
        "kind": "ocio_builtin",
        "uri": "ocio://foreign-show-v1.0_ocio-v2.5",
        "ocio_version": "2.5.2"
    });
    let loaded = load_project(&serde_json::to_string(&foreign)?)?;
    assert!(loaded.color_management_diagnostics().iter().any(|issue| {
        matches!(
            issue,
            ColorManagementIssue::SrgbSurfaceColorSpaceBindingMismatch { bound, project }
                if bound != project
        )
    }));
    assert!(matches!(
        loaded.resolved_color_management(),
        ResolvedColorManagementConfig::Unavailable { .. }
    ));
    let repaired_later: Value = serde_json::from_str(&loaded.save()?)?;
    assert_eq!(
        repaired_later["color_management"]["srgb_surface_space"]["config"],
        foreign["color_management"]["srgb_surface_space"]["config"]
    );
    Ok(())
}
