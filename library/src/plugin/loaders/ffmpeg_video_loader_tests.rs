use super::ffmpeg_runtime::{
    classify_ffmpeg_probe_failure, has_registered_ffmpeg_media_extension, initialize_ffmpeg,
    initializer_calls,
};
use super::ffmpeg_video::*;
use crate::cache::CacheManager;
use crate::error::LibraryError;
use crate::model::asset::{
    DecoderSourceColorAuthority, SourceColorAssumption, SourceColorDescription,
    SourceColorPrimaries, SourceColorRange, SourceMatrixCoefficients, SourceTransferCharacteristic,
};
use crate::plugin::{
    DecodedColorSpace, DecodedPixelBuffer, DecodedRgbConversion, LoadPlugin, LoadPluginError,
    LoadPluginResult, LoadRequest, LoadResponse,
};
use std::path::Path;
use std::sync::Arc;

#[test]
fn repeated_load_cache_hit_preserves_the_exact_decode_descriptor()
-> Result<(), Box<dyn std::error::Error>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../test_data/e2e_media/h264_24.mp4")
        .to_string_lossy()
        .into_owned();
    let loader = FfmpegVideoLoader::new();
    let cache = CacheManager::new();
    let request = LoadRequest::VideoFrame {
        path,
        source_time: 0.0,
        stream_index: None,
        source_color_authority: compatibility_authority(),
    };

    let first = loader.load(&request, &cache)?;
    let first_description = first.decoded().clone();
    let first_image = into_rgba32f(first);
    let second = loader.load(&request, &cache)?;
    let second_description = second.decoded().clone();
    let second_image = into_rgba32f(second);

    assert_eq!(second_description, first_description);
    assert_eq!(second_image, first_image);
    Ok(())
}

#[test]
fn complete_source_override_controls_conversion_and_partitions_the_frame_cache()
-> Result<(), Box<dyn std::error::Error>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../test_data/e2e_media/h264_24.mp4")
        .to_string_lossy()
        .into_owned();
    let loader = FfmpegVideoLoader::new();
    let cache = CacheManager::new();
    let request = |matrix| LoadRequest::VideoFrame {
        path: path.clone(),
        source_time: 0.0,
        stream_index: None,
        source_color_authority: Some(DecoderSourceColorAuthority::CompleteUserOverride(
            SourceColorDescription {
                assumption: None,
                primaries: Some(SourceColorPrimaries::Bt709),
                transfer: Some(SourceTransferCharacteristic::Bt709),
                matrix: Some(matrix),
                range: Some(SourceColorRange::Limited),
                bit_depth: Some(8),
                profile: None,
            },
        )),
    };

    let bt709 = loader.load(&request(SourceMatrixCoefficients::Bt709), &cache)?;
    assert_applied_matrix(&bt709, SourceMatrixCoefficients::Bt709);
    let bt709_pixels = into_rgba32f(bt709).2;

    let bt2020 = loader.load(
        &request(SourceMatrixCoefficients::Bt2020NonConstantLuminance),
        &cache,
    )?;
    assert_applied_matrix(
        &bt2020,
        SourceMatrixCoefficients::Bt2020NonConstantLuminance,
    );
    let bt2020_pixels = into_rgba32f(bt2020).2;
    assert_ne!(
        bt2020_pixels, bt709_pixels,
        "a changed source override must not reuse another matrix's cached pixels"
    );
    Ok(())
}

#[test]
fn complete_untagged_override_does_not_inherit_frame_tags() -> Result<(), Box<dyn std::error::Error>>
{
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../test_data/e2e_media/color_rec709_limited.mp4")
        .to_string_lossy()
        .into_owned();
    let error = FfmpegVideoLoader::new()
        .load(
            &LoadRequest::VideoFrame {
                path,
                source_time: 0.0,
                stream_index: None,
                source_color_authority: Some(DecoderSourceColorAuthority::CompleteUserOverride(
                    SourceColorDescription::default(),
                )),
            },
            &CacheManager::new(),
        )
        .expect_err("an incomplete override must fail before an unverified pixel fallback");
    assert!(error.to_string().contains("refusing unverified"));
    Ok(())
}

