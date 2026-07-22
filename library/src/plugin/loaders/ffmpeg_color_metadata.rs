//! Translation from FFmpeg/H.273 stream/codec tags into the persisted source
//! model.
//!
//! Actual AVFrame tags have first priority. Missing frame fields fall back to
//! the current decoder/stream metadata, never stale Project detection. This is
//! required for codecs that expose matrix/range on AVCodecContext but leave
//! otherwise valid decoded frames unspecified.

use crate::model::asset::{
    DecoderSourceColorAuthority, SourceColorAssumption, SourceColorDescription,
    SourceColorPrimaries, SourceColorRange, SourceMatrixCoefficients, SourceTransferCharacteristic,
};
use crate::plugin::{DecodedPixelDescription, YuvToRgbOperation};
use ffmpeg_next as ffmpeg;

use super::ffmpeg_yuv_color::{self, ChromaLocationResolution, H273YuvToRgb};

/// Color operation selected from the metadata carried by one decoded frame.
///
/// The plan travels with the resulting pixels. A verified variant is emitted
/// only when the exact matrix and range can be evaluated by RuViE's versioned
/// descriptor-driven H.273 floating-point conversion.
#[derive(Clone, Debug)]
pub(super) struct DecodeColorPlan {
    source: SourceColorDescription,
    operation: DecodeColorOperation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DecodePixelStorage {
    Rgba8,
    Rgba32FDirectRgb,
    Rgba32FDirectYuv,
    Rejected,
}

#[derive(Clone, Debug)]
enum DecodeColorOperation {
    AlreadyFullRangeRgb,
    ApplyYuvToFullRangeRgb {
        conversion: H273YuvToRgb,
        source_matrix: SourceMatrixCoefficients,
        source_range: SourceColorRange,
    },
    Unverified {
        reason: String,
    },
}

impl DecodeColorPlan {
    pub(super) fn from_frame(
        frame: &ffmpeg::util::frame::Video,
        decoder: &ffmpeg::decoder::Video,
        source_authority: Option<&DecoderSourceColorAuthority>,
    ) -> Self {
        let source = source_from_frame(frame, decoder, source_authority);
        let color_space = ffmpeg_space(source.matrix.as_ref());
        let color_range = ffmpeg_range(source.range.as_ref());
        Self::from_metadata_with_chroma_location(
            frame.format(),
            color_space,
            color_range,
            source,
            ChromaLocationResolution::from_frame_and_decoder(
                frame.chroma_location(),
                decoder.chroma_location(),
            ),
        )
    }

    #[cfg(test)]
    pub(super) fn from_metadata(
        pixel_format: ffmpeg::format::Pixel,
        color_space: ffmpeg::color::Space,
        color_range: ffmpeg::color::Range,
        source: SourceColorDescription,
    ) -> Self {
        Self::from_metadata_with_chroma_location(
            pixel_format,
            color_space,
            color_range,
            source,
            ChromaLocationResolution::deferred_frame(),
        )
    }

    fn from_metadata_with_chroma_location(
        pixel_format: ffmpeg::format::Pixel,
        color_space: ffmpeg::color::Space,
        color_range: ffmpeg::color::Range,
        source: SourceColorDescription,
        chroma_location: ChromaLocationResolution,
    ) -> Self {
        let is_rgb = pixel_format_is_rgb(pixel_format);
        if is_rgb {
            return if color_space == ffmpeg::color::Space::RGB
                && color_range == ffmpeg::color::Range::JPEG
            {
                Self {
                    source,
                    operation: DecodeColorOperation::AlreadyFullRangeRgb,
                }
            } else {
                Self::unverified(
                    source,
                    format!(
                        "decoded RGB frame '{}' has non-authoritative matrix/range tags ({}, {}); no range conversion was claimed",
                        pixel_format_name(pixel_format),
                        color_space_name(color_space),
                        color_range_name(color_range),
                    ),
                )
            };
        }

        let Some((matrix, source_matrix)) = ffmpeg_yuv_color::matrix(color_space) else {
            return Self::unverified(
                source,
                format!(
                    "decoded non-RGB frame '{}' uses unsupported or unspecified matrix '{}'",
                    pixel_format_name(pixel_format),
                    color_space_name(color_space),
                ),
            );
        };
        let (range, source_range) = match ffmpeg_yuv_color::range(color_range) {
            Some(range) => range,
            None => {
                return Self::unverified(
                    source,
                    format!(
                        "decoded non-RGB frame '{}' has unspecified range for matrix '{}'",
                        pixel_format_name(pixel_format),
                        color_space_name(color_space),
                    ),
                );
            }
        };

        Self {
            source,
            operation: DecodeColorOperation::ApplyYuvToFullRangeRgb {
                conversion: H273YuvToRgb::new(matrix, range, chroma_location),
                source_matrix,
                source_range,
            },
        }
    }

