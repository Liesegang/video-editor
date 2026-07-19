use crate::core::audio::cache::{
    AudioChunk, AudioChunkKey, AudioDecodeFormat, AudioFileIdentity, AudioSourceKey,
};
use crate::core::audio::engine::{AudioEngine, AudioFlushHandle};
use crate::core::audio::loader::AudioLoader;
use crate::core::audio::mixer::{
    audio_window_requests_for_composition, mix_samples, render_samples,
};
use crate::core::cache::CacheManager;
use crate::model::project::Project;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use uuid::Uuid;

const SCRUB_PREVIEW_SECONDS: f64 = 0.05;
const MAX_MIX_SAMPLES_PER_PUMP: usize = 16_384;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct PendingAudioLoad {
    generation: u64,
    key: AudioChunkKey,
}

type SourceFailure = (u64, String, Option<usize>, AudioDecodeFormat);

pub struct AudioService {
    project: Arc<RwLock<Project>>,
    audio_engine: Rc<AudioEngine>,
    cache_manager: Arc<CacheManager>,
    active_composition_id: Mutex<Option<Uuid>>,
    generation: Arc<AtomicU64>,
    pending: Arc<Mutex<HashSet<PendingAudioLoad>>>,
    source_failures: Arc<Mutex<HashSet<SourceFailure>>>,
    next_write_sample: Arc<AtomicU64>,
    is_playing: AtomicBool,
    pending_scrub: Mutex<Option<(u64, usize)>>,
}

impl AudioService {
    pub fn new(
        project: Arc<RwLock<Project>>,
        audio_engine: Rc<AudioEngine>,
        cache_manager: Arc<CacheManager>,
    ) -> Self {
        let active_composition_id = project.read().ok().and_then(|project| {
            project
                .compositions
                .first()
                .map(|composition| composition.id)
        });
        Self {
            project,
            audio_engine,
            cache_manager,
            active_composition_id: Mutex::new(active_composition_id),
            generation: Arc::new(AtomicU64::new(0)),
            pending: Arc::new(Mutex::new(HashSet::new())),
            source_failures: Arc::new(Mutex::new(HashSet::new())),
            next_write_sample: Arc::new(AtomicU64::new(0)),
            is_playing: AtomicBool::new(false),
            pending_scrub: Mutex::new(None),
        }
    }

    pub fn get_audio_engine(&self) -> Rc<AudioEngine> {
        Rc::clone(&self.audio_engine)
    }

    /// Select the sole Composition used for preview/playback audio.
    /// `None` is an explicit muted state; it never falls back to index zero.
    pub fn set_active_composition(&self, composition_id: Option<Uuid>, time: f64) -> bool {
        let changed = self
            .active_composition_id
            .lock()
            .map(|mut active| {
                if *active == composition_id {
                    false
                } else {
                    *active = composition_id;
                    true
                }
            })
            .unwrap_or(false);
        if changed {
            self.bump_generation();
            self.audio_engine.set_time(time);
            let sample = seconds_to_sample(time, self.audio_engine.get_sample_rate());
            self.next_write_sample.store(sample, Ordering::Relaxed);
            if let Ok(mut scrub) = self.pending_scrub.lock() {
                let frames = (SCRUB_PREVIEW_SECONDS
                    * f64::from(self.audio_engine.get_sample_rate()))
                    as usize;
                *scrub = Some((sample, frames));
            }
        }
        changed
    }

    pub fn set_playing(&self, is_playing: bool) {
        update_playback_state(&self.is_playing, is_playing, || {
            // The device stream intentionally remains alive for low-latency
            // scrubbing, so pausing it is not what stops queued audio. Drop
            // the producer backlog exactly once on the playing -> paused
            // transition instead.
            self.audio_engine.flush();
        });
    }

    /// Cancel every in-flight decode before adopting another authoritative
    /// Project. Workers from the previous generation are forbidden to commit.
    pub fn invalidate_project(&self) {
        if let Ok(mut active) = self.active_composition_id.lock() {
            *active = None;
        }
        if let Ok(mut scrub) = self.pending_scrub.lock() {
            *scrub = None;
        }
        self.bump_generation();
        self.audio_engine.flush();
    }

