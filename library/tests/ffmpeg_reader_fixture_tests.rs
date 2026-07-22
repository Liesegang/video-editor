use anyhow::{Context, Result, bail};
use library::model::asset::{
    DecoderSourceColorAuthority, SourceColorAssumption, SourceColorDescription,
    SourceColorPrimaries, SourceColorRange, SourceMatrixCoefficients, SourceTransferCharacteristic,
};
use library::plugin::loaders::ffmpeg_video::VideoReader;
use library::plugin::{DecodedPixelBuffer, LoadResponse};
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
enum PixelSignature {
    Rgba8(Vec<u8>),
    Rgba16F(Vec<u16>),
    Rgba32F(Vec<u32>),
}

#[derive(Debug, PartialEq, Eq)]
struct FrameSample {
    dimensions: (u32, u32),
    pixels: PixelSignature,
    has_transparent_alpha: bool,
}

impl FrameSample {
    fn from_response(response: &LoadResponse) -> Self {
        let pixels = response.pixels();
        let (signature, has_transparent_alpha) = match pixels {
            DecodedPixelBuffer::StraightRgba8(image) => (
                PixelSignature::Rgba8(image.data().to_vec()),
                image.data().chunks_exact(4).any(|pixel| pixel[3] < 255),
            ),
            DecodedPixelBuffer::StraightRgba16F(image) => (
                PixelSignature::Rgba16F(
                    image
                        .data()
                        .iter()
                        .flat_map(|pixel| pixel.iter().map(|value| value.to_bits()))
                        .collect(),
                ),
                image.data().iter().any(|pixel| pixel[3].to_f32() < 1.0),
            ),
            DecodedPixelBuffer::StraightRgba32F(image) => (
                PixelSignature::Rgba32F(
                    image
                        .data()
                        .iter()
                        .flat_map(|pixel| pixel.iter().map(|value| value.to_bits()))
                        .collect(),
                ),
                image.data().iter().any(|pixel| pixel[3] < 1.0),
            ),
        };
        Self {
            dimensions: (pixels.width(), pixels.height()),
            pixels: signature,
            has_transparent_alpha,
        }
    }

