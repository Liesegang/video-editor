use super::{AudioService, PendingAudioLoad, SourceFailure};
use crate::core::audio::cache::{
    AudioChunk, AudioChunkKey, AudioDecodeFormat, AudioFileIdentity, AudioSourceKey,
};
use crate::core::audio::loader::AudioLoader;
use crate::core::audio::waveform::{AudioWaveformChunk, AudioWaveformWindow};
use crate::core::cache::CacheManager;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const MAX_CONCURRENT_WAVEFORM_DECODES: usize = 2;
const MAX_WAVEFORM_WINDOW_CHUNKS: u64 = 16_384;

/// Waveform preview jobs have their own worker and failure state so UI cache
/// misses can neither starve playback decodes nor suppress playback recovery.
pub(super) struct WaveformJobs {
    pending: Arc<Mutex<HashSet<PendingAudioLoad>>>,
    source_failures: Mutex<HashSet<SourceFailure>>,
}

impl Default for WaveformJobs {
    fn default() -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashSet::new())),
            source_failures: Mutex::new(HashSet::new()),
        }
    }
}

impl WaveformJobs {
    pub(super) fn clear(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
        if let Ok(mut failures) = self.source_failures.lock() {
            failures.clear();
        }
    }

    pub(super) fn has_pending_work(&self) -> bool {
        self.pending.lock().is_ok_and(|pending| !pending.is_empty())
    }

    fn available_workers(&self) -> usize {
        MAX_CONCURRENT_WAVEFORM_DECODES.saturating_sub(
            self.pending
                .lock()
                .map_or(MAX_CONCURRENT_WAVEFORM_DECODES, |pending| pending.len()),
        )
    }

    fn insert(&self, load: PendingAudioLoad) -> bool {
        self.pending
            .lock()
            .map(|mut pending| pending.insert(load))
            .unwrap_or(false)
    }

    fn remember_source_failure(&self, failure: SourceFailure) -> bool {
        self.source_failures
            .lock()
            .map(|mut failures| failures.insert(failure))
            .unwrap_or(false)
    }
}

