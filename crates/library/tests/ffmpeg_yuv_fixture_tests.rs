use anyhow::Result;
use library::cache::CacheManager;
use library::model::AssetKind;
use library::model::asset::{
    DecoderSourceColorAuthority, SourceColorDescription, SourceColorPrimaries, SourceColorRange,
    SourceMatrixCoefficients, SourceTransferCharacteristic,
};
use library::plugin::loaders::ffmpeg_video::FfmpegVideoLoader;
use library::plugin::{LoadPlugin, LoadRequest};
use std::path::{Path, PathBuf};

#[test]
fn unspecified_subsampled_chroma_fails_closed_per_selected_stream() -> Result<()> {
    let loader = FfmpegVideoLoader::new();
    let cache = CacheManager::new();
    let path = fixture("multistream.mkv").to_string_lossy().into_owned();
    let streams = loader.open(&path)?;
    assert_eq!(
        streams
            .iter()
            .filter(|stream| stream.kind == AssetKind::Video)
            .count(),
        2
    );
    for stream_index in 0..=1 {
        let error = loader
            .load(
                &LoadRequest::VideoFrame {
                    path: path.clone(),
                    source_time: 0.0,
                    stream_index: Some(stream_index),
                    source_color_authority: bt709_limited_override(),
                },
                &cache,
            )
            .expect_err("subsampled YUV without frame or decoder siting must be rejected");
        let message = error.to_string();
        assert!(message.contains("cannot resolve chroma location"));
        assert!(message.contains("frame and decoder both have unspecified"));
    }
    assert_eq!(loader.cached_reader_count(), 2);

    let audio_path = fixture("tone.mp3");
    let audio_streams = loader.open(audio_path.to_string_lossy().as_ref())?;
    assert_eq!(audio_streams.len(), 1);
    assert_eq!(audio_streams[0].kind, AssetKind::Audio);
    assert_eq!(loader.cached_reader_count(), 2);
    Ok(())
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test_data/e2e_media")
        .join(name)
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
