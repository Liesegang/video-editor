//! Pure semantic validation and stable identity derivation.
//!
//! Validation operates only on persisted Project state. Backend availability,
//! filesystem existence, and runtime re-hashing belong to resource opening.

use std::path::Path;

use sha2::{Digest, Sha256};

use super::super::asset::{Asset, AssetKind};
use super::{
    ColorConfigCacheIdentity, ColorConfigIdentity, ColorManagementConfig, ColorManagementField,
    ColorManagementIssue,
};

pub(super) fn diagnostics(
    config: &ColorManagementConfig,
    assets: &[Asset],
) -> Vec<ColorManagementIssue> {
    let mut diagnostics = Vec::new();
    validate_config_identity(&config.config, assets, &mut diagnostics);
    validate_named_field(
        &config.working_space,
        ColorManagementField::WorkingSpace,
        &mut diagnostics,
    );
    validate_named_field(
        &config.preview.display,
        ColorManagementField::PreviewDisplay,
        &mut diagnostics,
    );
    validate_preview_contract(config, &mut diagnostics);
    validate_named_field(
        &config.export.output_space,
        ColorManagementField::OutputSpace,
        &mut diagnostics,
    );
    diagnostics
}

pub(super) fn stable_cache_identity(config: &ColorManagementConfig) -> ColorConfigCacheIdentity {
    let mut hasher = Sha256::new();
    hash_part(&mut hasher, "ruvie-color-config-cache-v1");
    match &config.config {
        ColorConfigIdentity::Bundled { id } => {
            hash_part(&mut hasher, "bundled");
            hash_part(&mut hasher, id);
        }
        ColorConfigIdentity::OcioBuiltin { uri, ocio_version } => {
            hash_part(&mut hasher, "ocio-builtin");
            hash_part(&mut hasher, uri);
            hash_part(&mut hasher, ocio_version);
        }
        ColorConfigIdentity::ProjectAsset {
            asset_id,
            sha256,
            ocio_version,
        } => {
            hash_part(&mut hasher, "project-asset");
            hash_part(&mut hasher, &asset_id.to_string());
            hash_part(&mut hasher, &sha256.to_ascii_lowercase());
            hash_part(&mut hasher, ocio_version);
        }
    }
    hash_part(&mut hasher, &config.working_space);
    hash_part(&mut hasher, &config.preview.display);
    hash_part(
        &mut hasher,
        config.preview.view.as_deref().unwrap_or("<direct>"),
    );
    hash_part(&mut hasher, &config.export.output_space);
    ColorConfigCacheIdentity(format!("sha256:{:x}", hasher.finalize()))
}

fn validate_preview_contract(
    config: &ColorManagementConfig,
    diagnostics: &mut Vec<ColorManagementIssue>,
) {
    match (&config.config, config.preview.view.as_deref()) {
        (ColorConfigIdentity::Bundled { .. }, Some(view)) if !view.trim().is_empty() => {
            diagnostics.push(ColorManagementIssue::UnsupportedBundledPreviewView {
                view: view.to_string(),
            });
        }
        (ColorConfigIdentity::Bundled { .. }, Some(_)) => {
            diagnostics.push(ColorManagementIssue::BlankIdentifier {
                field: ColorManagementField::PreviewView,
            });
        }
        (ColorConfigIdentity::Bundled { .. }, None) => {}
        (_, None) => diagnostics.push(ColorManagementIssue::MissingRequiredPreviewView),
        (_, Some(view)) => {
            validate_named_field(view, ColorManagementField::PreviewView, diagnostics)
        }
    }
}

