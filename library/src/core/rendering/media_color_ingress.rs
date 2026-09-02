//! Authoritative decoded-media checks shared by Project and unmanaged paths.

use ruvie_color_management::StandardColorSpaceId;

use crate::error::LibraryError;
use crate::model::asset::{
    Asset, AssetKind, SourceColorDescription, SourceColorPrimaries, SourceTransferCharacteristic,
};
use crate::model::frame::entity::ImageSurface;
use crate::plugin::{DecodedColorSpace, DecodedPixelBuffer, DecodedPixelDescription};

use super::managed_color_backend::ProjectColorAuthority;

#[cfg(test)]
use crate::model::project::Project;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MediaAssetKind {
    Image,
    Video,
}

impl MediaAssetKind {
    fn matches(self, actual: &AssetKind) -> bool {
        matches!(
            (self, actual),
            (Self::Image, AssetKind::Image) | (Self::Video, AssetKind::Video)
        )
    }

    fn label(self) -> &'static str {
        match self {
            Self::Image => "Image",
            Self::Video => "Video",
        }
    }
}

pub(crate) fn source_asset<'a>(
    project: &'a dyn ProjectColorAuthority,
    surface: &ImageSurface,
    expected_kind: MediaAssetKind,
) -> Result<Option<&'a Asset>, LibraryError> {
    let Some(asset_id) = surface.asset_id else {
        return Ok(None);
    };
    let asset = project
        .assets()
        .iter()
        .find(|asset| asset.id == asset_id)
        .ok_or_else(|| LibraryError::Render(format!("source Asset {asset_id} no longer exists")))?;
    if !expected_kind.matches(&asset.kind) || asset.path != surface.file_path {
        return Err(LibraryError::Render(format!(
            "source {:?} does not match {} Asset {asset_id}",
            surface.file_path,
            expected_kind.label()
        )));
    }
    Ok(Some(asset))
}

/// Reject a known precision-losing loader output without conflating numeric
/// storage with source color-space semantics. Float managed ingress remains
/// valid for high-bit sources; RGBA8 cannot claim to preserve them.
pub(crate) fn validate_decoded_storage_fidelity(
    asset: &Asset,
    decoded: &DecodedPixelDescription,
    pixels: &DecodedPixelBuffer,
) -> Result<(), LibraryError> {
    if matches!(pixels, DecodedPixelBuffer::StraightRgba8(_))
        && let Some(bit_depth) = maximum_source_bit_depth(asset, decoded).filter(|depth| *depth > 8)
    {
        return Err(LibraryError::Render(format!(
            "Asset {} source precision {bit_depth}-bit was quantized to RGBA8 before scene-linear conversion",
            asset.id
        )));
    }
    Ok(())
}

/// The Project-free path exists only for versioned plugin diagnostics and
/// tests. It accepts the ABI's exact sRGB contract, never inferred file tags.
pub(crate) fn require_unmanaged_abi_srgb(
    decoded: &DecodedPixelDescription,
    pixels: &DecodedPixelBuffer,
) -> Result<(), LibraryError> {
    if !decoded.rgb_matrix_applied() || !decoded.full_range() {
        return Err(LibraryError::Render(
            "unmanaged loader output is not proven full-range RGB".to_string(),
        ));
    }
    if !matches!(decoded.color_space(), DecodedColorSpace::Srgb) {
        return Err(LibraryError::Render(
            "Project-free rendering accepts only the versioned loader ABI's explicit sRGB pixels"
                .to_string(),
        ));
    }
    if !matches!(pixels, DecodedPixelBuffer::StraightRgba8(_)) {
        return Err(LibraryError::Render(format!(
            "Project-free rendering requires RGBA8, loader returned {}",
            pixels.storage_name()
        )));
    }
    Ok(())
}