    fn bump_generation(&self) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
        if let Ok(mut failures) = self.source_failures.lock() {
            failures.clear();
        }
        self.cache_manager.clear_audio_failures();
        generation
    }

    pub fn reset_audio_pump(&self, time: f64) {
        self.audio_engine.set_time(time);

        let sample_rate = self.audio_engine.get_sample_rate();
        let channels = self.audio_engine.get_channels();
        let sample_pos = seconds_to_sample(time, sample_rate);
        let frames = (SCRUB_PREVIEW_SECONDS * f64::from(sample_rate)) as usize;
        self.next_write_sample.store(sample_pos, Ordering::Relaxed);
        if let Ok(mut scrub) = self.pending_scrub.lock() {
            *scrub = Some((sample_pos, frames));
        }
        self.pump_pending_scrub(sample_rate, channels);
    }

    pub fn pump_audio(&self) {
        let sample_rate = self.audio_engine.get_sample_rate();
        let channels = self.audio_engine.get_channels();
        if self.audio_engine.flush_pending() {
            return;
        }
        if !self.pump_pending_scrub(sample_rate, channels) {
            return;
        }
        if !self.is_playing.load(Ordering::Acquire) {
            return;
        }
        let available = self.audio_engine.available_slots();
        if available == 0 {
            return;
        }

        let channels_usize = usize::from(channels);
        let chunk_size = available.min(MAX_MIX_SAMPLES_PER_PUMP);
        if chunk_size < channels_usize {
            return;
        }
        let frames_to_write = chunk_size / channels_usize;
        let start_sample = self.next_write_sample.load(Ordering::Relaxed);

        if !self.schedule_window(start_sample, frames_to_write, sample_rate, channels) {
            // Already-buffered samples remain valid while the next chunk is
            // decoding. Hold only once the consumer catches the producer
            // cursor; flushing earlier would discard valid, unplayed audio.
            if self.audio_engine.get_current_sample() >= start_sample {
                self.audio_engine
                    .set_time(start_sample as f64 / f64::from(sample_rate));
            }
            return;
        }
        let mix_buffer = self.mix_active(
            start_sample,
            frames_to_write,
            sample_rate,
            u32::from(channels),
        );
        let written = self.audio_engine.push_samples(&mix_buffer);
        let written_frames = written / channels_usize;
        self.next_write_sample
            .fetch_add(written_frames as u64, Ordering::Relaxed);
    }

    pub fn render_audio(&self, start_time: f64, duration: f64) -> Vec<f32> {
        let sample_rate = self.audio_engine.get_sample_rate();
        let channels = self.audio_engine.get_channels();
        let start_sample = seconds_to_sample(start_time, sample_rate);
        let frames = if duration.is_finite() && duration > 0.0 {
            (duration * f64::from(sample_rate)).round() as usize
        } else {
            0
        };
        let silence = || vec![0.0; frames.saturating_mul(usize::from(channels))];
        let Ok(project) = self.project.read() else {
            return silence();
        };
        let composition_id = self
            .active_composition_id
            .lock()
            .ok()
            .and_then(|active| *active);
        let Some(composition) = active_composition(&project, composition_id) else {
            return silence();
        };
        render_samples(
            &project.assets,
            &project,
            composition,
            &self.cache_manager,
            start_sample,
            frames,
            sample_rate,
            u32::from(channels),
        )
    }

    fn pump_pending_scrub(&self, sample_rate: u32, channels: u16) -> bool {
        if self.audio_engine.flush_pending() {
            return false;
        }
        let request = self.pending_scrub.lock().ok().and_then(|scrub| *scrub);
        let Some((sample_pos, frames)) = request else {
            return true;
        };
        if !self.schedule_window(sample_pos, frames, sample_rate, channels) {
            return false;
        }
        let samples = self.mix_active(sample_pos, frames, sample_rate, u32::from(channels));
        self.audio_engine.push_samples(&samples);
        self.next_write_sample
            .store(sample_pos.saturating_add(frames as u64), Ordering::Relaxed);
        if let Ok(mut scrub) = self.pending_scrub.lock()
            && *scrub == request
        {
            *scrub = None;
        }
        true
    }

    fn mix_active(
        &self,
        start_sample: u64,
        frames: usize,
        sample_rate: u32,
        channels: u32,
    ) -> Vec<f32> {
        let silence = || vec![0.0; frames.saturating_mul(channels as usize)];
        let Ok(project) = self.project.read() else {
            return silence();
        };
        let composition_id = self
            .active_composition_id
            .lock()
            .ok()
            .and_then(|active| *active);
        let Some(composition) = active_composition(&project, composition_id) else {
            return silence();
        };
        mix_samples(
            &project.assets,
            &project,
            composition,
            &self.cache_manager,
            start_sample,
            frames,
            sample_rate,
            channels,
        )
    }

    fn schedule_window(
        &self,
        start_frame: u64,
        frame_count: usize,
        sample_rate: u32,
        channels: u16,
    ) -> bool {
        if frame_count == 0 {
            return true;
        }
        let Some(format) = AudioDecodeFormat::new(sample_rate, channels) else {
            return true;
        };
        let sources = {
            let Ok(project) = self.project.read() else {
                return true;
            };
            let composition_id = self
                .active_composition_id
                .lock()
                .ok()
                .and_then(|active| *active);
            let Some(composition) = active_composition(&project, composition_id) else {
                return true;
            };
            audio_window_requests_for_composition(
                &project,
                composition,
                start_frame,
                frame_count,
                sample_rate,
            )
        };
        let generation = self.generation.load(Ordering::Acquire);
        let mut all_ready = true;
        let mut keys = HashSet::new();
        for request in sources {
            let source = request.source;
            let source_key = match AudioSourceKey::read(&source.path, source.stream_index, format) {
                Ok(key) => key,
                Err(error) => {
                    let failure = (generation, source.path.clone(), source.stream_index, format);
                    let is_new = self
                        .source_failures
                        .lock()
                        .map(|mut failures| failures.insert(failure))
                        .unwrap_or(false);
                    if is_new {
                        log::error!(
                            "Failed to identify audio source {:?} stream {:?}: {error}",
                            source.path,
                            source.stream_index
                        );
                        self.audio_engine.flush();
                    }
                    continue;
                }
            };
            let chunk_frames = source_key.format.chunk_frames().max(1);
            let first_chunk = request.first_source_frame / chunk_frames;
            let final_chunk = request.last_source_frame / chunk_frames;
            for chunk_index in first_chunk..=final_chunk {
                keys.insert(AudioChunkKey {
                    source: source_key.clone(),
                    chunk_index,
                });
            }
        }
        for key in keys {
            if self.cache_manager.get_audio_chunk(&key).is_some()
                || self.cache_manager.audio_chunk_failed(&key)
            {
                continue;
            }
            all_ready = false;
            self.schedule_chunk(key);
        }
        all_ready
    }

    fn schedule_chunk(&self, key: AudioChunkKey) {
        if self.cache_manager.get_audio_chunk(&key).is_some()
            || self.cache_manager.audio_chunk_failed(&key)
        {
            return;
        }
        let generation = self.generation.load(Ordering::Acquire);
        let pending_load = PendingAudioLoad {
            generation,
            key: key.clone(),
        };
        let inserted = self
            .pending
            .lock()
            .map(|mut pending| pending.insert(pending_load.clone()))
            .unwrap_or(false);
        if !inserted {
            return;
        }

        let cache = Arc::clone(&self.cache_manager);
        let current_generation = Arc::clone(&self.generation);
        let pending = Arc::clone(&self.pending);
        let flush = self.audio_engine.flush_handle();
        std::thread::spawn(move || {
            let result = AudioLoader::decode_chunk(&key);
            finish_audio_decode(
                &cache,
                &current_generation,
                &pending,
                &flush,
                pending_load,
                result,
            );
        });
    }

    pub fn get_cache_manager(&self) -> Arc<CacheManager> {
        Arc::clone(&self.cache_manager)
    }

    #[doc(hidden)]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn has_pending_work(&self) -> bool {
        self.audio_engine.flush_pending()
            || self
                .pending_scrub
                .lock()
                .is_ok_and(|scrub| scrub.is_some())
            || self.pending.lock().is_ok_and(|pending| !pending.is_empty())
    }
}

