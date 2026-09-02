//! Authoritative PNG header metadata decoding.
//!
//! The PNG v3 color chunks overlap deliberately. Their physical order does
//! not define their authority: cICP wins over iCCP, which wins over sRGB,
//! which wins over the legacy cHRM/gAMA pair. Keeping that rule here avoids
//! letting a generic decoder's convenience fallback choose source color.

use super::native_png_chunk_inventory::inventory_png_color_chunks;
use crate::model::asset::{
    SourceColorDescription, SourceColorPrimaries, SourceColorProfile, SourceColorRange,
    SourceMatrixCoefficients, SourceTransferCharacteristic,
};
use png::{
    BitDepth, CodingIndependentCodePoints, DecodeOptions, Info, ScaledFloat, SourceChromaticities,
};
use sha2::{Digest, Sha256};
use std::io::{self, BufRead, Read, Seek, SeekFrom};

const MAX_ICC_PROFILE_BYTES: usize = 16 * 1024 * 1024;
const MAX_PNG_HEADER_BYTES: usize = 64 * 1024 * 1024;
// The decoder holds the compressed iCCP chunk and its decompressed profile at
// once. Leave bounded headroom for the PNG row/header structures too.
const PNG_HEADER_ALLOCATION_BYTES: usize = MAX_ICC_PROFILE_BYTES * 2 + 1024 * 1024;

#[derive(Debug)]
pub(super) struct PngMetadataProbe {
    pub width: u32,
    pub height: u32,
    pub source_color: SourceColorDescription,
}

pub(super) fn probe_png_metadata<R>(mut reader: R) -> io::Result<PngMetadataProbe>
where
    R: BufRead + Seek,
{
    let inventory = inventory_png_color_chunks(&mut reader)?;
    let mut options = DecodeOptions::default();
    options.set_ignore_checksums(false);
    options.set_ignore_text_chunk(true);
    options.set_skip_ancillary_crc_failures(false);

    let bounded_reader = BoundedHeaderReader::new(reader, MAX_PNG_HEADER_BYTES)?;
    let mut decoder = png::Decoder::new_with_options(bounded_reader, options);
    decoder.set_limits(png::Limits {
        bytes: PNG_HEADER_ALLOCATION_BYTES,
    });
    let reader = decoder.read_info().map_err(invalid_png_metadata)?;
    let info = reader.info();
    inventory.validate_decoded_info(info)?;
    let bit_depth = encoded_bit_depth(info.bit_depth);
    let mut source_color = authoritative_source_color(info)?;
    source_color.bit_depth = Some(bit_depth);

    Ok(PngMetadataProbe {
        width: info.width,
        height: info.height,
        source_color,
    })
}

/// Caps bytes consumed before the first IDAT without duplicating PNG chunk
/// parsing. `png::Decoder` remains the sole syntax/CRC authority.
struct BoundedHeaderReader<R> {
    inner: R,
    origin: u64,
    position: u64,
    limit: u64,
}

impl<R: Seek> BoundedHeaderReader<R> {
    fn new(mut inner: R, maximum: usize) -> io::Result<Self> {
        let origin = inner.stream_position()?;
        let maximum = u64::try_from(maximum).map_err(|_| Self::exhausted_error())?;
        let limit = origin
            .checked_add(maximum)
            .ok_or_else(Self::exhausted_error)?;
        Ok(Self {
            inner,
            origin,
            position: origin,
            limit,
        })
    }

    fn exhausted_error() -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("PNG header exceeds the {MAX_PNG_HEADER_BYTES} byte limit"),
        )
    }

    fn remaining(&self) -> usize {
        usize::try_from(self.limit.saturating_sub(self.position)).unwrap_or(usize::MAX)
    }

    fn advance(&mut self, amount: usize) -> io::Result<()> {
        let amount = u64::try_from(amount).map_err(|_| Self::exhausted_error())?;
        self.position = self
            .position
            .checked_add(amount)
            .filter(|position| *position <= self.limit)
            .ok_or_else(Self::exhausted_error)?;
        Ok(())
    }
}

impl<R: Read + Seek> Read for BoundedHeaderReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let remaining = self.remaining();
        if remaining == 0 {
            return Err(Self::exhausted_error());
        }
        let allowed = buffer.len().min(remaining);
        let read = self.inner.read(&mut buffer[..allowed])?;
        self.advance(read)?;
        Ok(read)
    }
}

