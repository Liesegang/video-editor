use super::ffmpeg_color_metadata::DecodeColorPlan;
use super::ffmpeg_pixel_decode::decode_frame_pixels;
use crate::model::asset::{
    SourceColorDescription, SourceColorPrimaries, SourceColorRange, SourceMatrixCoefficients,
    SourceTransferCharacteristic,
};
use crate::plugin::{DecodedPixelBuffer, DecodedRgbConversion, YuvToRgbOperation};
use ffmpeg_next as ffmpeg;

#[test]
fn legal_bt709_codes_preserve_negative_red_and_blue_above_one() {
    let mut frame = frame(ffmpeg::format::Pixel::YUV444P, 1, 1);
    write_constant_plane(&mut frame, 0, 64, 8);
    write_constant_plane(&mut frame, 1, 240, 8);
    write_constant_plane(&mut frame, 2, 16, 8);
    let decoded = decode(
        &frame,
        ffmpeg::color::Space::BT709,
        ffmpeg::color::Range::MPEG,
        8,
    );
    let pixel = rgba32f(&decoded)[0];
    let expected = [-0.568_221_9, 0.359_578_07, 1.146_978_1, 1.0];
    assert_pixel_close(pixel, expected, 2.0e-6);
    assert!(pixel[0] < 0.0, "red was clipped at the YUV boundary");
    assert!(pixel[2] > 1.0, "blue was clipped at the YUV boundary");
}

#[test]
fn fixed_h273_oracle_covers_full_and_limited_bt709_and_bt2020() {
    let cases = [
        (
            ffmpeg::color::Space::BT709,
            ffmpeg::color::Range::MPEG,
            [0.010_952_715, 0.455_018_55, 0.772_906_3],
        ),
        (
            ffmpeg::color::Space::BT709,
            ffmpeg::color::Range::JPEG,
            [0.064_655_33, 0.453_592_75, 0.732_016_44],
        ),
        (
            ffmpeg::color::Space::BT2020NCL,
            ffmpeg::color::Range::MPEG,
            [0.034_660_75, 0.484_221_1, 0.778_319_66],
        ),
        (
            ffmpeg::color::Space::BT2020NCL,
            ffmpeg::color::Range::JPEG,
            [0.085_420_14, 0.479_169_96, 0.736_757_76],
        ),
    ];
    for (space, range, expected) in cases {
        let mut frame = frame(ffmpeg::format::Pixel::YUV444P10LE, 1, 1);
        write_constant_plane(&mut frame, 0, 400, 10);
        write_constant_plane(&mut frame, 1, 700, 10);
        write_constant_plane(&mut frame, 2, 300, 10);
        let decoded = decode(&frame, space, range, 10);
        let actual = rgba32f(&decoded)[0];
        assert_pixel_close(actual, [expected[0], expected[1], expected[2], 1.0], 2.0e-6);
    }
}

#[test]
fn all_supported_matrix_coefficients_execute_without_a_hidden_fallback() {
    for (space, expected) in [
        (
            ffmpeg::color::Space::BT709,
            [0.064_655_33, 0.453_592_75, 0.732_016_44],
        ),
        (
            ffmpeg::color::Space::FCC,
            [0.100_879_766, 0.477_541_3, 0.718_123_14],
        ),
        (
            ffmpeg::color::Space::BT470BG,
            [0.100_465_3, 0.475_756_85, 0.716_653],
        ),
        (
            ffmpeg::color::Space::SMPTE170M,
            [0.100_465_3, 0.475_756_85, 0.716_653],
        ),
        (
            ffmpeg::color::Space::SMPTE240M,
            [0.064_406_65, 0.448_131_9, 0.726_576_75],
        ),
        (
            ffmpeg::color::Space::BT2020NCL,
            [0.085_420_14, 0.479_169_96, 0.736_757_76],
        ),
    ] {
        let mut frame = frame(ffmpeg::format::Pixel::YUV444P10LE, 1, 1);
        write_constant_plane(&mut frame, 0, 400, 10);
        write_constant_plane(&mut frame, 1, 700, 10);
        write_constant_plane(&mut frame, 2, 300, 10);
        let decoded = decode(&frame, space, ffmpeg::color::Range::JPEG, 10);
        let pixel = rgba32f(&decoded)[0];
        assert_pixel_close(pixel, [expected[0], expected[1], expected[2], 1.0], 2.0e-6);
    }
}

