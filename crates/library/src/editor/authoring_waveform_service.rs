//! Shared, bounded waveform requests for authoring UI surfaces.
//!
//! A request reads only cached summaries on its caller's thread. Missing
//! source chunks are decoded by a cache-owned, bounded worker pool. Both raw
//! playback chunks and waveform-only jobs publish into the same summary cache.

use std::collections::HashSet;
use std::sync::Arc;

use crate::core::audio::authoring::{AUTHORING_AUDIO_CHANNELS, AUTHORING_AUDIO_SAMPLE_RATE};
use crate::core::audio::cache::{
    AudioChunk, AudioChunkKey, AudioDecodeFormat, AudioFileIdentity, AudioSourceKey,
};
use crate::core::audio::loader::AudioLoader;
use crate::core::audio::waveform::{AudioWaveformChunk, AudioWaveformWindow};
use crate::core::cache::{AuthoringWaveformJobs, CacheManager};

const MAX_WAVEFORM_WINDOW_CHUNKS: u64 = 16_384;

/// Cheap cloneable handle to the authoritative authoring waveform pipeline.
///
/// Job ownership lives in [`CacheManager`], so handles created independently
/// by Timeline and Inspector still share one pending set and worker bound.
#[derive(Clone)]
pub struct AuthoringWaveformService {
    cache: Arc<CacheManager>,
    jobs: Arc<AuthoringWaveformJobs>,
    format: AudioDecodeFormat,
}

impl AuthoringWaveformService {
    pub fn new(cache: Arc<CacheManager>) -> Self {
        Self::from_format(
            cache,
            AudioDecodeFormat {
                sample_rate: AUTHORING_AUDIO_SAMPLE_RATE,
                channels: AUTHORING_AUDIO_CHANNELS,
            },
        )
    }

    pub fn with_format(cache: Arc<CacheManager>, sample_rate: u32, channels: u16) -> Option<Self> {
        let format = AudioDecodeFormat::new(sample_rate, channels)?;
        Some(Self::from_format(cache, format))
    }

    fn from_format(cache: Arc<CacheManager>, format: AudioDecodeFormat) -> Self {
        let jobs = cache.authoring_waveform_jobs();
        Self {
            cache,
            jobs,
            format,
        }
    }

    pub const fn format(&self) -> AudioDecodeFormat {
        self.format
    }

    /// Return the currently cached summary window and schedule its missing
    /// chunks. This function performs file identity checks but never media
    /// decode on the caller's thread.
    pub fn request_window(
        &self,
        path: &str,
        stream_index: Option<usize>,
        first_source_frame: u64,
        final_source_frame: u64,
    ) -> Option<AudioWaveformWindow> {
        if first_source_frame > final_source_frame {
            return None;
        }
        let source = match AudioSourceKey::read(path, stream_index, self.format) {
            Ok(source) => source,
            Err(error) => {
                if self
                    .jobs
                    .remember_source_failure(path, stream_index, self.format)
                {
                    log::error!(
                        "Failed to identify waveform source {path:?} stream {stream_index:?}: {error}"
                    );
                }
                return None;
            }
        };
        let chunk_frames = source.format.chunk_frames().max(1);
        let first_chunk = first_source_frame / chunk_frames;
        let requested_final_chunk = final_source_frame / chunk_frames;
        let final_chunk = requested_final_chunk
            .min(first_chunk.saturating_add(MAX_WAVEFORM_WINDOW_CHUNKS.saturating_sub(1)));
        let truncated = final_chunk < requested_final_chunk;
        let requested_chunks = usize::try_from(final_chunk - first_chunk + 1).ok()?;
        let (chunks, failed) =
            self.cache
                .audio_waveform_window_state(&source, first_chunk, final_chunk);
        let ready = chunks
            .iter()
            .map(|chunk| chunk.key().chunk_index)
            .collect::<HashSet<_>>();
        for chunk_index in first_chunk..=final_chunk {
            if ready.contains(&chunk_index) || failed.contains(&chunk_index) {
                continue;
            }
            self.schedule_chunk(AudioChunkKey {
                source: source.clone(),
                chunk_index,
            });
        }
        Some(AudioWaveformWindow::new(
            source,
            chunks,
            requested_chunks,
            failed.len(),
            truncated,
        ))
    }

    pub fn has_pending_work(&self) -> bool {
        self.jobs.has_pending_work()
    }

