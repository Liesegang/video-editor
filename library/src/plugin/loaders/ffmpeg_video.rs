use super::super::{LoadPlugin, LoadRequest, LoadResponse, Plugin};
use crate::cache::CacheManager;
use crate::editor::color_service::{ColorSpaceManager, OcioProcessor};
use crate::error::LibraryError;
use crate::model::frame::Image;
use ffmpeg_next as ffmpeg;
use std::collections::HashMap;
use std::sync::Mutex;

// ============================================================================
// VideoReader - Decodes video frames using FFmpeg
// ============================================================================

pub struct VideoReader {
    input_context: ffmpeg::format::context::Input,
    video_stream_index: usize,
    decoder: ffmpeg::decoder::Video,
    next_frame_number: Option<u64>,
    fps: f64,
    ocio_processor: Option<OcioProcessor>,
    current_color_space: Option<(String, String)>,
}

impl VideoReader {
    pub fn new(file_path: &str) -> Result<Self, LibraryError> {
        Self::new_with_stream(file_path, None)
    }

    pub fn new_with_stream(
        file_path: &str,
        stream_index: Option<usize>,
    ) -> Result<Self, LibraryError> {
        ffmpeg::init()?;

        let input_context = ffmpeg::format::input(&file_path)?;
        let input = if let Some(idx) = stream_index {
            input_context.stream(idx).ok_or(LibraryError::FfmpegOther(
                "Specified stream not found".to_string(),
            ))?
        } else {
            input_context
                .streams()
                .best(ffmpeg::media::Type::Video)
                .ok_or(LibraryError::FfmpegOther(
                    "No video stream found".to_string(),
                ))?
        };

        if input.parameters().medium() != ffmpeg::media::Type::Video {
            return Err(LibraryError::FfmpegOther(
                "Specified stream is not a video stream".to_string(),
            ));
        }

        let video_stream_index = input.index();

        let context_decoder = ffmpeg::codec::context::Context::from_parameters(input.parameters())?;
        let decoder = context_decoder.decoder().video()?;

        let stream = input_context
            .stream(video_stream_index)
            .ok_or(LibraryError::FfmpegOther("Stream not found".to_string()))?;
        let avg_frame_rate = stream.avg_frame_rate();
        let fps = if avg_frame_rate.denominator() > 0 {
            avg_frame_rate.numerator() as f64 / avg_frame_rate.denominator() as f64
        } else {
            30.0
        };

        Ok(Self {
            input_context,
            video_stream_index,
            decoder,
            next_frame_number: None,
            fps,
            ocio_processor: None,
            current_color_space: None,
        })
    }

    pub fn get_fps(&self) -> f64 {
        self.fps
    }

    pub fn get_duration(&self) -> Option<f64> {
        if self.input_context.duration() == ffmpeg::ffi::AV_NOPTS_VALUE {
            None
        } else {
            Some(self.input_context.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64)
        }
    }

    pub fn get_dimensions(&self) -> (u32, u32) {
        (self.decoder.width(), self.decoder.height())
    }

    pub fn set_color_space(&mut self, src: &str, dst: &str) {
        if let Some((current_src, current_dst)) = &self.current_color_space {
            if current_src == src && current_dst == dst {
                return;
            }
        }
        self.ocio_processor = ColorSpaceManager::create_processor(src, dst);
        self.current_color_space = Some((src.to_string(), dst.to_string()));
    }

