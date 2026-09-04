use crate::core::audio::cache::{AudioChunk, AudioChunkKey, AudioDecodeFormat, AudioSourceKey};
use crate::core::audio::waveform::AudioWaveformChunk;
use crate::core::model_resource::{
    MeshScene, ModelDecodeLimits, ModelResourceError, ModelResourceKey,
};
use crate::model::frame::Image;
use lru::LruCache;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

const DEFAULT_IMAGE_CACHE_SIZE: usize = 64;
const DEFAULT_VIDEO_CACHE_SIZE: usize = 128;
const DEFAULT_AUDIO_CHUNK_CACHE_SIZE: usize = 32;
const DEFAULT_MODEL_SCENE_CACHE_SIZE: usize = 32;
const DEFAULT_MODEL_SCENE_CACHE_BYTES: usize = 512 * 1024 * 1024;
// 16,384 one-second summaries retain about 4.5 hours while bounding the peak
// payload to 8 MiB (16,384 * 128 * sizeof(f32)), plus LRU/key overhead. Range
// reads below address requested chunk keys directly and never scan this cache.
const DEFAULT_AUDIO_WAVEFORM_CHUNK_CACHE_SIZE: usize = 16_384;
pub(crate) const MAX_CONCURRENT_AUTHORING_WAVEFORM_DECODES: usize = 2;
const MAX_AUTHORING_WAVEFORM_SOURCE_FAILURES: usize = 256;

pub type SharedCacheManager = Arc<CacheManager>;

fn lock_cache<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| {
        log::error!("cache lock was poisoned; recovering the last committed state");
        poisoned.into_inner()
    })
}

type WaveformSourceFailure = (String, Option<usize>, AudioDecodeFormat);

struct ModelSceneCache {
    entries: LruCache<ModelResourceKey, Arc<MeshScene>>,
    resident_bytes: usize,
    max_resident_bytes: usize,
}

type ModelSceneFlightKey = (ModelResourceKey, ModelDecodeLimits);

struct ModelSceneFlight {
    result: Mutex<Option<Result<Arc<MeshScene>, ModelResourceError>>>,
    ready: Condvar,
}

impl ModelSceneFlight {
    fn new() -> Self {
        Self {
            result: Mutex::new(None),
            ready: Condvar::new(),
        }
    }

    fn wait(&self) -> Result<Arc<MeshScene>, ModelResourceError> {
        let mut result = lock_cache(&self.result);
        loop {
            if let Some(result) = result.as_ref() {
                return result.clone();
            }
            result = self.ready.wait(result).unwrap_or_else(|poisoned| {
                log::error!("model decode flight lock was poisoned; recovering its result");
                poisoned.into_inner()
            });
        }
    }

    fn finish(&self, result: Result<Arc<MeshScene>, ModelResourceError>) {
        *lock_cache(&self.result) = Some(result);
        self.ready.notify_all();
    }
}

impl ModelSceneCache {
    fn new() -> Self {
        Self {
            entries: LruCache::new(
                NonZeroUsize::new(DEFAULT_MODEL_SCENE_CACHE_SIZE).unwrap_or(NonZeroUsize::MIN),
            ),
            resident_bytes: 0,
            max_resident_bytes: DEFAULT_MODEL_SCENE_CACHE_BYTES,
        }
    }

    fn get(&mut self, key: &ModelResourceKey) -> Option<Arc<MeshScene>> {
        self.entries.get(key).cloned()
    }

    fn put(&mut self, key: ModelResourceKey, scene: Arc<MeshScene>) {
        let scene_bytes = scene.estimated_resident_bytes();
        if scene_bytes > self.max_resident_bytes {
            return;
        }
        if let Some((_, replaced)) = self.entries.push(key, scene) {
            self.resident_bytes = self
                .resident_bytes
                .saturating_sub(replaced.estimated_resident_bytes());
        }
        self.resident_bytes = self.resident_bytes.saturating_add(scene_bytes);
        while self.resident_bytes > self.max_resident_bytes {
            let Some((_, evicted)) = self.entries.pop_lru() else {
                break;
            };
            self.resident_bytes = self
                .resident_bytes
                .saturating_sub(evicted.estimated_resident_bytes());
        }
    }
}

/// Work coordination owned by the shared cache, not by an editor surface.
/// Timeline and Inspector requests therefore deduplicate the same decode and
/// cannot establish competing background worker pools.
pub(crate) struct AuthoringWaveformJobs {
    pending: Mutex<HashSet<AudioChunkKey>>,
    source_failures: Mutex<LruCache<WaveformSourceFailure, ()>>,
}