impl<R: BufRead + Seek> BufRead for BoundedHeaderReader<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        let remaining = self.remaining();
        if remaining == 0 {
            return Err(Self::exhausted_error());
        }
        self.inner
            .fill_buf()
            .map(|available| &available[..available.len().min(remaining)])
    }

    fn consume(&mut self, amount: usize) {
        let consumed = amount.min(self.remaining());
        self.inner.consume(consumed);
        self.position = self
            .position
            .saturating_add(u64::try_from(consumed).unwrap_or(u64::MAX))
            .min(self.limit);
    }
}

impl<R: Seek> Seek for BoundedHeaderReader<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let previous = self.position;
        let absolute = self.inner.seek(position)?;
        if absolute < self.origin || absolute > self.limit {
            self.inner.seek(SeekFrom::Start(previous))?;
            return Err(Self::exhausted_error());
        }
        self.position = absolute;
        Ok(absolute)
    }
}

fn authoritative_source_color(info: &Info<'_>) -> io::Result<SourceColorDescription> {
    if let Some(cicp) = info.coding_independent_code_points {
        return Ok(source_from_cicp(cicp));
    }
    if let Some(profile) = info.icc_profile.as_deref() {
        if profile.len() > MAX_ICC_PROFILE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "PNG iCCP expands to {} bytes, above the {} byte limit",
                    profile.len(),
                    MAX_ICC_PROFILE_BYTES
                ),
            ));
        }
        return Ok(SourceColorDescription {
            profile: Some(icc_source_profile(profile)),
            ..SourceColorDescription::default()
        });
    }
    if info.srgb.is_some() {
        return Ok(srgb_description());
    }
    Ok(source_from_legacy_chunks(
        info.chrm_chunk,
        info.gama_chunk.map(png::ScaledFloat::into_scaled),
    ))
}

fn source_from_cicp(cicp: CodingIndependentCodePoints) -> SourceColorDescription {
    let primaries = cicp_primaries(cicp.color_primaries);
    let transfer = cicp_transfer(cicp.transfer_function);
    let matrix = cicp_matrix(cicp.matrix_coefficients);

    // PNG samples are RGB. H.273 matrix code points other than identity are
    // therefore impossible declarations even when the numeric code is known.
    if primaries.is_none()
        || transfer.is_none()
        || matrix != Some(SourceMatrixCoefficients::Identity)
    {
        return SourceColorDescription {
            primaries: Some(
                primaries.unwrap_or(SourceColorPrimaries::UnknownCode(i32::from(
                    cicp.color_primaries,
                ))),
            ),
            transfer: Some(
                transfer.unwrap_or(SourceTransferCharacteristic::UnknownCode(i32::from(
                    cicp.transfer_function,
                ))),
            ),
            matrix: Some(
                matrix.unwrap_or(SourceMatrixCoefficients::UnknownCode(i32::from(
                    cicp.matrix_coefficients,
                ))),
            ),
            range: Some(cicp_range(cicp)),
            profile: Some(SourceColorProfile::Other {
                profile_kind: "png-cicp".to_string(),
                identity: format!(
                    "cicp:{},{},{},{}",
                    cicp.color_primaries,
                    cicp.transfer_function,
                    cicp.matrix_coefficients,
                    u8::from(cicp.is_video_full_range_image)
                ),
            }),
            ..SourceColorDescription::default()
        };
    }

    SourceColorDescription {
        primaries,
        transfer,
        matrix,
        range: Some(cicp_range(cicp)),
        ..SourceColorDescription::default()
    }
}

const fn cicp_range(cicp: CodingIndependentCodePoints) -> SourceColorRange {
    if cicp.is_video_full_range_image {
        SourceColorRange::Full
    } else {
        SourceColorRange::Limited
    }
}

fn cicp_primaries(code: u8) -> Option<SourceColorPrimaries> {
    Some(match code {
        1 => SourceColorPrimaries::Bt709,
        4 => SourceColorPrimaries::Bt470M,
        5 => SourceColorPrimaries::Bt470Bg,
        6 => SourceColorPrimaries::Smpte170M,
        7 => SourceColorPrimaries::Smpte240M,
        8 => SourceColorPrimaries::Film,
        9 => SourceColorPrimaries::Bt2020,
        10 => SourceColorPrimaries::Smpte428,
        11 => SourceColorPrimaries::DciP3,
        12 => SourceColorPrimaries::DisplayP3,
        22 => SourceColorPrimaries::Ebu3213,
        _ => return None,
    })
}

