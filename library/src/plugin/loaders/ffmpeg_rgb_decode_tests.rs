use super::ffmpeg_color_metadata::DecodeColorPlan;
use super::ffmpeg_pixel_decode::decode_frame_pixels;
use crate::model::asset::{
    SourceColorDescription, SourceColorPrimaries, SourceColorRange, SourceMatrixCoefficients,
    SourceTransferCharacteristic,
};
use crate::plugin::DecodedPixelBuffer;
use ffmpeg_next as ffmpeg;

#[test]
fn float_rgb_with_missing_bit_depth_still_uses_the_direct_float_reader() {
    let format = ffmpeg::format::Pixel::GBRPF32LE;
    let mut frame = ffmpeg::util::frame::Video::new(format, 1, 1);
    frame.set_color_space(ffmpeg::color::Space::RGB);
    frame.set_color_range(ffmpeg::color::Range::JPEG);
    write_f32(&mut frame, 0, 1.5);
    write_f32(&mut frame, 1, -0.25);
    write_f32(&mut frame, 2, 2.0);
    let decoded = decode_frame_pixels(&frame, &rgb_plan(format, None))
        .expect("actual FLOAT flag must select direct decode despite missing bit-depth metadata");
    let DecodedPixelBuffer::StraightRgba32F(decoded) = decoded else {
        panic!("floating RGB crossed an RGBA8 compatibility boundary");
    };
    assert_eq!(decoded.data(), &[[2.0, 1.5, -0.25, 1.0]]);
}

#[test]
fn rejected_float_rgb_does_not_bypass_color_authority() {
    let format = ffmpeg::format::Pixel::GBRPF32LE;
    let mut frame = ffmpeg::util::frame::Video::new(format, 1, 1);
    frame.set_color_space(ffmpeg::color::Space::RGB);
    frame.set_color_range(ffmpeg::color::Range::Unspecified);
    let plan = DecodeColorPlan::from_metadata(
        format,
        frame.color_space(),
        frame.color_range(),
        SourceColorDescription {
            matrix: Some(SourceMatrixCoefficients::Identity),
            bit_depth: Some(32),
            ..SourceColorDescription::default()
        },
    );
    let error = decode_frame_pixels(&frame, &plan)
        .expect_err("actual FLOAT flag must not override a rejected color plan");
    assert!(error.to_string().contains("refusing unverified"));
}

#[test]
fn direct_float_and_integer_rgb_reject_negative_linesize_before_slice_creation() {
    let float_format = ffmpeg::format::Pixel::GBRPF32LE;
    let mut float_frame = ffmpeg::util::frame::Video::new(float_format, 1, 1);
    float_frame.set_color_space(ffmpeg::color::Space::RGB);
    float_frame.set_color_range(ffmpeg::color::Range::JPEG);
    assert_negative_linesize_rejected(&mut float_frame, &rgb_plan(float_format, Some(32)));

    let integer_format = ffmpeg::format::Pixel::RGB48LE;
    let mut integer_frame = ffmpeg::util::frame::Video::new(integer_format, 1, 1);
    integer_frame.set_color_space(ffmpeg::color::Space::RGB);
    integer_frame.set_color_range(ffmpeg::color::Range::JPEG);
    assert_negative_linesize_rejected(&mut integer_frame, &rgb_plan(integer_format, Some(16)));
}

fn assert_negative_linesize_rejected(
    frame: &mut ffmpeg::util::frame::Video,
    plan: &DecodeColorPlan,
) {
    // SAFETY: the frame is exclusively borrowed. The original scalar is
    // restored before the FFmpeg-owned frame is dropped.
    let original = unsafe {
        let pointer = frame.as_mut_ptr();
        let original = (*pointer).linesize[0];
        (*pointer).linesize[0] = -original.abs().max(1);
        original
    };
    let error = decode_frame_pixels(frame, plan)
        .expect_err("negative linesize must fail before frame.data creates a slice");
    assert!(error.to_string().contains("non-positive stride"));
    // SAFETY: same exclusive frame borrow; restoring its valid allocation
    // metadata preserves the destructor's normal invariants.
    unsafe {
        (*frame.as_mut_ptr()).linesize[0] = original;
    }
}

fn rgb_plan(format: ffmpeg::format::Pixel, bit_depth: Option<u8>) -> DecodeColorPlan {
    DecodeColorPlan::from_metadata(
        format,
        ffmpeg::color::Space::RGB,
        ffmpeg::color::Range::JPEG,
        SourceColorDescription {
            assumption: None,
            primaries: Some(SourceColorPrimaries::Bt709),
            transfer: Some(SourceTransferCharacteristic::Linear),
            matrix: Some(SourceMatrixCoefficients::Identity),
            range: Some(SourceColorRange::Full),
            bit_depth,
            profile: None,
        },
    )
}

fn write_f32(frame: &mut ffmpeg::util::frame::Video, plane: usize, value: f32) {
    frame.data_mut(plane)[..4].copy_from_slice(&value.to_le_bytes());
}