    pub fn decode_frame(&mut self, frame_number: u64) -> Result<Image, LibraryError> {
        let stream = self
            .input_context
            .stream(self.video_stream_index)
            .ok_or(LibraryError::FfmpegOther("Stream not found".to_string()))?;
        let time_base = stream.time_base();
        let fps = self.fps;

        let need_seek = match self.next_frame_number {
            Some(next) => next != frame_number,
            None => true,
        };
        if need_seek {
            let timestamp = (frame_number as f64 / fps / f64::from(time_base.numerator())
                * f64::from(time_base.denominator())) as i64;
            self.input_context.seek(timestamp, ..timestamp)?;
            self.decoder.flush();
        }

        let mut decoded_frame = None;

        for (stream, packet) in self.input_context.packets() {
            if stream.index() == self.video_stream_index {
                self.decoder.send_packet(&packet)?;
                let mut frame = ffmpeg::util::frame::Video::empty();
                while self.decoder.receive_frame(&mut frame).is_ok() {
                    let pts = frame.pts().unwrap_or(0);
                    let frame_num = (pts as f64 * f64::from(time_base.numerator())
                        / f64::from(time_base.denominator())
                        * fps)
                        .round() as u64;

                    if frame_num >= frame_number {
                        decoded_frame = Some(frame.clone());
                        self.next_frame_number = Some(frame_num + 1);
                        break;
                    }
                }
                if decoded_frame.is_some() {
                    break;
                }
            }
        }

        if decoded_frame.is_none() {
            self.decoder.send_eof()?;
            let mut frame = ffmpeg::util::frame::Video::empty();
            while self.decoder.receive_frame(&mut frame).is_ok() {
                let pts = frame.pts().unwrap_or(0);
                let frame_num = (pts as f64 * f64::from(time_base.numerator())
                    / f64::from(time_base.denominator())
                    * fps)
                    .round() as u64;
                if frame_num >= frame_number {
                    decoded_frame = Some(frame.clone());
                    self.next_frame_number = Some(frame_num + 1);
                    break;
                }
            }
        }

        let frame = decoded_frame.ok_or(LibraryError::FfmpegOther(
            "Failed to decode the specified frame".to_string(),
        ))?;

        let mut scaler = ffmpeg::software::scaling::context::Context::get(
            self.decoder.format(),
            self.decoder.width(),
            self.decoder.height(),
            ffmpeg::format::Pixel::RGBA,
            self.decoder.width(),
            self.decoder.height(),
            ffmpeg::software::scaling::flag::Flags::BILINEAR,
        )?;
        let mut rgba_frame = ffmpeg::util::frame::Video::empty();
        scaler.run(&frame, &mut rgba_frame)?;

        let width = rgba_frame.width();
        let height = rgba_frame.height();
        let row_bytes = (width * 4) as usize;
        let mut data = Vec::with_capacity(row_bytes * height as usize);
        let stride = rgba_frame.stride(0) as usize;
        let plane = rgba_frame.data(0);
        for y in 0..(height as usize) {
            let start = y * stride;
            let end = start + row_bytes;
            data.extend_from_slice(&plane[start..end]);
        }

        if let Some(processor) = &self.ocio_processor {
            data = processor.apply_rgba(&data);
        }

        Ok(Image {
            width,
            height,
            data,
        })
    }
}

// ============================================================================
// MediaProbe - Probes media files for metadata
// ============================================================================

pub struct MediaProbe {
    input_context: ffmpeg::format::context::Input,
}

impl MediaProbe {
    pub fn new(file_path: &str) -> Result<Self, LibraryError> {
        ffmpeg::init()?;
        let input_context = ffmpeg::format::input(&file_path)?;
        Ok(Self { input_context })
    }

    pub fn get_duration(&self) -> Option<f64> {
        if self.input_context.duration() == ffmpeg::ffi::AV_NOPTS_VALUE {
            None
        } else {
            Some(self.input_context.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64)
        }
    }

    pub fn get_fps(&self) -> f64 {
        if let Some(stream) = self
            .input_context
            .streams()
            .best(ffmpeg::media::Type::Video)
        {
            let avg_frame_rate = stream.avg_frame_rate();
            if avg_frame_rate.denominator() > 0 {
                return avg_frame_rate.numerator() as f64 / avg_frame_rate.denominator() as f64;
            }
        }
        0.0
    }

    pub fn get_dimensions(&self) -> (u32, u32) {
        if let Some(stream) = self
            .input_context
            .streams()
            .best(ffmpeg::media::Type::Video)
        {
            if let Ok(decoder) =
                ffmpeg::codec::context::Context::from_parameters(stream.parameters())
                    .and_then(|c| c.decoder().video())
            {
                return (decoder.width(), decoder.height());
            }
        }
        (0, 0)
    }

    pub fn has_video(&self) -> bool {
        self.input_context
            .streams()
            .best(ffmpeg::media::Type::Video)
            .is_some()
    }

    pub fn has_audio(&self) -> bool {
        self.input_context
            .streams()
            .best(ffmpeg::media::Type::Audio)
            .is_some()
    }

    pub fn get_available_streams(&self) -> Vec<crate::plugin::AssetMetadata> {
        let mut streams = Vec::new();
        let duration = self.get_duration();

        for stream in self.input_context.streams() {
            let params = stream.parameters();
            let medium = params.medium();

            let kind = match medium {
                ffmpeg::media::Type::Video => crate::model::project::asset::AssetKind::Video,
                ffmpeg::media::Type::Audio => crate::model::project::asset::AssetKind::Audio,
                _ => continue,
            };

            let mut fps = None;
            let mut width = None;
            let mut height = None;

            if kind == crate::model::project::asset::AssetKind::Video {
                let avg_frame_rate = stream.avg_frame_rate();
                if avg_frame_rate.denominator() > 0 {
                    fps = Some(
                        avg_frame_rate.numerator() as f64 / avg_frame_rate.denominator() as f64,
                    );
                }

                if let Ok(ctx) = ffmpeg::codec::context::Context::from_parameters(params.clone()) {
                    if let Ok(decoder) = ctx.decoder().video() {
                        width = Some(decoder.width());
                        height = Some(decoder.height());
                    }
                }
            }

            streams.push(crate::plugin::AssetMetadata {
                kind,
                duration,
                fps,
                width,
                height,
                stream_index: Some(stream.index()),
            });
        }
        streams
    }
}

// ============================================================================
// FfmpegVideoLoader Plugin
// ============================================================================

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
