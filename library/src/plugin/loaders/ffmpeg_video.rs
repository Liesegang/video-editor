use super::super::{
    LoadPlugin, LoadPluginError, LoadPluginResult, LoadRequest, LoadResponse, Plugin,
};
use crate::cache::CacheManager;
use crate::editor::color_service::{ColorSpaceManager, OcioProcessor};
use crate::error::LibraryError;
use crate::model::frame::Image;
use ffmpeg::Rescale;
use ffmpeg_next as ffmpeg;
use lru::LruCache;
use std::collections::HashSet;
use std::ffi::{CStr, c_void};
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::UNIX_EPOCH;

const MAX_FORWARD_DECODE_GAP_FRAMES: i64 = 32;
const DEFAULT_READER_CACHE_SIZE: usize = 8;

/// Decoder work performed by the most recent [`VideoReader::decode_frame`] call.
///
/// This is intentionally operation-based rather than wall-clock based so tests
/// can detect a broken seek without depending on machine performance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DecodeStats {
    pub target_pts: i64,
    pub selected_pts: Option<i64>,
    pub seek_count: u64,
    pub video_packets_read: u64,
    pub frames_decoded: u64,
    pub timestamp_repairs: u64,
}

struct BufferedFrame {
    frame: ffmpeg::util::frame::Video,
    pts: i64,
    fallback_end_pts: i64,
}

#[derive(Debug)]
struct TimestampNormalizer {
    last_pts: Option<i64>,
    next_fallback_pts: i64,
    nominal_duration: i64,
}

impl TimestampNormalizer {
    fn new(nominal_duration: i64) -> Self {
        Self {
            last_pts: None,
            next_fallback_pts: 0,
            nominal_duration: nominal_duration.max(1),
        }
    }

    fn reset(&mut self, fallback_pts: i64) {
        self.last_pts = None;
        self.next_fallback_pts = fallback_pts;
    }

    fn normalize(
        &mut self,
        timestamp: Option<i64>,
        packet_duration: i64,
        stats: &mut DecodeStats,
    ) -> (i64, i64) {
        let mut pts = timestamp.unwrap_or_else(|| {
            stats.timestamp_repairs += 1;
            self.next_fallback_pts
        });
        if let Some(last_pts) = self.last_pts
            && pts <= last_pts
        {
            stats.timestamp_repairs += 1;
            pts = self.next_fallback_pts.max(last_pts.saturating_add(1));
        }
        let duration = packet_duration.max(self.nominal_duration).max(1);
        self.last_pts = Some(pts);
        self.next_fallback_pts = pts.saturating_add(duration);
        (pts, duration)
    }
}

// ============================================================================
// VideoReader - Decodes video frames using FFmpeg
// ============================================================================

pub struct VideoReader {
    file_path: String,
    input_context: ffmpeg::format::context::Input,
    video_stream_index: usize,
    decoder: ffmpeg::decoder::Video,
    stream_time_base: ffmpeg::Rational,
    stream_start_time: i64,
    stream_duration: Option<i64>,
    nominal_frame_duration: i64,
    fps: f64,
    frame_count: Option<u64>,
    current_frame: Option<BufferedFrame>,
    lookahead_frame: Option<BufferedFrame>,
    last_target_pts: Option<i64>,
    decoder_eof: bool,
    timestamp_normalizer: TimestampNormalizer,
    ocio_processor: Option<OcioProcessor>,
    current_color_space: Option<(String, String)>,
    last_decode_stats: DecodeStats,
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
        let frame_rate = if avg_frame_rate.numerator() > 0 && avg_frame_rate.denominator() > 0 {
            avg_frame_rate
        } else {
            ffmpeg::Rational(30, 1)
        };
        let fps = frame_rate.numerator() as f64 / frame_rate.denominator() as f64;
        let stream_time_base = stream.time_base();
        let stream_start_time = match stream.start_time() {
            ffmpeg::ffi::AV_NOPTS_VALUE => 0,
            start_time => start_time,
        };
        let raw_stream_duration = stream.duration();
        let stream_duration = (raw_stream_duration != ffmpeg::ffi::AV_NOPTS_VALUE
            && raw_stream_duration > 0)
            .then_some(raw_stream_duration);
        let frame_time_base = ffmpeg::Rational(frame_rate.denominator(), frame_rate.numerator());
        let nominal_frame_duration = 1_i64.rescale(frame_time_base, stream_time_base).max(1);
        let frame_count = u64::try_from(stream.frames())
            .ok()
            .filter(|frames| *frames > 0);