fn cicp_transfer(code: u8) -> Option<SourceTransferCharacteristic> {
    Some(match code {
        1 => SourceTransferCharacteristic::Bt709,
        4 => SourceTransferCharacteristic::Gamma22,
        5 => SourceTransferCharacteristic::Gamma28,
        6 => SourceTransferCharacteristic::Smpte170M,
        7 => SourceTransferCharacteristic::Smpte240M,
        8 => SourceTransferCharacteristic::Linear,
        9 => SourceTransferCharacteristic::Log100,
        10 => SourceTransferCharacteristic::Log316,
        11 => SourceTransferCharacteristic::Iec61966_2_4,
        12 => SourceTransferCharacteristic::Bt1361,
        13 => SourceTransferCharacteristic::Srgb,
        14 => SourceTransferCharacteristic::Bt2020_10,
        15 => SourceTransferCharacteristic::Bt2020_12,
        16 => SourceTransferCharacteristic::Pq,
        17 => SourceTransferCharacteristic::Smpte428,
        18 => SourceTransferCharacteristic::Hlg,
        _ => return None,
    })
}

fn cicp_matrix(code: u8) -> Option<SourceMatrixCoefficients> {
    Some(match code {
        0 => SourceMatrixCoefficients::Identity,
        1 => SourceMatrixCoefficients::Bt709,
        4 => SourceMatrixCoefficients::Fcc,
        5 => SourceMatrixCoefficients::Bt470Bg,
        6 => SourceMatrixCoefficients::Smpte170M,
        7 => SourceMatrixCoefficients::Smpte240M,
        8 => SourceMatrixCoefficients::YCgCo,
        9 => SourceMatrixCoefficients::Bt2020NonConstantLuminance,
        10 => SourceMatrixCoefficients::Bt2020ConstantLuminance,
        11 => SourceMatrixCoefficients::Smpte2085,
        12 => SourceMatrixCoefficients::ChromaDerivedNonConstantLuminance,
        13 => SourceMatrixCoefficients::ChromaDerivedConstantLuminance,
        14 => SourceMatrixCoefficients::ICtCp,
        _ => return None,
    })
}

fn source_from_legacy_chunks(
    chromaticities: Option<SourceChromaticities>,
    gamma: Option<u32>,
) -> SourceColorDescription {
    if chromaticities.is_none() && gamma.is_none() {
        return SourceColorDescription::default();
    }
    if gamma == Some(45_455) && chromaticities.is_some_and(is_exact_srgb_chromaticities) {
        return srgb_description();
    }

    let mut declaration = Vec::with_capacity(56);
    declaration.extend_from_slice(b"png-cHRM-gAMA-v1\0");
    match chromaticities {
        Some(value) => {
            declaration.push(1);
            declaration.extend_from_slice(&value.to_be_bytes());
        }
        None => declaration.push(0),
    }
    match gamma {
        Some(value) => {
            declaration.push(1);
            declaration.extend_from_slice(&value.to_be_bytes());
        }
        None => declaration.push(0),
    }
    other_description(
        "png-chrm-gama",
        format!("sha256:{:x}", Sha256::digest(declaration)),
    )
}

fn is_exact_srgb_chromaticities(chromaticities: SourceChromaticities) -> bool {
    chromaticities == exact_srgb_chromaticities()
}

fn exact_srgb_chromaticities() -> SourceChromaticities {
    SourceChromaticities {
        white: (
            ScaledFloat::from_scaled(31_270),
            ScaledFloat::from_scaled(32_900),
        ),
        red: (
            ScaledFloat::from_scaled(64_000),
            ScaledFloat::from_scaled(33_000),
        ),
        green: (
            ScaledFloat::from_scaled(30_000),
            ScaledFloat::from_scaled(60_000),
        ),
        blue: (
            ScaledFloat::from_scaled(15_000),
            ScaledFloat::from_scaled(6_000),
        ),
    }
}

fn srgb_description() -> SourceColorDescription {
    SourceColorDescription {
        primaries: Some(SourceColorPrimaries::Bt709),
        transfer: Some(SourceTransferCharacteristic::Srgb),
        ..SourceColorDescription::default()
    }
}

fn other_description(kind: &str, identity: String) -> SourceColorDescription {
    SourceColorDescription {
        profile: Some(SourceColorProfile::Other {
            profile_kind: kind.to_string(),
            identity,
        }),
        ..SourceColorDescription::default()
    }
}

