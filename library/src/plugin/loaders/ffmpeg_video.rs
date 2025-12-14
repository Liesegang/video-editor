use super::super::{LoadPlugin, LoadRequest, LoadResponse, Plugin, PluginCategory};
use crate::cache::CacheManager;
use crate::error::LibraryError;
use crate::loader::video::VideoReader;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct FfmpegVideoLoader {
    readers: Mutex<HashMap<String, VideoReader>>,
}

impl FfmpegVideoLoader {
    pub fn new() -> Self {
        Self {
            readers: Mutex::new(HashMap::new()),
        }
    }
}

impl Plugin for FfmpegVideoLoader {
    fn id(&self) -> &'static str {
        "ffmpeg_video_loader"
    }

    fn name(&self) -> String {
        "FFmpeg Video Loader".to_string()
    }

    fn category(&self) -> String {
        "Media".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl LoadPlugin for FfmpegVideoLoader {
    fn supports(&self, request: &LoadRequest) -> bool {
        matches!(request, LoadRequest::VideoFrame { .. })
    }

    fn load(
        &self,
        request: &LoadRequest,
        cache: &CacheManager,
    ) -> Result<LoadResponse, LibraryError> {
        if let LoadRequest::VideoFrame { path, frame_number } = request {
            if let Some(image) = cache.get_video_frame(path, *frame_number) {
                return Ok(LoadResponse::Image(image));
            }

            use std::collections::hash_map::Entry;

            let image = {
                let mut readers = self.readers.lock().unwrap();
                let reader = match readers.entry(path.clone()) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => entry.insert(VideoReader::new(path)?),
                };
                reader.decode_frame(*frame_number)?
            };

            cache.put_video_frame(path, *frame_number, &image);
            Ok(LoadResponse::Image(image))
        } else {
            Err(LibraryError::Plugin(
                "FfmpegVideoLoader received unsupported request".to_string(),
            ))
        }
    }

    fn get_asset_kind(&self, path: &str) -> Option<crate::model::project::asset::AssetKind> {
        let ext = std::path::Path::new(path)
            .extension()?
            .to_str()?
            .to_lowercase();
        // ffmpeg supports audio too, but here we currently assume video loader.
        // Ideally we should distinguish or support both.
        match ext.as_str() {
            "mp4" | "mov" | "avi" | "mkv" | "webm" => {
                Some(crate::model::project::asset::AssetKind::Video)
            }
            "mp3" | "wav" | "aac" | "flac" | "ogg" => {
                Some(crate::model::project::asset::AssetKind::Audio)
            }
            _ => None,
        }
    }

    fn get_duration(&self, path: &str) -> Option<f64> {
        // We need to access the VideoReader to get duration.
        // If it's not in the cache/map, we should temporarily open it.
        // However, self.readers is a Mutex<HashMap<String, VideoReader>>.
        // So we can check if it exists or create a new temporary one.

        // Note: Creating a new VideoReader involves ffmpeg overhead.
        // But get_duration is usually called once at import time.

        let mut readers = self.readers.lock().unwrap();

        // Check if we already have a reader for this path
        if let Some(reader) = readers.get(path) {
            return reader.get_duration();
        }

        // If not, try to create one
        match VideoReader::new(path) {
            Ok(reader) => {
                let duration = reader.get_duration();
                // Should we cache this reader? Maybe not if we are just probing.
                // But for now, let's cache it as we might use it soon for thumbnails.
                readers.insert(path.to_string(), reader);
                duration
            }
            Err(_) => None,
        }
    }

    fn get_fps(&self, path: &str) -> Option<f64> {
        let mut readers = self.readers.lock().unwrap();
        if let Some(reader) = readers.get(path) {
            return Some(reader.get_fps());
        }

        match VideoReader::new(path) {
            Ok(reader) => {
                let fps = reader.get_fps();
                readers.insert(path.to_string(), reader);
                Some(fps)
            }
            Err(_) => None,
        }
    }

    fn get_dimensions(&self, path: &str) -> Option<(u32, u32)> {
        let mut readers = self.readers.lock().unwrap();
        if let Some(reader) = readers.get(path) {
            return Some(reader.get_dimensions());
        }

        match VideoReader::new(path) {
            Ok(reader) => {
                let dim = reader.get_dimensions();
                readers.insert(path.to_string(), reader);
                Some(dim)
            }
            Err(_) => None,
        }
    }
}
