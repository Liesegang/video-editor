use anyhow::{Context, Result};
use library::model::project::{
    ColorConfigIdentity, ColorManagementConfig, ColorManagementField, ColorManagementIssue,
    ColorManagementStructureIssue, DEFAULT_BUNDLED_COLOR_CONFIG_ID, DEFAULT_OUTPUT_COLOR_SPACE,
    DEFAULT_PREVIEW_DISPLAY, DEFAULT_PREVIEW_VIEW, DEFAULT_WORKING_COLOR_SPACE, ExportColorConfig,
    PreviewColorConfig, Project, RequestedColorManagementConfig, ResolvedColorManagementConfig,
};
use library::model::{Asset, AssetKind};
use serde_json::{Value, json};

const EXACT_OCIO_CONFIG: &str = "ocio://studio-config-v4.0.0_aces-v2.0_ocio-v2.5";
const DIFFERENT_SHA256: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const TEST_OCIO_BYTES: &[u8] = br#"ocio_profile_version: 2
roles:
  scene_linear: ACEScg
"#;

fn aces_config(identity: ColorConfigIdentity) -> ColorManagementConfig {
    ColorManagementConfig::new(
        identity,
        "ACEScg",
        PreviewColorConfig::new(
            "sRGB - Display",
            Some("ACES 2.0 - SDR 100 nits".to_string()),
        ),
        ExportColorConfig::new("Output - Rec.2020"),
    )
}

fn requested(project: &Project) -> Result<&ColorManagementConfig> {
    project
        .requested_color_management_config()
        .context("test Project has structurally valid color management")
}

#[test]
fn project_default_is_builtin_compatible_and_roundtrips_without_schema_version() -> Result<()> {
    let project = Project::new("color defaults");
    let requested = requested(&project)?;
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
    assert!(matches!(
        project.resolved_color_management(),
        ResolvedColorManagementConfig::Ready(_)
    ));

    let encoded = project.save()?;
    let value: Value = serde_json::from_str(&encoded)?;
    assert!(value.get("color_management").is_some());
    assert!(value.get("schema_version").is_none());
    assert!(value.get("migration").is_none());
    assert_eq!(Project::load(&encoded)?, project);
    Ok(())
}

#[test]
fn project_without_color_management_loads_with_current_pre_v1_default() -> Result<()> {
    let project = Project::new("old pre-v1 project");
    let mut value = serde_json::to_value(&project)?;
    let object = value
        .as_object_mut()
        .context("Project serialization is an object")?;
    object.remove("color_management");

    let loaded = Project::load(&serde_json::to_string(&value)?)?;
    assert_eq!(requested(&loaded)?, &ColorManagementConfig::default());
    assert!(matches!(
        loaded.resolved_color_management(),
        ResolvedColorManagementConfig::Ready(_)
    ));
    Ok(())
}

#[test]
fn existing_asset_without_imported_checksum_remains_compatible() -> Result<()> {
    let mut project = Project::new("pre-checksum asset");
    project
        .assets
        .push(Asset::new("existing image", "image.png", AssetKind::Image));
    let mut value = serde_json::to_value(&project)?;
    value["assets"][0]
        .as_object_mut()
        .context("Asset serialization is an object")?
        .remove("imported_content_sha256");

    let loaded = Project::load(&serde_json::to_string(&value)?)?;
    assert_eq!(loaded.assets[0].imported_content_sha256(), None);
    assert_eq!(Project::load(&loaded.save()?)?, loaded);
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
    assert_eq!(requested(&loaded)?, &ColorManagementConfig::default());
    assert!(loaded.color_management_diagnostics().is_empty());
    Ok(())
}

#[test]
fn exact_ocio_registry_config_and_non_srgb_spaces_are_model_validated() -> Result<()> {
    let mut project = Project::new("aces project");
    let config = aces_config(ColorConfigIdentity::OcioBuiltin {
        uri: EXACT_OCIO_CONFIG.to_string(),
        ocio_version: "2.5.2".to_string(),
    });
    project
        .set_color_management(config.clone())
        .map_err(|issues| anyhow::anyhow!("unexpected color issues: {issues:?}"))?;

    let loaded = Project::load(&project.save()?)?;
    assert_eq!(requested(&loaded)?, &config);
    assert_eq!(
        loaded
            .resolved_color_management()
            .model_validated_intent()
            .map(|intent| intent.config()),
        Some(&config),
    );
    assert!(loaded.color_management_diagnostics().is_empty());
    Ok(())
}

