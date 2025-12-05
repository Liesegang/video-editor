use super::super::{LoadPlugin, LoadRequest, LoadResponse, Plugin, PluginCategory};
use crate::cache::CacheManager;
use crate::loader::video::VideoReader;
use crate::error::LibraryError;
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

    fn category(&self) -> PluginCategory {
        PluginCategory::Load
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
            Err(LibraryError::Plugin("FfmpegVideoLoader received unsupported request".to_string()))
        }
    }
}
