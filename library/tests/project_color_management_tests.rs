use anyhow::Result;
use library::model::project::{
    ColorConfigIdentity, ColorManagementConfig, ColorManagementField, ColorManagementIssue,
    DEFAULT_BUNDLED_COLOR_CONFIG_ID, DEFAULT_OUTPUT_COLOR_SPACE, DEFAULT_PREVIEW_DISPLAY,
    DEFAULT_PREVIEW_VIEW, DEFAULT_WORKING_COLOR_SPACE, ExportColorConfig, PreviewColorConfig,
    Project,
};
use library::model::{Asset, AssetKind};
use serde_json::{Value, json};

const EXACT_OCIO_CONFIG: &str = "ocio://studio-config-v4.0.0_aces-v2.0_ocio-v2.5";
const CONFIG_SHA256: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn aces_config(identity: ColorConfigIdentity) -> ColorManagementConfig {
    ColorManagementConfig::new(
        identity,
        "ACEScg",
        PreviewColorConfig::new("sRGB - Display", "ACES 2.0 - SDR 100 nits"),
        ExportColorConfig::new("Output - Rec.2020"),
    )
}

#[test]
fn project_default_is_explicit_stable_and_roundtrips_without_schema_version() -> Result<()> {
    let project = Project::new("color defaults");
    let requested = project.requested_color_management();
    assert_eq!(
        requested.config(),
        &ColorConfigIdentity::Bundled {
            id: DEFAULT_BUNDLED_COLOR_CONFIG_ID.to_string(),
        }
    );
    assert_eq!(requested.working_space(), DEFAULT_WORKING_COLOR_SPACE);
    assert_eq!(requested.preview().display(), DEFAULT_PREVIEW_DISPLAY);
    assert_eq!(requested.preview().view(), DEFAULT_PREVIEW_VIEW);
    assert_eq!(
        requested.export().output_space(),
        DEFAULT_OUTPUT_COLOR_SPACE
    );
    assert!(project.color_management_diagnostics().is_empty());

    let encoded = project.save()?;
    let value: Value = serde_json::from_str(&encoded)?;
    assert!(value.get("color_management").is_some());
    assert!(value.get("schema_version").is_none());
    assert!(value.get("migration").is_none());
    assert_eq!(Project::load(&encoded)?, project);
    Ok(())
}

#[test]
fn project_without_color_management_loads_with_current_safe_defaults() -> Result<()> {
    let project = Project::new("old pre-v1 project");
    let mut value = serde_json::to_value(&project)?;
    value
        .as_object_mut()
        .expect("Project serialization is an object")
        .remove("color_management");

    let loaded = Project::load(&serde_json::to_string(&value)?)?;
    assert_eq!(
        loaded.requested_color_management(),
        &ColorManagementConfig::default()
    );
    assert!(!loaded.resolved_color_management().used_safe_fallback());
    Ok(())
}

#[test]
fn partial_color_management_object_uses_field_defaults_without_a_migration() -> Result<()> {
    let project = Project::new("partial pre-v1 color config");
    let mut value = serde_json::to_value(&project)?;
    value["color_management"] = json!({
        "config": {
            "kind": "bundled",
            "id": DEFAULT_BUNDLED_COLOR_CONFIG_ID
        }
    });

    let loaded = Project::load(&serde_json::to_string(&value)?)?;
    assert_eq!(
        loaded.requested_color_management(),
        &ColorManagementConfig::default()
    );
    assert!(loaded.color_management_diagnostics().is_empty());
    Ok(())
}

#[test]
fn exact_ocio_registry_config_and_non_srgb_spaces_roundtrip() -> Result<()> {
    let mut project = Project::new("aces project");
    let config = aces_config(ColorConfigIdentity::OcioBuiltin {
        uri: EXACT_OCIO_CONFIG.to_string(),
        ocio_version: "2.5.2".to_string(),
    });
    project
        .set_color_management(config.clone())
        .map_err(|issues| anyhow::anyhow!("unexpected color issues: {issues:?}"))?;

    let loaded = Project::load(&project.save()?)?;
    assert_eq!(loaded.requested_color_management(), &config);
    assert_eq!(loaded.resolved_color_management().effective(), &config);
    assert!(loaded.color_management_diagnostics().is_empty());
    Ok(())
}

#[test]
fn external_ocio_config_is_pinned_to_project_asset_and_checksum() -> Result<()> {
    let mut project = Project::new("external config");
    let asset = Asset::new("show config", "color/show-v12.ocio", AssetKind::Other);
    let asset_id = asset.id;
    project.assets.push(asset);
    let config = aces_config(ColorConfigIdentity::ProjectAsset {
        asset_id,
        sha256: CONFIG_SHA256.to_string(),
        ocio_version: "2.5.2".to_string(),
    });
    project
        .set_color_management(config.clone())
        .map_err(|issues| anyhow::anyhow!("unexpected color issues: {issues:?}"))?;

    let encoded = project.save()?;
    let loaded = Project::load(&encoded)?;
    assert_eq!(loaded.requested_color_management(), &config);
    assert!(!loaded.resolved_color_management().used_safe_fallback());
    Ok(())
}