impl AuthoringWaveformJobs {
    fn new() -> Self {
        Self {
            pending: Mutex::new(HashSet::new()),
            source_failures: Mutex::new(LruCache::new(
                NonZeroUsize::new(MAX_AUTHORING_WAVEFORM_SOURCE_FAILURES)
                    .unwrap_or(NonZeroUsize::MIN),
            )),
        }
    }

    pub(crate) fn reserve(&self, key: AudioChunkKey) -> bool {
        let mut pending = lock_cache(&self.pending);
        if pending.len() >= MAX_CONCURRENT_AUTHORING_WAVEFORM_DECODES {
            return false;
        }
        pending.insert(key)
    }

    pub(crate) fn finish(&self, key: &AudioChunkKey) {
        lock_cache(&self.pending).remove(key);
    }

    pub(crate) fn is_pending(&self, key: &AudioChunkKey) -> bool {
        lock_cache(&self.pending).contains(key)
    }

    pub(crate) fn has_pending_work(&self) -> bool {
        !lock_cache(&self.pending).is_empty()
    }

    pub(crate) fn remember_source_failure(
        &self,
        path: &str,
        stream_index: Option<usize>,
        format: AudioDecodeFormat,
    ) -> bool {
        let key = (path.to_string(), stream_index, format);
        let mut failures = lock_cache(&self.source_failures);
        if failures.contains(&key) {
            return false;
        }
        failures.put(key, ());
        true
    }

    pub(crate) fn clear_failures(&self) {
        lock_cache(&self.source_failures).clear();
    }
}

pub struct CacheManager {
    image_cache: Mutex<LruCache<String, Image>>,
    video_cache: Mutex<LruCache<String, Image>>,
    audio_cache: Mutex<LruCache<AudioChunkKey, Arc<AudioChunk>>>,
    audio_waveform_cache: Mutex<LruCache<AudioChunkKey, Arc<AudioWaveformChunk>>>,
    audio_failures: Mutex<LruCache<AudioChunkKey, ()>>,
    audio_waveform_failures: Mutex<LruCache<AudioChunkKey, ()>>,
    authoring_waveform_jobs: Arc<AuthoringWaveformJobs>,
    model_scene_cache: Mutex<ModelSceneCache>,
    model_scene_flights: Mutex<HashMap<ModelSceneFlightKey, Arc<ModelSceneFlight>>>,
    model_scene_decode_count: AtomicUsize,
}

impl Default for CacheManager {
    fn default() -> Self {
        Self::with_audio_chunk_capacity(DEFAULT_AUDIO_CHUNK_CACHE_SIZE)
    }
}

impl CacheManager {
    #[doc(hidden)]
    pub fn with_audio_chunk_capacity(audio_capacity: usize) -> Self {
        Self::with_audio_cache_capacities(audio_capacity, DEFAULT_AUDIO_WAVEFORM_CHUNK_CACHE_SIZE)
    }

