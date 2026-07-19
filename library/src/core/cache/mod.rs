use crate::core::audio::cache::{AudioChunk, AudioChunkKey};
use crate::model::frame::Image;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard};

const DEFAULT_IMAGE_CACHE_SIZE: usize = 64;
const DEFAULT_VIDEO_CACHE_SIZE: usize = 128;
const DEFAULT_AUDIO_CHUNK_CACHE_SIZE: usize = 32;

pub type SharedCacheManager = Arc<CacheManager>;

fn lock_cache<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| {
        log::error!("cache lock was poisoned; recovering the last committed state");
        poisoned.into_inner()
    })
}

pub struct CacheManager {
    image_cache: Mutex<LruCache<String, Image>>,
    video_cache: Mutex<LruCache<String, Image>>,
    audio_cache: Mutex<LruCache<AudioChunkKey, Arc<AudioChunk>>>,
    audio_failures: Mutex<LruCache<AudioChunkKey, ()>>,
}

impl Default for CacheManager {
    fn default() -> Self {
        Self::with_audio_chunk_capacity(DEFAULT_AUDIO_CHUNK_CACHE_SIZE)
    }
}

impl CacheManager {
    #[doc(hidden)]
    pub fn with_audio_chunk_capacity(audio_capacity: usize) -> Self {
        let image_capacity =
            NonZeroUsize::new(DEFAULT_IMAGE_CACHE_SIZE).unwrap_or(NonZeroUsize::MIN);
        let video_capacity =
            NonZeroUsize::new(DEFAULT_VIDEO_CACHE_SIZE).unwrap_or(NonZeroUsize::MIN);
        let audio_capacity = NonZeroUsize::new(audio_capacity).unwrap_or(NonZeroUsize::MIN);

        Self {
            image_cache: Mutex::new(LruCache::new(image_capacity)),
            video_cache: Mutex::new(LruCache::new(video_capacity)),
            audio_cache: Mutex::new(LruCache::new(audio_capacity)),
            audio_failures: Mutex::new(LruCache::new(audio_capacity)),
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
        lock_cache(&self.audio_failures).pop(&key);
        lock_cache(&self.audio_cache).put(key, chunk);
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

    pub fn clear_audio_failures(&self) {
        lock_cache(&self.audio_failures).clear();
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::audio::cache::{AudioDecodeFormat, AudioSourceKey};

    struct TempSource(std::path::PathBuf);

    impl TempSource {
        fn create(contents: &[u8]) -> Self {
            let path = std::env::temp_dir().join(format!(
                "ruvie-audio-cache-{}.source",
                uuid::Uuid::new_v4()
            ));
            std::fs::write(&path, contents).unwrap();
            Self(path)
        }
    }

    impl Drop for TempSource {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
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
        assert_eq!(cache.get_audio_chunk(&old_key).unwrap().samples(), &[0.25; 4]);
    }
}
