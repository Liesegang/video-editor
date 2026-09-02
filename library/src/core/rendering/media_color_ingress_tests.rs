use super::*;
use crate::model::asset::{
    SourceColorAssumption, SourceColorProfile, SourceColorRange, SourceMatrixCoefficients,
};
use crate::model::authoring::AuthoringProject;
use crate::model::frame::Image;
use crate::model::frame::transform::Transform;
use crate::plugin::{DecodedStraightRgba8, DecodedStraightRgba32F};

fn surface(asset: &Asset) -> ImageSurface {
    ImageSurface {
        asset_id: Some(asset.id),
        file_path: asset.path.clone(),
        effects: Vec::new(),
        input_color_space: None,
        output_color_space: None,
        transform: Transform::default(),
    }
}

fn rgba8_pixel() -> DecodedPixelBuffer {
    DecodedPixelBuffer::StraightRgba8(
        DecodedStraightRgba8::new(Image::new(1, 1, vec![1, 2, 3, 255])).expect("valid test pixel"),
    )
}

#[test]
fn source_asset_requires_exact_kind_path_and_identity() {
    let image = Asset::new("source", "source.png", AssetKind::Image);
    let mut project = AuthoringProject::new("source authority", 1, 1, 24.0, 1.0).unwrap();
    project.assets.push(image.clone());

    assert_eq!(
        source_asset(&project, &surface(&image), MediaAssetKind::Image)
            .unwrap()
            .map(|asset| asset.id),
        Some(image.id)
    );
    assert!(source_asset(&project, &surface(&image), MediaAssetKind::Video).is_err());

    let mut wrong_path = surface(&image);
    wrong_path.file_path = "relinked.png".to_string();
    assert!(source_asset(&project, &wrong_path, MediaAssetKind::Image).is_err());
}

#[test]
fn source_precision_cannot_be_hidden_by_rgba8_storage() {
    let mut asset = Asset::new("ten bit", "ten-bit.png", AssetKind::Image);
    asset.source_color.replace_detected(SourceColorDescription {
        bit_depth: Some(10),
        ..SourceColorDescription::default()
    });
    let decoded = DecodedPixelDescription::full_range_rgb(DecodedColorSpace::SourceEncoded(
        SourceColorDescription {
            bit_depth: Some(10),
            ..SourceColorDescription::default()
        },
    ));

    let error = validate_decoded_storage_fidelity(&asset, &decoded, &rgba8_pixel())
        .expect_err("RGBA8 cannot preserve ten-bit source precision");
    assert!(error.to_string().contains("10-bit was quantized to RGBA8"));

    let float = DecodedPixelBuffer::StraightRgba32F(
        DecodedStraightRgba32F::new(1, 1, vec![[0.25, 0.5, 2.0, 1.0]]).expect("valid float source"),
    );
    validate_decoded_storage_fidelity(&asset, &decoded, &float)
        .expect("float storage preserves high-bit source ingress");
}

#[test]
fn unmanaged_abi_accepts_only_explicit_full_range_srgb_rgba8() {
    require_unmanaged_abi_srgb(&DecodedPixelDescription::abi_v1_srgb(), &rgba8_pixel())
        .expect("versioned unmanaged ABI remains supported");

    let unknown = DecodedPixelDescription::unverified("decoder omitted authority");
    assert!(require_unmanaged_abi_srgb(&unknown, &rgba8_pixel()).is_err());

    let float = DecodedPixelBuffer::StraightRgba32F(
        DecodedStraightRgba32F::new(1, 1, vec![[0.25, 0.5, 2.0, 1.0]]).expect("valid float source"),
    );
    assert!(require_unmanaged_abi_srgb(&DecodedPixelDescription::abi_v1_srgb(), &float).is_err());
}

#[test]
fn standard_source_description_maps_only_exact_supported_semantics() {
    let display_p3 = SourceColorDescription {
        primaries: Some(SourceColorPrimaries::DisplayP3),
        transfer: Some(SourceTransferCharacteristic::Srgb),
        ..SourceColorDescription::default()
    };
    assert_eq!(
        standard_space_for_description(&display_p3),
        Ok(StandardColorSpaceId::DisplayP3)
    );

    let profiled = SourceColorDescription {
        profile: Some(SourceColorProfile::Other {
            profile_kind: "fixture".to_string(),
            identity: "exact-profile".to_string(),
        }),
        ..display_p3
    };
    assert!(standard_space_for_description(&profiled).is_err());
}

#[test]
fn unapplied_video_assumption_never_backfills_the_current_frame() {
    let imported = SourceColorDescription {
        assumption: Some(SourceColorAssumption::UntaggedYuvBt709LimitedV1),
        primaries: Some(SourceColorPrimaries::Bt709),
        transfer: Some(SourceTransferCharacteristic::Bt709),
        matrix: Some(SourceMatrixCoefficients::Bt709),
        range: Some(SourceColorRange::Limited),
        bit_depth: Some(8),
        profile: None,
    };
    let actual_tagged_frame = SourceColorDescription {
        primaries: Some(SourceColorPrimaries::Bt709),
        transfer: Some(SourceTransferCharacteristic::Bt709),
        bit_depth: Some(8),
        ..SourceColorDescription::default()
    };
    assert_eq!(
        reconcile_detected_source(&imported, &actual_tagged_frame),
        Ok(actual_tagged_frame),
        "consumed matrix/range must not be reconstructed from an old assumption"
    );

    let relinked_high_bit = SourceColorDescription {
        bit_depth: Some(10),
        ..SourceColorDescription::default()
    };
    assert_eq!(
        reconcile_detected_source(&imported, &relinked_high_bit),
        Err("bit_depth")
    );
}