    fn unverified(source: SourceColorDescription, reason: String) -> Self {
        Self {
            source,
            operation: DecodeColorOperation::Unverified { reason },
        }
    }

    pub(super) fn decoded_description(&self) -> DecodedPixelDescription {
        match &self.operation {
            DecodeColorOperation::AlreadyFullRangeRgb => {
                DecodedPixelDescription::already_full_range_rgb_from_source(self.source.clone())
            }
            DecodeColorOperation::ApplyYuvToFullRangeRgb {
                conversion,
                source_matrix,
                source_range,
            } => DecodedPixelDescription::applied_yuv_to_full_range_rgb(
                self.source.clone(),
                YuvToRgbOperation::H273PlanarF32V1,
                conversion.applied_chroma_location(),
                source_matrix.clone(),
                source_range.clone(),
            ),
            DecodeColorOperation::Unverified { reason } => {
                DecodedPixelDescription::unverified(reason.clone())
            }
        }
    }

    /// Every verified YUV conversion uses unclamped RGBA32F. RGBA8 remains a
    /// compatibility path only for already-full-range, <=8-bit RGB.
    pub(super) fn pixel_storage(&self) -> DecodePixelStorage {
        match &self.operation {
            DecodeColorOperation::ApplyYuvToFullRangeRgb { .. } => {
                DecodePixelStorage::Rgba32FDirectYuv
            }
            DecodeColorOperation::AlreadyFullRangeRgb if self.rgb_requires_f32() => {
                DecodePixelStorage::Rgba32FDirectRgb
            }
            DecodeColorOperation::AlreadyFullRangeRgb => DecodePixelStorage::Rgba8,
            DecodeColorOperation::Unverified { .. } => DecodePixelStorage::Rejected,
        }
    }

    pub(super) fn verified_yuv_conversion(&self) -> Option<H273YuvToRgb> {
        match self.operation {
            DecodeColorOperation::ApplyYuvToFullRangeRgb { conversion, .. } => Some(conversion),
            DecodeColorOperation::AlreadyFullRangeRgb | DecodeColorOperation::Unverified { .. } => {
                None
            }
        }
    }

    pub(super) fn rejection_reason(&self) -> Option<&str> {
        match &self.operation {
            DecodeColorOperation::Unverified { reason } => Some(reason),
            DecodeColorOperation::AlreadyFullRangeRgb
            | DecodeColorOperation::ApplyYuvToFullRangeRgb { .. } => None,
        }
    }

    fn rgb_requires_f32(&self) -> bool {
        self.source.bit_depth.is_some_and(|depth| depth > 8)
            || matches!(
                self.source.transfer,
                Some(SourceTransferCharacteristic::Pq | SourceTransferCharacteristic::Hlg)
            )
            || matches!(
                self.source.primaries,
                Some(
                    SourceColorPrimaries::Bt2020
                        | SourceColorPrimaries::Smpte428
                        | SourceColorPrimaries::DciP3
                        | SourceColorPrimaries::DisplayP3
                )
            )
    }
}

pub(super) fn from_decoder(decoder: &ffmpeg::decoder::Video) -> SourceColorDescription {
    let detected = SourceColorDescription {
        assumption: None,
        primaries: primaries(decoder.color_primaries()),
        transfer: transfer(decoder.color_transfer_characteristic()),
        matrix: matrix(decoder.color_space()),
        range: range(decoder.color_range()),
        bit_depth: maximum_bit_depth(
            decoder_raw_bit_depth(decoder),
            pixel_bit_depth(decoder.format()),
        ),
        profile: None,
    };
    apply_compatibility_assumption(
        detected,
        decoder.format(),
        &SourceColorAssumption::UntaggedYuvBt709LimitedV1,
    )
}

fn apply_compatibility_assumption(
    detected: SourceColorDescription,
    pixel_format: ffmpeg::format::Pixel,
    assumption: &SourceColorAssumption,
) -> SourceColorDescription {
    let is_completely_untagged = detected.assumption.is_none()
        && detected.primaries.is_none()
        && detected.transfer.is_none()
        && detected.matrix.is_none()
        && detected.range.is_none()
        && detected.profile.is_none();
    if !is_completely_untagged
        || !pixel_format_supports_limited_yuv_assumption(pixel_format)
        || !matches!(detected.bit_depth, Some(1..=8))
    {
        return detected;
    }
    match assumption {
        SourceColorAssumption::UntaggedYuvBt709LimitedV1 => SourceColorDescription {
            assumption: Some(assumption.clone()),
            primaries: Some(SourceColorPrimaries::Bt709),
            transfer: Some(SourceTransferCharacteristic::Bt709),
            matrix: Some(SourceMatrixCoefficients::Bt709),
            range: Some(SourceColorRange::Limited),
            ..detected
        },
    }
}

fn source_from_frame(
    frame: &ffmpeg::util::frame::Video,
    decoder: &ffmpeg::decoder::Video,
    source_authority: Option<&DecoderSourceColorAuthority>,
) -> SourceColorDescription {
    let detected = SourceColorDescription {
        assumption: None,
        primaries: primaries(frame.color_primaries())
            .or_else(|| primaries(decoder.color_primaries())),
        transfer: transfer(frame.color_transfer_characteristic())
            .or_else(|| transfer(decoder.color_transfer_characteristic())),
        matrix: matrix(frame.color_space()).or_else(|| matrix(decoder.color_space())),
        range: range(frame.color_range()).or_else(|| range(decoder.color_range())),
        bit_depth: maximum_bit_depth(
            pixel_bit_depth(frame.format()),
            maximum_bit_depth(
                decoder_raw_bit_depth(decoder),
                pixel_bit_depth(decoder.format()),
            ),
        ),
        profile: None,
    };
    resolve_source_authority(detected, frame.format(), source_authority)
}

fn resolve_source_authority(
    detected: SourceColorDescription,
    pixel_format: ffmpeg::format::Pixel,
    source_authority: Option<&DecoderSourceColorAuthority>,
) -> SourceColorDescription {
    match source_authority {
        None => detected,
        Some(DecoderSourceColorAuthority::CompatibilityAssumption(assumption)) => {
            apply_compatibility_assumption(detected, pixel_format, assumption)
        }
        Some(DecoderSourceColorAuthority::CompleteUserOverride(source_override)) => {
            let mut authored = source_override.clone();
            // A complete override owns color interpretation, but it may never
            // erase actual decoded storage precision. Conservatively retain
            // the larger value when authored and decoded metadata disagree.
            authored.bit_depth = maximum_bit_depth(authored.bit_depth, detected.bit_depth);
            authored
        }
    }
}

fn maximum_bit_depth(left: Option<u8>, right: Option<u8>) -> Option<u8> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (left, right) => left.or(right),
    }
}

