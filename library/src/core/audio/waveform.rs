//! Bounded, transient waveform summaries derived from decoded audio chunks.
//!
//! These values are a UI cache. They are never persisted in the authoritative
//! Project and cannot affect audio rendering.

use std::sync::Arc;

use super::cache::{AudioChunk, AudioChunkKey, AudioSourceKey};

pub(crate) const AUDIO_WAVEFORM_BUCKETS_PER_CHUNK: usize = 128;

#[derive(Clone, Debug)]
pub(crate) struct AudioWaveformChunk {
    key: AudioChunkKey,
    end_frame_exclusive: u64,
    peaks: Box<[f32]>,
}

impl AudioWaveformChunk {
    pub(crate) fn from_audio_chunk(chunk: &AudioChunk) -> Self {
        let key = chunk.key().clone();
        let capacity = key.frame_capacity().max(1);
        let channels = usize::from(key.source.format.channels).max(1);
        let mut peaks = vec![0.0_f32; AUDIO_WAVEFORM_BUCKETS_PER_CHUNK];
        for (frame_index, frame) in chunk.samples().chunks_exact(channels).enumerate() {
            let bucket = frame_index
                .saturating_mul(AUDIO_WAVEFORM_BUCKETS_PER_CHUNK)
                .checked_div(capacity)
                .unwrap_or_default()
                .min(AUDIO_WAVEFORM_BUCKETS_PER_CHUNK - 1);
            let peak = frame
                .iter()
                .copied()
                .filter(|sample| sample.is_finite())
                .map(f32::abs)
                .fold(0.0_f32, f32::max);
            peaks[bucket] = peaks[bucket].max(peak);
        }
        Self {
            key,
            end_frame_exclusive: chunk
                .key()
                .start_frame()
                .saturating_add(u64::try_from(chunk.frame_count()).unwrap_or(u64::MAX)),
            peaks: peaks.into_boxed_slice(),
        }
    }

    pub(crate) fn key(&self) -> &AudioChunkKey {
        &self.key
    }

    /// Return the maximum channel amplitude in an absolute, half-open source
    /// frame range. The range is clipped to this one-second summary.
    pub(crate) fn peak_between(&self, start_frame: u64, end_frame_exclusive: u64) -> Option<f32> {
        let chunk_start = self.key.start_frame();
        let chunk_end = self.end_frame_exclusive;
        let start = start_frame.max(chunk_start);
        let end = end_frame_exclusive.min(chunk_end);
        if start >= end {
            return None;
        }

        let capacity = self.key.frame_capacity().max(1);
        let relative_start = usize::try_from(start - chunk_start).ok()?;
        let relative_end = usize::try_from(end - chunk_start).ok()?;
        let first_bucket =
            relative_start.saturating_mul(AUDIO_WAVEFORM_BUCKETS_PER_CHUNK) / capacity;
        let final_bucket_exclusive = relative_end
            .saturating_mul(AUDIO_WAVEFORM_BUCKETS_PER_CHUNK)
            .div_ceil(capacity)
            .min(AUDIO_WAVEFORM_BUCKETS_PER_CHUNK);
        self.peaks
            .get(first_bucket..final_bucket_exclusive)
            .map(|peaks| peaks.iter().copied().fold(0.0_f32, f32::max))
    }
}

#[derive(Clone, Debug)]
pub struct AudioWaveformWindow {
    source: AudioSourceKey,
    chunks: Vec<Arc<AudioWaveformChunk>>,
    requested_chunks: usize,
    failed_chunks: usize,
    truncated: bool,
}

impl AudioWaveformWindow {
    pub(crate) fn new(
        source: AudioSourceKey,
        mut chunks: Vec<Arc<AudioWaveformChunk>>,
        requested_chunks: usize,
        failed_chunks: usize,
        truncated: bool,
    ) -> Self {
        chunks.sort_by_key(|chunk| chunk.key().chunk_index);
        Self {
            source,
            chunks,
            requested_chunks,
            failed_chunks,
            truncated,
        }
    }

    pub fn source(&self) -> &AudioSourceKey {
        &self.source
    }

    pub fn ready_chunks(&self) -> usize {
        self.chunks.len()
    }

    pub fn requested_chunks(&self) -> usize {
        self.requested_chunks
    }

    pub fn failed_chunks(&self) -> usize {
        self.failed_chunks
    }

    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    pub fn has_pending_chunks(&self) -> bool {
        self.ready_chunks().saturating_add(self.failed_chunks) < self.requested_chunks
    }

    pub fn is_settled(&self) -> bool {
        !self.has_pending_chunks()
    }

    pub fn is_complete(&self) -> bool {
        !self.truncated && self.is_settled()
    }

    pub fn is_ready(&self) -> bool {
        self.is_complete() && self.failed_chunks == 0
    }