pub(super) fn icc_source_profile(profile: &[u8]) -> SourceColorProfile {
    const ICC_SIGNATURE: std::ops::Range<usize> = 36..40;
    const ICC_PROFILE_ID: std::ops::Range<usize> = 84..100;
    let profile_id = if profile.len() >= 128 && profile.get(ICC_SIGNATURE) == Some(b"acsp") {
        profile
            .get(ICC_PROFILE_ID)
            .filter(|id| id.iter().any(|byte| *byte != 0))
            .map(|id| id.iter().map(|byte| format!("{byte:02x}")).collect())
    } else {
        None
    };
    SourceColorProfile::Icc {
        sha256: format!("{:x}", Sha256::digest(profile)),
        byte_length: u64::try_from(profile.len()).unwrap_or(u64::MAX),
        profile_id,
    }
}

const fn encoded_bit_depth(depth: BitDepth) -> u8 {
    match depth {
        BitDepth::One => 1,
        BitDepth::Two => 2,
        BitDepth::Four => 4,
        BitDepth::Eight => 8,
        BitDepth::Sixteen => 16,
    }
}

fn invalid_png_metadata(error: png::DecodingError) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid PNG header metadata: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::{cicp_matrix, cicp_primaries, cicp_transfer, probe_png_metadata, source_from_cicp};
    use crate::SkiaRenderer;
    use crate::cache::CacheManager;
    use crate::core::rendering::managed_color_backend::{
        ManagedRenderDestination, ProjectColorPipeline,
    };
    use crate::core::rendering::managed_color_source::ingest_loaded_media;
    use crate::core::rendering::media_color_ingress::MediaAssetKind;
    use crate::core::rendering::renderer::RenderOutput;
    use crate::editor::render_service::{RenderDestination, RenderService};
    use crate::model::asset::{
        Asset, AssetKind, SourceColorPrimaries, SourceColorProfile, SourceColorRange,
        SourceMatrixCoefficients, SourceTransferCharacteristic,
    };
    use crate::model::authoring::AuthoringProject;
    use crate::model::frame::color::Color;
    use crate::model::frame::entity::ImageSurface;
    use crate::model::frame::entity::{FrameContent, FrameItem, FrameObject};
    use crate::model::frame::frame::FrameInfo;
    use crate::model::frame::transform::Transform;
    use crate::model::project::{ColorManagementConfig, HdrColorSettings};
    use crate::plugin::{
        DecodedColorSpace, LoadPlugin, LoadRequest, NativeImageLoader, PluginManager,
    };
    use ordered_float::OrderedFloat;
    use png::chunk::{ChunkType, PLTE, cHRM, cICP, gAMA, iCCP, sRGB};
    use png::{BitDepth, CodingIndependentCodePoints, ColorType, Info, SourceChromaticities};
    use sha2::{Digest, Sha256};
    use std::borrow::Cow;
    use std::io::{Cursor, Read};
    use std::sync::Arc;
    use uuid::Uuid;

    #[test]
    fn cicp_beats_srgb_independent_of_chunk_order() {
        let cicp = [9, 16, 0, 1];
        for chunks in [
            [(sRGB, vec![0]), (cICP, cicp.to_vec())],
            [(cICP, cicp.to_vec()), (sRGB, vec![0])],
        ] {
            let probe = probe(&encode_with_chunks(&chunks)).unwrap();
            assert_eq!(
                probe.source_color.primaries,
                Some(SourceColorPrimaries::Bt2020)
            );
            assert_eq!(
                probe.source_color.transfer,
                Some(SourceTransferCharacteristic::Pq)
            );
            assert_eq!(
                probe.source_color.matrix,
                Some(SourceMatrixCoefficients::Identity)
            );
            assert_eq!(probe.source_color.range, Some(SourceColorRange::Full));
            assert!(probe.source_color.profile.is_none());
        }
    }

    #[test]
    fn cicp_precedence_survives_native_loader_and_managed_ingress() {
        let file = tempfile::NamedTempFile::with_suffix(".png").unwrap();
        std::fs::write(
            file.path(),
            encode_with_chunks(&[(sRGB, vec![0]), (cICP, vec![9, 16, 0, 1])]),
        )
        .unwrap();
        let path = file.path().to_string_lossy().into_owned();
        let loader = NativeImageLoader::new();
        let detected = loader.open(&path).unwrap().remove(0).source_color;
        let response = loader
            .load(
                &LoadRequest::Image { path: path.clone() },
                &CacheManager::new(),
            )
            .unwrap();

        assert!(response.decoded().rgb_matrix_applied());
        assert!(response.decoded().full_range());
        assert!(matches!(
            response.decoded().color_space(),
            DecodedColorSpace::SourceEncoded(source)
                if source.primaries == Some(SourceColorPrimaries::Bt2020)
                    && source.transfer == Some(SourceTransferCharacteristic::Pq)
                    && source.matrix.is_none()
                    && source.range.is_none()
        ));

        let mut asset = Asset::new("cICP source", &path, AssetKind::Image);
        asset.source_color.replace_detected(detected);
        let surface = ImageSurface {
            asset_id: Some(asset.id),
            file_path: path,
            effects: Vec::new(),
            input_color_space: None,
            output_color_space: None,
            transform: Transform::default(),
        };
        let mut project = AuthoringProject::new("PNG cICP precedence", 1, 1, 24.0, 1.0).unwrap();
        project
            .set_color_management(ColorManagementConfig::default().with_hdr_settings(
                HdrColorSettings::for_pq(203.0).expect("valid PQ reference white"),
            ))
            .unwrap();
        project.assets.push(asset.clone());

        let cache = Arc::new(CacheManager::new());
        let plugins = Arc::new(PluginManager::new());
        plugins.register_load_plugin(Arc::new(NativeImageLoader::new()));
        let renderer =
            SkiaRenderer::new(1, 1, Color::black(), false, None, Some(Arc::clone(&cache))).unwrap();
        let mut service = RenderService::new(renderer, plugins, cache);
        let frame = FrameInfo {
            width: 1,
            height: 1,
            background_color: Color::black(),
            color_profile: "sRGB".to_string(),
            render_scale: OrderedFloat(1.0),
            now_time: OrderedFloat(0.0),
            region: None,
            items: vec![FrameItem::Object(FrameObject {
                source_node_id: Uuid::new_v4(),
                spatial_transform_node_id: None,
                spatial_transform: Box::new(Transform::default()),
                content_bounds: None,
                content: FrameContent::Image { surface },
            })],
        };
        let output = service
            .render_authoring_frame(&project, &frame, RenderDestination::Preview)
            .expect("native PNG must reach the production managed RenderService ingress");
        let RenderOutput::Image(rendered) = output else {
            panic!("preview terminal must return an owned display image");
        };
        assert_eq!((rendered.width, rendered.height), (1, 1));
    }

    #[test]
    fn limited_range_png_cicp_is_not_mislabeled_as_full_range_rgb() {
        let file = tempfile::NamedTempFile::with_suffix(".png").unwrap();
        std::fs::write(
            file.path(),
            encode_with_chunks(&[(cICP, vec![1, 13, 0, 0])]),
        )
        .unwrap();
        let path = file.path().to_string_lossy().into_owned();
        let loader = NativeImageLoader::new();
        let detected = loader.open(&path).unwrap().remove(0).source_color;
        let response = loader
            .load(
                &LoadRequest::Image { path: path.clone() },
                &CacheManager::new(),
            )
            .unwrap();
        assert!(!response.decoded().rgb_matrix_applied());
        assert!(!response.decoded().full_range());
        assert!(matches!(
            response.decoded().color_space(),
            DecodedColorSpace::Unknown { reason }
                if reason.contains("identity matrix and full range")
        ));

        let mut asset = Asset::new("limited cICP source", &path, AssetKind::Image);
        asset.source_color.replace_detected(detected);
        let surface = ImageSurface {
            asset_id: Some(asset.id),
            file_path: path,
            effects: Vec::new(),
            input_color_space: None,
            output_color_space: None,
            transform: Transform::default(),
        };
        let mut project = AuthoringProject::new("limited PNG cICP", 1, 1, 24.0, 1.0).unwrap();
        project.assets.push(asset.clone());
        let pipeline =
            ProjectColorPipeline::for_project(&project, ManagedRenderDestination::Preview).unwrap();
        let error = ingest_loaded_media(
            &project,
            &pipeline,
            &surface,
            MediaAssetKind::Image,
            response,
        )
        .expect_err("limited-range PNG must fail at the current managed ingress");
        assert!(
            error
                .to_string()
                .contains("did not prove full-range RGB matrix/range conversion"),
            "limited cICP must be rejected at the matrix/range proof boundary: {error}"
        );
    }

    #[test]
    fn cicp_beats_iccp_independent_of_chunk_order() {
        let profile = valid_icc_profile();
        let iccp = iccp_payload(&profile);
        let cicp = vec![1, 13, 0, 1];
        for chunks in [
            [(iCCP, iccp.clone()), (cICP, cicp.clone())],
            [(cICP, cicp), (iCCP, iccp)],
        ] {
            let source = probe(&encode_with_chunks(&chunks)).unwrap().source_color;
            assert_eq!(source.primaries, Some(SourceColorPrimaries::Bt709));
            assert_eq!(source.transfer, Some(SourceTransferCharacteristic::Srgb));
            assert_eq!(source.range, Some(SourceColorRange::Full));
            assert!(source.profile.is_none());
        }
    }

    #[test]
    fn iccp_beats_srgb_and_keeps_exact_profile_identity() {
        let profile = valid_icc_profile();
        let expected_sha = format!("{:x}", Sha256::digest(&profile));
        let iccp = iccp_payload(&profile);
        for chunks in [
            [(sRGB, vec![0]), (iCCP, iccp.clone())],
            [(iCCP, iccp), (sRGB, vec![0])],
        ] {
            let source = probe(&encode_with_chunks(&chunks)).unwrap().source_color;
            let Some(SourceColorProfile::Icc {
                sha256,
                byte_length,
                profile_id,
            }) = source.profile.as_ref()
            else {
                panic!("iCCP must remain the authoritative exact profile");
            };
            assert_eq!(sha256, &expected_sha);
            assert_eq!(*byte_length, 128);
            assert_eq!(
                profile_id.as_deref(),
                Some("0123456789abcdef1032547698badcfe")
            );
            assert!(source.primaries.is_none());
            assert!(source.transfer.is_none());
        }
    }

    #[test]
    fn unknown_cicp_is_identifiable_and_never_falls_back_to_srgb() {
        let source = probe(&encode_with_chunks(&[
            (cICP, vec![255, 13, 0, 1]),
            (sRGB, vec![0]),
        ]))
        .unwrap()
        .source_color;
        assert!(matches!(
            source.profile,
            Some(SourceColorProfile::Other { ref profile_kind, ref identity })
                if profile_kind == "png-cicp" && identity == "cicp:255,13,0,1"
        ));
        assert_eq!(
            source.primaries,
            Some(SourceColorPrimaries::UnknownCode(255))
        );
        assert_eq!(source.matrix, Some(SourceMatrixCoefficients::Identity));
        assert_eq!(source.range, Some(SourceColorRange::Full));
    }

    #[test]
    fn impossible_png_cicp_matrix_is_fail_closed() {
        let source = source_from_cicp(CodingIndependentCodePoints {
            color_primaries: 1,
            transfer_function: 13,
            matrix_coefficients: 1,
            is_video_full_range_image: true,
        });
        assert!(matches!(
            source.profile,
            Some(SourceColorProfile::Other { ref profile_kind, ref identity })
                if profile_kind == "png-cicp" && identity == "cicp:1,13,1,1"
        ));

        let encoded = encode_with_chunks(&[(sRGB, vec![0]), (cICP, vec![1, 13, 1, 1])]);
        probe(&encoded)
            .expect_err("invalid PNG cICP matrix must not fall back to the lower-priority sRGB");
    }

    #[test]
    fn h273_code_maps_cover_every_existing_typed_source_enum_variant() {
        for (code, expected) in [
            (1, SourceColorPrimaries::Bt709),
            (4, SourceColorPrimaries::Bt470M),
            (5, SourceColorPrimaries::Bt470Bg),
            (6, SourceColorPrimaries::Smpte170M),
            (7, SourceColorPrimaries::Smpte240M),
            (8, SourceColorPrimaries::Film),
            (9, SourceColorPrimaries::Bt2020),
            (10, SourceColorPrimaries::Smpte428),
            (11, SourceColorPrimaries::DciP3),
            (12, SourceColorPrimaries::DisplayP3),
            (22, SourceColorPrimaries::Ebu3213),
        ] {
            assert_eq!(cicp_primaries(code), Some(expected));
        }
        for (code, expected) in [
            (1, SourceTransferCharacteristic::Bt709),
            (4, SourceTransferCharacteristic::Gamma22),
            (5, SourceTransferCharacteristic::Gamma28),
            (6, SourceTransferCharacteristic::Smpte170M),
            (7, SourceTransferCharacteristic::Smpte240M),
            (8, SourceTransferCharacteristic::Linear),
            (9, SourceTransferCharacteristic::Log100),
            (10, SourceTransferCharacteristic::Log316),
            (11, SourceTransferCharacteristic::Iec61966_2_4),
            (12, SourceTransferCharacteristic::Bt1361),
            (13, SourceTransferCharacteristic::Srgb),
            (14, SourceTransferCharacteristic::Bt2020_10),
            (15, SourceTransferCharacteristic::Bt2020_12),
            (16, SourceTransferCharacteristic::Pq),
            (17, SourceTransferCharacteristic::Smpte428),
            (18, SourceTransferCharacteristic::Hlg),
        ] {
            assert_eq!(cicp_transfer(code), Some(expected));
        }
        for (code, expected) in [
            (0, SourceMatrixCoefficients::Identity),
            (1, SourceMatrixCoefficients::Bt709),
            (4, SourceMatrixCoefficients::Fcc),
            (5, SourceMatrixCoefficients::Bt470Bg),
            (6, SourceMatrixCoefficients::Smpte170M),
            (7, SourceMatrixCoefficients::Smpte240M),
            (8, SourceMatrixCoefficients::YCgCo),
            (9, SourceMatrixCoefficients::Bt2020NonConstantLuminance),
            (10, SourceMatrixCoefficients::Bt2020ConstantLuminance),
            (11, SourceMatrixCoefficients::Smpte2085),
            (
                12,
                SourceMatrixCoefficients::ChromaDerivedNonConstantLuminance,
            ),
            (13, SourceMatrixCoefficients::ChromaDerivedConstantLuminance),
            (14, SourceMatrixCoefficients::ICtCp),
        ] {
            assert_eq!(cicp_matrix(code), Some(expected));
        }
        assert!(cicp_primaries(255).is_none());
        assert!(cicp_transfer(255).is_none());
        assert!(cicp_matrix(255).is_none());
    }

    #[test]
    fn crc_invalid_cicp_is_rejected_instead_of_becoming_untagged() {
        let mut png = encode_with_chunks(&[(cICP, vec![1, 13, 0, 1])]);
        corrupt_chunk_crc(&mut png, cICP);
        let error = probe(&png).expect_err("CRC-invalid cICP must fail closed");
        assert!(error.to_string().contains("CRC error"));
    }

    #[test]
    fn invalid_srgb_declaration_cannot_fall_back_to_valid_gama() {
        let bytes = encode_with_chunks(&[
            (gAMA, 45_455_u32.to_be_bytes().to_vec()),
            (sRGB, vec![u8::MAX]),
        ]);
        let error = probe(&bytes)
            .expect_err("malformed higher-priority sRGB must not disappear into gAMA fallback");
        assert!(
            error.to_string().contains("sRGB"),
            "error must identify the rejected declaration: {error}"
        );
    }

    #[test]
    fn duplicate_conflicting_cicp_declarations_are_rejected() {
        let bytes = encode_with_chunks(&[(cICP, vec![1, 13, 0, 1]), (cICP, vec![9, 16, 0, 1])]);
        let error = probe(&bytes).expect_err("duplicate cICP declarations are ambiguous");
        assert!(
            error.to_string().contains("duplicate cICP"),
            "error must identify the duplicate declaration: {error}"
        );
    }

    #[test]
    fn color_declaration_after_plte_is_rejected() {
        let bytes = encode_with_chunks(&[(PLTE, vec![0, 0, 0]), (cICP, vec![1, 13, 0, 1])]);
        let error = probe(&bytes).expect_err("color declarations after PLTE are forbidden");
        assert!(
            error.to_string().contains("after PLTE"),
            "error must identify the invalid ordering: {error}"
        );
    }

    #[test]
    fn raw_inventory_scans_after_idat_and_rejects_late_color_declaration() {
        let mut bytes = encode_with_chunks(&[(cICP, vec![1, 13, 0, 1])]);
        move_chunk_immediately_before(&mut bytes, cICP, png::chunk::IEND);
        let error = probe(&bytes).expect_err("color declarations after IDAT are forbidden");
        assert!(
            error.to_string().contains("after IDAT"),
            "error must identify the invalid ordering: {error}"
        );
    }

    #[test]
    fn exact_legacy_srgb_pair_is_typed_but_partial_or_different_is_other() {
        let chromaticities = SourceChromaticities::new(
            (0.3127, 0.3290),
            (0.6400, 0.3300),
            (0.3000, 0.6000),
            (0.1500, 0.0600),
        );
        let exact = probe(&encode_with_chunks(&[
            (cHRM, chromaticities.to_be_bytes().to_vec()),
            (gAMA, 45_455_u32.to_be_bytes().to_vec()),
        ]))
        .unwrap()
        .source_color;
        assert_eq!(exact.primaries, Some(SourceColorPrimaries::Bt709));
        assert_eq!(exact.transfer, Some(SourceTransferCharacteristic::Srgb));

        let partial = probe(&encode_with_chunks(&[(
            gAMA,
            45_455_u32.to_be_bytes().to_vec(),
        )]))
        .unwrap()
        .source_color;
        assert!(matches!(
            partial.profile,
            Some(SourceColorProfile::Other { ref profile_kind, ref identity })
                if profile_kind == "png-chrm-gama" && identity.starts_with("sha256:")
        ));
    }

    #[test]
    fn header_reader_stops_at_its_byte_budget() {
        let mut reader = super::BoundedHeaderReader::new(Cursor::new([1_u8, 2, 3]), 2).unwrap();
        let mut accepted = [0_u8; 2];
        reader.read_exact(&mut accepted).unwrap();
        assert_eq!(accepted, [1, 2]);
        let error = reader
            .read(&mut [0_u8; 1])
            .expect_err("header reader must not consume beyond its byte budget");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn header_reader_seek_cannot_bypass_its_absolute_limit() {
        use std::io::{Seek, SeekFrom};

        let mut reader = super::BoundedHeaderReader::new(Cursor::new([1_u8, 2, 3]), 2).unwrap();
        let error = reader
            .seek(SeekFrom::Start(3))
            .expect_err("seeking beyond the header budget must fail closed");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);

        reader.seek(SeekFrom::Start(0)).unwrap();
        let mut accepted = [0_u8; 2];
        reader.read_exact(&mut accepted).unwrap();
        assert_eq!(accepted, [1, 2]);
        assert!(reader.read(&mut [0_u8; 1]).is_err());
    }

    fn probe(bytes: &[u8]) -> std::io::Result<super::PngMetadataProbe> {
        probe_png_metadata(Cursor::new(bytes))
    }

    fn encode_with_chunks(chunks: &[(ChunkType, Vec<u8>)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(ColorType::Rgba);
            encoder.set_depth(BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            for (kind, payload) in chunks {
                writer.write_chunk(*kind, payload).unwrap();
            }
            writer.write_image_data(&[1, 2, 3, 255]).unwrap();
        }
        bytes
    }

    fn iccp_payload(profile: &[u8]) -> Vec<u8> {
        let mut info = Info::default();
        info.width = 1;
        info.height = 1;
        info.color_type = ColorType::Rgba;
        info.bit_depth = BitDepth::Eight;
        info.icc_profile = Some(Cow::Owned(profile.to_vec()));
        let mut bytes = Vec::new();
        {
            let encoder = png::Encoder::with_info(&mut bytes, info).unwrap();
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[1, 2, 3, 255]).unwrap();
        }
        chunk_payload(&bytes, iCCP).to_vec()
    }

    fn chunk_payload(bytes: &[u8], wanted: ChunkType) -> &[u8] {
        let mut offset = 8_usize;
        while offset + 12 <= bytes.len() {
            let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            let kind = ChunkType(bytes[offset + 4..offset + 8].try_into().unwrap());
            let payload_start = offset + 8;
            let payload_end = payload_start + length;
            if kind == wanted {
                return &bytes[payload_start..payload_end];
            }
            offset = payload_end + 4;
        }
        panic!("missing PNG chunk {wanted:?}");
    }

    fn move_chunk_immediately_before(bytes: &mut Vec<u8>, moved: ChunkType, before: ChunkType) {
        let moved_range = chunk_range(bytes, moved);
        let chunk = bytes.drain(moved_range).collect::<Vec<_>>();
        let insertion = chunk_range(bytes, before).start;
        bytes.splice(insertion..insertion, chunk);
    }

    fn chunk_range(bytes: &[u8], wanted: ChunkType) -> std::ops::Range<usize> {
        let mut offset = 8_usize;
        while offset + 12 <= bytes.len() {
            let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            let kind = ChunkType(bytes[offset + 4..offset + 8].try_into().unwrap());
            let chunk_end = offset + 12 + length;
            if kind == wanted {
                return offset..chunk_end;
            }
            offset = chunk_end;
        }
        panic!("missing PNG chunk {wanted:?}");
    }

    fn corrupt_chunk_crc(bytes: &mut [u8], wanted: ChunkType) {
        let mut offset = 8_usize;
        while offset + 12 <= bytes.len() {
            let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            let kind = ChunkType(bytes[offset + 4..offset + 8].try_into().unwrap());
            let crc_end = offset + 12 + length;
            if kind == wanted {
                bytes[crc_end - 1] ^= 1;
                return;
            }
            offset = crc_end;
        }
        panic!("missing PNG chunk {wanted:?}");
    }

    fn valid_icc_profile() -> Vec<u8> {
        let mut profile = vec![0_u8; 128];
        profile[..4].copy_from_slice(&128_u32.to_be_bytes());
        profile[36..40].copy_from_slice(b"acsp");
        profile[84..100].copy_from_slice(&[
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x10, 0x32, 0x54, 0x76, 0x98, 0xba,
            0xdc, 0xfe,
        ]);
        profile
    }
}