    fn with_audio_cache_capacities(audio_capacity: usize, waveform_capacity: usize) -> Self {
        let image_capacity =
            NonZeroUsize::new(DEFAULT_IMAGE_CACHE_SIZE).unwrap_or(NonZeroUsize::MIN);
        let video_capacity =
            NonZeroUsize::new(DEFAULT_VIDEO_CACHE_SIZE).unwrap_or(NonZeroUsize::MIN);
        let audio_capacity = NonZeroUsize::new(audio_capacity).unwrap_or(NonZeroUsize::MIN);
        let waveform_capacity = NonZeroUsize::new(waveform_capacity).unwrap_or(NonZeroUsize::MIN);

        Self {
            image_cache: Mutex::new(LruCache::new(image_capacity)),
            video_cache: Mutex::new(LruCache::new(video_capacity)),
            audio_cache: Mutex::new(LruCache::new(audio_capacity)),
            audio_waveform_cache: Mutex::new(LruCache::new(waveform_capacity)),
            audio_failures: Mutex::new(LruCache::new(audio_capacity)),
            // Waveform-only requests may span farther than the small raw cache.
            // Keep their failures independent so a UI preview cannot mute or
            // flush authoritative playback.
            audio_waveform_failures: Mutex::new(LruCache::new(waveform_capacity)),
            authoring_waveform_jobs: Arc::new(AuthoringWaveformJobs::new()),
            model_scene_cache: Mutex::new(ModelSceneCache::new()),
            model_scene_flights: Mutex::new(HashMap::new()),
            model_scene_decode_count: AtomicUsize::new(0),
        }
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_image(&self, path: &str) -> Option<Image> {
        lock_cache(&self.image_cache).get(path).cloned()
    }

    pub fn put_image(&self, path: &str, image: &Image) {
        lock_cache(&self.image_cache).put(path.to_string(), image.clone());
    }

    pub fn get_video_frame(&self, path: &str, target_pts: i64) -> Option<Image> {
        let key = Self::video_key(path, target_pts);
        lock_cache(&self.video_cache).get(&key).cloned()
    }

    pub fn put_video_frame(&self, path: &str, target_pts: i64, image: &Image) {
        let key = Self::video_key(path, target_pts);
        lock_cache(&self.video_cache).put(key, image.clone());
    }

    fn video_key(path: &str, target_pts: i64) -> String {
        format!("{path}::pts={target_pts}")
    }

    pub fn get_audio_chunk(&self, key: &AudioChunkKey) -> Option<Arc<AudioChunk>> {
        lock_cache(&self.audio_cache).get(key).cloned()
    }

    pub fn put_audio_chunk(&self, chunk: AudioChunk) {
        self.put_audio_chunk_arc(Arc::new(chunk));
    }

    pub fn put_audio_chunk_arc(&self, chunk: Arc<AudioChunk>) {
        let key = chunk.key().clone();
        let waveform = Arc::new(AudioWaveformChunk::from_audio_chunk(&chunk));
        lock_cache(&self.audio_failures).pop(&key);
        lock_cache(&self.audio_waveform_failures).pop(&key);
        lock_cache(&self.audio_waveform_cache).put(key.clone(), waveform);
        lock_cache(&self.audio_cache).put(key, chunk);
    }

    pub(crate) fn put_audio_waveform_chunk(&self, chunk: AudioWaveformChunk) {
        let key = chunk.key().clone();
        lock_cache(&self.audio_waveform_failures).pop(&key);
        lock_cache(&self.audio_waveform_cache).put(key, Arc::new(chunk));
    }

    pub(crate) fn get_audio_waveform_chunk(
        &self,
        key: &AudioChunkKey,
    ) -> Option<Arc<AudioWaveformChunk>> {
        lock_cache(&self.audio_waveform_cache).get(key).cloned()
    }

    pub(crate) fn audio_waveform_window_state(
        &self,
        source: &AudioSourceKey,
        first_chunk: u64,
        final_chunk: u64,
    ) -> (Vec<Arc<AudioWaveformChunk>>, HashSet<u64>) {
        if first_chunk > final_chunk {
            return (Vec::new(), HashSet::new());
        }
        let chunks: Vec<Arc<AudioWaveformChunk>> = {
            let mut cache = lock_cache(&self.audio_waveform_cache);
            (first_chunk..=final_chunk)
                .filter_map(|chunk_index| {
                    cache
                        .get(&AudioChunkKey {
                            source: source.clone(),
                            chunk_index,
                        })
                        .cloned()
                })
                .collect()
        };
        let ready = chunks
            .iter()
            .map(|chunk| chunk.key().chunk_index)
            .collect::<HashSet<_>>();
        let failures = {
            let failures = lock_cache(&self.audio_waveform_failures);
            (first_chunk..=final_chunk)
                .filter(|chunk_index| {
                    !ready.contains(chunk_index)
                        && failures.contains(&AudioChunkKey {
                            source: source.clone(),
                            chunk_index: *chunk_index,
                        })
                })
                .collect()
        };
        (chunks, failures)
    }

    pub fn remove_audio_chunk(&self, key: &AudioChunkKey) {
        lock_cache(&self.audio_cache).pop(key);
    }

    pub fn mark_audio_chunk_failed(&self, key: AudioChunkKey) {
        lock_cache(&self.audio_cache).pop(&key);
        lock_cache(&self.audio_failures).put(key, ());
    }

    pub fn audio_chunk_failed(&self, key: &AudioChunkKey) -> bool {
        lock_cache(&self.audio_failures).contains(key)
    }

    pub(crate) fn mark_audio_waveform_chunk_failed(&self, key: AudioChunkKey) {
        lock_cache(&self.audio_waveform_failures).put(key, ());
    }

    pub(crate) fn audio_waveform_chunk_failed(&self, key: &AudioChunkKey) -> bool {
        lock_cache(&self.audio_waveform_failures).contains(key)
    }

    pub fn clear_audio_failures(&self) {
        lock_cache(&self.audio_failures).clear();
        lock_cache(&self.audio_waveform_failures).clear();
        self.authoring_waveform_jobs.clear_failures();
    }

    pub(crate) fn get_model_scene(&self, key: &ModelResourceKey) -> Option<Arc<MeshScene>> {
        lock_cache(&self.model_scene_cache).get(key)
    }

    pub(crate) fn put_model_scene(&self, key: ModelResourceKey, scene: Arc<MeshScene>) {
        lock_cache(&self.model_scene_cache).put(key, scene);
    }

    pub(crate) fn get_or_decode_model_scene<F>(
        &self,
        key: ModelResourceKey,
        limits: ModelDecodeLimits,
        decode: F,
    ) -> Result<Arc<MeshScene>, ModelResourceError>
    where
        F: FnOnce() -> Result<Arc<MeshScene>, ModelResourceError>,
    {
        if let Some(scene) = self.get_model_scene(&key) {
            return Ok(scene);
        }

        let flight_key = (key.clone(), limits);
        let (flight, is_leader) = {
            let mut flights = lock_cache(&self.model_scene_flights);
            if let Some(flight) = flights.get(&flight_key) {
                (Arc::clone(flight), false)
            } else {
                let flight = Arc::new(ModelSceneFlight::new());
                flights.insert(flight_key.clone(), Arc::clone(&flight));
                (flight, true)
            }
        };
        if !is_leader {
            return flight.wait();
        }

        // Close the miss/flight-registration race before invoking a decoder.
        let result = if let Some(scene) = self.get_model_scene(&key) {
            Ok(scene)
        } else {
            self.model_scene_decode_count
                .fetch_add(1, Ordering::Relaxed);
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(decode)) {
                Ok(result) => result,
                Err(_) => {
                    log::error!("model resource decoder panicked; publishing a failed flight");
                    Err(ModelResourceError::Decode {
                        detail: "model resource decoder panicked".to_string(),
                    })
                }
            }
        };
        if let Ok(scene) = &result {
            self.put_model_scene(key, Arc::clone(scene));
        }
        flight.finish(result.clone());
        {
            let mut flights = lock_cache(&self.model_scene_flights);
            if flights
                .get(&flight_key)
                .is_some_and(|active| Arc::ptr_eq(active, &flight))
            {
                flights.remove(&flight_key);
            }
        }
        result
    }