#[test]
fn planar_depth_subsampling_endian_and_padded_stride_catalog_decodes() {
    let formats = [
        (ffmpeg::format::Pixel::YUV444P, 8),
        (ffmpeg::format::Pixel::YUV422P, 8),
        (ffmpeg::format::Pixel::YUV420P, 8),
        (ffmpeg::format::Pixel::YUV444P9LE, 9),
        (ffmpeg::format::Pixel::YUV422P9BE, 9),
        (ffmpeg::format::Pixel::YUV420P9LE, 9),
        (ffmpeg::format::Pixel::YUV444P10BE, 10),
        (ffmpeg::format::Pixel::YUV422P10LE, 10),
        (ffmpeg::format::Pixel::YUV420P10BE, 10),
        (ffmpeg::format::Pixel::YUV444P12LE, 12),
        (ffmpeg::format::Pixel::YUV422P12BE, 12),
        (ffmpeg::format::Pixel::YUV420P12LE, 12),
        (ffmpeg::format::Pixel::YUV444P14BE, 14),
        (ffmpeg::format::Pixel::YUV422P14LE, 14),
        (ffmpeg::format::Pixel::YUV420P14BE, 14),
        (ffmpeg::format::Pixel::YUV444P16LE, 16),
        (ffmpeg::format::Pixel::YUV422P16BE, 16),
        (ffmpeg::format::Pixel::YUV420P16LE, 16),
    ];
    for (format, depth) in formats {
        let mut frame = frame(format, 3, 3);
        set_chroma_location(&mut frame, ffmpeg::util::chroma::Location::Left);
        let center = 1_u16 << (depth - 1);
        write_constant_plane(&mut frame, 0, center, depth);
        write_constant_plane(&mut frame, 1, center, depth);
        write_constant_plane(&mut frame, 2, center, depth);
        assert!(
            frame.stride(0) > 3 * usize::from(depth).div_ceil(8),
            "{format:?} fixture did not exercise padded stride"
        );
        let decoded = decode(
            &frame,
            ffmpeg::color::Space::BT709,
            ffmpeg::color::Range::JPEG,
            depth,
        );
        assert_eq!(
            rgba32f(&decoded).len(),
            9,
            "failed catalog format {format:?}"
        );
    }
}

#[test]
fn chroma_location_changes_bilinear_reconstruction_for_subsampled_frames() {
    let left = decode_varying_422(ffmpeg::util::chroma::Location::Left);
    let center = decode_varying_422(ffmpeg::util::chroma::Location::Center);
    // Pixel x=1 samples halfway between U codes for left-sited chroma and one
    // quarter of the interval for centered chroma. These are fixed results of
    // the H.273 sample positions, not values calculated by the decoder helper.
    assert!((left[1][2] - 0.462_080_78).abs() <= 2.0e-6);
    assert!((center[1][2] - 0.231_040_39).abs() <= 2.0e-6);
    assert!(left[1][2] > center[1][2]);
}

#[test]
fn odd_sized_yuv420_uses_vertical_and_horizontal_chroma_location() {
    let mut frame = frame(ffmpeg::format::Pixel::YUV420P, 3, 3);
    set_chroma_location(&mut frame, ffmpeg::util::chroma::Location::TopLeft);
    write_constant_plane(&mut frame, 0, 128, 8);
    write_constant_plane(&mut frame, 1, 128, 8);
    write_constant_plane(&mut frame, 2, 128, 8);
    write_plane_code(&mut frame, 1, 1, 1, 255, 8);
    let decoded = decode(
        &frame,
        ffmpeg::color::Space::BT709,
        ffmpeg::color::Range::JPEG,
        8,
    );
    assert_eq!(rgba32f(&decoded).len(), 9);
    assert!(rgba32f(&decoded)[4][2] > rgba32f(&decoded)[0][2]);
}

#[test]
fn yuva_alpha_is_full_range_straight_and_independent_of_rgb() {
    let mut frame = frame(ffmpeg::format::Pixel::YUVA420P10LE, 3, 3);
    set_chroma_location(&mut frame, ffmpeg::util::chroma::Location::Left);
    write_constant_plane(&mut frame, 0, 512, 10);
    write_constant_plane(&mut frame, 1, 512, 10);
    write_constant_plane(&mut frame, 2, 512, 10);
    write_constant_plane(&mut frame, 3, 257, 10);
    let decoded = decode(
        &frame,
        ffmpeg::color::Space::BT709,
        ffmpeg::color::Range::JPEG,
        10,
    );
    let pixel = rgba32f(&decoded)[0];
    assert!((pixel[3] - 257.0 / 1_023.0).abs() <= 1.0e-7);
    assert!((pixel[0] - 512.0 / 1_023.0).abs() <= 1.0e-6);
}

