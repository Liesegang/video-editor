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
use crate::plugin::PluginManager;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use uuid::Uuid;

mod waveform;

use waveform::WaveformJobs;

const SCRUB_PREVIEW_SECONDS: f64 = 0.05;
const MAX_MIX_SECONDS_PER_PUMP: usize = 1;
const MAX_CONCURRENT_AUDIO_DECODES: usize = 4;

/// Resolve enabled Media leaves for one Clip through the canonical typed
/// Audio graph. This intentionally exposes only the Timeline's Clip query;
/// the mixer's generic owner traversal remains crate-internal.
pub fn routed_audio_media_nodes_for_clip(project: &Project, clip_id: Uuid) -> Vec<Uuid> {
    crate::core::audio::mixer::routed_audio_media_nodes(
        project,
        crate::model::project::PortOwner::Clip(clip_id),
    )
}

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
    plugin_manager: Arc<PluginManager>,
    active_composition_id: Mutex<Option<Uuid>>,
    generation: Arc<AtomicU64>,
    pending: Arc<Mutex<HashSet<PendingAudioLoad>>>,
    waveform_jobs: WaveformJobs,
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
        plugin_manager: Arc<PluginManager>,
    ) -> Self {
        Self {
            project,
            audio_engine,
            cache_manager,
            plugin_manager,
            // Selection is supplied explicitly by the view layer. An absent
            // selection is mute, never an implicit compositions[0] fallback.
            active_composition_id: Mutex::new(None),
            generation: Arc::new(AtomicU64::new(0)),
            pending: Arc::new(Mutex::new(HashSet::new())),
            waveform_jobs: WaveformJobs::default(),
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
        update_playback_state(
            &self.is_playing,
            is_playing,
            || {
                if let Err(error) = self.audio_engine.play() {
                    log::error!("Failed to activate the audio playback clock: {error}");
                }
            },
            || {
                if let Err(error) = self.audio_engine.pause() {
                    log::error!("Failed to pause the audio playback clock: {error}");
                }
                // The device stream intentionally remains alive for low-latency
                // scrubbing, so pausing it is not what stops queued audio. Drop
                // the producer backlog exactly once on the playing -> paused
                // transition instead.
                if let Ok(mut scrub) = self.pending_scrub.lock() {
                    *scrub = None;
                }
                self.audio_engine.flush();
            },
        );
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
        self.waveform_jobs.clear();
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
        let Some(output_generation) = self.audio_engine.output_generation() else {
            return;
        };
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
        let available_frames = available / channels_usize;
        if available_frames == 0 {
            return;
        }
        let requested_frames = available_frames.min(
            usize::try_from(sample_rate)
                .unwrap_or(usize::MAX)
                .saturating_mul(MAX_MIX_SECONDS_PER_PUMP),
        );
        let start_sample = self.next_write_sample.load(Ordering::Relaxed);

        let (frames_to_write, all_ready) =
            self.prepare_window(start_sample, requested_frames, sample_rate, channels);
        if !all_ready {
            // The callback holds the playback clock at the producer cursor on
            // an underrun. Re-seeking here would repeatedly flush valid queued
            // frames while an asynchronous decode is still in flight.
            return;
        }
        let mix_buffer = self.mix_active(
            start_sample,
            frames_to_write,
            sample_rate,
            u32::from(channels),
        );
        let written = self
            .audio_engine
            .push_samples(&mix_buffer, output_generation);
        if written > 0 {
            commit_write_cursor(
                &self.next_write_sample,
                start_sample,
                written,
                channels_usize,
            );
        }
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
            self.plugin_manager.as_ref(),
        )
    }

    fn pump_pending_scrub(&self, sample_rate: u32, channels: u16) -> bool {
        if self.audio_engine.flush_pending() {
            return false;
        }
        let Some(output_generation) = self.audio_engine.output_generation() else {
            return false;
        };
        let request = self.pending_scrub.lock().ok().and_then(|scrub| *scrub);
        let Some((sample_pos, frames)) = request else {
            return true;
        };
        let (prepared_frames, all_ready) =
            self.prepare_window(sample_pos, frames, sample_rate, channels);
        if !all_ready {
            return false;
        }
        let samples = self.mix_active(
            sample_pos,
            prepared_frames,
            sample_rate,
            u32::from(channels),
        );
        let written = self.audio_engine.push_samples(&samples, output_generation);
        let written_frames = if written > 0 {
            commit_write_cursor(
                &self.next_write_sample,
                sample_pos,
                written,
                usize::from(channels),
            )
        } else {
            0
        };
        if let Ok(mut scrub) = self.pending_scrub.lock()
            && *scrub == request
        {
            let remaining = frames.saturating_sub(written_frames);
            *scrub = (remaining > 0)
                .then_some((sample_pos.saturating_add(written_frames as u64), remaining));
        }
        written_frames == frames
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
            self.plugin_manager.as_ref(),
        )
    }

    fn prepare_window(
        &self,
        start_frame: u64,
        requested_frames: usize,
        sample_rate: u32,
        channels: u16,
    ) -> (usize, bool) {
        if requested_frames == 0 {
            return (0, true);
        }
        let Some(format) = AudioDecodeFormat::new(sample_rate, channels) else {
            return (requested_frames, true);
        };
        let generation = self.generation.load(Ordering::Acquire);
        let capacity = self.cache_manager.audio_chunk_cache_capacity().max(1);
        let mut frame_count = requested_frames;
        let keys = loop {
            let keys = self.window_keys(start_frame, frame_count, sample_rate, format, generation);
            if keys.len() <= capacity || frame_count <= 1 {
                break keys;
            }
            frame_count = (frame_count / 2).max(1);
        };
        let mut keys = keys.into_iter().collect::<Vec<_>>();
        keys.sort_by(|left, right| {
            left.source
                .identity
                .canonical_path
                .cmp(&right.source.identity.canonical_path)
                .then_with(|| left.source.stream_index.cmp(&right.source.stream_index))
                .then_with(|| {
                    left.source
                        .format
                        .sample_rate
                        .cmp(&right.source.format.sample_rate)
                })
                .then_with(|| {
                    left.source
                        .format
                        .channels
                        .cmp(&right.source.format.channels)
                })
                .then_with(|| left.chunk_index.cmp(&right.chunk_index))
        });

        let mut all_ready = true;
        let worker_limit = capacity.min(MAX_CONCURRENT_AUDIO_DECODES);
        let (pending_count, mut busy_sources) = self.pending.lock().map_or_else(
            |_| (worker_limit, HashSet::new()),
            |pending| {
                (
                    pending.len(),
                    pending
                        .iter()
                        .map(|load| load.key.source.clone())
                        .collect::<HashSet<_>>(),
                )
            },
        );
        let mut available_workers = worker_limit.saturating_sub(pending_count);
        for key in keys {
            if self.cache_manager.get_audio_chunk(&key).is_some()
                || self.cache_manager.audio_chunk_failed(&key)
            {
                continue;
            }
            all_ready = false;
            let source = key.source.clone();
            if available_workers > 0 && !busy_sources.contains(&source) && self.schedule_chunk(key)
            {
                available_workers -= 1;
                busy_sources.insert(source);
            }
        }
        (frame_count, all_ready)
    }

    fn window_keys(
        &self,
        start_frame: u64,
        frame_count: usize,
        sample_rate: u32,
        format: AudioDecodeFormat,
        generation: u64,
    ) -> HashSet<AudioChunkKey> {
        let sources = {
            let Ok(project) = self.project.read() else {
                return HashSet::new();
            };
            let composition_id = self
                .active_composition_id
                .lock()
                .ok()
                .and_then(|active| *active);
            let Some(composition) = active_composition(&project, composition_id) else {
                return HashSet::new();
            };
            audio_window_requests_for_composition(
                &project,
                composition,
                start_frame,
                frame_count,
                sample_rate,
                self.plugin_manager.as_ref(),
            )
        };
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
        keys
    }

    fn schedule_chunk(&self, key: AudioChunkKey) -> bool {
        if self.cache_manager.get_audio_chunk(&key).is_some()
            || self.cache_manager.audio_chunk_failed(&key)
        {
            return false;
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
            return false;
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
        true
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
            || self.pending_scrub.lock().is_ok_and(|scrub| scrub.is_some())
            || self.pending.lock().is_ok_and(|pending| !pending.is_empty())
            || self.waveform_jobs.has_pending_work()
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

fn commit_write_cursor(
    cursor: &AtomicU64,
    start_sample: u64,
    written_samples: usize,
    channels: usize,
) -> usize {
    let written_frames = written_samples / channels.max(1);
    cursor.store(
        start_sample.saturating_add(written_frames as u64),
        Ordering::Release,
    );
    written_frames
}

fn active_composition(
    project: &Project,
    active_id: Option<Uuid>,
) -> Option<&crate::model::Composition> {
    active_id.and_then(|id| project.get_composition(id))
}

fn update_playback_state(
    state: &AtomicBool,
    is_playing: bool,
    on_play: impl FnOnce(),
    on_pause: impl FnOnce(),
) {
    let was_playing = state.swap(is_playing, Ordering::AcqRel);
    match (was_playing, is_playing) {
        (false, true) => on_play(),
        (true, false) => on_pause(),
        _ => {}
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
            drop(std::fs::remove_file(&self.0));
        }
    }

    #[test]
    fn pause_transition_flushes_once_without_flushing_idle_frames() {
        let state = AtomicBool::new(false);
        let flushes = AtomicUsize::new(0);
        let pending_scrub = Mutex::new(Some((10_u64, 50_usize)));
        let set = |playing| {
            update_playback_state(
                &state,
                playing,
                || {},
                || {
                    *pending_scrub.lock().unwrap() = None;
                    flushes.fetch_add(1, Ordering::Relaxed);
                },
            );
        };

        set(false);
        set(true);
        set(true);
        set(false);
        set(false);
        assert_eq!(flushes.load(Ordering::Relaxed), 1);
        assert_eq!(*pending_scrub.lock().unwrap(), None);
    }

    #[test]
    fn active_composition_never_falls_back_to_the_first_entry() {
        let mut project = Project::new("active audio composition");
        let (first, first_track) = Composition::new("first", 16, 16, 30.0, 1.0);
        let (second, second_track) = Composition::new("second", 16, 16, 30.0, 1.0);
        let first_id = first.id;
        let second_id = second.id;
        assert!(
            project.add_track(first_track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_track(second_track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_composition(first).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_composition(second).is_ok(),
            "container structural Merge insertion must succeed"
        );

        assert_eq!(
            active_composition(&project, Some(second_id)).unwrap().id,
            second_id
        );
        assert!(active_composition(&project, None).is_none());
        assert!(active_composition(&project, Some(Uuid::new_v4())).is_none());
        assert_ne!(first_id, second_id);
    }

    #[test]
    fn producer_cursor_advances_by_committed_interleaved_frames_only() {
        let cursor = AtomicU64::new(999);
        assert_eq!(commit_write_cursor(&cursor, 40, 4, 2), 2);
        assert_eq!(cursor.load(Ordering::Acquire), 42);

        // The engine rejects partial frames, but cursor accounting remains
        // defensive if a future producer reports a malformed sample count.
        assert_eq!(commit_write_cursor(&cursor, 42, 3, 2), 1);
        assert_eq!(cursor.load(Ordering::Acquire), 43);
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

        finish_audio_decode(&cache, &generation, &pending, &flush, load, Ok(chunk));

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