#[test]
fn asset_source_space_is_bound_to_the_owning_ocio_config() -> Result<()> {
    let identity = ColorConfigIdentity::OcioBuiltin {
        uri: EXACT_OCIO_CONFIG.to_string(),
        ocio_version: "2.5.2".to_string(),
    };
    let config = aces_config(identity);
    let mut project = Project::new("config-bound asset source");
    let mut asset = Asset::new("log plate", "plate.exr", AssetKind::Image);
    asset
        .source_color
        .assign_space(config.source_space_binding("ACES2065-1")?);
    project.assets.push(asset);
    project
        .set_color_management(config)
        .map_err(|issues| anyhow::anyhow!("unexpected color issues: {issues:?}"))?;

    let loaded = Project::load(&project.save()?)?;
    assert!(loaded.color_management_diagnostics().is_empty());
    let resolved = loaded.resolved_color_management();
    let assigned = resolved
        .model_validated_intent()
        .context("matching Project color config is ready")?
        .assigned_source_space(&loaded.assets[0])?
        .context("explicit source assignment is available")?;
    assert_eq!(assigned.config(), requested(&loaded)?.config());
    assert_eq!(assigned.color_space(), "ACES2065-1");
    Ok(())
}

#[test]
fn mismatched_asset_source_config_is_diagnostic_but_project_remains_loadable() -> Result<()> {
    let assigned = ColorConfigIdentity::OcioBuiltin {
        uri: "ocio://studio-config-v3.0.0_aces-v1.3_ocio-v2.4".to_string(),
        ocio_version: "2.4.2".to_string(),
    };
    let project_identity = ColorConfigIdentity::OcioBuiltin {
        uri: EXACT_OCIO_CONFIG.to_string(),
        ocio_version: "2.5.2".to_string(),
    };
    let mut project = Project::new("repairable source config mismatch");
    let mut asset = Asset::new("old-config plate", "plate.exr", AssetKind::Image);
    let asset_id = asset.id;
    let assigned_config = aces_config(assigned.clone());
    asset
        .source_color
        .assign_space(assigned_config.source_space_binding("ACEScg")?);
    project.assets.push(asset);

    let mut value = serde_json::to_value(&project)?;
    value["color_management"] = serde_json::to_value(aces_config(project_identity.clone()))?;
    let loaded = Project::load(&serde_json::to_string(&value)?)?;
    assert!(loaded.color_management_diagnostics().contains(
        &ColorManagementIssue::AssetSourceColorConfigMismatch {
            asset_id,
            assigned: Box::new(assigned),
            project: Box::new(project_identity),
        }
    ));
    assert!(loaded.validation_issues().is_empty());
    let resolved = loaded.resolved_color_management();
    assert!(matches!(resolved, ResolvedColorManagementConfig::Ready(_)));
    assert!(
        resolved
            .model_validated_intent()
            .context("Project config remains usable")?
            .assigned_source_space(&loaded.assets[0])
            .is_err()
    );
    assert_eq!(Project::load(&loaded.save()?)?, loaded);
    Ok(())
}

#[test]
fn blank_asset_source_space_is_repairable_and_project_config_stays_ready() -> Result<()> {
    let identity = ColorConfigIdentity::OcioBuiltin {
        uri: EXACT_OCIO_CONFIG.to_string(),
        ocio_version: "2.5.2".to_string(),
    };
    assert!(
        aces_config(identity.clone())
            .source_space_binding("  ")
            .is_err()
    );
    let mut project = Project::new("repairable blank source space");
    let asset = Asset::new("untitled source", "plate.exr", AssetKind::Image);
    let asset_id = asset.id;
    project.assets.push(asset);
    let mut value = serde_json::to_value(&project)?;
    value["color_management"] = serde_json::to_value(aces_config(identity))?;
    value["assets"][0]["source_color"] = json!({
        "assigned_space": {
            "config": {
                "kind": "ocio_builtin",
                "uri": EXACT_OCIO_CONFIG,
                "ocio_version": "2.5.2"
            },
            "color_space": "  "
        }
    });

    let loaded = Project::load(&serde_json::to_string(&value)?)?;
    assert!(
        loaded
            .color_management_diagnostics()
            .contains(&ColorManagementIssue::AssetSourceColorSpaceBlank { asset_id })
    );
    assert!(loaded.assets[0].source_color.assigned_space().is_some());
    assert!(matches!(
        loaded.resolved_color_management(),
        ResolvedColorManagementConfig::Ready(_)
    ));
    Ok(())
}

