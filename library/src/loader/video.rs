use crate::error::LibraryError;
use crate::loader::image::Image;
use crate::service::color_space_manager::{ColorSpaceManager, OcioProcessor};
use ffmpeg_next as ffmpeg;

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
        ffmpeg::init()?;

        let input_context = ffmpeg::format::input(&file_path)?;
        let input = input_context
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or(LibraryError::FfmpegOther(
                "動画ストリームが見つかりません".to_string(),
            ))?;
        let video_stream_index = input.index();

        let context_decoder = ffmpeg::codec::context::Context::from_parameters(input.parameters())?;
        let decoder = context_decoder.decoder().video()?;

        let stream = input_context
            .stream(video_stream_index)
            .ok_or(LibraryError::FfmpegOther(
                "ストリームが見つかりません".to_string(),
            ))?;
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
            // Duration is in AV_TIME_BASE units (microseconds)
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
        let stream =
            self.input_context
                .stream(self.video_stream_index)
                .ok_or(LibraryError::FfmpegOther(
                    "ストリームが見つかりません".to_string(),
                ))?;
        let time_base = stream.time_base();
        let fps = self.fps;

        // Determine if we need to seek
        let need_seek = match self.next_frame_number {
            Some(next) => next != frame_number,
            None => true,
        };
        if need_seek {
            let timestamp = (frame_number as f64 / fps / f64::from(time_base.numerator())
                * f64::from(time_base.denominator())) as i64;

            // Seek keyframe before the target (using a wide range backwards)
            // seeking to ..timestamp means "up to timestamp".
            // Often seeking to timestamp with backwards flag (which range implies if min is low) works.
            // Let's try seeking to exactly the timestamp but relying on ffmpeg to find the previous keyframe by default behavior for some formats,
            // or providing a range from 0 to timestamp.
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
                        // Update next frame expectation
                        self.next_frame_number = Some(frame_num + 1);
                        break;
                    }
                }
                if decoded_frame.is_some() {
                    break;
                }
            }
        }

        // If we still don't have a frame (e.g. EOF reached during search), check EOF frames
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

        // If we hit EOF but didn't find the exact frame (maybe it's past the end?), handling it gracefully would be good,
        // but for now let's error as before or return the last frame?
        // original logic errored.

        let frame = decoded_frame.ok_or(LibraryError::FfmpegOther(
            "指定したフレームをデコードできませんでした".to_string(),
        ))?;

        // Scaler setup
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

        // Apply OCIO transform if processor is set
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

#[allow(dead_code)]
pub fn decode_video_frame(file_path: &str, frame_number: u64) -> Result<Image, LibraryError> {
    let mut reader = VideoReader::new(file_path)?;
    reader.decode_frame(frame_number)
}

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
}