    #[doc(hidden)]
    pub fn model_scene_cache_len(&self) -> usize {
        lock_cache(&self.model_scene_cache).entries.len()
    }

    #[doc(hidden)]
    pub fn model_scene_cache_resident_bytes(&self) -> usize {
        lock_cache(&self.model_scene_cache).resident_bytes
    }

    #[doc(hidden)]
    pub fn model_scene_decode_count(&self) -> usize {
        self.model_scene_decode_count.load(Ordering::Relaxed)
    }

    pub(crate) fn authoring_waveform_jobs(&self) -> Arc<AuthoringWaveformJobs> {
        Arc::clone(&self.authoring_waveform_jobs)
    }

    #[doc(hidden)]
    pub fn audio_chunk_cache_len(&self) -> usize {
        lock_cache(&self.audio_cache).len()
    }

    #[doc(hidden)]
    pub fn audio_chunk_cache_capacity(&self) -> usize {
        lock_cache(&self.audio_cache).cap().get()
    }

    #[doc(hidden)]
    pub fn cached_audio_sample_count(&self) -> usize {
        lock_cache(&self.audio_cache)
            .iter()
            .map(|(_, chunk)| chunk.samples().len())
            .sum()
    }

    #[doc(hidden)]
    pub fn audio_waveform_chunk_cache_len(&self) -> usize {
        lock_cache(&self.audio_waveform_cache).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::audio::cache::{AudioDecodeFormat, AudioSourceKey};

    struct TempSource(std::path::PathBuf);

    impl TempSource {
        fn create(contents: &[u8]) -> Self {
            let path = std::env::temp_dir()
                .join(format!("ruvie-audio-cache-{}.source", uuid::Uuid::new_v4()));
            std::fs::write(&path, contents).unwrap();
            Self(path)
        }
    }

    impl Drop for TempSource {
        fn drop(&mut self) {
            drop(std::fs::remove_file(&self.0));
        }
    }

    #[test]
    fn waveform_peak_payload_budget_is_eight_mebibytes() {
        assert_eq!(
            DEFAULT_AUDIO_WAVEFORM_CHUNK_CACHE_SIZE
                * crate::core::audio::waveform::AUDIO_WAVEFORM_BUCKETS_PER_CHUNK
                * std::mem::size_of::<f32>(),
            8 * 1024 * 1024
        );
    }

    #[test]
    fn audio_cache_is_bounded_by_chunk_count_and_sample_capacity() {
        let file = TempSource::create(b"cache identity");
        let format = AudioDecodeFormat::new(4, 2).unwrap();
        let source = AudioSourceKey::read(&file.0, None, format).unwrap();
        let cache = CacheManager::with_audio_chunk_capacity(2);

        for chunk_index in 0..3 {
            let key = AudioChunkKey {
                source: source.clone(),
                chunk_index,
            };
            cache.put_audio_chunk(AudioChunk::new(key, vec![0.5; 8]).unwrap());
        }

        assert_eq!(cache.audio_chunk_cache_capacity(), 2);
        assert_eq!(cache.audio_chunk_cache_len(), 2);
        assert_eq!(cache.cached_audio_sample_count(), 16);
        assert_eq!(cache.audio_waveform_chunk_cache_len(), 3);
        let evicted_raw_key = AudioChunkKey {
            source,
            chunk_index: 0,
        };
        assert!(cache.get_audio_chunk(&evicted_raw_key).is_none());
        assert!(cache.get_audio_waveform_chunk(&evicted_raw_key).is_some());
    }

    #[test]
    fn waveform_summary_cache_evicts_at_its_independent_bound() {
        let file = TempSource::create(b"bounded waveform identity");
        let format = AudioDecodeFormat::new(4, 1).unwrap();
        let source = AudioSourceKey::read(&file.0, None, format).unwrap();
        let cache = CacheManager::with_audio_cache_capacities(1, 2);

        for chunk_index in 0..3 {
            cache.put_audio_chunk(
                AudioChunk::new(
                    AudioChunkKey {
                        source: source.clone(),
                        chunk_index,
                    },
                    vec![chunk_index as f32; 4],
                )
                .unwrap(),
            );
        }

        assert_eq!(cache.audio_chunk_cache_len(), 1);
        assert_eq!(cache.audio_waveform_chunk_cache_len(), 2);
        assert!(
            cache
                .get_audio_waveform_chunk(&AudioChunkKey {
                    source: source.clone(),
                    chunk_index: 0,
                })
                .is_none()
        );
        assert!(
            cache
                .get_audio_waveform_chunk(&AudioChunkKey {
                    source: source.clone(),
                    chunk_index: 2,
                })
                .is_some()
        );

        for chunk_index in 0..3 {
            cache.mark_audio_waveform_chunk_failed(AudioChunkKey {
                source: source.clone(),
                chunk_index,
            });
        }
        assert!(!cache.audio_waveform_chunk_failed(&AudioChunkKey {
            source: source.clone(),
            chunk_index: 0,
        }));
        assert!(cache.audio_waveform_chunk_failed(&AudioChunkKey {
            source,
            chunk_index: 2,
        }));
    }

    #[test]
    fn replacing_a_file_cannot_reuse_chunks_from_the_previous_identity() {
        let file = TempSource::create(b"first source bytes");
        let format = AudioDecodeFormat::new(4, 1).unwrap();
        let old_source = AudioSourceKey::read(&file.0, None, format).unwrap();
        let old_key = AudioChunkKey {
            source: old_source,
            chunk_index: 0,
        };
        let cache = CacheManager::with_audio_chunk_capacity(2);
        cache.put_audio_chunk(AudioChunk::new(old_key.clone(), vec![0.25; 4]).unwrap());

        let replacement = TempSource::create(b"different replacement source bytes");
        std::fs::rename(&replacement.0, &file.0).unwrap();
        let new_source = AudioSourceKey::read(&file.0, None, format).unwrap();
        let new_key = AudioChunkKey {
            source: new_source,
            chunk_index: 0,
        };

        assert_ne!(old_key, new_key);
        assert!(cache.get_audio_chunk(&new_key).is_none());
        assert_eq!(
            cache.get_audio_chunk(&old_key).unwrap().samples(),
            &[0.25; 4]
        );
    }
}