#[test]
fn malformed_asset_source_binding_roundtrips_without_blocking_project_open() -> Result<()> {
    let mut project = Project::new("repairable future source binding");
    project
        .assets
        .push(Asset::new("future plate", "plate.exr", AssetKind::Image));
    let asset_id = project.assets[0].id;
    let raw = json!({
        "config": { "kind": "future_config", "identity": "show-v9" },
        "color_space": "Future Log"
    });
    let mut value = serde_json::to_value(&project)?;
    value["assets"][0]["source_color"] = json!({ "assigned_space": raw });

    let loaded = Project::load(&serde_json::to_string(&value)?)?;
    assert!(
        loaded
            .color_management_diagnostics()
            .iter()
            .any(|issue| matches!(
                issue,
                ColorManagementIssue::AssetSourceColorBindingMalformed { asset_id: id, .. }
                    if *id == asset_id
            ))
    );
    assert!(matches!(
        loaded.resolved_color_management(),
        ResolvedColorManagementConfig::Ready(_)
    ));
    assert!(loaded.validation_issues().is_empty());
    let saved: Value = serde_json::from_str(&loaded.save()?)?;
    assert_eq!(saved["assets"][0]["source_color"]["assigned_space"], raw);
    Ok(())
}

#[test]
fn changing_project_config_preserves_and_diagnoses_old_asset_assignment() -> Result<()> {
    let old_identity = ColorConfigIdentity::OcioBuiltin {
        uri: "ocio://studio-config-v3.0.0_aces-v1.3_ocio-v2.4".to_string(),
        ocio_version: "2.4.2".to_string(),
    };
    let new_identity = ColorConfigIdentity::OcioBuiltin {
        uri: EXACT_OCIO_CONFIG.to_string(),
        ocio_version: "2.5.2".to_string(),
    };
    let mut project = Project::new("source assignment survives config switch");
    let mut asset = Asset::new("plate", "plate.exr", AssetKind::Image);
    let asset_id = asset.id;
    let old_config = aces_config(old_identity.clone());
    asset
        .source_color
        .assign_space(old_config.source_space_binding("ACEScg")?);
    project.assets.push(asset);
    project
        .set_color_management(aces_config(new_identity.clone()))
        .map_err(|issues| anyhow::anyhow!("asset-local issue blocked config switch: {issues:?}"))?;

    assert!(project.color_management_diagnostics().contains(
        &ColorManagementIssue::AssetSourceColorConfigMismatch {
            asset_id,
            assigned: Box::new(old_identity),
            project: Box::new(new_identity),
        }
    ));
    assert!(project.assets[0].source_color.assigned_space().is_some());
    assert!(matches!(
        project.resolved_color_management(),
        ResolvedColorManagementConfig::Ready(_)
    ));
    Ok(())
}

#[test]
fn external_ocio_config_requires_imported_bytes_and_matching_model_identity() -> Result<()> {
    let mut project = Project::new("external config");
    let mut asset = Asset::new("show config", "color/show-v12.ocio", AssetKind::Other);
    let checksum = asset.verify_imported_content(TEST_OCIO_BYTES);
    let asset_id = asset.id;
    project.assets.push(asset);
    let config = aces_config(ColorConfigIdentity::ProjectAsset {
        asset_id,
        sha256: checksum.clone(),
        ocio_version: "2.5.2".to_string(),
    });
    project
        .set_color_management(config.clone())
        .map_err(|issues| anyhow::anyhow!("unexpected color issues: {issues:?}"))?;

    let loaded = Project::load(&project.save()?)?;
    assert_eq!(requested(&loaded)?, &config);
    assert_eq!(
        loaded.assets[0].imported_content_sha256(),
        Some(checksum.as_str())
    );
    assert!(matches!(
        loaded.resolved_color_management(),
        ResolvedColorManagementConfig::Ready(_)
    ));
    Ok(())
}