#[test]
fn setter_rejects_moving_or_blank_identifiers_without_mutating_project() {
    let mut project = Project::new("invalid candidate");
    let original = project.requested_color_management().clone();
    let candidate = ColorManagementConfig::new(
        ColorConfigIdentity::OcioBuiltin {
            uri: "ocio://default".to_string(),
            ocio_version: "latest".to_string(),
        },
        " ",
        PreviewColorConfig::new("", " "),
        ExportColorConfig::new(""),
    );

    let issues = project
        .set_color_management(candidate)
        .expect_err("moving and blank identifiers must be rejected");
    assert!(issues.iter().any(|issue| matches!(
        issue,
        ColorManagementIssue::MovingConfigIdentifier { identifier }
            if identifier == "ocio://default"
    )));
    assert!(issues.iter().any(|issue| matches!(
        issue,
        ColorManagementIssue::InvalidOcioVersion { version } if version == "latest"
    )));
    for field in [
        ColorManagementField::WorkingSpace,
        ColorManagementField::PreviewDisplay,
        ColorManagementField::PreviewView,
        ColorManagementField::OutputSpace,
    ] {
        assert!(issues.contains(&ColorManagementIssue::BlankIdentifier { field }));
    }
    assert_eq!(project.requested_color_management(), &original);
}

#[test]
fn invalid_persisted_color_request_loads_with_diagnostics_and_safe_runtime_fallback() -> Result<()>
{
    let project = Project::new("repairable project");
    let mut value = serde_json::to_value(&project)?;
    value["color_management"] = json!({
        "config": {
            "kind": "ocio_builtin",
            "uri": "ocio://default",
            "ocio_version": "2.5"
        },
        "working_space": "",
        "preview": { "display": "sRGB - Display", "view": "" },
        "export": { "output_space": "Output - Rec.709" }
    });

    let loaded = Project::load(&serde_json::to_string(&value)?)?;
    let diagnostics = loaded.color_management_diagnostics();
    assert!(!diagnostics.is_empty());
    assert!(
        loaded.validation_issues().is_empty(),
        "non-fatal color diagnostics must not reject Project adoption"
    );
    let resolved = loaded.resolved_color_management();
    assert!(resolved.used_safe_fallback());
    assert_eq!(resolved.diagnostics(), diagnostics);
    assert_eq!(resolved.effective(), &ColorManagementConfig::default());
    Ok(())
}

#[test]
fn missing_asset_or_bad_checksum_is_non_fatal_but_never_becomes_effective() -> Result<()> {
    let project = Project::new("missing external config");
    let missing_id = uuid::Uuid::new_v4();
    let mut value = serde_json::to_value(&project)?;
    value["color_management"] = json!({
        "config": {
            "kind": "project_asset",
            "asset_id": missing_id,
            "sha256": "not-a-checksum",
            "ocio_version": "2.5.2"
        },
        "working_space": "ACEScg",
        "preview": { "display": "sRGB - Display", "view": "ACES 2.0" },
        "export": { "output_space": "Output - Rec.2020" }
    });

    let loaded = Project::load(&serde_json::to_string(&value)?)?;
    let diagnostics = loaded.color_management_diagnostics();
    assert!(
        diagnostics.contains(&ColorManagementIssue::ConfigAssetNotFound {
            asset_id: missing_id,
        })
    );
    assert!(diagnostics.iter().any(|issue| matches!(
        issue,
        ColorManagementIssue::InvalidConfigChecksum { asset_id, .. } if *asset_id == missing_id
    )));
    assert!(loaded.resolved_color_management().used_safe_fallback());
    Ok(())
}

#[test]
fn ocio_registry_uri_must_include_an_exact_registry_version() {
    let mut project = Project::new("unpinned ocio");
    let candidate = aces_config(ColorConfigIdentity::OcioBuiltin {
        uri: "ocio://studio-config-aces".to_string(),
        ocio_version: "2.5.2".to_string(),
    });
    let issues = project
        .set_color_management(candidate)
        .expect_err("unversioned OCIO registry URI must be rejected");
    assert!(
        issues.contains(&ColorManagementIssue::UnpinnedOcioBuiltinUri {
            uri: "ocio://studio-config-aces".to_string(),
        })
    );
}

#[test]
fn ocio_processor_version_must_pin_a_patch_release() {
    let mut project = Project::new("unpinned ocio processor");
    let candidate = aces_config(ColorConfigIdentity::OcioBuiltin {
        uri: EXACT_OCIO_CONFIG.to_string(),
        ocio_version: "2.5".to_string(),
    });
    let issues = project
        .set_color_management(candidate)
        .expect_err("an OCIO major/minor family must not be treated as an exact version");
    assert!(issues.contains(&ColorManagementIssue::InvalidOcioVersion {
        version: "2.5".to_string(),
    }));
}
