//! Translation from FFmpeg/H.273 stream/codec tags into the persisted source
//! model.
//!
//! Only values represented by the linked FFmpeg version's safe enums are
//! mapped. Per-frame metadata is deliberately outside this foundation until
//! the loader response and cache can transport it without dropping it.

use crate::model::asset::{
    SourceColorDescription, SourceColorPrimaries, SourceColorRange, SourceMatrixCoefficients,
    SourceTransferCharacteristic,
};
use crate::plugin::{DecodedColorSpace, DecodedPixelDescription};
use ffmpeg_next as ffmpeg;

pub(super) fn from_decoder(decoder: &ffmpeg::decoder::Video) -> SourceColorDescription {
    SourceColorDescription {
        primaries: primaries(decoder.color_primaries()),
        transfer: transfer(decoder.color_transfer_characteristic()),
        matrix: matrix(decoder.color_space()),
        range: range(decoder.color_range()),
        bit_depth: decoder_raw_bit_depth(decoder).or_else(|| pixel_bit_depth(decoder.format())),
        profile: None,
    }
}

pub(super) fn decoded_rgba8(
    decoder: &ffmpeg::decoder::Video,
    output_color_space: Option<&str>,
) -> DecodedPixelDescription {
    let color_space = if let Some(output) = output_color_space {
        DecodedColorSpace::Named(output.to_string())
    } else {
        let mut source = from_decoder(decoder);
        // swscale has already expanded YUV matrix/range into full-range RGB.
        source.matrix = None;
        source.range = None;
        source.bit_depth = None;
        DecodedColorSpace::SourceEncoded(source)
    };
    DecodedPixelDescription::straight_rgba8(color_space)
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
    use super::{matrix, pixel_bit_depth, primaries, range, transfer};
    use crate::model::asset::{
        SourceColorPrimaries, SourceColorRange, SourceMatrixCoefficients,
        SourceTransferCharacteristic,
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
    fn pixel_depth_requires_uniform_active_components() {
        use ffmpeg_next::format::Pixel;

        assert_eq!(pixel_bit_depth(Pixel::RGB565LE), None);
        assert_eq!(pixel_bit_depth(Pixel::YUV420P10LE), Some(10));
    }
}