        Ok(Self {
            file_path: file_path.to_string(),
            input_context,
            video_stream_index,
            decoder,
            stream_time_base,
            stream_start_time,
            stream_duration,
            nominal_frame_duration,
            fps,
            frame_count,
            current_frame: None,
            lookahead_frame: None,
            last_target_pts: None,
            decoder_eof: false,
            timestamp_normalizer: TimestampNormalizer::new(nominal_frame_duration),
            ocio_processor: None,
            current_color_space: None,
            last_decode_stats: DecodeStats::default(),
        })
    }

    pub fn get_fps(&self) -> f64 {
        self.fps
    }

    pub fn get_duration(&self) -> Option<f64> {
        self.stream_duration
            .map(|duration| self.stream_ticks_to_seconds(duration))
            .or_else(|| {
                (self.input_context.duration() != ffmpeg::ffi::AV_NOPTS_VALUE).then(|| {
                    self.input_context.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64
                })
            })
    }

    pub fn get_frame_count(&self) -> Option<u64> {
        self.frame_count
    }

    pub fn get_stream_index(&self) -> usize {
        self.video_stream_index
    }

    pub fn get_stream_time_base(&self) -> (i32, i32) {
        (
            self.stream_time_base.numerator(),
            self.stream_time_base.denominator(),
        )
    }

    pub fn get_dimensions(&self) -> (u32, u32) {
        (self.decoder.width(), self.decoder.height())
    }

    pub fn set_color_space(&mut self, src: &str, dst: &str) {
        if let Some((current_src, current_dst)) = &self.current_color_space
            && current_src == src
            && current_dst == dst
        {
            return;
        }
        self.ocio_processor = ColorSpaceManager::create_processor(src, dst);
        self.current_color_space = Some((src.to_string(), dst.to_string()));
    }

    pub fn clear_color_space(&mut self) {
        self.ocio_processor = None;
        self.current_color_space = None;
    }

    pub fn last_decode_stats(&self) -> DecodeStats {
        self.last_decode_stats
    }

    /// Convenience API for callers that have an authoritative CFR ordinal.
    /// Runtime rendering uses [`Self::decode_at_time`] so VFR targeting has one
    /// source of truth: stream PTS.
    pub fn decode_frame(&mut self, frame_number: u64) -> Result<Image, LibraryError> {
        if let Some(frame_count) = self.frame_count
            && frame_number >= frame_count
        {
            self.last_decode_stats = DecodeStats::default();
            return Err(LibraryError::VideoFrameOutOfRange {
                path: self.file_path.clone(),
                stream_index: self.video_stream_index,
                frame_number,
                frame_count,
            });
        }
        let source_time = frame_number as f64 / self.fps;
        self.decode_at_time(source_time)
            .map_err(|source| LibraryError::VideoFrameDecode {
                path: self.file_path.clone(),
                stream_index: self.video_stream_index,
                frame_number,
                source: Box::new(source),
            })
    }

    /// Decodes the frame displayed at `source_time` seconds. The target and
    /// all validity comparisons use the selected stream's time base and PTS;
    /// average FPS is metadata only and never an ordinal bound.
    pub fn decode_at_time(&mut self, source_time: f64) -> Result<Image, LibraryError> {
        let target_pts = self.target_pts_for_time(source_time)?;
        let mut stats = DecodeStats {
            target_pts,
            ..DecodeStats::default()
        };
        if self.target_is_past_known_end(target_pts) {
            self.last_decode_stats = stats;
            return Err(self.timestamp_out_of_range(source_time));
        }

        let result = self.decode_at_pts(target_pts, &mut stats);
        if result.is_ok() {
            stats.selected_pts = self.current_frame.as_ref().map(|frame| frame.pts);
        }
        self.last_decode_stats = stats;
        match result {
            Ok(()) => self.render_current_frame(),
            Err(error @ LibraryError::VideoTimestampOutOfRange { .. }) => Err(error),
            Err(source) => Err(LibraryError::VideoTimestampDecode {
                path: self.file_path.clone(),
                stream_index: self.video_stream_index,
                source_time,
                source: Box::new(source),
            }),
        }
    }

    pub fn target_pts_for_time(&self, source_time: f64) -> Result<i64, LibraryError> {
        if !source_time.is_finite() || source_time < 0.0 {
            return Err(LibraryError::FfmpegOther(format!(
                "Invalid video source time {source_time} for {:?}",
                self.file_path
            )));
        }
        let numerator = self.stream_time_base.numerator();
        let denominator = self.stream_time_base.denominator();
        if numerator <= 0 || denominator <= 0 {
            return Err(LibraryError::FfmpegOther(format!(
                "Invalid video stream time base {numerator}/{denominator} for {:?}",
                self.file_path
            )));
        }
        let ticks = source_time * f64::from(denominator) / f64::from(numerator);
        if !ticks.is_finite() || ticks > i64::MAX as f64 {
            return Err(LibraryError::FfmpegOther(format!(
                "Video source time {source_time} is too large for {:?}",
                self.file_path
            )));
        }
        Ok(self.stream_start_time.saturating_add(ticks.round() as i64))
    }

    fn stream_ticks_to_seconds(&self, ticks: i64) -> f64 {
        ticks as f64 * f64::from(self.stream_time_base.numerator())
            / f64::from(self.stream_time_base.denominator())
    }

    fn target_is_past_known_end(&self, target_pts: i64) -> bool {
        self.stream_duration
            .is_some_and(|duration| target_pts >= self.stream_start_time.saturating_add(duration))
    }

    fn timestamp_out_of_range(&self, source_time: f64) -> LibraryError {
        LibraryError::VideoTimestampOutOfRange {
            path: self.file_path.clone(),
            stream_index: self.video_stream_index,
            source_time,
            duration: self
                .stream_duration
                .map(|duration| self.stream_ticks_to_seconds(duration)),
        }
    }

    fn render_current_frame(&self) -> Result<Image, LibraryError> {
        let frame = &self
            .current_frame
            .as_ref()
            .ok_or_else(|| {
                LibraryError::FfmpegOther("Decoder produced no current video frame".to_string())
            })?
            .frame;
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
        scaler.run(frame, &mut rgba_frame)?;

        let width = rgba_frame.width();
        let height = rgba_frame.height();
        let row_bytes = (width * 4) as usize;
        let mut data = Vec::with_capacity(row_bytes * height as usize);
        let stride = rgba_frame.stride(0);
        let plane = rgba_frame.data(0);
        for y in 0..(height as usize) {
            let start = y * stride;
            let end = start + row_bytes;
            data.extend_from_slice(&plane[start..end]);
        }

        if let Some(processor) = &self.ocio_processor {
            data = processor.apply_rgba(&data);
        }

        Ok(Image::new(width, height, data))
    }

    fn decode_at_pts(
        &mut self,
        target_pts: i64,
        stats: &mut DecodeStats,
    ) -> Result<(), LibraryError> {
        if buffered_target_is_ready(
            target_pts,
            &mut self.current_frame,
            &mut self.lookahead_frame,
            self.decoder_eof,
        ) {
            self.last_target_pts = Some(target_pts);
            return Ok(());
        }

        let maximum_forward_gap = self
            .nominal_frame_duration
            .saturating_mul(MAX_FORWARD_DECODE_GAP_FRAMES);
        let need_seek = self.last_target_pts.is_none_or(|last_target| {
            target_pts < last_target || target_pts.saturating_sub(last_target) > maximum_forward_gap
        }) || self.current_frame.is_none();

        if need_seek {
            // `Input::seek` calls avformat_seek_file with stream_index = -1, so
            // the timestamp must use AV_TIME_BASE (microseconds), not the
            // selected stream's time base.
            let timestamp = self.global_seek_timestamp(target_pts);
            self.input_context.seek(timestamp, ..timestamp)?;
            self.decoder.flush();
            self.current_frame = None;
            self.lookahead_frame = None;
            self.decoder_eof = false;
            self.timestamp_normalizer.reset(target_pts);
            stats.seek_count += 1;
        } else if receive_until_target(
            &mut self.decoder,
            target_pts,
            &mut self.timestamp_normalizer,
            &mut self.current_frame,
            &mut self.lookahead_frame,
            self.stream_start_time,
            stats,
        )? {
            self.last_target_pts = Some(target_pts);
            return Ok(());
        }

        for (stream, packet) in self.input_context.packets() {
            if stream.index() != self.video_stream_index {
                continue;
            }

            stats.video_packets_read += 1;
            self.decoder.send_packet(&packet)?;
            if receive_until_target(
                &mut self.decoder,
                target_pts,
                &mut self.timestamp_normalizer,
                &mut self.current_frame,
                &mut self.lookahead_frame,
                self.stream_start_time,
                stats,
            )? {
                self.last_target_pts = Some(target_pts);
                return Ok(());
            }
        }

        if !self.decoder_eof {
            match self.decoder.send_eof() {
                Ok(()) | Err(ffmpeg::Error::Eof) => self.decoder_eof = true,
                Err(error) => return Err(error.into()),
            }
        }
        if receive_until_target(
            &mut self.decoder,
            target_pts,
            &mut self.timestamp_normalizer,
            &mut self.current_frame,
            &mut self.lookahead_frame,
            self.stream_start_time,
            stats,
        )? || buffered_target_is_ready(
            target_pts,
            &mut self.current_frame,
            &mut self.lookahead_frame,
            true,
        ) {
            self.last_target_pts = Some(target_pts);
            return Ok(());
        }

        Err(self.timestamp_out_of_range(
            self.stream_ticks_to_seconds(target_pts.saturating_sub(self.stream_start_time)),
        ))
    }

    fn global_seek_timestamp(&self, target_pts: i64) -> i64 {
        target_pts.rescale(
            self.stream_time_base,
            ffmpeg::util::mathematics::rescale::TIME_BASE,
        )
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
        collect_asset_metadata(&self.input_context)
    }
}