impl AudioService {
    /// Request transient waveform summaries for one visible source-local
    /// range. Missing chunks are decoded only on bounded background workers;
    /// this method never performs media decode on the UI thread.
    pub fn request_waveform_window(
        &self,
        path: &str,
        stream_index: Option<usize>,
        first_source_frame: u64,
        final_source_frame: u64,
    ) -> Option<AudioWaveformWindow> {
        if first_source_frame > final_source_frame {
            return None;
        }
        let sample_rate = self.audio_engine.get_sample_rate();
        let channels = self.audio_engine.get_channels();
        let format = AudioDecodeFormat::new(sample_rate, channels)?;
        let source = match AudioSourceKey::read(path, stream_index, format) {
            Ok(source) => source,
            Err(error) => {
                let generation = self.generation.load(Ordering::Acquire);
                let failure = (generation, path.to_string(), stream_index, format);
                if self.waveform_jobs.remember_source_failure(failure) {
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
            self.cache_manager
                .audio_waveform_window_state(&source, first_chunk, final_chunk);
        let ready = chunks
            .iter()
            .map(|chunk| chunk.key().chunk_index)
            .collect::<HashSet<_>>();
        let mut available_workers = self.waveform_jobs.available_workers();
        for chunk_index in first_chunk..=final_chunk {
            if ready.contains(&chunk_index) || failed.contains(&chunk_index) {
                continue;
            }
            if available_workers > 0
                && self.schedule_waveform_chunk(AudioChunkKey {
                    source: source.clone(),
                    chunk_index,
                })
            {
                available_workers -= 1;
            }
        }
        Some(AudioWaveformWindow::new(
            source,
            chunks,
            requested_chunks,
            failed.len(),
            truncated,
        ))
    }

    fn schedule_waveform_chunk(&self, key: AudioChunkKey) -> bool {
        if self.cache_manager.get_audio_waveform_chunk(&key).is_some()
            || self.cache_manager.audio_waveform_chunk_failed(&key)
        {
            return false;
        }
        let generation = self.generation.load(Ordering::Acquire);
        let pending_load = PendingAudioLoad {
            generation,
            key: key.clone(),
        };
        // A playback worker will also publish a peak summary. Prefer that work
        // when it is already in flight instead of opening the source twice.
        if self
            .pending
            .lock()
            .is_ok_and(|pending| pending.contains(&pending_load))
        {
            return false;
        }
        if !self.waveform_jobs.insert(pending_load.clone()) {
            return false;
        }

        let cache = Arc::clone(&self.cache_manager);
        let current_generation = Arc::clone(&self.generation);
        let pending = Arc::clone(&self.waveform_jobs.pending);
        std::thread::spawn(move || {
            let result = AudioLoader::decode_chunk(&key);
            finish_waveform_decode(&cache, &current_generation, &pending, pending_load, result);
        });
        true
    }
}

fn finish_waveform_decode(
    cache: &CacheManager,
    generation: &AtomicU64,
    pending: &Mutex<HashSet<PendingAudioLoad>>,
    pending_load: PendingAudioLoad,
    result: Result<AudioChunk, anyhow::Error>,
) {
    if generation.load(Ordering::Acquire) == pending_load.generation {
        match result {
            Ok(chunk)
                if chunk.key() == &pending_load.key
                    && AudioFileIdentity::read(
                        &pending_load.key.source.identity.canonical_path,
                    )
                    .is_ok_and(|identity| identity == pending_load.key.source.identity) =>
            {
                cache.put_audio_waveform_chunk(AudioWaveformChunk::from_audio_chunk(&chunk));
            }
            Ok(_) => {
                cache.mark_audio_waveform_chunk_failed(pending_load.key.clone());
                log::error!("Discarded waveform chunk because its source identity changed");
            }
            Err(error) => {
                cache.mark_audio_waveform_chunk_failed(pending_load.key.clone());
                log::error!(
                    "Failed to decode waveform {:?} stream {:?}, chunk {}: {error}",
                    pending_load.key.source.identity.canonical_path,
                    pending_load.key.source.stream_index,
                    pending_load.key.chunk_index
                );
            }
        }
    }
    if let Ok(mut pending) = pending.lock() {
        pending.remove(&pending_load);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempAudioIdentity(std::path::PathBuf);

    impl TempAudioIdentity {
        fn key() -> (Self, AudioChunkKey) {
            let path = std::env::temp_dir().join(format!(
                "ruvie-waveform-service-{}.source",
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
        let cache = CacheManager::with_audio_chunk_capacity(2);
        let generation = AtomicU64::new(5);
        let load = PendingAudioLoad {
            generation: 5,
            key: key.clone(),
        };
        let pending = Mutex::new(HashSet::from([load.clone()]));

        finish_waveform_decode(&cache, &generation, &pending, load, Ok(chunk));

        assert!(cache.get_audio_chunk(&key).is_none());
        assert!(cache.get_audio_waveform_chunk(&key).is_some());
        assert!(!cache.audio_chunk_failed(&key));
        assert!(!cache.audio_waveform_chunk_failed(&key));
        assert!(pending.lock().unwrap().is_empty());
    }

    #[test]
    fn worker_failure_does_not_mute_playback() {
        let (_file, key) = TempAudioIdentity::key();
        let cache = CacheManager::with_audio_chunk_capacity(2);
        let generation = AtomicU64::new(6);
        let load = PendingAudioLoad {
            generation: 6,
            key: key.clone(),
        };
        let pending = Mutex::new(HashSet::from([load.clone()]));

        finish_waveform_decode(
            &cache,
            &generation,
            &pending,
            load,
            Err(anyhow::anyhow!("deliberate waveform failure")),
        );

        assert!(cache.get_audio_chunk(&key).is_none());
        assert!(!cache.audio_chunk_failed(&key));
        assert!(cache.audio_waveform_chunk_failed(&key));
        assert!(pending.lock().unwrap().is_empty());
    }
}