#[test]
fn conditional_import_assumption_never_overrides_tagged_or_high_bit_frames()
-> Result<(), Box<dyn std::error::Error>> {
    let loader = FfmpegVideoLoader::new();
    let cache = CacheManager::new();
    let stale_assumption = || {
        Some(DecoderSourceColorAuthority::CompatibilityAssumption(
            SourceColorAssumption::UntaggedYuvBt709LimitedV1,
        ))
    };

    let tagged = loader.load(
        &LoadRequest::VideoFrame {
            path: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../test_data/e2e_media/color_rec709_limited.mp4")
                .to_string_lossy()
                .into_owned(),
            source_time: 0.0,
            stream_index: None,
            source_color_authority: stale_assumption(),
        },
        &cache,
    )?;
    let DecodedColorSpace::SourceEncoded(tagged_source) = tagged.decoded().color_space() else {
        panic!("tagged frame must retain its actual source encoding");
    };
    assert_eq!(tagged_source.assumption, None);
    assert_eq!(tagged_source.primaries, Some(SourceColorPrimaries::Bt709));
    assert_eq!(
        tagged_source.transfer,
        Some(SourceTransferCharacteristic::Bt709)
    );

    let high_bit = loader.load(
        &LoadRequest::VideoFrame {
            path: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../test_data/e2e_media/color_rec2020_pq.mp4")
                .to_string_lossy()
                .into_owned(),
            source_time: 0.0,
            stream_index: None,
            source_color_authority: stale_assumption(),
        },
        &cache,
    )?;
    let DecodedColorSpace::SourceEncoded(high_bit_source) = high_bit.decoded().color_space() else {
        panic!("10-bit tagged frame must retain its actual source encoding");
    };
    assert_eq!(high_bit_source.assumption, None);
    assert_eq!(high_bit_source.bit_depth, Some(10));
    assert_eq!(
        high_bit_source.primaries,
        Some(SourceColorPrimaries::Bt2020)
    );
    assert_eq!(
        high_bit_source.transfer,
        Some(SourceTransferCharacteristic::Pq)
    );
    assert!(matches!(
        high_bit.pixels(),
        DecodedPixelBuffer::StraightRgba32F(_)
    ));
    Ok(())
}

#[test]
fn authored_lower_bit_depth_cannot_hide_actual_ten_bit_storage()
-> Result<(), Box<dyn std::error::Error>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../test_data/e2e_media/color_rec2020_pq.mp4")
        .to_string_lossy()
        .into_owned();
    let response = FfmpegVideoLoader::new().load(
        &LoadRequest::VideoFrame {
            path,
            source_time: 0.0,
            stream_index: None,
            source_color_authority: Some(DecoderSourceColorAuthority::CompleteUserOverride(
                SourceColorDescription {
                    assumption: None,
                    primaries: Some(SourceColorPrimaries::Bt2020),
                    transfer: Some(SourceTransferCharacteristic::Pq),
                    matrix: Some(SourceMatrixCoefficients::Bt2020NonConstantLuminance),
                    range: Some(SourceColorRange::Limited),
                    bit_depth: Some(8),
                    profile: None,
                },
            )),
        },
        &CacheManager::new(),
    )?;

    let DecodedColorSpace::SourceEncoded(source) = response.decoded().color_space() else {
        panic!("verified override must retain source encoding");
    };
    assert_eq!(source.bit_depth, Some(10));
    assert!(matches!(
        response.pixels(),
        DecodedPixelBuffer::StraightRgba32F(_)
    ));
    Ok(())
}

fn assert_applied_matrix(
    response: &crate::plugin::LoadResponse,
    expected: SourceMatrixCoefficients,
) {
    let DecodedRgbConversion::AppliedYuvToFullRangeRgb(applied) =
        response.decoded().rgb_conversion()
    else {
        panic!("expected verified YUV conversion: {:?}", response.decoded());
    };
    assert_eq!(applied.source_matrix(), &expected);
    assert_eq!(applied.source_range(), &SourceColorRange::Limited);
}

fn into_rgba32f(response: LoadResponse) -> (u32, u32, Vec<[f32; 4]>) {
    let (pixels, _) = response.into_parts();
    let DecodedPixelBuffer::StraightRgba32F(pixels) = pixels else {
        panic!("verified FFmpeg YUV must use straight RGBA32F storage");
    };
    (pixels.width(), pixels.height(), pixels.into_data())
}

fn compatibility_authority() -> Option<DecoderSourceColorAuthority> {
    Some(DecoderSourceColorAuthority::CompatibilityAssumption(
        SourceColorAssumption::UntaggedYuvBt709LimitedV1,
    ))
}