fn collect_asset_metadata(
    input_context: &ffmpeg::format::context::Input,
) -> Vec<crate::plugin::AssetMetadata> {
    let container_duration = (input_context.duration() != ffmpeg::ffi::AV_NOPTS_VALUE)
        .then(|| input_context.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64);
    input_context
        .streams()
        .filter_map(|stream| {
            let parameters = stream.parameters();
            let kind = match parameters.medium() {
                ffmpeg::media::Type::Video => crate::model::asset::AssetKind::Video,
                ffmpeg::media::Type::Audio => crate::model::asset::AssetKind::Audio,
                _ => return None,
            };
            let time_base = stream.time_base();
            let duration = if stream.duration() != ffmpeg::ffi::AV_NOPTS_VALUE
                && stream.duration() > 0
                && time_base.denominator() > 0
            {
                Some(
                    stream.duration() as f64 * f64::from(time_base.numerator())
                        / f64::from(time_base.denominator()),
                )
            } else {
                container_duration
            };

            let (fps, width, height, frame_count) = if kind == crate::model::asset::AssetKind::Video
            {
                let average = stream.avg_frame_rate();
                let fps = (average.numerator() > 0 && average.denominator() > 0)
                    .then(|| f64::from(average.numerator()) / f64::from(average.denominator()));
                let dimensions = ffmpeg::codec::context::Context::from_parameters(parameters)
                    .ok()
                    .and_then(|context| context.decoder().video().ok())
                    .map(|decoder| (decoder.width(), decoder.height()));
                (
                    fps,
                    dimensions.map(|value| value.0),
                    dimensions.map(|value| value.1),
                    u64::try_from(stream.frames())
                        .ok()
                        .filter(|frames| *frames > 0),
                )
            } else {
                (None, None, None, None)
            };

            Some(crate::plugin::AssetMetadata {
                kind,
                duration,
                fps,
                width,
                height,
                stream_index: Some(stream.index()),
                frame_count,
                time_base: Some((time_base.numerator(), time_base.denominator())),
            })
        })
        .collect()
}

