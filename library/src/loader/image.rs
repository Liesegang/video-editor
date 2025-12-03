use lru::LruCache;
use once_cell::sync::Lazy;
use std::error::Error;
use std::num::NonZeroUsize;
use std::sync::Mutex;

const IMAGE_CACHE_SIZE: usize = 32;

static IMAGE_CACHE: Lazy<Mutex<LruCache<String, Image>>> = Lazy::new(|| {
  let capacity = NonZeroUsize::new(IMAGE_CACHE_SIZE).expect("IMAGE_CACHE_SIZE must be > 0");
  Mutex::new(LruCache::new(capacity))
});

pub fn load_image(path: &String) -> Result<Image, Box<dyn Error>> {
  if let Some(image) = IMAGE_CACHE.lock().unwrap().get(path).cloned() {
    return Ok(image);
  }

  let image = image::open(path)
    .map(|img| {
      let rgba_image = img.to_rgba8();
      Image {
        width: rgba_image.width(),
        height: rgba_image.height(),
        data: rgba_image.into_raw(),
      }
    })
    .map_err(|e| format!("画像ファイルを開けませんでした: {}", e))?;

  {
    let mut cache = IMAGE_CACHE.lock().unwrap();
    cache.put(path.clone(), image.clone());
  }

  Ok(image)
}

#[derive(Clone)]
pub struct Image {
  pub width: u32,
  pub height: u32,
  pub data: Vec<u8>,
}
