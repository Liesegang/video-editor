use super::super::{LoadPlugin, LoadRequest, LoadResponse, Plugin};
use crate::cache::CacheManager;
use crate::core::media::video::{MediaProbe, VideoReader};
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

    fn priority(&self) -> u32 {
        10
    }

    fn load(
        &self,
        request: &LoadRequest,
        cache: &CacheManager,
    ) -> Result<LoadResponse, LibraryError> {
        if let LoadRequest::VideoFrame {
            path,
            frame_number,
            stream_index,
            input_color_space,
            output_color_space,
        } = request
        {
            // Cache key needs to include stream_index to differentiate streams of same file
            let cache_key = if let Some(idx) = stream_index {
                format!("{}?stream={}", path, idx)
            } else {
                path.clone()
            };

            if let Some(image) = cache.get_video_frame(&cache_key, *frame_number) {
                return Ok(LoadResponse::Image(image));
            }

            use std::collections::hash_map::Entry;

            let image = {
                let mut readers = self.readers.lock().unwrap();
                // We use the same cache key logic for the readers map
                let reader = match readers.entry(cache_key.clone()) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => {
                        // Pass stream_index to VideoReader
                        entry.insert(VideoReader::new_with_stream(path, *stream_index)?)
                    }
                };

                // Configure Color Space if provided
                if let (Some(src), Some(dst)) = (input_color_space, output_color_space) {
                    reader.set_color_space(src, dst);
                }

                reader.decode_frame(*frame_number)?
            };

            cache.put_video_frame(&cache_key, *frame_number, &image);
            Ok(LoadResponse::Image(image))
        } else {
            Err(LibraryError::Plugin(
                "FfmpegVideoLoader received unsupported request".to_string(),
            ))
        }
    }

    fn get_available_streams(&self, path: &str) -> Option<Vec<crate::plugin::AssetMetadata>> {
        let ext = std::path::Path::new(path)
            .extension()?
            .to_str()?
            .to_lowercase();

        // Explicitly reject image extensions to let NativeImageLoader handle them
        match ext.as_str() {
            "png" | "jpg" | "jpeg" | "bmp" | "webp" | "tiff" | "tga" | "gif" | "ico" | "pnm" => {
                return None;
            }
            _ => {}
        }

        if let Ok(probe) = MediaProbe::new(path) {
            return Some(probe.get_available_streams());
        }
        None
    }

    fn get_asset_kind(&self, path: &str) -> Option<crate::model::project::asset::AssetKind> {
        let ext = std::path::Path::new(path)
            .extension()?
            .to_str()?
            .to_lowercase();

        // Explicitly reject image extensions to let NativeImageLoader handle them
        match ext.as_str() {
            "png" | "jpg" | "jpeg" | "bmp" | "webp" | "tiff" | "tga" | "gif" | "ico" | "pnm" => {
                return None;
            }
            _ => {}
        }

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
        let mut readers = self.readers.lock().unwrap();
        if let Some(reader) = readers.get(path) {
            return reader.get_duration();
        }

        if let Ok(reader) = VideoReader::new(path) {
            let duration = reader.get_duration();
            readers.insert(path.to_string(), reader);
            return duration;
        }

        if let Ok(probe) = MediaProbe::new(path) {
            return probe.get_duration();
        }

        None
    }

    fn get_fps(&self, path: &str) -> Option<f64> {
        let mut readers = self.readers.lock().unwrap();
        if let Some(reader) = readers.get(path) {
            return Some(reader.get_fps());
        }

        if let Ok(reader) = VideoReader::new(path) {
            let fps = reader.get_fps();
            readers.insert(path.to_string(), reader);
            return Some(fps);
        }

        if let Ok(probe) = MediaProbe::new(path) {
            return Some(probe.get_fps());
        }

        None
    }

    fn get_dimensions(&self, path: &str) -> Option<(u32, u32)> {
        let mut readers = self.readers.lock().unwrap();
        if let Some(reader) = readers.get(path) {
            return Some(reader.get_dimensions());
        }

        if let Ok(reader) = VideoReader::new(path) {
            let dim = reader.get_dimensions();
            readers.insert(path.to_string(), reader);
            return Some(dim);
        }

        if let Ok(probe) = MediaProbe::new(path) {
            return Some(probe.get_dimensions());
        }

        None
    }
    fn get_metadata(&self, path: &str) -> Option<crate::plugin::AssetMetadata> {
        let ext = std::path::Path::new(path)
            .extension()?
            .to_str()?
            .to_lowercase();

        // Explicitly reject image extensions to let NativeImageLoader handle them
        match ext.as_str() {
            "png" | "jpg" | "jpeg" | "bmp" | "webp" | "tiff" | "tga" | "gif" | "ico" | "pnm" => {
                return None;
            }
            _ => {}
        }

        // Check if cached
        {
            let readers = self.readers.lock().unwrap();
            if let Some(reader) = readers.get(path) {
                return Some(crate::plugin::AssetMetadata {
                    kind: crate::model::project::asset::AssetKind::Video,
                    duration: reader.get_duration(),
                    fps: Some(reader.get_fps()),
                    width: Some(reader.get_dimensions().0),
                    height: Some(reader.get_dimensions().1),
                    stream_index: None, // Default stream
                });
            }
        }

        // Try VideoReader (for caching)
        if let Ok(reader) = VideoReader::new(path) {
            let duration = reader.get_duration();
            let fps = reader.get_fps();
            let (w, h) = reader.get_dimensions();

            {
                let mut readers = self.readers.lock().unwrap();
                readers.insert(path.to_string(), reader);
            }

            return Some(crate::plugin::AssetMetadata {
                kind: crate::model::project::asset::AssetKind::Video,
                duration,
                fps: Some(fps),
                width: Some(w),
                height: Some(h),
                stream_index: None, // Default stream
            });
        }

        // Fallback to MediaProbe (e.g. for Audio)
        if let Ok(probe) = MediaProbe::new(path) {
            let kind = if probe.has_video() {
                crate::model::project::asset::AssetKind::Video
            } else if probe.has_audio() {
                crate::model::project::asset::AssetKind::Audio
            } else {
                return None;
            };

            return Some(crate::plugin::AssetMetadata {
                kind,
                duration: probe.get_duration(),
                fps: Some(probe.get_fps()),
                width: Some(probe.get_dimensions().0),
                height: Some(probe.get_dimensions().1),
                stream_index: None, // Implicit best stream
            });
        }

        None
    }
}