#[test]
fn syntactically_valid_but_unverified_external_checksum_is_unavailable() -> Result<()> {
    let mut project = Project::new("unverified config");
    let asset = Asset::new("show config", "color/show.ocio", AssetKind::Other);
    let asset_id = asset.id;
    project.assets.push(asset);
    let mut value = serde_json::to_value(&project)?;
    value["color_management"] = external_config_json(asset_id, DIFFERENT_SHA256);

    let loaded = Project::load(&serde_json::to_string(&value)?)?;
    assert!(
        loaded
            .color_management_diagnostics()
            .contains(&ColorManagementIssue::ConfigAssetChecksumUnverified { asset_id })
    );
    assert!(matches!(
        loaded.resolved_color_management(),
        ResolvedColorManagementConfig::Unavailable { .. }
    ));
    Ok(())
}

#[test]
fn missing_external_asset_and_invalid_expected_checksum_are_unavailable() -> Result<()> {
    let project = Project::new("missing external config");
    let missing_id = uuid::Uuid::new_v4();
    let mut value = serde_json::to_value(&project)?;
    value["color_management"] = external_config_json(missing_id, "not-a-sha256");

    let loaded = Project::load(&serde_json::to_string(&value)?)?;
    let diagnostics = loaded.color_management_diagnostics();
    assert!(
        diagnostics.contains(&ColorManagementIssue::ConfigAssetNotFound {
            asset_id: missing_id,
        })
    );
    assert!(diagnostics.iter().any(|issue| matches!(
        issue,
        ColorManagementIssue::InvalidConfigChecksum { asset_id, sha256 }
            if *asset_id == missing_id && sha256 == "not-a-sha256"
    )));
    assert!(matches!(
        loaded.resolved_color_management(),
        ResolvedColorManagementConfig::Unavailable { .. }
    ));
    Ok(())
}

#[test]
fn external_checksum_mismatch_and_non_ocio_asset_are_distinctly_unavailable() -> Result<()> {
    let mut project = Project::new("mismatched config");
    let mut asset = Asset::new("not a config", "color/show.txt", AssetKind::Image);
    let imported = asset.verify_imported_content(TEST_OCIO_BYTES);
    assert_ne!(imported, DIFFERENT_SHA256);
    let asset_id = asset.id;
    project.assets.push(asset);
    let mut value = serde_json::to_value(&project)?;
    value["color_management"] = external_config_json(asset_id, DIFFERENT_SHA256);

    let loaded = Project::load(&serde_json::to_string(&value)?)?;
    let diagnostics = loaded.color_management_diagnostics();
    assert!(diagnostics.contains(&ColorManagementIssue::ConfigAssetWrongKind { asset_id }));
    assert!(diagnostics.iter().any(|issue| matches!(
        issue,
        ColorManagementIssue::ConfigAssetNotOcio { asset_id: id, .. } if *id == asset_id
    )));
    assert!(diagnostics.iter().any(|issue| matches!(
        issue,
        ColorManagementIssue::ConfigAssetChecksumMismatch { asset_id: id, expected, imported: actual }
            if *id == asset_id && expected == DIFFERENT_SHA256 && actual == &imported
    )));
    assert!(
        loaded
            .resolved_color_management()
            .model_validated_intent()
            .is_none()
    );
    Ok(())
}

