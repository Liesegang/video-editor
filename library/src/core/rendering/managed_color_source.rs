//! Authoritative Asset/decoder reconciliation for managed media ingress.

use ruvie_color_management::{ManagedLinearWorkingImage, SRGB_SPACE_ID, StandardColorSpaceId};

use super::managed_color_backend::ProjectColorPipeline;
use super::media_color_ingress::{
    MediaAssetKind, reconcile_detected_source, source_asset_from_assets,
    standard_space_for_description, validate_decoded_storage_fidelity,
};
use crate::error::LibraryError;
use crate::model::asset::{Asset, AssetSourceInterpretation, SourceColorDescription};
#[cfg(test)]
use crate::model::project::Project;
use crate::plugin::{DecodedColorSpace, DecodedPixelDescription, LoadResponse};

#[cfg(test)]
pub(crate) fn ingest_loaded_media(
    project: &Project,
    pipeline: &ProjectColorPipeline,
    surface: &crate::model::frame::entity::ImageSurface,
    kind: MediaAssetKind,
    response: LoadResponse,
) -> Result<ManagedLinearWorkingImage, LibraryError> {
    ingest_loaded_media_from_assets(&project.assets, pipeline, surface, kind, response)
}

pub(crate) fn ingest_loaded_media_from_assets(
    assets: &[Asset],
    pipeline: &ProjectColorPipeline,
    surface: &crate::model::frame::entity::ImageSurface,
    kind: MediaAssetKind,
    response: LoadResponse,
) -> Result<ManagedLinearWorkingImage, LibraryError> {
    if surface.input_color_space.is_some() || surface.output_color_space.is_some() {
        return Err(LibraryError::Render(format!(
            "Asset source {:?} still carries legacy loader-side color-space fields; managed rendering requires Project Asset authority",
            surface.file_path
        )));
    }
    let asset = source_asset_from_assets(assets, surface, kind)?.ok_or_else(|| {
        LibraryError::Render(format!(
            "managed color rendering requires an authoritative Asset for {:?}",
            surface.file_path
        ))
    })?;
    validate_decoded_storage_fidelity(asset, response.decoded(), response.pixels())?;
    validate_rgb_conversion(asset, response.decoded())?;
    let source_name = source_space_name(pipeline, asset, response.decoded())?;
    let source = pipeline.resolve_source_space(&source_name)?;
    let (pixels, _) = response.into_parts();
    pipeline.ingest_pixels(&source, pixels)
}

fn source_space_name(
    pipeline: &ProjectColorPipeline,
    asset: &Asset,
    decoded: &DecodedPixelDescription,
) -> Result<String, LibraryError> {
    let assigned = pipeline
        .intent()
        .assigned_source_space(asset)
        .map_err(|issue| LibraryError::Render(issue.to_string()))?;
    if let Some(binding) = assigned {
        match decoded.color_space() {
            DecodedColorSpace::ConfigOwned(space) if space.config() != binding.config() => {
                return Err(LibraryError::Render(format!(
                    "Asset {} decoder returned color space '{}' under {:?}, but the Project assigns '{}' under {:?}",
                    asset.id,
                    space.name(),
                    space.config(),
                    binding.color_space(),
                    binding.config()
                )));
            }
            DecodedColorSpace::ConfigOwned(space) if space.name() != binding.color_space() => {
                return Err(LibraryError::Render(format!(
                    "Asset {} decoder transformed pixels to '{}', which conflicts with assigned source '{}'",
                    asset.id,
                    space.name(),
                    binding.color_space()
                )));
            }
            DecodedColorSpace::Unknown { reason } => {
                return Err(LibraryError::Render(format!(
                    "Asset {} decoded color is unverified: {reason}",
                    asset.id
                )));
            }
            DecodedColorSpace::Srgb
                if !binding.color_space().eq_ignore_ascii_case(SRGB_SPACE_ID) =>
            {
                return Err(LibraryError::Render(format!(
                    "Asset {} decoder explicitly transformed pixels to sRGB, which conflicts with assigned source '{}'",
                    asset.id,
                    binding.color_space()
                )));
            }
            DecodedColorSpace::Srgb
            | DecodedColorSpace::AssumedSrgb(_)
            | DecodedColorSpace::SourceEncoded(_)
            | DecodedColorSpace::ConfigOwned(_) => {}
        }
        return Ok(binding.color_space().to_string());
    }

    let AssetSourceInterpretation::Description(authoritative) =
        asset.source_color.authoritative_interpretation()
    else {
        return Err(LibraryError::Render(format!(
            "Asset {} source color authority was not resolved by Project validation",
            asset.id
        )));
    };
    match decoded.color_space() {
        DecodedColorSpace::Srgb => {
            if !has_color_identity(authoritative) && asset.source_color.user_override().is_none() {
                return Ok(SRGB_SPACE_ID.to_string());
            }
            let standard = standard_space_for_description(authoritative)
                .map_err(|reason| LibraryError::Render(format!("Asset {} {reason}", asset.id)))?;
            if standard == StandardColorSpaceId::Srgb {
                Ok(standard.as_str().to_string())
            } else {
                Err(LibraryError::Render(format!(
                    "Asset {} decoder explicitly transformed pixels to sRGB, which conflicts with authoritative source '{}'",
                    asset.id,
                    standard.as_str()
                )))
            }
        }
        DecodedColorSpace::AssumedSrgb(assumption) => {
            if asset.source_color.user_override().is_some() {
                return standard_space_for_description(authoritative)
                    .map(|space| space.as_str().to_string())
                    .map_err(|reason| {
                        LibraryError::Render(format!("Asset {} {reason}", asset.id))
                    });
            }
            let resolved = reconcile_detected_source(authoritative, assumption.detected_source())
                .map_err(|field| {
                    LibraryError::Render(format!(
                        "Asset {} untagged-still assumption no longer matches detected {field}; re-probe the Asset before rendering",
                        asset.id
                    ))
                })?;
            if has_color_identity(&resolved) {
                Err(LibraryError::Render(format!(
                    "Asset {} has detected color metadata that conflicts with loader policy '{}'",
                    asset.id,
                    assumption.policy().id()
                )))
            } else {
                Ok(SRGB_SPACE_ID.to_string())
            }
        }
        DecodedColorSpace::ConfigOwned(space)
            if space.config() == pipeline.intent().config().config() =>
        {
            Ok(space.name().to_string())
        }
        DecodedColorSpace::ConfigOwned(space) => Err(LibraryError::Render(format!(
            "Asset {} decoded color space '{}' belongs to {:?}, not the active Project config {:?}",
            asset.id,
            space.name(),
            space.config(),
            pipeline.intent().config().config()
        ))),
        DecodedColorSpace::SourceEncoded(decoded_source) => {
            let resolved = if asset.source_color.user_override().is_some() {
                authoritative.clone()
            } else {
                reconcile_detected_source(authoritative, decoded_source).map_err(|field| {
                    LibraryError::Render(format!(
                        "Asset {} decoded source {field} no longer matches the Project's detected metadata; re-probe the Asset before rendering",
                        asset.id
                    ))
                })?
            };
            standard_space_for_description(&resolved)
                .map(|space| space.as_str().to_string())
                .map_err(|reason| LibraryError::Render(format!("Asset {} {reason}", asset.id)))
        }
        DecodedColorSpace::Unknown { reason } => Err(LibraryError::Render(format!(
            "Asset {} decoded color is unverified: {reason}",
            asset.id
        ))),
    }
}

