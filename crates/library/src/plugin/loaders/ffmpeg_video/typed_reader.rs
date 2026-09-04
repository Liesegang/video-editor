//! Typed public decode surface for stateful `VideoReader` users.
//!
//! The legacy `Image` methods can represent only RGBA8. This surface preserves
//! the loader's actual RGBA32F YUV result and requires callers to supply the
//! same source-color authority used by the Project-owned loader request.

use super::VideoReader;
use crate::error::LibraryError;
use crate::model::asset::DecoderSourceColorAuthority;
use crate::plugin::LoadResponse;

impl VideoReader {
    pub fn decode_frame_typed(
        &mut self,
        frame_number: u64,
        source_color_authority: Option<&DecoderSourceColorAuthority>,
    ) -> Result<LoadResponse, LibraryError> {
        if let Some(frame_count) = self.frame_count
            && frame_number >= frame_count
        {
            self.last_decode_stats = super::DecodeStats::default();
            return Err(LibraryError::VideoFrameOutOfRange {
                path: self.file_path.clone(),
                stream_index: self.video_stream_index,
                frame_number,
                frame_count,
            });
        }
        let source_time = frame_number as f64 / self.fps;
        self.decode_at_time_typed(source_time, source_color_authority)
            .map_err(|source| LibraryError::VideoFrameDecode {
                path: self.file_path.clone(),
                stream_index: self.video_stream_index,
                frame_number,
                source: Box::new(source),
            })
    }

    pub fn decode_at_time_typed(
        &mut self,
        source_time: f64,
        source_color_authority: Option<&DecoderSourceColorAuthority>,
    ) -> Result<LoadResponse, LibraryError> {
        self.decode_at_time_with_description(source_time, source_color_authority)
            .map(|decoded| LoadResponse::new(decoded.pixels, decoded.decoded))
    }
}
