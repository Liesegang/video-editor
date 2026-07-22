//! Translation from FFmpeg/H.273 color tags into the persisted source model.
//!
//! FFmpeg's safe enums intentionally collapse unrecognized extension codes.
//! Reading the borrowed codec/frame fields preserves those codes so a newer
//! or vendor-specific tag survives a Project round trip.

use crate::model::asset::{
    SourceColorDescription, SourceColorPrimaries, SourceColorRange, SourceMatrixCoefficients,
    SourceTransferCharacteristic,
};
use ffmpeg_next as ffmpeg;

pub(super) fn from_decoder(decoder: &ffmpeg::decoder::Video) -> SourceColorDescription {
    // SAFETY: FFmpeg owns this AVCodecContext for the lifetime of `decoder`;
    // this function only copies scalar metadata while the borrow is active.
    let context = unsafe { &*decoder.as_ptr() };
    SourceColorDescription {
        primaries: primaries(context.color_primaries as i32),
        transfer: transfer(context.color_trc as i32),
        matrix: matrix(context.colorspace as i32),
        range: range(context.color_range as i32),
        bit_depth: positive_bit_depth(context.bits_per_raw_sample)
            .or_else(|| pixel_bit_depth(decoder.format())),
        profile: None,
    }
}

pub(super) fn from_frame(frame: &ffmpeg::util::frame::Video) -> SourceColorDescription {
    // SAFETY: FFmpeg owns this AVFrame for the lifetime of `frame`; this
    // function only copies scalar metadata while the borrow is active.
    let frame_ref = unsafe { &*frame.as_ptr() };
    SourceColorDescription {
        primaries: primaries(frame_ref.color_primaries as i32),
        transfer: transfer(frame_ref.color_trc as i32),
        matrix: matrix(frame_ref.colorspace as i32),
        range: range(frame_ref.color_range as i32),
        bit_depth: pixel_bit_depth(frame.format()),
        profile: None,
    }
}

fn positive_bit_depth(value: i32) -> Option<u8> {
    u8::try_from(value).ok().filter(|depth| *depth > 0)
}

fn pixel_bit_depth(pixel: ffmpeg::format::Pixel) -> Option<u8> {
    let descriptor = pixel.descriptor()?;
    if descriptor.nb_components() == 0 {
        return None;
    }
    // SAFETY: `descriptor` is returned by `av_pix_fmt_desc_get`, and at least
    // one component was checked above. We only copy the first component depth.
    let depth = unsafe { (*descriptor.as_ptr()).comp[0].depth };
    u8::try_from(depth).ok().filter(|depth| *depth > 0)
}

fn primaries(code: i32) -> Option<SourceColorPrimaries> {
    Some(match code {
        2 => return None, // AVCOL_PRI_UNSPECIFIED
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
        other => SourceColorPrimaries::UnknownCode(other),
    })
}

fn transfer(code: i32) -> Option<SourceTransferCharacteristic> {
    Some(match code {
        2 => return None, // AVCOL_TRC_UNSPECIFIED
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
        other => SourceTransferCharacteristic::UnknownCode(other),
    })
}

fn matrix(code: i32) -> Option<SourceMatrixCoefficients> {
    Some(match code {
        2 => return None, // AVCOL_SPC_UNSPECIFIED
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
        other => SourceMatrixCoefficients::UnknownCode(other),
    })
}

fn range(code: i32) -> Option<SourceColorRange> {
    Some(match code {
        0 => return None, // AVCOL_RANGE_UNSPECIFIED
        1 => SourceColorRange::Limited,
        2 => SourceColorRange::Full,
        other => SourceColorRange::UnknownCode(other),
    })
}

#[cfg(test)]
mod tests {
    use super::{matrix, primaries, range, transfer};
    use crate::model::asset::{
        SourceColorPrimaries, SourceColorRange, SourceMatrixCoefficients,
        SourceTransferCharacteristic,
    };

    #[test]
    fn h273_codes_map_without_guessing_unspecified_values() {
        assert_eq!(primaries(1), Some(SourceColorPrimaries::Bt709));
        assert_eq!(primaries(9), Some(SourceColorPrimaries::Bt2020));
        assert_eq!(transfer(16), Some(SourceTransferCharacteristic::Pq));
        assert_eq!(transfer(18), Some(SourceTransferCharacteristic::Hlg));
        assert_eq!(
            matrix(9),
            Some(SourceMatrixCoefficients::Bt2020NonConstantLuminance)
        );
        assert_eq!(range(1), Some(SourceColorRange::Limited));
        assert_eq!(range(2), Some(SourceColorRange::Full));
        assert_eq!(primaries(2), None);
        assert_eq!(transfer(2), None);
        assert_eq!(matrix(2), None);
        assert_eq!(range(0), None);
    }

    #[test]
    fn extension_codes_are_not_collapsed_to_unspecified() {
        assert_eq!(primaries(99), Some(SourceColorPrimaries::UnknownCode(99)));
        assert_eq!(
            transfer(100),
            Some(SourceTransferCharacteristic::UnknownCode(100))
        );
        assert_eq!(
            matrix(101),
            Some(SourceMatrixCoefficients::UnknownCode(101))
        );
        assert_eq!(range(102), Some(SourceColorRange::UnknownCode(102)));
    }
}