#[test]
fn malformed_color_management_never_prevents_project_load_and_roundtrips_raw() -> Result<()> {
    let base = Project::new("repairable malformed config");
    let cases = [
        (
            json!({ "config": { "kind": "future_config", "id": "future-v1" } }),
            ColorManagementStructureIssue::UnknownConfigKind {
                kind: "future_config".to_string(),
            },
        ),
        (
            json!({
                "config": {
                    "kind": "ocio_builtin",
                    "uri": EXACT_OCIO_CONFIG
                }
            }),
            ColorManagementStructureIssue::MissingField {
                path: "color_management.config.ocio_version".to_string(),
            },
        ),
        (
            Value::Null,
            ColorManagementStructureIssue::Null {
                path: "color_management".to_string(),
            },
        ),
        (
            json!({ "working_space": 42 }),
            ColorManagementStructureIssue::WrongType {
                path: "color_management.working_space".to_string(),
                expected: "string".to_string(),
                actual: "number".to_string(),
            },
        ),
        (
            json!({ "working_spaec": "ACEScg" }),
            ColorManagementStructureIssue::UnknownField {
                path: "color_management.working_spaec".to_string(),
            },
        ),
        (
            json!({
                "config": {
                    "kind": "bundled",
                    "id": DEFAULT_BUNDLED_COLOR_CONFIG_ID,
                    "moving_alias": "ocio://default"
                }
            }),
            ColorManagementStructureIssue::UnknownField {
                path: "color_management.config.moving_alias".to_string(),
            },
        ),
        (
            json!({
                "preview": {
                    "display": "srgb",
                    "view": null,
                    "veiw": "standard"
                }
            }),
            ColorManagementStructureIssue::UnknownField {
                path: "color_management.preview.veiw".to_string(),
            },
        ),
        (
            json!({
                "export": {
                    "output_space": "srgb",
                    "output_colour_space": "ACEScg"
                }
            }),
            ColorManagementStructureIssue::UnknownField {
                path: "color_management.export.output_colour_space".to_string(),
            },
        ),
    ];

    for (raw, expected_issue) in cases {
        let mut value = serde_json::to_value(&base)?;
        value["color_management"] = raw.clone();
        let loaded = Project::load(&serde_json::to_string(&value)?)?;

        assert_eq!(
            loaded.requested_color_management().malformed_raw(),
            Some(&raw)
        );
        assert!(loaded.color_management_diagnostics().contains(
            &ColorManagementIssue::MalformedStructure {
                issue: expected_issue,
            }
        ));
        assert!(
            loaded.validation_issues().is_empty(),
            "malformed color settings are repairable and not graph errors"
        );
        assert!(matches!(
            loaded.resolved_color_management(),
            ResolvedColorManagementConfig::Unavailable { .. }
        ));

        let saved: Value = serde_json::from_str(&loaded.save()?)?;
        assert_eq!(saved["color_management"], raw);
    }
    Ok(())
}

#[test]
fn invalid_semantic_request_is_unavailable_without_a_silent_default() -> Result<()> {
    let project = Project::new("repairable semantic config");
    let mut value = serde_json::to_value(&project)?;
    let raw = json!({
        "config": {
            "kind": "ocio_builtin",
            "uri": "ocio://default",
            "ocio_version": "2.5"
        },
        "working_space": "",
        "preview": { "display": "sRGB - Display", "view": null },
        "export": { "output_space": "Output - Rec.709" }
    });
    value["color_management"] = raw;

    let loaded = Project::load(&serde_json::to_string(&value)?)?;
    let resolved = loaded.resolved_color_management();
    assert!(resolved.model_validated_intent().is_none());
    assert!(resolved.unavailable_request().is_some());
    assert!(!resolved.diagnostics().is_empty());
    assert!(matches!(
        resolved.unavailable_request(),
        Some(RequestedColorManagementConfig::Config(config))
            if config != &ColorManagementConfig::default()
    ));
    Ok(())
}

#[test]
fn bundled_preview_is_direct_but_ocio_preview_requires_an_explicit_view() {
    let mut project = Project::new("preview contracts");
    assert!(project.color_management_diagnostics().is_empty());

    let bundled_with_named_view = ColorManagementConfig::new(
        ColorConfigIdentity::default(),
        DEFAULT_WORKING_COLOR_SPACE,
        PreviewColorConfig::new("srgb", Some("standard".to_string())),
        ExportColorConfig::default(),
    );
    assert!(
        project
            .set_color_management(bundled_with_named_view)
            .expect_err("built-in backend cannot honor a named view")
            .contains(&ColorManagementIssue::UnsupportedBundledPreviewView {
                view: "standard".to_string(),
            })
    );

    let ocio_without_view = ColorManagementConfig::new(
        ColorConfigIdentity::OcioBuiltin {
            uri: EXACT_OCIO_CONFIG.to_string(),
            ocio_version: "2.5.2".to_string(),
        },
        "ACEScg",
        PreviewColorConfig::direct("sRGB - Display"),
        ExportColorConfig::new("Output - Rec.709"),
    );
    assert!(
        project
            .set_color_management(ocio_without_view)
            .expect_err("OCIO display transform requires a named view")
            .contains(&ColorManagementIssue::MissingRequiredPreviewView)
    );
}