    fn is_rgba32f(&self) -> bool {
        matches!(&self.pixels, PixelSignature::Rgba32F(_))
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../test_data/e2e_media")
        .join(name)
}

fn untagged_yuv_authority() -> Option<DecoderSourceColorAuthority> {
    Some(DecoderSourceColorAuthority::CompatibilityAssumption(
        SourceColorAssumption::UntaggedYuvBt709LimitedV1,
    ))
}

fn bt709_limited_override() -> Option<DecoderSourceColorAuthority> {
    Some(DecoderSourceColorAuthority::CompleteUserOverride(
        SourceColorDescription {
            assumption: None,
            primaries: Some(SourceColorPrimaries::Bt709),
            transfer: Some(SourceTransferCharacteristic::Bt709),
            matrix: Some(SourceMatrixCoefficients::Bt709),
            range: Some(SourceColorRange::Limited),
            bit_depth: Some(8),
            profile: None,
        },
    ))
}

fn decode_three(
    name: &str,
    expected_dimensions: (u32, u32),
    expected_fps: f64,
    frames: [u64; 3],
    authority: Option<DecoderSourceColorAuthority>,
    expect_rgba32f: bool,
) -> Result<Vec<FrameSample>> {
    let mut reader = VideoReader::new(fixture(name).to_string_lossy().as_ref())?;
    assert_eq!(reader.get_dimensions(), expected_dimensions);
    assert!((reader.get_fps() - expected_fps).abs() < 0.001);
    frames
        .into_iter()
        .map(|frame| {
            let response = reader.decode_frame_typed(frame, authority.as_ref())?;
            let sample = FrameSample::from_response(&response);
            assert_eq!(sample.dimensions, expected_dimensions);
            assert_eq!(sample.is_rgba32f(), expect_rgba32f);
            Ok(sample)
        })
        .collect()
}

#[test]
fn ffmpeg_reader_preserves_container_codec_dimensions_alpha_and_float_yuv() -> Result<()> {
    let mp4 = decode_three(
        "h264_24.mp4",
        (12, 8),
        24.0,
        [0, 36, 71],
        untagged_yuv_authority(),
        true,
    )?;
    let mov = decode_three(
        "h264_24.mov",
        (12, 8),
        24.0,
        [0, 36, 71],
        untagged_yuv_authority(),
        true,
    )?;
    assert_eq!(
        mp4, mov,
        "remuxing H.264 between MP4 and MOV must not change decoded pixels"
    );
    assert_ne!(mp4[0].pixels, mp4[1].pixels);
    assert_ne!(mp4[1].pixels, mp4[2].pixels);

    let webm = decode_three(
        "vp9_odd.webm",
        (9, 7),
        15.0,
        [0, 15, 29],
        bt709_limited_override(),
        true,
    )?;
    assert!(webm.windows(2).all(|pair| pair[0].pixels != pair[1].pixels));

    let ffv1 = decode_three("ffv1_alpha.mkv", (7, 5), 12.0, [0, 6, 11], None, false)?;
    assert!(ffv1.iter().all(|sample| sample.has_transparent_alpha));
    assert!(ffv1.windows(2).all(|pair| pair[0].pixels != pair[1].pixels));
    Ok(())
}

#[test]
fn vfr_sampling_uses_pts_instead_of_advertised_fps_ordinals() -> Result<()> {
    let path = fixture("vfr_pts.mkv");
    let mut sequential = VideoReader::new(path.to_string_lossy().as_ref())?;
    assert_eq!(sequential.get_stream_time_base(), (1, 1000));
    assert!((sequential.get_fps() - 10.0).abs() < 0.001);
    assert_eq!(
        sequential.get_frame_count(),
        None,
        "duration multiplied by advertised FPS must not fabricate an ordinal bound"
    );

    let first = sequential.decode_at_time_typed(0.0, None)?;
    assert_eq!(sequential.last_decode_stats().selected_pts, Some(0));
    let second = sequential.decode_at_time_typed(0.1, None)?;
    assert_eq!(sequential.last_decode_stats().selected_pts, Some(100));
    assert_ne!(
        FrameSample::from_response(&first).pixels,
        FrameSample::from_response(&second).pixels
    );
    let at_half_second = sequential.decode_at_time_typed(0.5, None)?;
    assert_eq!(sequential.last_decode_stats().selected_pts, Some(500));
    let held_at_one_second = sequential.decode_at_time_typed(1.0, None)?;
    assert_eq!(sequential.last_decode_stats().selected_pts, Some(500));
    let tail = sequential.decode_at_time_typed(1.85, None)?;
    assert_eq!(sequential.last_decode_stats().selected_pts, Some(1800));
    assert_eq!(
        FrameSample::from_response(&at_half_second).pixels,
        FrameSample::from_response(&held_at_one_second).pixels,
        "the 0.5s frame must remain displayed until the next PTS at 1.8s"
    );
    assert_ne!(
        FrameSample::from_response(&held_at_one_second).pixels,
        FrameSample::from_response(&tail).pixels
    );

    let mut random = VideoReader::new(path.to_string_lossy().as_ref())?;
    let random_at_one_second = random.decode_at_time_typed(1.0, None)?;
    assert_eq!(
        FrameSample::from_response(&random_at_one_second).pixels,
        FrameSample::from_response(&held_at_one_second).pixels
    );
    let stats = random.last_decode_stats();
    assert_eq!(stats.target_pts, 1000);
    assert_eq!(stats.selected_pts, Some(500));
    assert_eq!(stats.seek_count, 1);
    assert!(stats.frames_decoded <= 4);
    Ok(())
}

#[test]
fn timestamp_range_errors_report_the_selected_stream_duration() -> Result<()> {
    let path = fixture("av_duration_mismatch.mp4");
    let mut reader = VideoReader::new_with_stream(path.to_string_lossy().as_ref(), Some(0))?;
    let authority = untagged_yuv_authority();
    let error = match reader.decode_at_time_typed(1.0, authority.as_ref()) {
        Ok(_) => bail!("the selected video stream unexpectedly reached the padded container"),
        Err(error) => error,
    };
    let library::LibraryError::VideoTimestampOutOfRange {
        stream_index,
        duration,
        ..
    } = error
    else {
        bail!("expected a timestamp range error, got {error}");
    };
    assert_eq!(stream_index, 0);
    let duration = duration.context("fixture video stream declares its own duration")?;
    assert!((duration - 1.0).abs() < f64::EPSILON);
    assert!(
        (duration - 2.0).abs() > f64::EPSILON,
        "the two-second container/audio duration must not leak into the video error"
    );
    Ok(())
}