fn ffmpeg_space(matrix: Option<&SourceMatrixCoefficients>) -> ffmpeg::color::Space {
    match matrix {
        Some(SourceMatrixCoefficients::Identity) => ffmpeg::color::Space::RGB,
        Some(SourceMatrixCoefficients::Bt709) => ffmpeg::color::Space::BT709,
        Some(SourceMatrixCoefficients::Fcc) => ffmpeg::color::Space::FCC,
        Some(SourceMatrixCoefficients::Bt470Bg) => ffmpeg::color::Space::BT470BG,
        Some(SourceMatrixCoefficients::Smpte170M) => ffmpeg::color::Space::SMPTE170M,
        Some(SourceMatrixCoefficients::Smpte240M) => ffmpeg::color::Space::SMPTE240M,
        Some(SourceMatrixCoefficients::Bt2020NonConstantLuminance) => {
            ffmpeg::color::Space::BT2020NCL
        }
        Some(SourceMatrixCoefficients::Bt2020ConstantLuminance) => ffmpeg::color::Space::BT2020CL,
        Some(SourceMatrixCoefficients::YCgCo) => ffmpeg::color::Space::YCGCO,
        Some(SourceMatrixCoefficients::Smpte2085) => ffmpeg::color::Space::SMPTE2085,
        Some(SourceMatrixCoefficients::ChromaDerivedNonConstantLuminance) => {
            ffmpeg::color::Space::ChromaDerivedNCL
        }
        Some(SourceMatrixCoefficients::ChromaDerivedConstantLuminance) => {
            ffmpeg::color::Space::ChromaDerivedCL
        }
        Some(SourceMatrixCoefficients::ICtCp) => ffmpeg::color::Space::ICTCP,
        Some(SourceMatrixCoefficients::UnknownCode(_) | SourceMatrixCoefficients::Other(_))
        | None => ffmpeg::color::Space::Unspecified,
    }
}

fn ffmpeg_range(range: Option<&SourceColorRange>) -> ffmpeg::color::Range {
    match range {
        Some(SourceColorRange::Limited) => ffmpeg::color::Range::MPEG,
        Some(SourceColorRange::Full) => ffmpeg::color::Range::JPEG,
        Some(SourceColorRange::UnknownCode(_) | SourceColorRange::Other(_)) | None => {
            ffmpeg::color::Range::Unspecified
        }
    }
}

fn pixel_format_is_rgb(pixel: ffmpeg::format::Pixel) -> bool {
    let Some(descriptor) = pixel.descriptor() else {
        return false;
    };
    // SAFETY: descriptors come from FFmpeg's immutable pixel-format table and
    // remain valid for the process lifetime. Only the integer flags are read.
    let flags = unsafe { std::ptr::addr_of!((*descriptor.as_ptr()).flags).read() };
    let Ok(rgb_flag) = u64::try_from(ffmpeg::ffi::AV_PIX_FMT_FLAG_RGB) else {
        return false;
    };
    flags & rgb_flag != 0
}

