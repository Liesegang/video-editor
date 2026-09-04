use anyhow::{Context, Result};
use library::model::project::{
    ColorConfigIdentity, DEFAULT_BUNDLED_COLOR_CONFIG_ID, LEGACY_BUNDLED_COLOR_CONFIG_V1_ID,
    Project, ResolvedColorManagementConfig,
};
use serde_json::{Value, json};

#[test]
fn former_builtin_v1_project_keeps_its_exact_identity_without_a_migration() -> Result<()> {
    let project = Project::new("former bundled v1");
    let mut value = serde_json::to_value(&project)?;
    value["color_management"] = json!({
        "config": {
            "kind": "bundled",
            "id": LEGACY_BUNDLED_COLOR_CONFIG_V1_ID
        },
        "working_space": "linear-srgb",
        "preview": {
            "display": "srgb",
            "view": null
        },
        "export": {
            "output_space": "srgb"
        }
    });

    let loaded = Project::load(&serde_json::to_string(&value)?)?;
    let config = loaded
        .requested_color_management_config()
        .context("v1 Project retains a structurally valid color config")?;
    assert_eq!(
        config.config(),
        &ColorConfigIdentity::Bundled {
            id: LEGACY_BUNDLED_COLOR_CONFIG_V1_ID.to_string(),
        }
    );
    assert_eq!(
        config
            .srgb_surface_space()
            .context("v1 receives its exact built-in sRGB surface binding")?
            .config(),
        config.config()
    );
    assert!(loaded.color_management_diagnostics().is_empty());
    assert!(matches!(
        loaded.resolved_color_management(),
        ResolvedColorManagementConfig::Ready(_)
    ));

    let saved: Value = serde_json::from_str(&loaded.save()?)?;
    assert_eq!(
        saved["color_management"]["config"]["id"],
        LEGACY_BUNDLED_COLOR_CONFIG_V1_ID
    );
    assert_ne!(
        saved["color_management"]["config"]["id"],
        DEFAULT_BUNDLED_COLOR_CONFIG_ID
    );
    Ok(())
}