    fn schedule_chunk(&self, key: AudioChunkKey) {
        if self.cache.get_audio_waveform_chunk(&key).is_some()
            || self.cache.audio_waveform_chunk_failed(&key)
            || self.jobs.is_pending(&key)
            || !self.jobs.reserve(key.clone())
        {
            return;
        }
        let cache = Arc::clone(&self.cache);
        let jobs = Arc::clone(&self.jobs);
        std::thread::spawn(move || {
            let result = AudioLoader::decode_chunk(&key);
            finish_waveform_decode(&cache, &jobs, key, result);
        });
    }
}

fn finish_waveform_decode(
    cache: &CacheManager,
    jobs: &AuthoringWaveformJobs,
    key: AudioChunkKey,
    result: Result<AudioChunk, anyhow::Error>,
) {
    match result {
        Ok(chunk)
            if chunk.key() == &key
                && AudioFileIdentity::read(&key.source.identity.canonical_path)
                    .is_ok_and(|identity| identity == key.source.identity) =>
        {
            cache.put_audio_waveform_chunk(AudioWaveformChunk::from_audio_chunk(&chunk));
        }
        Ok(_) => {
            cache.mark_audio_waveform_chunk_failed(key.clone());
            log::error!("Discarded waveform chunk because its source identity changed");
        }
        Err(error) => {
            cache.mark_audio_waveform_chunk_failed(key.clone());
            log::error!(
                "Failed to decode waveform {:?} stream {:?}, chunk {}: {error}",
                key.source.identity.canonical_path,
                key.source.stream_index,
                key.chunk_index
            );
        }
    }
    jobs.finish(&key);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempAudioIdentity(std::path::PathBuf);

    impl TempAudioIdentity {
        fn key() -> (Self, AudioChunkKey) {
            let path = std::env::temp_dir().join(format!(
                "ruvie-authoring-waveform-service-{}.source",
                uuid::Uuid::new_v4()
            ));
            std::fs::write(&path, b"bounded waveform worker test source").unwrap();
            let format = AudioDecodeFormat::new(4, 1).unwrap();
            let source = AudioSourceKey::read(&path, None, format).unwrap();
            (
                Self(path),
                AudioChunkKey {
                    source,
                    chunk_index: 0,
                },
            )
        }
    }

    impl Drop for TempAudioIdentity {
        fn drop(&mut self) {
            drop(std::fs::remove_file(&self.0));
        }
    }

    #[test]
    fn worker_success_publishes_only_a_peak_summary() {
        let (_file, key) = TempAudioIdentity::key();
        let chunk = AudioChunk::new(key.clone(), vec![0.0, -0.75, 0.25, 0.0]).unwrap();
        let cache = Arc::new(CacheManager::with_audio_chunk_capacity(2));
        let jobs = cache.authoring_waveform_jobs();
        assert!(jobs.reserve(key.clone()));

        finish_waveform_decode(&cache, &jobs, key.clone(), Ok(chunk));

        assert!(cache.get_audio_chunk(&key).is_none());
        assert!(cache.get_audio_waveform_chunk(&key).is_some());
        assert!(!cache.audio_chunk_failed(&key));
        assert!(!cache.audio_waveform_chunk_failed(&key));
        assert!(!jobs.has_pending_work());
    }

    #[test]
    fn worker_failure_does_not_mute_playback() {
        let (_file, key) = TempAudioIdentity::key();
        let cache = Arc::new(CacheManager::with_audio_chunk_capacity(2));
        let jobs = cache.authoring_waveform_jobs();
        assert!(jobs.reserve(key.clone()));

        finish_waveform_decode(
            &cache,
            &jobs,
            key.clone(),
            Err(anyhow::anyhow!("deliberate waveform failure")),
        );

        assert!(cache.get_audio_chunk(&key).is_none());
        assert!(!cache.audio_chunk_failed(&key));
        assert!(cache.audio_waveform_chunk_failed(&key));
        assert!(!jobs.has_pending_work());
    }

    #[test]
    fn handles_for_one_cache_share_the_worker_reservation() {
        let (_file, key) = TempAudioIdentity::key();
        let cache = Arc::new(CacheManager::with_audio_chunk_capacity(2));
        let left = AuthoringWaveformService::with_format(Arc::clone(&cache), 4, 1).unwrap();
        let right = AuthoringWaveformService::with_format(cache, 4, 1).unwrap();

        assert!(left.jobs.reserve(key.clone()));
        assert!(right.jobs.is_pending(&key));
        assert!(!right.jobs.reserve(key.clone()));
        left.jobs.finish(&key);
    }
}
