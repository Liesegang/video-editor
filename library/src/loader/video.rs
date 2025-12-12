use crate::error::LibraryError;
use crate::loader::image::Image;
use ffmpeg_next as ffmpeg;

pub struct VideoReader {
    input_context: ffmpeg::format::context::Input,
    video_stream_index: usize,
    decoder: ffmpeg::decoder::Video,
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

        Ok(Self {
            input_context,
            video_stream_index,
            decoder,
        })
    }

    pub fn get_duration(&self) -> Option<f64> {
        if self.input_context.duration() == ffmpeg::ffi::AV_NOPTS_VALUE {
            None
        } else {
            // Duration is in AV_TIME_BASE units (microseconds)
            Some(self.input_context.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64)
        }
    }

    pub fn decode_frame(&mut self, frame_number: u64) -> Result<Image, LibraryError> {
        // Seek to the approximate position
        // Note: Seeking in ffmpeg is complex and might not land on the exact frame.
        // For simplicity, we might need to seek and then decode forward, or just decode from start if performance isn't an issue yet.
        // Given the previous implementation was linear scan from start for a single frame call (which is slow but correct),
        // we'll stick to a simple seek + decode loop, or just loop if seek is tricky.
        // But since this is a Reader struct now, we can optimize state later.
        // For now, let's re-implement the logic to find the frame.
        // CAUTION: Persistent state like `decoder` and `input_context` means we can't just seek casually without flushing buffers.

        // To properly seek, we should use self.input_context.seek().
        // However, exact frame seeking is hard. Let's try a naive seek to timestamp.

        let stream =
            self.input_context
                .stream(self.video_stream_index)
                .ok_or(LibraryError::FfmpegOther(
                    "ストリームが見つかりません".to_string(),
                ))?;
        let time_base = stream.time_base();
        // rough timestamp for the frame number. This assumes constant frame rate which might not be true.
        // But we don't have fps info easily here without more parsing.
        // Let's assume 30fps for calculation or use the stream info if available.
        let avg_frame_rate = stream.avg_frame_rate();
        let fps = if avg_frame_rate.denominator() > 0 {
            avg_frame_rate.numerator() as f64 / avg_frame_rate.denominator() as f64
        } else {
            30.0
        };

        let timestamp = (frame_number as f64 / fps / f64::from(time_base.numerator())
            * f64::from(time_base.denominator())) as i64;

        // Seek keyframe before the target
        self.input_context.seek(timestamp, ..timestamp)?;
        self.decoder.flush();

        let mut decoded_frame = None;
        // Removed unused current_frame_pts

        // We need to decode packets until we hit the right PTS.
        // This logic is simplified and might need robustness improvements for variable framerate.
        // For now, let's just decode next frames and check PTS.

        for (stream, packet) in self.input_context.packets() {
            if stream.index() == self.video_stream_index {
                self.decoder.send_packet(&packet)?;
                let mut frame = ffmpeg::util::frame::Video::empty();
                while self.decoder.receive_frame(&mut frame).is_ok() {
                    // Calculate frame number from PTS
                    let pts = frame.pts().unwrap_or(0);
                    let frame_num = (pts as f64 * f64::from(time_base.numerator())
                        / f64::from(time_base.denominator())
                        * fps)
                        .round() as u64;

                    if frame_num >= frame_number {
                        decoded_frame = Some(frame.clone());
                        break;
                    }
                }
                if decoded_frame.is_some() {
                    break;
                }
            }
        }

        // If we still don't have a frame, try EOF
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
                    break;
                }
            }
        }

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