fn receive_until_target(
    decoder: &mut ffmpeg::decoder::Video,
    target_pts: i64,
    timestamp_normalizer: &mut TimestampNormalizer,
    current_frame: &mut Option<BufferedFrame>,
    lookahead_frame: &mut Option<BufferedFrame>,
    stream_start_time: i64,
    stats: &mut DecodeStats,
) -> Result<bool, LibraryError> {
    loop {
        let mut frame = ffmpeg::util::frame::Video::empty();
        match decoder.receive_frame(&mut frame) {
            Ok(()) => {
                stats.frames_decoded += 1;
                let packet_duration = frame.packet().duration;
                let (pts, duration) = timestamp_normalizer.normalize(
                    frame.timestamp().or_else(|| frame.pts()),
                    packet_duration,
                    stats,
                );
                let decoded = BufferedFrame {
                    frame,
                    pts,
                    fallback_end_pts: pts.saturating_add(duration),
                };
                if accept_decoded_frame(
                    decoded,
                    target_pts,
                    stream_start_time,
                    current_frame,
                    lookahead_frame,
                )? {
                    return Ok(true);
                }
            }
            Err(ffmpeg::Error::Other {
                errno: ffmpeg::error::EAGAIN,
            })
            | Err(ffmpeg::Error::Eof) => return Ok(false),
            Err(error) => return Err(error.into()),
        }
    }
}