fn validate_config_identity(
    identity: &ColorConfigIdentity,
    assets: &[Asset],
    diagnostics: &mut Vec<ColorManagementIssue>,
) {
    match identity {
        ColorConfigIdentity::Bundled { id } => {
            validate_named_field(id, ColorManagementField::ConfigIdentifier, diagnostics);
            if is_moving_identifier(id) {
                diagnostics.push(ColorManagementIssue::MovingConfigIdentifier {
                    identifier: id.clone(),
                });
            } else if !id.starts_with("ruvie://color-config/") || !contains_version_token(id) {
                diagnostics.push(ColorManagementIssue::InvalidBundledConfigId {
                    identifier: id.clone(),
                });
            }
        }
        ColorConfigIdentity::OcioBuiltin { uri, ocio_version } => {
            validate_named_field(uri, ColorManagementField::ConfigIdentifier, diagnostics);
            if is_moving_identifier(uri) {
                diagnostics.push(ColorManagementIssue::MovingConfigIdentifier {
                    identifier: uri.clone(),
                });
            } else if !uri.starts_with("ocio://") {
                diagnostics.push(ColorManagementIssue::InvalidOcioBuiltinUri { uri: uri.clone() });
            } else if !contains_ocio_registry_version(uri) {
                diagnostics.push(ColorManagementIssue::UnpinnedOcioBuiltinUri { uri: uri.clone() });
            }
            validate_ocio_version(ocio_version, diagnostics);
        }
        ColorConfigIdentity::ProjectAsset {
            asset_id,
            sha256,
            ocio_version,
        } => {
            if let Some(asset) = assets.iter().find(|asset| asset.id == *asset_id) {
                validate_config_asset(asset, sha256, diagnostics);
            } else {
                diagnostics.push(ColorManagementIssue::ConfigAssetNotFound {
                    asset_id: *asset_id,
                });
            }
            if !is_sha256(sha256) {
                diagnostics.push(ColorManagementIssue::InvalidConfigChecksum {
                    asset_id: *asset_id,
                    sha256: sha256.clone(),
                });
            }
            validate_ocio_version(ocio_version, diagnostics);
        }
    }
}

fn validate_config_asset(
    asset: &Asset,
    expected_sha256: &str,
    diagnostics: &mut Vec<ColorManagementIssue>,
) {
    if asset.kind != AssetKind::Other {
        diagnostics.push(ColorManagementIssue::ConfigAssetWrongKind { asset_id: asset.id });
    }
    if !Path::new(&asset.path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ocio"))
    {
        diagnostics.push(ColorManagementIssue::ConfigAssetNotOcio {
            asset_id: asset.id,
            path: asset.path.clone(),
        });
    }
    match asset.imported_content_sha256() {
        None => diagnostics
            .push(ColorManagementIssue::ConfigAssetChecksumUnverified { asset_id: asset.id }),
        Some(imported) if !is_sha256(imported) => {
            diagnostics.push(ColorManagementIssue::InvalidImportedContentChecksum {
                asset_id: asset.id,
                sha256: imported.to_string(),
            });
        }
        Some(imported) if !imported.eq_ignore_ascii_case(expected_sha256) => {
            diagnostics.push(ColorManagementIssue::ConfigAssetChecksumMismatch {
                asset_id: asset.id,
                expected: expected_sha256.to_string(),
                imported: imported.to_string(),
            });
        }
        Some(_) => {}
    }
}

fn validate_named_field(
    value: &str,
    field: ColorManagementField,
    diagnostics: &mut Vec<ColorManagementIssue>,
) {
    if value.trim().is_empty() {
        diagnostics.push(ColorManagementIssue::BlankIdentifier { field });
    }
}

fn validate_ocio_version(version: &str, diagnostics: &mut Vec<ColorManagementIssue>) {
    if version.trim().is_empty() {
        diagnostics.push(ColorManagementIssue::BlankIdentifier {
            field: ColorManagementField::OcioVersion,
        });
    } else if !is_pinned_ocio_version(version) {
        diagnostics.push(ColorManagementIssue::InvalidOcioVersion {
            version: version.to_string(),
        });
    }
}

fn is_moving_identifier(identifier: &str) -> bool {
    matches!(
        identifier
            .trim()
            .trim_end_matches('/')
            .to_ascii_lowercase()
            .as_str(),
        "default"
            | "latest"
            | "ocio://default"
            | "ocio://latest"
            | "ruvie://color-config/default"
            | "ruvie://color-config/latest"
    )
}

fn contains_ocio_registry_version(uri: &str) -> bool {
    uri.rsplit_once("_ocio-v")
        .is_some_and(|(_, version)| is_version_number(version))
}

fn contains_version_token(identifier: &str) -> bool {
    identifier
        .split(['/', '-', '_'])
        .any(|part| part.strip_prefix('v').is_some_and(is_version_number))
}

fn is_pinned_ocio_version(version: &str) -> bool {
    let version = version.strip_prefix('v').unwrap_or(version);
    version.split('.').count() == 3 && is_version_number(version)
}

fn is_version_number(version: &str) -> bool {
    !version.is_empty()
        && version.split('.').all(|part| {
            !part.is_empty() && part.bytes().all(|character| character.is_ascii_digit())
        })
}

fn is_sha256(checksum: &str) -> bool {
    checksum.len() == 64
        && checksum
            .bytes()
            .all(|character| character.is_ascii_hexdigit())
}

fn hash_part(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}