fn finish_audio_decode(
    cache: &CacheManager,
    generation: &AtomicU64,
    pending: &Mutex<HashSet<PendingAudioLoad>>,
    flush: &AudioFlushHandle,
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
                cache.put_audio_chunk(chunk);
            }
            Ok(_) => {
                cache.mark_audio_chunk_failed(pending_load.key.clone());
                flush.request();
                log::error!("Discarded audio chunk because its source identity changed");
            }
            Err(error) => {
                cache.mark_audio_chunk_failed(pending_load.key.clone());
                flush.request();
                log::error!(
                    "Failed to decode audio {:?} stream {:?}, chunk {}: {error}",
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

fn seconds_to_sample(time: f64, sample_rate: u32) -> u64 {
    if !time.is_finite() || time <= 0.0 {
        return 0;
    }
    (time * f64::from(sample_rate)).round() as u64
}

fn active_composition(project: &Project, active_id: Option<Uuid>) -> Option<&crate::model::Composition> {
    active_id.and_then(|id| project.get_composition(id))
}

fn update_playback_state(
    state: &AtomicBool,
    is_playing: bool,
    on_pause: impl FnOnce(),
) {
    let was_playing = state.swap(is_playing, Ordering::AcqRel);
    if was_playing && !is_playing {
        on_pause();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::audio::cache::{AudioChunkKey, AudioDecodeFormat, AudioSourceKey};
    use crate::model::Composition;
    use std::sync::atomic::AtomicUsize;

    struct TempAudioIdentity(std::path::PathBuf);

    impl TempAudioIdentity {
        fn key() -> (Self, AudioChunkKey) {
            let path = std::env::temp_dir().join(format!(
                "ruvie-audio-service-{}.source",
                uuid::Uuid::new_v4()
            ));
            std::fs::write(&path, b"bounded audio worker test source").unwrap();
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
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn pause_transition_flushes_once_without_flushing_idle_frames() {
        let state = AtomicBool::new(false);
        let flushes = AtomicUsize::new(0);
        let set = |playing| {
            update_playback_state(&state, playing, || {
                flushes.fetch_add(1, Ordering::Relaxed);
            });
        };

        set(false);
        set(true);
        set(true);
        set(false);
        set(false);
        assert_eq!(flushes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn active_composition_never_falls_back_to_the_first_entry() {
        let mut project = Project::new("active audio composition");
        let (first, first_track) = Composition::new("first", 16, 16, 30.0, 1.0);
        let (second, second_track) = Composition::new("second", 16, 16, 30.0, 1.0);
        let first_id = first.id;
        let second_id = second.id;
        project.add_track(first_track);
        project.add_track(second_track);
        project.add_composition(first);
        project.add_composition(second);

        assert_eq!(active_composition(&project, Some(second_id)).unwrap().id, second_id);
        assert!(active_composition(&project, None).is_none());
        assert!(active_composition(&project, Some(Uuid::new_v4())).is_none());
        assert_ne!(first_id, second_id);
    }

    #[test]
    fn stale_worker_completion_cannot_commit_or_poison_the_current_generation() {
        let (_file, key) = TempAudioIdentity::key();
        let chunk = AudioChunk::new(key.clone(), vec![0.25; 4]).unwrap();
        let cache = CacheManager::with_audio_chunk_capacity(2);
        let generation = AtomicU64::new(8);
        let load = PendingAudioLoad {
            generation: 7,
            key: key.clone(),
        };
        let pending = Mutex::new(HashSet::from([load.clone()]));
        let flush = AudioFlushHandle::for_test();

        finish_audio_decode(
            &cache,
            &generation,
            &pending,
            &flush,
            load,
            Ok(chunk),
        );

        assert!(cache.get_audio_chunk(&key).is_none());
        assert!(!cache.audio_chunk_failed(&key));
        assert!(!flush.pending());
        assert!(pending.lock().unwrap().is_empty());
    }

    #[test]
    fn current_worker_failure_mutes_the_chunk_and_requests_a_flush() {
        let (_file, key) = TempAudioIdentity::key();
        let cache = CacheManager::with_audio_chunk_capacity(2);
        let generation = AtomicU64::new(4);
        let load = PendingAudioLoad {
            generation: 4,
            key: key.clone(),
        };
        let pending = Mutex::new(HashSet::from([load.clone()]));
        let flush = AudioFlushHandle::for_test();

        finish_audio_decode(
            &cache,
            &generation,
            &pending,
            &flush,
            load,
            Err(anyhow::anyhow!("deliberate decoder failure")),
        );

        assert!(cache.get_audio_chunk(&key).is_none());
        assert!(cache.audio_chunk_failed(&key));
        assert!(flush.pending());
        assert!(pending.lock().unwrap().is_empty());
    }
}