fn accept_decoded_frame(
    decoded: BufferedFrame,
    target_pts: i64,
    stream_start_time: i64,
    current_frame: &mut Option<BufferedFrame>,
    lookahead_frame: &mut Option<BufferedFrame>,
) -> Result<bool, LibraryError> {
    if decoded.pts <= target_pts {
        *current_frame = Some(decoded);
        return Ok(current_frame
            .as_ref()
            .is_some_and(|current| current.pts == target_pts));
    }

    if let Some(current) = current_frame.as_mut() {
        current.fallback_end_pts = decoded.pts;
        *lookahead_frame = Some(decoded);
    } else if may_use_first_frame_at_start(target_pts, stream_start_time) {
        // A stream may declare a start before its first displayable frame. At
        // that one boundary the first frame is the only meaningful result.
        *current_frame = Some(decoded);
    } else {
        return Err(LibraryError::FfmpegOther(format!(
            "Seek landed at PTS {} after requested PTS {target_pts}",
            decoded.pts
        )));
    }
    Ok(true)
}

fn may_use_first_frame_at_start(target_pts: i64, stream_start_time: i64) -> bool {
    target_pts == stream_start_time
}

fn buffered_target_is_ready(
    target_pts: i64,
    current_frame: &mut Option<BufferedFrame>,
    lookahead_frame: &mut Option<BufferedFrame>,
    decoder_eof: bool,
) -> bool {
    if lookahead_frame
        .as_ref()
        .is_some_and(|lookahead| target_pts >= lookahead.pts)
    {
        *current_frame = lookahead_frame.take();
    }
    let Some(current) = current_frame.as_ref() else {
        return false;
    };
    if target_pts < current.pts {
        return false;
    }
    if current.pts == target_pts {
        return true;
    }
    if let Some(lookahead) = lookahead_frame.as_ref() {
        return target_pts < lookahead.pts;
    }
    decoder_eof && target_pts < current.fallback_end_pts
}

// ============================================================================
// FfmpegVideoLoader Plugin
// ============================================================================

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) struct FileIdentity {
    canonical_path: PathBuf,
    length: u64,
    modified_nanos: u128,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    change_seconds: i64,
    #[cfg(unix)]
    change_nanos: i64,
}

impl FileIdentity {
    pub(crate) fn read(path: &str) -> Result<Self, LibraryError> {
        let canonical_path = Path::new(path).canonicalize()?;
        let metadata = std::fs::metadata(&canonical_path)?;
        let modified_nanos = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(Self {
                canonical_path,
                length: metadata.len(),
                modified_nanos,
                device: metadata.dev(),
                inode: metadata.ino(),
                change_seconds: metadata.ctime(),
                change_nanos: metadata.ctime_nsec(),
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {
                canonical_path,
                length: metadata.len(),
                modified_nanos,
            })
        }
    }

    pub(crate) fn cache_token(&self) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    pub(crate) fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct ReaderKey {
    identity: FileIdentity,
    stream_index: Option<usize>,
}

pub struct FfmpegVideoLoader {
    /// FFmpeg contexts are stateful and may not be used concurrently. The map
    /// lock protects only lookup/insertion; each reader has its own lock so
    /// unrelated files or streams can decode in parallel.
    readers: Mutex<LruCache<ReaderKey, Arc<Mutex<VideoReader>>>>,
}

impl Default for FfmpegVideoLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl FfmpegVideoLoader {
    pub fn new() -> Self {
        Self::with_reader_capacity(DEFAULT_READER_CACHE_SIZE)
    }

    #[doc(hidden)]
    pub fn with_reader_capacity(capacity: usize) -> Self {
        let capacity = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::MIN);
        Self {
            readers: Mutex::new(LruCache::new(capacity)),
        }
    }

