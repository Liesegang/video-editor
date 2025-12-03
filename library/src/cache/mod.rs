use crate::loader::image::Image;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

const DEFAULT_IMAGE_CACHE_SIZE: usize = 64;
const DEFAULT_VIDEO_CACHE_SIZE: usize = 128;

pub type SharedCacheManager = Arc<CacheManager>;

pub struct CacheManager {
  image_cache: Mutex<LruCache<String, Image>>,
  video_cache: Mutex<LruCache<String, Image>>,
}

impl CacheManager {
  pub fn new() -> Self {
    let image_capacity =
      NonZeroUsize::new(DEFAULT_IMAGE_CACHE_SIZE).expect("DEFAULT_IMAGE_CACHE_SIZE must be > 0");
    let video_capacity =
      NonZeroUsize::new(DEFAULT_VIDEO_CACHE_SIZE).expect("DEFAULT_VIDEO_CACHE_SIZE must be > 0");

    Self {
      image_cache: Mutex::new(LruCache::new(image_capacity)),
      video_cache: Mutex::new(LruCache::new(video_capacity)),
    }
  }

  pub fn get_image(&self, path: &str) -> Option<Image> {
    self.image_cache.lock().unwrap().get(path).cloned()
  }

  pub fn put_image(&self, path: &str, image: &Image) {
    self
      .image_cache
      .lock()
      .unwrap()
      .put(path.to_string(), image.clone());
  }

  pub fn get_video_frame(&self, path: &str, frame_number: u64) -> Option<Image> {
    let key = Self::video_key(path, frame_number);
    self.video_cache.lock().unwrap().get(&key).cloned()
  }

  pub fn put_video_frame(&self, path: &str, frame_number: u64, image: &Image) {
    let key = Self::video_key(path, frame_number);
    self.video_cache.lock().unwrap().put(key, image.clone());
  }

  fn video_key(path: &str, frame_number: u64) -> String {
    format!("{}::{}", path, frame_number)
  }
}