/// Positive allow-list for the <=8-bit YUV layouts covered by the versioned
/// limited-range compatibility convention. In particular this excludes
/// grayscale, palette, Bayer, hardware, RGB/GBR, and legacy `yuvj*` formats;
/// the latter carry full-range semantics and must never be relabeled limited.
fn pixel_format_supports_limited_yuv_assumption(pixel: ffmpeg::format::Pixel) -> bool {
    let Some(descriptor) = pixel.descriptor() else {
        return false;
    };
    matches!(
        descriptor.name(),
        "yuv420p"
            | "yuyv422"
            | "yuv422p"
            | "yuv444p"
            | "yuv410p"
            | "yuv411p"
            | "uyvy422"
            | "uyyvyy411"
            | "nv12"
            | "nv21"
            | "yuv440p"
            | "yuva420p"
            | "yvyu422"
            | "yuva444p"
            | "yuva422p"
            | "nv16"
            | "nv24"
            | "nv42"
            | "vuya"
            | "vuyx"
            | "ayuv"
    )
}

fn pixel_format_name(pixel: ffmpeg::format::Pixel) -> &'static str {
    pixel
        .descriptor()
        .map_or("unknown-pixel-format", |descriptor| descriptor.name())
}

fn color_space_name(space: ffmpeg::color::Space) -> &'static str {
    space.name().unwrap_or("unspecified")
}

fn color_range_name(range: ffmpeg::color::Range) -> &'static str {
    range.name().unwrap_or("unspecified")
}

fn decoder_raw_bit_depth(decoder: &ffmpeg::decoder::Video) -> Option<u8> {
    // SAFETY: `decoder` owns a live, aligned AVCodecContext. No Rust reference
    // to the foreign struct is created; only its plain integer field is copied
    // while `decoder` remains borrowed.
    let context = unsafe { decoder.as_ptr() };
    // SAFETY: The same live-context invariant applies to this integer copy.
    let value = unsafe { std::ptr::addr_of!((*context).bits_per_raw_sample).read() };
    positive_bit_depth(value)
}

fn positive_bit_depth(value: i32) -> Option<u8> {
    u8::try_from(value).ok().filter(|depth| *depth > 0)
}

fn pixel_bit_depth(pixel: ffmpeg::format::Pixel) -> Option<u8> {
    let descriptor = pixel.descriptor()?;
    let component_count = usize::from(descriptor.nb_components());
    if component_count == 0 || component_count > 4 {
        return None;
    }

    // SAFETY: `descriptor` comes from FFmpeg's static pixel-format table. We
    // form a raw address to its fixed component array without a Rust reference.
    let components = unsafe { std::ptr::addr_of!((*descriptor.as_ptr()).comp) }
        .cast::<ffmpeg::ffi::AVComponentDescriptor>();
    let mut uniform_depth = None;
    for index in 0..component_count {
        // SAFETY: AVPixFmtDescriptor has four component descriptors and the
        // checked `component_count` bounds this access. Only `depth` is copied.
        let depth = unsafe { std::ptr::addr_of!((*components.add(index)).depth).read() };
        let depth = positive_bit_depth(depth)?;
        match uniform_depth {
            Some(expected) if expected != depth => return None,
            Some(_) => {}
            None => uniform_depth = Some(depth),
        }
    }
    uniform_depth
}

fn primaries(value: ffmpeg::color::Primaries) -> Option<SourceColorPrimaries> {
    use ffmpeg::color::Primaries as Ffmpeg;
    match value {
        Ffmpeg::BT709 => Some(SourceColorPrimaries::Bt709),
        Ffmpeg::BT470M => Some(SourceColorPrimaries::Bt470M),
        Ffmpeg::BT470BG => Some(SourceColorPrimaries::Bt470Bg),
        Ffmpeg::SMPTE170M => Some(SourceColorPrimaries::Smpte170M),
        Ffmpeg::SMPTE240M => Some(SourceColorPrimaries::Smpte240M),
        Ffmpeg::Film => Some(SourceColorPrimaries::Film),
        Ffmpeg::BT2020 => Some(SourceColorPrimaries::Bt2020),
        Ffmpeg::SMPTE428 => Some(SourceColorPrimaries::Smpte428),
        Ffmpeg::SMPTE431 => Some(SourceColorPrimaries::DciP3),
        Ffmpeg::SMPTE432 => Some(SourceColorPrimaries::DisplayP3),
        Ffmpeg::JEDEC_P22 => Some(SourceColorPrimaries::Ebu3213),
        Ffmpeg::Unspecified | Ffmpeg::Reserved0 | Ffmpeg::Reserved => None,
    }
}