    fn reader(
        &self,
        path: &str,
        stream_index: Option<usize>,
    ) -> Result<(ReaderKey, Arc<Mutex<VideoReader>>), LibraryError> {
        for _attempt in 0..3 {
            let identity = FileIdentity::read(path)?;
            let key = ReaderKey {
                identity: identity.clone(),
                stream_index,
            };
            {
                let mut readers = self.readers.lock().map_err(|_| {
                    LibraryError::Plugin("FFmpeg reader cache lock poisoned".to_string())
                })?;
                if let Some(reader) = readers.get(&key) {
                    return Ok((key, Arc::clone(reader)));
                }
            }

            // Opening FFmpeg can be relatively slow. Do it outside the map
            // lock, then verify that the file did not change during open.
            let canonical_path = identity.canonical_path.to_string_lossy().into_owned();
            let candidate = match VideoReader::new_with_stream(&canonical_path, stream_index) {
                Ok(reader) => Arc::new(Mutex::new(reader)),
                Err(error) => {
                    if FileIdentity::read(path)? != identity {
                        continue;
                    }
                    return Err(error);
                }
            };
            if FileIdentity::read(path)? != identity {
                continue;
            }

            let mut readers = self.readers.lock().map_err(|_| {
                LibraryError::Plugin("FFmpeg reader cache lock poisoned".to_string())
            })?;
            if let Some(reader) = readers.get(&key) {
                return Ok((key, Arc::clone(reader)));
            }
            let stale_keys = readers
                .iter()
                .filter(|(candidate_key, _)| {
                    candidate_key.stream_index == stream_index
                        && candidate_key.identity.canonical_path == identity.canonical_path
                        && candidate_key.identity != identity
                })
                .map(|(candidate_key, _)| candidate_key.clone())
                .collect::<Vec<_>>();
            for stale_key in stale_keys {
                readers.pop(&stale_key);
            }
            readers.put(key.clone(), Arc::clone(&candidate));
            return Ok((key, candidate));
        }
        Err(LibraryError::Plugin(format!(
            "Media file changed repeatedly while opening {path:?}"
        )))
    }

    fn has_cached_reader(&self, path: &str, stream_index: Option<usize>) -> bool {
        let Ok(identity) = FileIdentity::read(path) else {
            return false;
        };
        let key = ReaderKey {
            identity,
            stream_index,
        };
        self.readers
            .lock()
            .ok()
            .is_some_and(|readers| readers.peek(&key).is_some())
    }

    fn claim_video_path(&self, path: &str, stream_index: Option<usize>) -> LoadPluginResult<()> {
        if self.has_cached_reader(path, stream_index) {
            return Ok(());
        }
        initialize_ffmpeg_for_path(path)?;
        if has_registered_ffmpeg_media_extension(path) {
            return Ok(());
        }
        match ffmpeg::format::input(path) {
            Ok(input) => {
                drop(input);
                Ok(())
            }
            Err(_) => Err(LoadPluginError::Unsupported),
        }
    }

    fn cache_key(
        reader_key: &ReaderKey,
        input_color_space: Option<&str>,
        output_color_space: Option<&str>,
    ) -> String {
        format!(
            "{}\0identity={}\0stream={}\0input={}\0output={}",
            reader_key.identity.canonical_path.display(),
            reader_key.identity.cache_token(),
            reader_key
                .stream_index
                .map(|index| index.to_string())
                .unwrap_or_else(|| "best".to_string()),
            input_color_space.unwrap_or("none"),
            output_color_space.unwrap_or("none"),
        )
    }

    #[doc(hidden)]
    pub fn cached_reader_count(&self) -> usize {
        self.readers
            .lock()
            .map(|readers| readers.len())
            .unwrap_or_default()
    }

    #[doc(hidden)]
    pub fn reader_capacity(&self) -> usize {
        self.readers
            .lock()
            .map(|readers| readers.cap().get())
            .unwrap_or_default()
    }

