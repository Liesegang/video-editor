//! Pure semantic validation and stable identity derivation.
//!
//! Validation operates only on persisted Project state. Backend availability,
//! filesystem existence, and runtime re-hashing belong to resource opening.

use std::path::Path;

use sha2::{Digest, Sha256};

use super::super::asset::{
    Asset, AssetKind, AssetSourceColorSpaceBinding, AssetSourceInterpretation,
    SourceTransferCharacteristic,
};
use super::{
    ColorConfigCacheIdentity, ColorConfigIdentity, ColorManagementConfig, ColorManagementField,
    ColorManagementIssue, HdrColorField, PreviewSurfaceEncoding,
};
use ruvie_color_management::StandardColorSpaceId;

pub(super) fn diagnostics(
    config: &ColorManagementConfig,
    assets: &[Asset],
) -> Vec<ColorManagementIssue> {
    let mut diagnostics = blocking_diagnostics(config, assets);
    validate_hdr(config, assets, &mut diagnostics);
    validate_preview_surface_encoding(config, &mut diagnostics);
    diagnostics.extend(asset_source_diagnostics(config, assets));
    diagnostics
}

pub(super) fn blocking_diagnostics(
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
    validate_srgb_surface_binding(config, &mut diagnostics);
    validate_preview_contract(config, &mut diagnostics);
    validate_named_field(
        &config.export.output_space,
        ColorManagementField::OutputSpace,
        &mut diagnostics,
    );
    diagnostics
}

fn asset_source_diagnostics(
    config: &ColorManagementConfig,
    assets: &[Asset],
) -> Vec<ColorManagementIssue> {
    let mut diagnostics = Vec::new();
    for asset in assets {
        if let Err(issue) = validate_asset_source_binding(config, asset) {
            diagnostics.push(issue);
        }
    }
    diagnostics
}

pub(super) fn validate_asset_source_binding<'a>(
    config: &ColorManagementConfig,
    asset: &'a Asset,
) -> Result<Option<&'a AssetSourceColorSpaceBinding>, ColorManagementIssue> {
    let binding = match asset.source_color.authoritative_interpretation() {
        AssetSourceInterpretation::Assigned(binding) => binding,
        AssetSourceInterpretation::Description(_) => return Ok(None),
        AssetSourceInterpretation::Malformed { detail, .. } => {
            return Err(ColorManagementIssue::AssetSourceColorBindingMalformed {
                asset_id: asset.id,
                detail: detail.to_string(),
            });
        }
    };
    if binding.color_space().trim().is_empty() {
        return Err(ColorManagementIssue::AssetSourceColorSpaceBlank { asset_id: asset.id });
    }
    if binding.config() != &config.config {
        return Err(ColorManagementIssue::AssetSourceColorConfigMismatch {
            asset_id: asset.id,
            assigned: Box::new(binding.config().clone()),
            project: Box::new(config.config.clone()),
        });
    }
    Ok(Some(binding))
}

pub(super) fn stable_cache_identity(config: &ColorManagementConfig) -> ColorConfigCacheIdentity {
    let mut hasher = Sha256::new();
    hash_part(&mut hasher, "ruvie-color-config-cache-v3");
    hash_config_identity(&mut hasher, &config.config);
    hash_part(&mut hasher, &config.working_space);
    hash_part(&mut hasher, &config.preview.display);
    hash_part(
        &mut hasher,
        config.preview.view.as_deref().unwrap_or("<direct>"),
    );
    hash_part(&mut hasher, config.preview.surface_encoding.as_str());
    hash_part(
        &mut hasher,
        config
            .preview
            .view_output_color_space
            .as_deref()
            .unwrap_or("<no-view-output-space>"),
    );
    if let Some(binding) = &config.srgb_surface_space {
        hash_part(&mut hasher, "srgb-surface-binding");
        hash_config_identity(&mut hasher, binding.config());
        hash_part(&mut hasher, binding.color_space());
    } else {
        hash_part(&mut hasher, "<no-srgb-surface-binding>");
    }
    hash_part(&mut hasher, &config.export.output_space);
    hash_optional_number(
        &mut hasher,
        "reference-white-nits",
        config.hdr.reference_white_nits(),
    );
    hash_part(&mut hasher, "pq-linearization-policy");
    hash_part(
        &mut hasher,
        config
            .hdr
            .pq_linearization_policy()
            .map_or("none", |policy| policy.context_value()),
    );
    ColorConfigCacheIdentity(format!("sha256:{:x}", hasher.finalize()))
}

fn hash_config_identity(hasher: &mut Sha256, identity: &ColorConfigIdentity) {
    match identity {
        ColorConfigIdentity::Bundled { id } => {
            hash_part(hasher, "bundled");
            hash_part(hasher, id);
        }
        ColorConfigIdentity::OcioBuiltin { uri, ocio_version } => {
            hash_part(hasher, "ocio-builtin");
            hash_part(hasher, uri);
            hash_part(hasher, ocio_version);
        }
        ColorConfigIdentity::ProjectAsset {
            asset_id,
            sha256,
            ocio_version,
        } => {
            hash_part(hasher, "project-asset");
            hash_part(hasher, &asset_id.to_string());
            hash_part(hasher, &sha256.to_ascii_lowercase());
            hash_part(hasher, ocio_version);
        }
    }
}

fn validate_srgb_surface_binding(
    config: &ColorManagementConfig,
    diagnostics: &mut Vec<ColorManagementIssue>,
) {
    if let Err(issue) = validated_srgb_surface_binding(config) {
        diagnostics.push(issue);
    }
}