    pub fn peak_between(&self, start_frame: u64, end_frame_exclusive: u64) -> Option<f32> {
        if start_frame >= end_frame_exclusive {
            return None;
        }
        let chunk_frames = self.source.format.chunk_frames().max(1);
        let first_chunk = start_frame / chunk_frames;
        let final_chunk = end_frame_exclusive.saturating_sub(1) / chunk_frames;
        let first_index = self
            .chunks
            .partition_point(|chunk| chunk.key().chunk_index < first_chunk);
        let final_index = self
            .chunks
            .partition_point(|chunk| chunk.key().chunk_index <= final_chunk);
        self.chunks[first_index..final_index]
            .iter()
            .filter_map(|chunk| chunk.peak_between(start_frame, end_frame_exclusive))
            .reduce(f32::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::audio::cache::{AudioDecodeFormat, AudioSourceKey};

    #[test]
    fn summary_maps_absolute_ranges_across_bucket_boundaries() {
        let path = std::env::temp_dir().join(format!("waveform-{}.raw", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"waveform identity").unwrap();
        let format = AudioDecodeFormat::new(128, 2).unwrap();
        let source = AudioSourceKey::read(&path, None, format).unwrap();
        let key = AudioChunkKey {
            source,
            chunk_index: 3,
        };
        let mut samples = vec![0.0; 256];
        samples[2 * 12 + 1] = -0.75;
        samples[2 * 96] = 0.25;
        let chunk = AudioChunk::new(key, samples).unwrap();
        let summary = AudioWaveformChunk::from_audio_chunk(&chunk);

        assert_eq!(summary.peak_between(3 * 128 + 12, 3 * 128 + 13), Some(0.75));
        assert_eq!(
            summary.peak_between(3 * 128 + 90, 3 * 128 + 100),
            Some(0.25)
        );
        assert_eq!(summary.peak_between(0, 128), None);
        drop(std::fs::remove_file(path));
    }

    #[test]
    fn final_partial_chunk_excludes_undecoded_capacity() {
        let path = std::env::temp_dir().join(format!("waveform-{}.raw", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"partial waveform identity").unwrap();
        let format = AudioDecodeFormat::new(8, 1).unwrap();
        let source = AudioSourceKey::read(&path, None, format).unwrap();
        let key = AudioChunkKey {
            source,
            chunk_index: 2,
        };
        let chunk = AudioChunk::new(key, vec![0.1, 0.2, -0.7]).unwrap();
        let summary = AudioWaveformChunk::from_audio_chunk(&chunk);

        assert_eq!(summary.peak_between(16, 19), Some(0.7));
        assert_eq!(summary.peak_between(18, 19), Some(0.7));
        assert_eq!(summary.peak_between(19, 24), None);
        drop(std::fs::remove_file(path));
    }

    #[test]
    fn window_peak_is_continuous_across_chunk_boundaries() {
        let path = std::env::temp_dir().join(format!("waveform-{}.raw", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"boundary waveform identity").unwrap();
        let format = AudioDecodeFormat::new(8, 1).unwrap();
        let source = AudioSourceKey::read(&path, None, format).unwrap();
        let first = AudioChunk::new(
            AudioChunkKey {
                source: source.clone(),
                chunk_index: 0,
            },
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.4],
        )
        .unwrap();
        let second = AudioChunk::new(
            AudioChunkKey {
                source: source.clone(),
                chunk_index: 1,
            },
            vec![0.9, 0.0, 0.0],
        )
        .unwrap();
        let window = AudioWaveformWindow::new(
            source,
            vec![
                Arc::new(AudioWaveformChunk::from_audio_chunk(&second)),
                Arc::new(AudioWaveformChunk::from_audio_chunk(&first)),
            ],
            2,
            0,
            false,
        );

        assert_eq!(window.peak_between(7, 9), Some(0.9));
        assert_eq!(window.peak_between(7, 8), Some(0.4));
        assert_eq!(window.peak_between(8, 9), Some(0.9));
        assert!(window.is_complete());
        drop(std::fs::remove_file(path));
    }

    #[test]
    fn failed_and_truncated_windows_settle_without_claiming_complete_coverage() {
        let path = std::env::temp_dir().join(format!("waveform-{}.raw", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"settled waveform identity").unwrap();
        let format = AudioDecodeFormat::new(8, 1).unwrap();
        let source = AudioSourceKey::read(&path, None, format).unwrap();

        let failed = AudioWaveformWindow::new(source.clone(), Vec::new(), 2, 2, false);
        assert!(!failed.has_pending_chunks());
        assert!(failed.is_settled());
        assert!(failed.is_complete());
        assert!(!failed.is_ready());
        assert_eq!(failed.peak_between(0, 16), None);

        let truncated = AudioWaveformWindow::new(source, Vec::new(), 0, 0, true);
        assert!(!truncated.has_pending_chunks());
        assert!(truncated.is_settled());
        assert!(!truncated.is_complete());
        assert!(!truncated.is_ready());
        drop(std::fs::remove_file(path));
    }
}