fn has_color_identity(source: &SourceColorDescription) -> bool {
    source.primaries.is_some() || source.transfer.is_some() || source.profile.is_some()
}

fn validate_rgb_conversion(
    asset: &Asset,
    decoded: &DecodedPixelDescription,
) -> Result<(), LibraryError> {
    if decoded.rgb_matrix_applied() && decoded.full_range() {
        Ok(())
    } else {
        Err(LibraryError::Render(format!(
            "Asset {} decoder did not prove full-range RGB matrix/range conversion",
            asset.id
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::standard_space_for_description;
    use crate::model::asset::{
        SourceColorDescription, SourceColorPrimaries, SourceTransferCharacteristic,
    };
    use ruvie_color_management::StandardColorSpaceId;

    fn source(
        primaries: SourceColorPrimaries,
        transfer: SourceTransferCharacteristic,
    ) -> SourceColorDescription {
        SourceColorDescription {
            primaries: Some(primaries),
            transfer: Some(transfer),
            ..SourceColorDescription::default()
        }
    }

    #[test]
    fn cicp_primaries_and_transfer_map_to_typed_standard_spaces_only() {
        let cases = [
            (
                source(
                    SourceColorPrimaries::Bt709,
                    SourceTransferCharacteristic::Srgb,
                ),
                StandardColorSpaceId::Srgb,
            ),
            (
                source(
                    SourceColorPrimaries::Bt709,
                    SourceTransferCharacteristic::Bt709,
                ),
                StandardColorSpaceId::Bt709,
            ),
            (
                source(
                    SourceColorPrimaries::DisplayP3,
                    SourceTransferCharacteristic::Srgb,
                ),
                StandardColorSpaceId::DisplayP3,
            ),
            (
                source(
                    SourceColorPrimaries::Bt2020,
                    SourceTransferCharacteristic::Bt2020_10,
                ),
                StandardColorSpaceId::Rec2020Sdr10,
            ),
            (
                source(
                    SourceColorPrimaries::Bt2020,
                    SourceTransferCharacteristic::Bt2020_12,
                ),
                StandardColorSpaceId::Rec2020Sdr12,
            ),
            (
                source(
                    SourceColorPrimaries::Bt2020,
                    SourceTransferCharacteristic::Pq,
                ),
                StandardColorSpaceId::Rec2100Pq,
            ),
            (
                source(
                    SourceColorPrimaries::Bt2020,
                    SourceTransferCharacteristic::Hlg,
                ),
                StandardColorSpaceId::Rec2100Hlg,
            ),
        ];
        for (source, expected) in cases {
            assert_eq!(standard_space_for_description(&source), Ok(expected));
        }
    }

    #[test]
    fn incomplete_or_nonstandard_cicp_needs_an_exact_config_owned_assignment() {
        assert!(standard_space_for_description(&SourceColorDescription::default()).is_err());
        assert!(
            standard_space_for_description(&source(
                SourceColorPrimaries::DciP3,
                SourceTransferCharacteristic::Gamma22,
            ))
            .is_err()
        );
        assert!(
            standard_space_for_description(&source(
                SourceColorPrimaries::Bt709,
                SourceTransferCharacteristic::Bt2020_10,
            ))
            .is_err()
        );
    }
}
