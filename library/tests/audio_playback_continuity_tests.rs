use anyhow::{Context, Result};
use library::audio::cache::{AudioChunkKey, AudioDecodeFormat, AudioSourceKey};
use library::audio::loader::AudioLoader;

#[test]
fn compressed_chunk_preroll_prevents_artificial_boundary_silence() -> Result<()> {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../test_data/test_sound2.mp3");
    let format = AudioDecodeFormat::new(48_000, 2).context("stereo test format")?;
    let source = AudioSourceKey::read(path, None, format)?;
    for index in [0, 3, 7] {
        // Decode the later chunk first so each boundary exercises a random
        // access and codec preroll rather than the sequential-session fast path.
        let after = AudioLoader::decode_chunk(&AudioChunkKey {
            source: source.clone(),
            chunk_index: index + 1,
        })?;
        let before = AudioLoader::decode_chunk(&AudioChunkKey {
            source: source.clone(),
            chunk_index: index,
        })?;
        let leading_silent_frames = after
            .samples()
            .chunks_exact(2)
            .take_while(|frame| frame[0] == 0.0 && frame[1] == 0.0)
            .count();
        assert!(
            leading_silent_frames < 16,
            "compressed chunk {} began with {leading_silent_frames} artificial silent frames",
            index + 1
        );

        for channel in 0..2 {
            let boundary_step = (after.samples()[channel]
                - before.samples()[before.samples().len() - 2 + channel])
                .abs();
            let tail_start = before.frame_count().saturating_sub(2_048) * 2;
            let before_slope = before.samples()[tail_start..]
                .chunks_exact(2)
                .map(|frame| frame[channel])
                .collect::<Vec<_>>()
                .windows(2)
                .map(|pair| (pair[1] - pair[0]).abs())
                .fold(0.0_f32, f32::max);
            let after_slope = after
                .samples()
                .chunks_exact(2)
                .take(2_048)
                .map(|frame| frame[channel])
                .collect::<Vec<_>>()
                .windows(2)
                .map(|pair| (pair[1] - pair[0]).abs())
                .fold(0.0_f32, f32::max);
            let largest_local_slope = before_slope.max(after_slope);
            assert!(
                boundary_step <= largest_local_slope + f32::EPSILON,
                "compressed chunk {index}->{next} channel {channel} introduced boundary step {boundary_step}, adjacent-slope max {largest_local_slope}",
                next = index + 1,
            );
        }
    }
    Ok(())
}