#[test]
fn unsupported_layout_metadata_and_unspecified_chroma_fail_closed() {
    for format in [
        ffmpeg::format::Pixel::NV12,
        ffmpeg::format::Pixel::P010LE,
        ffmpeg::format::Pixel::YUYV422,
        ffmpeg::format::Pixel::YUV440P,
    ] {
        let mut frame = frame(format, 4, 4);
        set_chroma_location(&mut frame, ffmpeg::util::chroma::Location::Left);
        let error = decode_frame_pixels(
            &frame,
            &plan(
                format,
                ffmpeg::color::Space::BT709,
                ffmpeg::color::Range::MPEG,
                10,
            ),
        )
        .expect_err("unsupported YUV layout must not reach a clipping fallback");
        assert!(
            error
                .to_string()
                .contains("unsupported verified YUV layout")
        );
    }

    let subsampled_frame = frame(ffmpeg::format::Pixel::YUV420P, 2, 2);
    let error = decode_frame_pixels(
        &subsampled_frame,
        &plan(
            subsampled_frame.format(),
            ffmpeg::color::Space::BT709,
            ffmpeg::color::Range::MPEG,
            8,
        ),
    )
    .expect_err("unspecified subsampled chroma location must fail closed");
    assert!(error.to_string().contains("unspecified chroma location"));

    for (space, range) in [
        (ffmpeg::color::Space::BT2020CL, ffmpeg::color::Range::MPEG),
        (
            ffmpeg::color::Space::Unspecified,
            ffmpeg::color::Range::MPEG,
        ),
        (
            ffmpeg::color::Space::BT709,
            ffmpeg::color::Range::Unspecified,
        ),
    ] {
        let frame = frame(ffmpeg::format::Pixel::YUV444P, 1, 1);
        let error = decode_frame_pixels(&frame, &plan(frame.format(), space, range, 8))
            .expect_err("unsupported matrix/range must fail before a scaler fallback");
        assert!(error.to_string().contains("refusing unverified"));
    }
}

#[test]
fn verified_provenance_names_the_in_house_f32_operation() {
    let frame = frame(ffmpeg::format::Pixel::YUV444P, 1, 1);
    let plan = plan(
        frame.format(),
        ffmpeg::color::Space::BT709,
        ffmpeg::color::Range::MPEG,
        8,
    );
    let description = plan.decoded_description();
    let DecodedRgbConversion::AppliedYuvToFullRangeRgb(applied) = description.rgb_conversion()
    else {
        panic!("verified YUV plan lost its provenance");
    };
    assert_eq!(applied.operation(), YuvToRgbOperation::H273PlanarF32V1);
    assert_eq!(
        applied.operation().id(),
        "ruvie.h273-planar-yuv-to-rgb-f32-v1"
    );
}

#[test]
fn yuv_negative_linesize_fails_before_constructing_a_plane_slice() {
    let mut frame = frame(ffmpeg::format::Pixel::YUV444P, 1, 1);
    let plan = plan(
        frame.format(),
        ffmpeg::color::Space::BT709,
        ffmpeg::color::Range::MPEG,
        8,
    );
    // SAFETY: the frame is exclusively borrowed and the scalar allocation
    // metadata is restored before the FFmpeg frame is dropped.
    let original = unsafe {
        let pointer = frame.as_mut_ptr();
        let original = (*pointer).linesize[0];
        (*pointer).linesize[0] = -original.abs().max(1);
        original
    };
    let error = decode_frame_pixels(&frame, &plan)
        .expect_err("negative YUV linesize must fail before slice construction");
    assert!(error.to_string().contains("non-positive stride"));
    // SAFETY: same exclusive frame, restoring its original valid metadata.
    unsafe {
        (*frame.as_mut_ptr()).linesize[0] = original;
    }
}

fn decode_varying_422(location: ffmpeg::util::chroma::Location) -> Vec<[f32; 4]> {
    let mut frame = frame(ffmpeg::format::Pixel::YUV422P, 3, 1);
    set_chroma_location(&mut frame, location);
    write_constant_plane(&mut frame, 0, 0, 8);
    write_constant_plane(&mut frame, 1, 128, 8);
    write_plane_code(&mut frame, 1, 1, 0, 255, 8);
    write_constant_plane(&mut frame, 2, 128, 8);
    rgba32f(&decode(
        &frame,
        ffmpeg::color::Space::BT709,
        ffmpeg::color::Range::JPEG,
        8,
    ))
    .to_vec()
}

fn frame(format: ffmpeg::format::Pixel, width: u32, height: u32) -> ffmpeg::util::frame::Video {
    ffmpeg::util::frame::Video::new(format, width, height)
}