fn transfer(value: ffmpeg::color::TransferCharacteristic) -> Option<SourceTransferCharacteristic> {
    use ffmpeg::color::TransferCharacteristic as Ffmpeg;
    match value {
        Ffmpeg::BT709 => Some(SourceTransferCharacteristic::Bt709),
        Ffmpeg::GAMMA22 => Some(SourceTransferCharacteristic::Gamma22),
        Ffmpeg::GAMMA28 => Some(SourceTransferCharacteristic::Gamma28),
        Ffmpeg::SMPTE170M => Some(SourceTransferCharacteristic::Smpte170M),
        Ffmpeg::SMPTE240M => Some(SourceTransferCharacteristic::Smpte240M),
        Ffmpeg::Linear => Some(SourceTransferCharacteristic::Linear),
        Ffmpeg::Log => Some(SourceTransferCharacteristic::Log100),
        Ffmpeg::LogSqrt => Some(SourceTransferCharacteristic::Log316),
        Ffmpeg::IEC61966_2_4 => Some(SourceTransferCharacteristic::Iec61966_2_4),
        Ffmpeg::BT1361_ECG => Some(SourceTransferCharacteristic::Bt1361),
        Ffmpeg::IEC61966_2_1 => Some(SourceTransferCharacteristic::Srgb),
        Ffmpeg::BT2020_10 => Some(SourceTransferCharacteristic::Bt2020_10),
        Ffmpeg::BT2020_12 => Some(SourceTransferCharacteristic::Bt2020_12),
        Ffmpeg::SMPTE2084 => Some(SourceTransferCharacteristic::Pq),
        Ffmpeg::SMPTE428 => Some(SourceTransferCharacteristic::Smpte428),
        Ffmpeg::ARIB_STD_B67 => Some(SourceTransferCharacteristic::Hlg),
        Ffmpeg::Unspecified | Ffmpeg::Reserved0 | Ffmpeg::Reserved => None,
    }
}

fn matrix(value: ffmpeg::color::Space) -> Option<SourceMatrixCoefficients> {
    use ffmpeg::color::Space as Ffmpeg;
    match value {
        Ffmpeg::RGB => Some(SourceMatrixCoefficients::Identity),
        Ffmpeg::BT709 => Some(SourceMatrixCoefficients::Bt709),
        Ffmpeg::FCC => Some(SourceMatrixCoefficients::Fcc),
        Ffmpeg::BT470BG => Some(SourceMatrixCoefficients::Bt470Bg),
        Ffmpeg::SMPTE170M => Some(SourceMatrixCoefficients::Smpte170M),
        Ffmpeg::SMPTE240M => Some(SourceMatrixCoefficients::Smpte240M),
        Ffmpeg::YCGCO => Some(SourceMatrixCoefficients::YCgCo),
        Ffmpeg::BT2020NCL => Some(SourceMatrixCoefficients::Bt2020NonConstantLuminance),
        Ffmpeg::BT2020CL => Some(SourceMatrixCoefficients::Bt2020ConstantLuminance),
        Ffmpeg::SMPTE2085 => Some(SourceMatrixCoefficients::Smpte2085),
        Ffmpeg::ChromaDerivedNCL => {
            Some(SourceMatrixCoefficients::ChromaDerivedNonConstantLuminance)
        }
        Ffmpeg::ChromaDerivedCL => Some(SourceMatrixCoefficients::ChromaDerivedConstantLuminance),
        Ffmpeg::ICTCP => Some(SourceMatrixCoefficients::ICtCp),
        // Newer FFmpeg matrix variants that do not yet have a typed Project
        // representation remain unknown rather than being guessed.
        _ => None,
    }
}

