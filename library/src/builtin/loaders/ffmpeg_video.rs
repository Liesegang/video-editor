use super::{LoadPlugin, LoadRequest, LoadResponse};
use crate::cache::CacheManager;
use crate::error::LibraryError;
use crate::plugin::Plugin;
use crate::rendering::color::{ColorSpaceManager, OcioProcessor};
use crate::runtime::Image;
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
        let container_duration = self.get_duration();

        for stream in self.input_context.streams() {
            let params = stream.parameters();
            let medium = params.medium();

            let kind = match medium {
                ffmpeg::media::Type::Video => crate::project::asset::AssetKind::Video,
                ffmpeg::media::Type::Audio => crate::project::asset::AssetKind::Audio,
                _ => continue,
            };

            // Calculate stream-specific duration
            let duration = {
                // First try to get duration from the stream itself
                let stream_duration = stream.duration();
                if stream_duration != ffmpeg::ffi::AV_NOPTS_VALUE && stream_duration > 0 {
                    let time_base = stream.time_base();
                    if time_base.denominator() > 0 {
                        Some(
                            stream_duration as f64 * time_base.numerator() as f64
                                / time_base.denominator() as f64,
                        )
                    } else {
                        container_duration
                    }
                } else {
                    // Fallback to container duration
                    container_duration
                }
            };

            let mut fps = None;
            let mut width = None;
            let mut height = None;

            if kind == crate::project::asset::AssetKind::Video {
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
    readers: Mutex<HashMap<u64, VideoReader>>,
    /// Maps path to existing context_id to avoid re-opening the same file.
    path_to_context: Mutex<HashMap<String, u64>>,
    next_context_id: std::sync::atomic::AtomicU64,
}

impl FfmpegVideoLoader {
    pub fn new() -> Self {
        Self {
            readers: Mutex::new(HashMap::new()),
            path_to_context: Mutex::new(HashMap::new()),
            next_context_id: std::sync::atomic::AtomicU64::new(1),
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
    fn open(&self, path: &str) -> Result<Vec<crate::plugin::AssetMetadata>, LibraryError> {
        // Check if already opened
        {
            let path_map = self.path_to_context.lock().unwrap();
            if let Some(&context_id) = path_map.get(path) {
                let readers = self.readers.lock().unwrap();
                if let Some(reader) = readers.get(&context_id) {
                    return Ok(reader.get_available_streams());
                }
            }
        }

        // Open new reader
        let reader = VideoReader::new(path)?;
        let streams = reader.get_available_streams();

        if streams.is_empty() {
            return Err(LibraryError::plugin("No video or audio stream".to_string()));
        }

        // Generate context ID and store
        let context_id = self
            .next_context_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        self.path_to_context
            .lock()
            .unwrap()
            .insert(path.to_string(), context_id);
        self.readers.lock().unwrap().insert(context_id, reader);

        Ok(streams)
    }

    fn load(
        &self,
        request: &LoadRequest,
        cache: &CacheManager,
    ) -> Result<LoadResponse, LibraryError> {
        if let LoadRequest::VideoFrame {
            path,
            frame_number,
            stream_index: _,
            input_color_space,
            output_color_space,
        } = request
        {
            // Get context_id from path, auto-open if needed
            let context_id = {
                let mut path_map = self.path_to_context.lock().unwrap();
                if let Some(&id) = path_map.get(path) {
                    id
                } else {
                    // Open new reader inline to avoid lock/unlock cycles
                    let reader = VideoReader::new(path)?;
                    let streams = reader.get_available_streams();
                    if streams.is_empty() {
                        return Err(LibraryError::plugin("No video or audio stream".to_string()));
                    }
                    let id = self
                        .next_context_id
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    path_map.insert(path.to_string(), id);
                    self.readers.lock().unwrap().insert(id, reader);
                    id
                }
            };

            let cache_key = path.as_str();

            let image = {
                let mut readers = self.readers.lock().unwrap();
                let reader = readers.get_mut(&context_id).ok_or_else(|| {
                    LibraryError::plugin(format!("Reader for {} not found", path))
                })?;

                if let (Some(src), Some(dst)) = (input_color_space, output_color_space) {
                    reader.set_color_space(src, dst);
                }

                if let Some(cached) = cache.get_video_frame(cache_key, *frame_number) {
                    cached
                } else {
                    let decoded = reader.decode_frame(*frame_number)?;
                    cache.put_video_frame(cache_key, *frame_number, &decoded);
                    decoded
                }
            };

            Ok(LoadResponse { image })
        } else {
            Err(LibraryError::plugin(
                "FfmpegVideoLoader received unsupported request".to_string(),
            ))
        }
    }
}