fn decode(
    frame: &ffmpeg::util::frame::Video,
    space: ffmpeg::color::Space,
    range: ffmpeg::color::Range,
    depth: u8,
) -> DecodedPixelBuffer {
    decode_frame_pixels(frame, &plan(frame.format(), space, range, depth))
        .expect("verified planar YUV fixture should decode")
}

fn plan(
    format: ffmpeg::format::Pixel,
    space: ffmpeg::color::Space,
    range: ffmpeg::color::Range,
    depth: u8,
) -> DecodeColorPlan {
    let matrix = match space {
        ffmpeg::color::Space::BT709 => Some(SourceMatrixCoefficients::Bt709),
        ffmpeg::color::Space::FCC => Some(SourceMatrixCoefficients::Fcc),
        ffmpeg::color::Space::BT470BG => Some(SourceMatrixCoefficients::Bt470Bg),
        ffmpeg::color::Space::SMPTE170M => Some(SourceMatrixCoefficients::Smpte170M),
        ffmpeg::color::Space::SMPTE240M => Some(SourceMatrixCoefficients::Smpte240M),
        ffmpeg::color::Space::BT2020NCL => {
            Some(SourceMatrixCoefficients::Bt2020NonConstantLuminance)
        }
        ffmpeg::color::Space::BT2020CL => Some(SourceMatrixCoefficients::Bt2020ConstantLuminance),
        _ => None,
    };
    let source_range = match range {
        ffmpeg::color::Range::MPEG => Some(SourceColorRange::Limited),
        ffmpeg::color::Range::JPEG => Some(SourceColorRange::Full),
        ffmpeg::color::Range::Unspecified => None,
    };
    DecodeColorPlan::from_metadata(
        format,
        space,
        range,
        SourceColorDescription {
            assumption: None,
            primaries: Some(if space == ffmpeg::color::Space::BT2020NCL {
                SourceColorPrimaries::Bt2020
            } else {
                SourceColorPrimaries::Bt709
            }),
            transfer: Some(SourceTransferCharacteristic::Bt709),
            matrix,
            range: source_range,
            bit_depth: Some(depth),
            profile: None,
        },
    )
}

fn rgba32f(decoded: &DecodedPixelBuffer) -> &[[f32; 4]] {
    let DecodedPixelBuffer::StraightRgba32F(decoded) = decoded else {
        panic!("verified YUV must always use RGBA32F storage");
    };
    decoded.data()
}

fn set_chroma_location(
    frame: &mut ffmpeg::util::frame::Video,
    location: ffmpeg::util::chroma::Location,
) {
    // SAFETY: the frame is exclusively borrowed, and only the scalar metadata
    // field is set to a value defined by FFmpeg's public enum.
    unsafe {
        (*frame.as_mut_ptr()).chroma_location = location.into();
    }
}

fn write_constant_plane(
    frame: &mut ffmpeg::util::frame::Video,
    plane: usize,
    code: u16,
    depth: u8,
) {
    let width = usize::try_from(frame.plane_width(plane)).expect("fixture plane width");
    let height = usize::try_from(frame.plane_height(plane)).expect("fixture plane height");
    for row in 0..height {
        for column in 0..width {
            write_plane_code(frame, plane, column, row, code, depth);
        }
    }
}

fn write_plane_code(
    frame: &mut ffmpeg::util::frame::Video,
    plane: usize,
    column: usize,
    row: usize,
    code: u16,
    depth: u8,
) {
    let byte_width = usize::from(depth).div_ceil(8);
    let offset = row * frame.stride(plane) + column * byte_width;
    let big_endian = frame
        .format()
        .descriptor()
        .is_some_and(|descriptor| unsafe {
            // SAFETY: FFmpeg owns this immutable pixel-format descriptor for
            // the lifetime of the process; only its scalar flags field is read.
            let flag = u64::try_from(ffmpeg::ffi::AV_PIX_FMT_FLAG_BE).unwrap_or_default();
            std::ptr::addr_of!((*descriptor.as_ptr()).flags).read() & flag != 0
        });
    let bytes = if big_endian {
        code.to_be_bytes()
    } else {
        code.to_le_bytes()
    };
    let source = if big_endian {
        &bytes[2 - byte_width..]
    } else {
        &bytes[..byte_width]
    };
    frame.data_mut(plane)[offset..offset + byte_width].copy_from_slice(source);
}

fn assert_pixel_close(actual: [f32; 4], expected: [f32; 4], tolerance: f32) {
    for (name, (actual, expected)) in ["r", "g", "b", "a"]
        .into_iter()
        .zip(actual.into_iter().zip(expected))
    {
        assert!(
            (actual - expected).abs() <= tolerance,
            "{name}: expected {expected}, got {actual}"
        );
    }
}