fn range(value: ffmpeg::color::Range) -> Option<SourceColorRange> {
    match value {
        ffmpeg::color::Range::MPEG => Some(SourceColorRange::Limited),
        ffmpeg::color::Range::JPEG => Some(SourceColorRange::Full),
        ffmpeg::color::Range::Unspecified => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DecodeColorPlan, DecodePixelStorage, apply_compatibility_assumption, matrix,
        pixel_bit_depth, pixel_format_supports_limited_yuv_assumption, primaries, range,
        resolve_source_authority, transfer,
    };
    use crate::model::asset::{
        DecoderSourceColorAuthority, SourceColorAssumption, SourceColorDescription,
        SourceColorPrimaries, SourceColorRange, SourceMatrixCoefficients,
        SourceTransferCharacteristic,
    };
    use crate::plugin::loaders::DecodedRgbConversion;
    use crate::plugin::loaders::ffmpeg_pixel_decode::decode_frame_pixels;
    use crate::plugin::loaders::ffmpeg_yuv_color::ChromaLocationResolution;
    use crate::plugin::{
        DecodedColorSpace, DecodedPixelBuffer, YuvChromaLocation, YuvChromaLocationSource,
    };

    #[test]
    fn ffmpeg_defined_values_map_without_guessing_unspecified_values() {
        use ffmpeg_next::color::{Primaries, Range, Space, TransferCharacteristic};

        assert_eq!(
            primaries(Primaries::BT709),
            Some(SourceColorPrimaries::Bt709)
        );
        assert_eq!(
            primaries(Primaries::BT2020),
            Some(SourceColorPrimaries::Bt2020)
        );
        assert_eq!(
            transfer(TransferCharacteristic::SMPTE2084),
            Some(SourceTransferCharacteristic::Pq)
        );
        assert_eq!(
            transfer(TransferCharacteristic::ARIB_STD_B67),
            Some(SourceTransferCharacteristic::Hlg)
        );
        assert_eq!(
            matrix(Space::BT2020NCL),
            Some(SourceMatrixCoefficients::Bt2020NonConstantLuminance)
        );
        assert_eq!(range(Range::MPEG), Some(SourceColorRange::Limited));
        assert_eq!(range(Range::JPEG), Some(SourceColorRange::Full));
        assert_eq!(primaries(Primaries::Unspecified), None);
        assert_eq!(transfer(TransferCharacteristic::Unspecified), None);
        assert_eq!(matrix(Space::Unspecified), None);
        assert_eq!(range(Range::Unspecified), None);
    }

    #[test]
    fn completely_untagged_eight_bit_yuv_gets_a_versioned_bt709_limited_authority() {
        let source = apply_compatibility_assumption(
            SourceColorDescription {
                bit_depth: Some(8),
                ..SourceColorDescription::default()
            },
            ffmpeg_next::format::Pixel::YUV420P,
            &SourceColorAssumption::UntaggedYuvBt709LimitedV1,
        );
        assert_eq!(
            source.assumption,
            Some(SourceColorAssumption::UntaggedYuvBt709LimitedV1)
        );
        assert_eq!(source.primaries, Some(SourceColorPrimaries::Bt709));
        assert_eq!(source.transfer, Some(SourceTransferCharacteristic::Bt709));
        assert_eq!(source.matrix, Some(SourceMatrixCoefficients::Bt709));
        assert_eq!(source.range, Some(SourceColorRange::Limited));

        let rgb = apply_compatibility_assumption(
            SourceColorDescription {
                bit_depth: Some(8),
                ..SourceColorDescription::default()
            },
            ffmpeg_next::format::Pixel::RGB24,
            &SourceColorAssumption::UntaggedYuvBt709LimitedV1,
        );
        assert!(rgb.assumption.is_none());
    }

    #[test]
    fn compatibility_assumption_is_conditional_on_the_current_frame() {
        let assumption = DecoderSourceColorAuthority::CompatibilityAssumption(
            SourceColorAssumption::UntaggedYuvBt709LimitedV1,
        );
        let tagged_frame = SourceColorDescription {
            primaries: Some(SourceColorPrimaries::Bt2020),
            transfer: Some(SourceTransferCharacteristic::Pq),
            matrix: Some(SourceMatrixCoefficients::Bt2020NonConstantLuminance),
            range: Some(SourceColorRange::Full),
            bit_depth: Some(8),
            ..SourceColorDescription::default()
        };
        assert_eq!(
            resolve_source_authority(
                tagged_frame.clone(),
                ffmpeg_next::format::Pixel::YUV420P,
                Some(&assumption),
            ),
            tagged_frame,
            "an actual AVFrame tag must beat a stale import assumption"
        );

        let high_bit_frame = SourceColorDescription {
            bit_depth: Some(10),
            ..SourceColorDescription::default()
        };
        assert_eq!(
            resolve_source_authority(
                high_bit_frame.clone(),
                ffmpeg_next::format::Pixel::YUV420P10LE,
                Some(&assumption),
            ),
            high_bit_frame,
            "the 8-bit v1 convention must not relabel a changed 10-bit source"
        );
    }

    #[test]
    fn compatibility_assumption_has_a_positive_yuv_allow_list() {
        assert!(pixel_format_supports_limited_yuv_assumption(
            ffmpeg_next::format::Pixel::YUV420P
        ));
        for pixel in [
            ffmpeg_next::format::Pixel::GRAY8,
            ffmpeg_next::format::Pixel::PAL8,
            ffmpeg_next::format::Pixel::BAYER_BGGR8,
            ffmpeg_next::format::Pixel::RGB24,
            ffmpeg_next::format::Pixel::YUVJ420P,
        ] {
            assert!(
                !pixel_format_supports_limited_yuv_assumption(pixel),
                "{pixel:?} must not inherit limited BT.709 semantics"
            );
        }

        let full_range = SourceColorDescription {
            range: Some(SourceColorRange::Full),
            bit_depth: Some(8),
            ..SourceColorDescription::default()
        };
        assert_eq!(
            resolve_source_authority(
                full_range.clone(),
                ffmpeg_next::format::Pixel::YUV420P,
                Some(&DecoderSourceColorAuthority::CompatibilityAssumption(
                    SourceColorAssumption::UntaggedYuvBt709LimitedV1,
                )),
            ),
            full_range,
            "an explicit full-range frame must never be relabeled limited"
        );
    }

    #[test]
    fn runtime_without_persisted_authority_does_not_invent_an_import_assumption() {
        let detected = SourceColorDescription {
            bit_depth: Some(8),
            ..SourceColorDescription::default()
        };
        assert_eq!(
            resolve_source_authority(detected.clone(), ffmpeg_next::format::Pixel::YUV420P, None,),
            detected
        );
    }

    #[test]
    fn pixel_depth_requires_uniform_active_components() {
        use ffmpeg_next::format::Pixel;

        assert_eq!(pixel_bit_depth(Pixel::RGB565LE), None);
        assert_eq!(pixel_bit_depth(Pixel::YUV420P10LE), Some(10));
    }

    #[test]
    fn every_verified_yuv_source_selects_direct_rgba32f() {
        let cases = [
            SourceColorDescription {
                bit_depth: Some(8),
                ..SourceColorDescription::default()
            },
            SourceColorDescription {
                bit_depth: Some(10),
                ..SourceColorDescription::default()
            },
            SourceColorDescription {
                transfer: Some(SourceTransferCharacteristic::Pq),
                bit_depth: Some(8),
                ..SourceColorDescription::default()
            },
            SourceColorDescription {
                transfer: Some(SourceTransferCharacteristic::Hlg),
                bit_depth: Some(8),
                ..SourceColorDescription::default()
            },
            SourceColorDescription {
                primaries: Some(SourceColorPrimaries::DisplayP3),
                bit_depth: Some(8),
                ..SourceColorDescription::default()
            },
        ];
        for source in cases {
            let plan = DecodeColorPlan::from_metadata(
                ffmpeg_next::format::Pixel::YUV420P,
                ffmpeg_next::color::Space::BT709,
                ffmpeg_next::color::Range::MPEG,
                SourceColorDescription {
                    matrix: Some(SourceMatrixCoefficients::Bt709),
                    range: Some(SourceColorRange::Limited),
                    ..source
                },
            );
            assert_eq!(plan.pixel_storage(), DecodePixelStorage::Rgba32FDirectYuv);
        }
    }

    #[test]
    fn bt709_limited_plan_echoes_the_exact_applied_conversion() {
        let plan = decode_plan(
            ffmpeg_next::color::Space::BT709,
            ffmpeg_next::color::Range::MPEG,
        );
        assert_applied(
            &plan,
            SourceMatrixCoefficients::Bt709,
            SourceColorRange::Limited,
        );
    }

    #[test]
    fn decoder_chroma_fallback_reaches_the_decoded_pixel_provenance() {
        let plan = DecodeColorPlan::from_metadata_with_chroma_location(
            ffmpeg_next::format::Pixel::YUV420P,
            ffmpeg_next::color::Space::BT709,
            ffmpeg_next::color::Range::MPEG,
            SourceColorDescription {
                matrix: Some(SourceMatrixCoefficients::Bt709),
                range: Some(SourceColorRange::Limited),
                bit_depth: Some(8),
                ..SourceColorDescription::default()
            },
            ChromaLocationResolution::from_frame_and_decoder(
                ffmpeg_next::util::chroma::Location::Unspecified,
                ffmpeg_next::util::chroma::Location::Left,
            ),
        );
        let description = plan.decoded_description();
        let DecodedRgbConversion::AppliedYuvToFullRangeRgb(applied) = description.rgb_conversion()
        else {
            panic!("decoder chroma fallback lost verified conversion provenance");
        };
        let chroma = applied.chroma_location().expect("recorded chroma fallback");
        assert_eq!(chroma.location(), YuvChromaLocation::Left);
        assert_eq!(chroma.source(), YuvChromaLocationSource::Decoder);
    }

    #[test]
    fn bt2020_limited_and_full_are_distinct_verified_plans() {
        for (range, expected_range) in [
            (ffmpeg_next::color::Range::MPEG, SourceColorRange::Limited),
            (ffmpeg_next::color::Range::JPEG, SourceColorRange::Full),
        ] {
            let plan = decode_plan(ffmpeg_next::color::Space::BT2020NCL, range);
            assert_applied(
                &plan,
                SourceMatrixCoefficients::Bt2020NonConstantLuminance,
                expected_range,
            );
        }
    }

    #[test]
    fn ten_bit_bt709_and_bt2020_matrix_range_golden_pixels_are_distinct() {
        // H.273 normalized equations for Y=400, Cb=700, Cr=300 at 10-bit.
        // Fixed independent H.273 oracle values for Y=400, Cb=700, Cr=300.
        // The decoder implementation must not derive these expectations.
        let cases = [
            (
                ffmpeg_next::color::Space::BT709,
                ffmpeg_next::color::Range::MPEG,
                [0.010_953, 0.455_019, 0.772_906],
            ),
            (
                ffmpeg_next::color::Space::BT709,
                ffmpeg_next::color::Range::JPEG,
                [0.064_655, 0.453_593, 0.732_016],
            ),
            (
                ffmpeg_next::color::Space::BT2020NCL,
                ffmpeg_next::color::Range::MPEG,
                [0.034_661, 0.484_221, 0.778_320],
            ),
            (
                ffmpeg_next::color::Space::BT2020NCL,
                ffmpeg_next::color::Range::JPEG,
                [0.085_420, 0.479_170, 0.736_758],
            ),
        ];

        for (space, range, expected) in cases {
            let actual = decode_yuv444p10_pixel(space, range);
            for (component, (actual, expected)) in ["r", "g", "b"]
                .into_iter()
                .zip(actual.into_iter().zip(expected))
            {
                assert!(
                    (actual - expected).abs() <= 2.0e-6,
                    "{space:?}/{range:?} {component}: expected {expected}, got {actual}"
                );
            }
        }
    }

    #[test]
    fn unsupported_or_unspecified_yuv_metadata_is_never_claimed_as_source_encoded_rgb() {
        for (space, range) in [
            (
                ffmpeg_next::color::Space::BT2020CL,
                ffmpeg_next::color::Range::MPEG,
            ),
            (
                ffmpeg_next::color::Space::Unspecified,
                ffmpeg_next::color::Range::MPEG,
            ),
            (
                ffmpeg_next::color::Space::BT709,
                ffmpeg_next::color::Range::Unspecified,
            ),
        ] {
            let description = decode_plan(space, range).decoded_description();
            assert!(matches!(
                description.rgb_conversion(),
                DecodedRgbConversion::Unverified { .. }
            ));
            assert!(matches!(
                description.color_space(),
                DecodedColorSpace::Unknown { .. }
            ));
            assert!(!description.rgb_matrix_applied());
            assert!(!description.full_range());
        }
    }

    fn decode_plan(
        space: ffmpeg_next::color::Space,
        range: ffmpeg_next::color::Range,
    ) -> DecodeColorPlan {
        DecodeColorPlan::from_metadata(
            ffmpeg_next::format::Pixel::YUV420P,
            space,
            range,
            SourceColorDescription {
                matrix: matrix(space),
                range: super::range(range),
                ..SourceColorDescription::default()
            },
        )
    }

    fn assert_applied(
        plan: &DecodeColorPlan,
        expected_matrix: SourceMatrixCoefficients,
        expected_range: SourceColorRange,
    ) {
        let description = plan.decoded_description();
        let DecodedRgbConversion::AppliedYuvToFullRangeRgb(applied) = description.rgb_conversion()
        else {
            panic!("expected a verified YUV conversion: {description:?}");
        };
        assert_eq!(applied.source_matrix(), &expected_matrix);
        assert_eq!(applied.source_range(), &expected_range);
        assert_eq!(
            applied.operation(),
            crate::plugin::YuvToRgbOperation::H273PlanarF32V1
        );
        assert!(description.rgb_matrix_applied());
        assert!(description.full_range());
    }

    fn decode_yuv444p10_pixel(
        space: ffmpeg_next::color::Space,
        range: ffmpeg_next::color::Range,
    ) -> [f32; 3] {
        let mut frame =
            ffmpeg_next::util::frame::Video::new(ffmpeg_next::format::Pixel::YUV444P10LE, 2, 2);
        frame.set_color_space(space);
        frame.set_color_range(range);
        fill_u16_plane(&mut frame, 0, 400);
        fill_u16_plane(&mut frame, 1, 700);
        fill_u16_plane(&mut frame, 2, 300);
        let plan = DecodeColorPlan::from_metadata(
            frame.format(),
            space,
            range,
            SourceColorDescription {
                assumption: None,
                primaries: Some(if space == ffmpeg_next::color::Space::BT2020NCL {
                    SourceColorPrimaries::Bt2020
                } else {
                    SourceColorPrimaries::Bt709
                }),
                transfer: Some(SourceTransferCharacteristic::Bt709),
                matrix: matrix(space),
                range: super::range(range),
                bit_depth: Some(10),
                profile: None,
            },
        );
        let decoded = decode_frame_pixels(&frame, &plan).expect("10-bit YUV conversion");
        let DecodedPixelBuffer::StraightRgba32F(decoded) = decoded else {
            panic!("10-bit frame was quantized to RGBA8");
        };
        let [r, g, b, alpha] = decoded.data()[0];
        assert!((alpha - 1.0).abs() <= f32::EPSILON);
        [r, g, b]
    }

    fn fill_u16_plane(frame: &mut ffmpeg_next::util::frame::Video, plane: usize, value: u16) {
        let width = frame.width() as usize;
        let height = frame.height() as usize;
        let stride = frame.stride(plane);
        let bytes = value.to_le_bytes();
        let data = frame.data_mut(plane);
        for row in 0..height {
            for column in 0..width {
                let offset = row * stride + column * 2;
                data[offset..offset + 2].copy_from_slice(&bytes);
            }
        }
    }
}