pub(super) fn validated_srgb_surface_binding(
    config: &ColorManagementConfig,
) -> Result<&super::SrgbSurfaceColorSpaceBinding, ColorManagementIssue> {
    let binding = config
        .srgb_surface_space
        .as_ref()
        .ok_or(ColorManagementIssue::MissingSrgbSurfaceColorSpaceBinding)?;
    if binding.color_space().trim().is_empty() {
        return Err(ColorManagementIssue::BlankIdentifier {
            field: ColorManagementField::SrgbSurfaceColorSpace,
        });
    }
    if binding.config() != &config.config {
        return Err(ColorManagementIssue::SrgbSurfaceColorSpaceBindingMismatch {
            bound: Box::new(binding.config().clone()),
            project: Box::new(config.config.clone()),
        });
    }
    Ok(binding)
}

fn validate_preview_surface_encoding(
    config: &ColorManagementConfig,
    diagnostics: &mut Vec<ColorManagementIssue>,
) {
    if let PreviewSurfaceEncoding::Unknown(_) = &config.preview.surface_encoding {
        diagnostics.push(ColorManagementIssue::UnsupportedPreviewSurfaceEncoding {
            encoding: config.preview.surface_encoding.as_str().to_string(),
        });
    }
    if config.preview.surface_encoding.is_srgb()
        && config.preview.view.is_none()
        && config
            .srgb_surface_space
            .as_ref()
            .is_some_and(|binding| config.preview.display != binding.color_space())
    {
        diagnostics.push(ColorManagementIssue::DirectPreviewSurfaceEncodingMismatch {
            destination: config.preview.display.clone(),
            surface_encoding: config.preview.surface_encoding.clone(),
        });
    }
    match (
        &config.config,
        config.preview.view.as_deref(),
        config.preview.view_output_color_space.as_deref(),
    ) {
        (ColorConfigIdentity::Bundled { .. }, None, Some(output_space)) => diagnostics.push(
            ColorManagementIssue::UnexpectedDirectPreviewViewOutputColorSpace {
                output_space: output_space.to_string(),
            },
        ),
        (
            ColorConfigIdentity::OcioBuiltin { .. } | ColorConfigIdentity::ProjectAsset { .. },
            Some(_),
            None,
        ) => diagnostics.push(ColorManagementIssue::MissingPreviewViewOutputColorSpace),
        (
            ColorConfigIdentity::OcioBuiltin { .. } | ColorConfigIdentity::ProjectAsset { .. },
            Some(_),
            Some(output_space),
        ) if output_space.trim().is_empty() => {
            diagnostics.push(ColorManagementIssue::BlankIdentifier {
                field: ColorManagementField::PreviewViewOutputColorSpace,
            });
        }
        _ => {}
    }
}

fn validate_hdr(
    config: &ColorManagementConfig,
    assets: &[Asset],
    diagnostics: &mut Vec<ColorManagementIssue>,
) {
    diagnostics.extend(
        config
            .hdr
            .semantic_issues()
            .into_iter()
            .map(|(field, detail)| ColorManagementIssue::InvalidHdrSetting { field, detail }),
    );
    for (purpose, space) in [
        ("Preview display", config.preview.display.as_str()),
        ("export output", config.export.output_space.as_str()),
    ] {
        if StandardColorSpaceId::from_id(space) == Some(StandardColorSpaceId::Rec2100Pq) {
            require_hdr_field(
                config.hdr.reference_white_nits(),
                HdrColorField::ReferenceWhiteNits,
                purpose,
                space,
                diagnostics,
            );
            require_hdr_field(
                config.hdr.pq_linearization_policy(),
                HdrColorField::PqLinearizationPolicy,
                purpose,
                space,
                diagnostics,
            );
        }
    }
    for asset in assets.iter().filter(|asset| asset_requires_pq(asset)) {
        let purpose = format!("Asset {} PQ source", asset.id);
        require_hdr_field(
            config.hdr.reference_white_nits(),
            HdrColorField::ReferenceWhiteNits,
            &purpose,
            ruvie_color_management::REC2100_PQ_SPACE_ID,
            diagnostics,
        );
        require_hdr_field(
            config.hdr.pq_linearization_policy(),
            HdrColorField::PqLinearizationPolicy,
            &purpose,
            ruvie_color_management::REC2100_PQ_SPACE_ID,
            diagnostics,
        );
    }
}

fn asset_requires_pq(asset: &Asset) -> bool {
    match asset.source_color.authoritative_interpretation() {
        AssetSourceInterpretation::Assigned(binding) => {
            StandardColorSpaceId::from_id(binding.color_space())
                == Some(StandardColorSpaceId::Rec2100Pq)
        }
        AssetSourceInterpretation::Description(description) => {
            description.transfer == Some(SourceTransferCharacteristic::Pq)
        }
        AssetSourceInterpretation::Malformed { .. } => false,
    }
}

fn require_hdr_field<T>(
    value: Option<T>,
    field: HdrColorField,
    purpose: &str,
    space: &str,
    diagnostics: &mut Vec<ColorManagementIssue>,
) {
    if value.is_none() {
        diagnostics.push(ColorManagementIssue::MissingHdrSetting {
            field,
            required_by: format!("{purpose} color space '{space}'"),
        });
    }
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

fn hash_optional_number(hasher: &mut Sha256, field: &str, value: Option<f64>) {
    hash_part(hasher, field);
    match value {
        Some(value) => hash_part(hasher, &format!("some:{:016x}", value.to_bits())),
        None => hash_part(hasher, "none"),
    }
}