    fn load_video_frame(
        &self,
        path: &str,
        source_time: f64,
        stream_index: Option<usize>,
        input_color_space: Option<&str>,
        output_color_space: Option<&str>,
        cache: &CacheManager,
    ) -> Result<LoadResponse, LibraryError> {
        for _attempt in 0..3 {
            let (reader_key, reader) = self.reader(path, stream_index)?;
            let cache_key = Self::cache_key(&reader_key, input_color_space, output_color_space);
            let mut reader = reader
                .lock()
                .map_err(|_| LibraryError::Plugin("FFmpeg reader lock poisoned".to_string()))?;
            let target_pts = reader.target_pts_for_time(source_time)?;

            if let Some(image) = cache.get_video_frame(&cache_key, target_pts) {
                drop(reader);
                if FileIdentity::read(path)? == reader_key.identity {
                    return Ok(LoadResponse { image });
                }
                continue;
            }

            match (input_color_space, output_color_space) {
                (Some(src), Some(dst)) => reader.set_color_space(src, dst),
                _ => reader.clear_color_space(),
            }
            let decoded = reader.decode_at_time(source_time);
            drop(reader);

            if FileIdentity::read(path)? != reader_key.identity {
                continue;
            }
            let image = decoded?;
            cache.put_video_frame(&cache_key, target_pts, &image);
            return Ok(LoadResponse { image });
        }
        Err(LibraryError::Plugin(format!(
            "Media file changed repeatedly while decoding {path:?}"
        )))
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

fn registered_ffmpeg_demuxer_extensions() -> &'static HashSet<String> {
    static EXTENSIONS: OnceLock<HashSet<String>> = OnceLock::new();
    EXTENSIONS.get_or_init(|| {
        let mut extensions = HashSet::new();
        let mut opaque: *mut c_void = std::ptr::null_mut();
        loop {
            // SAFETY: `opaque` starts null and is passed back only to
            // `av_demuxer_iterate`, as required by the FFmpeg iterator API.
            // The returned registry entry is static and is read before the
            // next iteration call.
            let input_format = unsafe { ffmpeg::ffi::av_demuxer_iterate(&mut opaque) };
            if input_format.is_null() {
                break;
            }
            // SAFETY: A non-null entry returned by `av_demuxer_iterate` points
            // to a registered `AVInputFormat` for the lifetime of libavformat.
            let extension_list = unsafe { (*input_format).extensions };
            if extension_list.is_null() {
                continue;
            }
            // SAFETY: `AVInputFormat.extensions` is either null or a
            // null-terminated, comma-separated string owned by libavformat.
            let extension_list = unsafe { CStr::from_ptr(extension_list) }.to_string_lossy();
            extensions.extend(
                extension_list
                    .split(',')
                    .map(str::trim)
                    .filter(|extension| !extension.is_empty())
                    .map(str::to_ascii_lowercase),
            );
        }
        extensions
    })
}

fn has_registered_ffmpeg_media_extension(path: &str) -> bool {
    let extension = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    extension.is_some_and(|extension| {
        registered_ffmpeg_demuxer_extensions().contains(extension.as_str())
    })
}

fn classify_ffmpeg_probe_failure(path: &str, error: LibraryError) -> LoadPluginError {
    if has_registered_ffmpeg_media_extension(path) {
        LoadPluginError::Failed(error)
    } else {
        LoadPluginError::Unsupported
    }
}

fn initialize_ffmpeg_for_path(path: &str) -> LoadPluginResult<()> {
    ffmpeg::init().map_err(|error| classify_ffmpeg_probe_failure(path, error.into()))
}

impl LoadPlugin for FfmpegVideoLoader {
    fn open(&self, path: &str) -> LoadPluginResult<Vec<crate::plugin::AssetMetadata>> {
        initialize_ffmpeg_for_path(path)?;
        let input_context = match ffmpeg::format::input(path) {
            Ok(input) => input,
            Err(error) => {
                return Err(classify_ffmpeg_probe_failure(path, error.into()));
            }
        };
        let streams = collect_asset_metadata(&input_context);

        if streams.is_empty() {
            return Err(LibraryError::Plugin("No video or audio stream".to_string()).into());
        }
        for stream in &streams {
            log::debug!(
                "FFmpeg stream metadata: path={:?}, stream={:?}, kind={:?}, duration={:?}, fps={:?}, time_base={:?}, frame_count={:?}, dimensions={:?}x{:?}",
                path,
                stream.stream_index,
                stream.kind,
                stream.duration,
                stream.fps,
                stream.time_base,
                stream.frame_count,
                stream.width,
                stream.height
            );
        }

        Ok(streams)
    }