fn maximum_source_bit_depth(asset: &Asset, decoded: &DecodedPixelDescription) -> Option<u8> {
    let decoded_depth = match decoded.color_space() {
        DecodedColorSpace::SourceEncoded(source) => source.bit_depth,
        DecodedColorSpace::AssumedSrgb(assumption) => assumption.detected_source().bit_depth,
        DecodedColorSpace::Srgb
        | DecodedColorSpace::ConfigOwned(_)
        | DecodedColorSpace::Unknown { .. } => None,
    };
    [
        asset.source_color.detected().bit_depth,
        asset
            .source_color
            .user_override()
            .and_then(|source| source.bit_depth),
        decoded_depth,
    ]
    .into_iter()
    .flatten()
    .max()
}

pub(super) fn standard_space_for_description(
    source: &SourceColorDescription,
) -> Result<StandardColorSpaceId, String> {
    if source.profile.is_some() {
        return Err(
            "has an embedded profile that requires an exact config-owned profile transform"
                .to_string(),
        );
    }
    let space = match (&source.primaries, &source.transfer) {
        (Some(SourceColorPrimaries::Bt709), Some(SourceTransferCharacteristic::Srgb)) => {
            StandardColorSpaceId::Srgb
        }
        (Some(SourceColorPrimaries::Bt709), Some(SourceTransferCharacteristic::Bt709)) => {
            StandardColorSpaceId::Bt709
        }
        (Some(SourceColorPrimaries::Bt709), Some(SourceTransferCharacteristic::Linear)) => {
            StandardColorSpaceId::LinearBt709
        }
        (Some(SourceColorPrimaries::DisplayP3), Some(SourceTransferCharacteristic::Srgb)) => {
            StandardColorSpaceId::DisplayP3
        }
        (Some(SourceColorPrimaries::DisplayP3), Some(SourceTransferCharacteristic::Linear)) => {
            StandardColorSpaceId::LinearDisplayP3
        }
        (Some(SourceColorPrimaries::Bt2020), Some(SourceTransferCharacteristic::Linear)) => {
            StandardColorSpaceId::LinearRec2020
        }
        (Some(SourceColorPrimaries::Bt2020), Some(SourceTransferCharacteristic::Bt2020_10)) => {
            StandardColorSpaceId::Rec2020Sdr10
        }
        (Some(SourceColorPrimaries::Bt2020), Some(SourceTransferCharacteristic::Bt2020_12)) => {
            StandardColorSpaceId::Rec2020Sdr12
        }
        (Some(SourceColorPrimaries::Bt2020), Some(SourceTransferCharacteristic::Pq)) => {
            StandardColorSpaceId::Rec2100Pq
        }
        (Some(SourceColorPrimaries::Bt2020), Some(SourceTransferCharacteristic::Hlg)) => {
            StandardColorSpaceId::Rec2100Hlg
        }
        (primaries, transfer) => {
            return Err(format!(
                "source primaries/transfer {primaries:?}/{transfer:?} do not identify one supported standard color space; assign an exact config-owned source space"
            ));
        }
    };
    Ok(space)
}

pub(crate) fn reconcile_detected_source(
    detected: &SourceColorDescription,
    decoded: &SourceColorDescription,
) -> Result<SourceColorDescription, &'static str> {
    let mut resolved = decoded.clone();
    // A persisted video assumption is conditional authority, not detected
    // source truth. If the current frame did not qualify for it, compare any
    // actual fields for relink drift but never backfill missing fields from
    // the assumption. This keeps partially tagged/full-range/high-bit frames
    // from silently regaining BT.709-limited semantics downstream.
    let conditional_assumption_not_applied =
        detected.assumption.is_some() && decoded.assumption.is_none();
    macro_rules! reconcile {
        ($field:ident) => {
            match (&detected.$field, &decoded.$field) {
                (Some(stored), Some(current)) if stored != current => {
                    return Err(stringify!($field));
                }
                (Some(stored), None) if !conditional_assumption_not_applied => {
                    resolved.$field = Some(stored.clone());
                }
                _ => {}
            }
        };
    }
    reconcile!(assumption);
    reconcile!(primaries);
    reconcile!(transfer);
    reconcile!(matrix);
    reconcile!(range);
    reconcile!(bit_depth);
    reconcile!(profile);
    Ok(resolved)
}

#[cfg(test)]
#[path = "media_color_ingress_tests.rs"]
mod tests;