#[test]
fn moving_alias_detection_does_not_reject_versioned_names_containing_default_or_latest() {
    let mut project = Project::new("legitimate names");
    let bundled = ColorManagementConfig::new(
        ColorConfigIdentity::Bundled {
            id: "ruvie://color-config/show-default-look-v12".to_string(),
        },
        DEFAULT_WORKING_COLOR_SPACE,
        PreviewColorConfig::default(),
        ExportColorConfig::default(),
    );
    assert!(project.set_color_management(bundled).is_ok());

    let ocio = aces_config(ColorConfigIdentity::OcioBuiltin {
        uri: "ocio://studio-default-look-v4.0_ocio-v2.5".to_string(),
        ocio_version: "2.5.2".to_string(),
    });
    assert!(project.set_color_management(ocio).is_ok());
}

#[test]
fn setter_rejects_exact_moving_alias_and_blank_identifiers_without_mutation() {
    let mut project = Project::new("invalid candidate");
    let original = project.requested_color_management().clone();
    let candidate = ColorManagementConfig::new(
        ColorConfigIdentity::OcioBuiltin {
            uri: "ocio://default".to_string(),
            ocio_version: "latest".to_string(),
        },
        " ",
        PreviewColorConfig::new("", Some(" ".to_string())),
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
fn cache_identity_is_stable_for_equal_model_validated_configs_and_changes_with_intent() -> Result<()>
{
    let first = Project::new("fingerprint one");
    let roundtrip = Project::load(&first.save()?)?;
    let first_resolution = first.resolved_color_management();
    let first_intent = first_resolution
        .model_validated_intent()
        .context("default is model-validated")?;
    let first_identity = first_intent.cache_identity().to_string();
    let roundtrip_resolution = roundtrip.resolved_color_management();
    let roundtrip_intent = roundtrip_resolution
        .model_validated_intent()
        .context("roundtrip default is model-validated")?;
    let roundtrip_identity = roundtrip_intent.cache_identity().to_string();
    assert_eq!(first_identity, roundtrip_identity);
    assert!(first_identity.starts_with("sha256:"));
    assert_eq!(first_identity.len(), "sha256:".len() + 64);

    let mut changed = Project::new("fingerprint changed");
    changed
        .set_color_management(ColorManagementConfig::new(
            ColorConfigIdentity::default(),
            DEFAULT_WORKING_COLOR_SPACE,
            PreviewColorConfig::default(),
            ExportColorConfig::new("linear-srgb"),
        ))
        .map_err(|issues| anyhow::anyhow!("unexpected color issues: {issues:?}"))?;
    let changed_resolution = changed.resolved_color_management();
    let changed_intent = changed_resolution
        .model_validated_intent()
        .context("changed config is model-validated")?;
    assert_ne!(first_identity, changed_intent.cache_identity().as_str());
    Ok(())
}

#[test]
fn ocio_registry_uri_and_processor_version_must_both_be_exact() {
    let mut project = Project::new("unpinned ocio");
    let candidate = aces_config(ColorConfigIdentity::OcioBuiltin {
        uri: "ocio://studio-config-aces".to_string(),
        ocio_version: "2.5".to_string(),
    });
    let issues = project
        .set_color_management(candidate)
        .expect_err("unversioned identities must be rejected");
    assert!(
        issues.contains(&ColorManagementIssue::UnpinnedOcioBuiltinUri {
            uri: "ocio://studio-config-aces".to_string(),
        })
    );
    assert!(issues.contains(&ColorManagementIssue::InvalidOcioVersion {
        version: "2.5".to_string(),
    }));
}

fn external_config_json(asset_id: uuid::Uuid, sha256: &str) -> Value {
    json!({
        "config": {
            "kind": "project_asset",
            "asset_id": asset_id,
            "sha256": sha256,
            "ocio_version": "2.5.2"
        },
        "working_space": "ACEScg",
        "preview": {
            "display": "sRGB - Display",
            "view": "ACES 2.0 - SDR 100 nits"
        },
        "export": { "output_space": "Output - Rec.2020" }
    })
}