#[test]
fn concurrent_open_load_and_initialization_call_ffmpeg_initializer_once()
-> Result<(), Box<dyn std::error::Error>> {
    const WORKER_COUNT: usize = 12;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../test_data/e2e_media/h264_24.mp4")
        .to_string_lossy()
        .into_owned();
    let loader = Arc::new(FfmpegVideoLoader::new());
    let barrier = Arc::new(std::sync::Barrier::new(WORKER_COUNT));

    std::thread::scope(|scope| -> Result<(), Box<dyn std::error::Error>> {
        let workers = (0..WORKER_COUNT)
            .map(|worker_index| {
                let loader = Arc::clone(&loader);
                let barrier = Arc::clone(&barrier);
                let path = path.clone();
                scope.spawn(move || -> LoadPluginResult<()> {
                    barrier.wait();
                    for rotation in 0..3 {
                        match (worker_index + rotation) % 3 {
                            0 => initialize_ffmpeg().map_err(LoadPluginError::from)?,
                            1 => {
                                let metadata = loader.open(&path)?;
                                assert!(metadata.iter().any(|stream| {
                                    stream.kind == crate::model::asset::AssetKind::Video
                                }));
                            }
                            _ => {
                                let loaded = loader.load(
                                    &LoadRequest::VideoFrame {
                                        path: path.clone(),
                                        source_time: 0.0,
                                        stream_index: None,
                                        source_color_authority: compatibility_authority(),
                                    },
                                    &CacheManager::new(),
                                )?;
                                assert!(loaded.pixels().width() > 0);
                                assert!(loaded.pixels().height() > 0);
                            }
                        }
                    }
                    assert!(has_registered_ffmpeg_media_extension("fixture.mp4"));
                    Ok(())
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker
                .join()
                .map_err(|_| std::io::Error::other("FFmpeg stress worker panicked"))??;
        }
        Ok(())
    })?;

    assert_eq!(
        initializer_calls(),
        1,
        "every FFmpeg entry point must share the process-wide initializer"
    );
    Ok(())
}

#[test]
fn demuxer_registry_claims_a_format_missing_from_the_replaced_legacy_table()
-> Result<(), Box<dyn std::error::Error>> {
    initialize_ffmpeg()?;
    assert!(
        has_registered_ffmpeg_media_extension("fixture.nut"),
        "the linked FFmpeg registry must expose its standard NUT demuxer"
    );
    let path = std::env::temp_dir().join(format!(
        "ffmpeg-registry-routing-{}.nut",
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&path, b"not a NUT stream")?;
    let result = FfmpegVideoLoader::new().open(&path.to_string_lossy());
    std::fs::remove_file(path)?;
    let Err(LoadPluginError::Failed(LibraryError::Ffmpeg(error))) = result else {
        return Err(std::io::Error::other(
            "a registry-known NUT path must preserve its concrete FFmpeg probe error",
        )
        .into());
    };
    assert!(!error.to_string().is_empty());
    Ok(())
}

#[test]
fn initialization_failure_declines_unknown_extensions_but_claims_registered_ones() {
    let unknown = classify_ffmpeg_probe_failure(
        "/fixtures/runtime.rgba-fixture",
        LibraryError::Plugin("synthetic init failure".to_string()),
    );
    assert!(matches!(unknown, LoadPluginError::Unsupported));

    assert!(has_registered_ffmpeg_media_extension(
        "/fixtures/broken.mp4"
    ));
    let known = classify_ffmpeg_probe_failure(
        "/fixtures/broken.mp4",
        LibraryError::Plugin("synthetic init failure".to_string()),
    );
    assert!(matches!(known, LoadPluginError::Failed(_)));
}

#[test]
fn valid_ffmpeg_content_with_an_unknown_extension_is_claimed_by_magic_probe()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test_data/e2e_media/h264_24.mp4");
    let path = std::env::temp_dir().join(format!(
        "ffmpeg-renamed-video-{}.asset",
        uuid::Uuid::new_v4()
    ));
    std::fs::copy(fixture, &path)?;
    let path_text = path.to_string_lossy().into_owned();
    let loader = FfmpegVideoLoader::new();
    let streams = loader.open(&path_text)?;
    assert!(
        streams
            .iter()
            .any(|stream| { stream.kind == crate::model::asset::AssetKind::Video })
    );
    let loaded = loader.load(
        &LoadRequest::VideoFrame {
            path: path_text,
            source_time: 0.0,
            stream_index: None,
            source_color_authority: compatibility_authority(),
        },
        &CacheManager::new(),
    )?;
    assert!(loaded.pixels().width() > 0);
    assert!(loaded.pixels().height() > 0);
    std::fs::remove_file(path)?;
    Ok(())
}

#[test]
fn unknown_probe_failure_is_unsupported_but_registered_media_failure_is_concrete()
-> Result<(), Box<dyn std::error::Error>> {
    let stem = std::env::temp_dir().join(format!("ffmpeg-probe-routing-{}", uuid::Uuid::new_v4()));
    let custom_path = stem.with_extension("rgba-fixture");
    let media_path = stem.with_extension("mp4");
    std::fs::write(&custom_path, b"not an ffmpeg container")?;
    std::fs::write(&media_path, b"not an ffmpeg container")?;
    let custom_path_text = custom_path.to_string_lossy().into_owned();
    let media_path_text = media_path.to_string_lossy().into_owned();
    let loader = FfmpegVideoLoader::new();
    assert!(matches!(
        loader.open(&custom_path_text),
        Err(LoadPluginError::Unsupported)
    ));
    assert!(matches!(
        loader.load(
            &LoadRequest::VideoFrame {
                path: custom_path_text,
                source_time: 0.0,
                stream_index: None,
                source_color_authority: None,
            },
            &CacheManager::new(),
        ),
        Err(LoadPluginError::Unsupported)
    ));
    let media_result = loader.open(&media_path_text);
    let media_load_result = loader.load(
        &LoadRequest::VideoFrame {
            path: media_path_text,
            source_time: 0.0,
            stream_index: None,
            source_color_authority: None,
        },
        &CacheManager::new(),
    );
    std::fs::remove_file(custom_path)?;
    std::fs::remove_file(media_path)?;
    assert!(matches!(
        media_result,
        Err(LoadPluginError::Failed(LibraryError::Ffmpeg(_)))
    ));
    assert!(matches!(
        media_load_result,
        Err(LoadPluginError::Failed(LibraryError::Ffmpeg(_)))
    ));
    Ok(())
}