    fn load(&self, request: &LoadRequest, cache: &CacheManager) -> LoadPluginResult<LoadResponse> {
        if let LoadRequest::VideoFrame {
            path,
            source_time,
            stream_index,
            input_color_space,
            output_color_space,
        } = request
        {
            self.claim_video_path(path, *stream_index)?;
            Ok(self.load_video_frame(
                path,
                *source_time,
                *stream_index,
                input_color_space.as_deref(),
                output_color_space.as_deref(),
                cache,
            )?)
        } else {
            Err(LoadPluginError::Unsupported)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_non_monotonic_timestamps_are_repaired_monotonically() {
        let mut normalizer = TimestampNormalizer::new(10);
        normalizer.reset(100);
        let mut stats = DecodeStats::default();
        assert_eq!(normalizer.normalize(Some(90), 10, &mut stats), (90, 10));
        assert_eq!(normalizer.normalize(None, 10, &mut stats), (100, 10));
        assert_eq!(normalizer.normalize(Some(95), 10, &mut stats), (110, 10));
        assert_eq!(stats.timestamp_repairs, 2);
    }

    #[test]
    fn a_seek_may_not_silently_substitute_a_future_frame() {
        assert!(may_use_first_frame_at_start(500, 500));
        assert!(!may_use_first_frame_at_start(501, 500));
        assert!(!may_use_first_frame_at_start(900, 500));
    }

    #[test]
    fn demuxer_registry_claims_a_format_missing_from_the_replaced_legacy_table()
    -> Result<(), Box<dyn std::error::Error>> {
        ffmpeg::init()?;
        assert!(
            registered_ffmpeg_demuxer_extensions().contains("nut"),
            "the linked FFmpeg registry must expose its standard NUT demuxer"
        );
        let path = std::env::temp_dir().join(format!(
            "ffmpeg-registry-routing-{}.nut",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&path, b"not a NUT stream")?;
        let result = FfmpegVideoLoader::new().open(&path.to_string_lossy());
        std::fs::remove_file(path)?;
        let Err(LoadPluginError::Failed(LibraryError::Ffmpeg(error))) = result else {
            return Err(std::io::Error::other(
                "a registry-known NUT path must preserve its concrete FFmpeg probe error",
            )
            .into());
        };
        assert!(!error.to_string().is_empty());
        Ok(())
    }

    #[test]
    fn initialization_failure_declines_unknown_extensions_but_claims_registered_ones() {
        let unknown = classify_ffmpeg_probe_failure(
            "/fixtures/runtime.rgba-fixture",
            LibraryError::Plugin("synthetic init failure".to_string()),
        );
        assert!(matches!(unknown, LoadPluginError::Unsupported));

        assert!(has_registered_ffmpeg_media_extension(
            "/fixtures/broken.mp4"
        ));
        let known = classify_ffmpeg_probe_failure(
            "/fixtures/broken.mp4",
            LibraryError::Plugin("synthetic init failure".to_string()),
        );
        assert!(matches!(known, LoadPluginError::Failed(_)));
    }

    #[test]
    fn valid_ffmpeg_content_with_an_unknown_extension_is_claimed_by_magic_probe()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../test_data/e2e_media/h264_24.mp4");
        let path = std::env::temp_dir().join(format!(
            "ffmpeg-renamed-video-{}.asset",
            uuid::Uuid::new_v4()
        ));
        std::fs::copy(fixture, &path)?;
        let path_text = path.to_string_lossy().into_owned();
        let loader = FfmpegVideoLoader::new();
        let streams = loader.open(&path_text)?;
        assert!(
            streams
                .iter()
                .any(|stream| { stream.kind == crate::model::asset::AssetKind::Video })
        );
        let loaded = loader.load(
            &LoadRequest::VideoFrame {
                path: path_text,
                source_time: 0.0,
                stream_index: None,
                input_color_space: None,
                output_color_space: None,
            },
            &CacheManager::new(),
        )?;
        assert!(loaded.image.width > 0);
        assert!(loaded.image.height > 0);
        std::fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn unknown_probe_failure_is_unsupported_but_registered_media_failure_is_concrete()
    -> Result<(), Box<dyn std::error::Error>> {
        let stem =
            std::env::temp_dir().join(format!("ffmpeg-probe-routing-{}", uuid::Uuid::new_v4()));
        let custom_path = stem.with_extension("rgba-fixture");
        let media_path = stem.with_extension("mp4");
        std::fs::write(&custom_path, b"not an ffmpeg container")?;
        std::fs::write(&media_path, b"not an ffmpeg container")?;
        let custom_path_text = custom_path.to_string_lossy().into_owned();
        let media_path_text = media_path.to_string_lossy().into_owned();
        let loader = FfmpegVideoLoader::new();
        assert!(matches!(
            loader.open(&custom_path_text),
            Err(LoadPluginError::Unsupported)
        ));
        assert!(matches!(
            loader.load(
                &LoadRequest::VideoFrame {
                    path: custom_path_text,
                    source_time: 0.0,
                    stream_index: None,
                    input_color_space: None,
                    output_color_space: None,
                },
                &CacheManager::new(),
            ),
            Err(LoadPluginError::Unsupported)
        ));
        let media_result = loader.open(&media_path_text);
        let media_load_result = loader.load(
            &LoadRequest::VideoFrame {
                path: media_path_text,
                source_time: 0.0,
                stream_index: None,
                input_color_space: None,
                output_color_space: None,
            },
            &CacheManager::new(),
        );
        std::fs::remove_file(custom_path)?;
        std::fs::remove_file(media_path)?;
        assert!(matches!(
            media_result,
            Err(LoadPluginError::Failed(LibraryError::Ffmpeg(_)))
        ));
        assert!(matches!(
            media_load_result,
            Err(LoadPluginError::Failed(LibraryError::Ffmpeg(_)))
        ));
        Ok(())
    }
}
