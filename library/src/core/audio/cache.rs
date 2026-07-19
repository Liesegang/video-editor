use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Audio is decoded and cached in one-second windows. Every resident entry is
/// therefore independently bounded even for multi-hour source files.
pub const AUDIO_CHUNK_SECONDS: u64 = 1;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct AudioFileIdentity {
    pub canonical_path: PathBuf,
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

impl AudioFileIdentity {
    pub fn read(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let canonical_path = path.as_ref().canonicalize()?;
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
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct AudioDecodeFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

impl AudioDecodeFormat {
    pub fn new(sample_rate: u32, channels: u16) -> Option<Self> {
        (sample_rate > 0 && channels > 0).then_some(Self {
            sample_rate,
            channels,
        })
    }

    pub fn chunk_frames(self) -> u64 {
        u64::from(self.sample_rate).saturating_mul(AUDIO_CHUNK_SECONDS)
    }
}

/// Stable identity for one decoded audio representation.
///
/// `stream_index` is the persisted, zero-based global container stream index,
/// not an ordinal among audio-only streams and not a decoder-specific Track ID.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct AudioSourceKey {
    pub identity: AudioFileIdentity,
    pub stream_index: Option<usize>,
    pub format: AudioDecodeFormat,
}

impl AudioSourceKey {
    pub fn read(
        path: impl AsRef<Path>,
        stream_index: Option<usize>,
        format: AudioDecodeFormat,
    ) -> std::io::Result<Self> {
        Ok(Self {
            identity: AudioFileIdentity::read(path)?,
            stream_index,
            format,
        })
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct AudioChunkKey {
    pub source: AudioSourceKey,
    pub chunk_index: u64,
}

impl AudioChunkKey {
    pub fn containing(source: AudioSourceKey, frame: u64) -> Self {
        let chunk_frames = source.format.chunk_frames().max(1);
        Self {
            source,
            chunk_index: frame / chunk_frames,
        }
    }

    pub fn start_frame(&self) -> u64 {
        self.chunk_index
            .saturating_mul(self.source.format.chunk_frames())
    }

    pub fn frame_capacity(&self) -> usize {
        usize::try_from(self.source.format.chunk_frames()).unwrap_or(usize::MAX)
    }
}

#[derive(Clone, Debug)]
pub struct AudioChunk {
    key: AudioChunkKey,
    /// Interleaved samples in `key.source.format` beginning at
    /// `key.start_frame()`.
    samples: Vec<f32>,
}

impl AudioChunk {
    pub fn new(key: AudioChunkKey, samples: Vec<f32>) -> Result<Self, String> {
        let channels = usize::from(key.source.format.channels);
        if !samples.len().is_multiple_of(channels) {
            return Err("audio chunk samples must contain complete interleaved frames".to_string());
        }
        let maximum_samples = key.frame_capacity().saturating_mul(channels);
        if samples.len() > maximum_samples {
            return Err(format!(
                "audio chunk has {} samples but its bounded capacity is {maximum_samples}",
                samples.len()
            ));
        }
        Ok(Self { key, samples })
    }

    pub fn key(&self) -> &AudioChunkKey {
        &self.key
    }

    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    pub fn frame_count(&self) -> usize {
        self.samples.len() / usize::from(self.key.source.format.channels)
    }

    pub fn sample(&self, absolute_frame: u64, channel: usize) -> Option<f32> {
        let relative_frame = absolute_frame.checked_sub(self.key.start_frame())?;
        let relative_frame = usize::try_from(relative_frame).ok()?;
        let channels = usize::from(self.key.source.format.channels);
        self.samples
            .get(relative_frame.checked_mul(channels)?.checked_add(channel)?)
            .copied()
    }
}
