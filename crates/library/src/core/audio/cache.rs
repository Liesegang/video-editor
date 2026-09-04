use crate::util::local_file::DirectRegularFile;
#[cfg(windows)]
use crate::util::local_file::WindowsFileIdentity;
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
    #[cfg(windows)]
    windows_identity: WindowsFileIdentity,
}

impl AudioFileIdentity {
    pub fn read(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let opened = DirectRegularFile::open(path)?;
        Self::from_opened(&opened)
    }

    pub(crate) fn from_opened(opened: &DirectRegularFile) -> std::io::Result<Self> {
        let canonical_path = opened.canonical_path().to_path_buf();
        let metadata = opened.file().metadata()?;
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
                #[cfg(windows)]
                windows_identity: opened.windows_identity()?,
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
    /// Frames backed by decoded source data. The allocation remains a fixed
    /// one-second window, but padding beyond decoder EOF is not a source
    /// handle and must be distinguishable from authored digital silence.
    valid_frame_range: std::ops::Range<usize>,
}

impl AudioChunk {
    pub fn new(key: AudioChunkKey, samples: Vec<f32>) -> Result<Self, String> {
        let channels = usize::from(key.source.format.channels);
        if channels == 0 {
            return Err("audio chunk format must have at least one channel".to_string());
        }
        let frame_count = samples.len() / channels;
        Self::new_with_valid_frame_range(key, samples, 0..frame_count)
    }

    pub(crate) fn new_with_valid_frame_range(
        key: AudioChunkKey,
        samples: Vec<f32>,
        valid_frame_range: std::ops::Range<usize>,
    ) -> Result<Self, String> {
        let channels = usize::from(key.source.format.channels);
        if channels == 0 {
            return Err("audio chunk format must have at least one channel".to_string());
        }
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
        let frame_count = samples.len() / channels;
        if valid_frame_range.start > valid_frame_range.end || valid_frame_range.end > frame_count {
            return Err("audio chunk valid source range exceeds its samples".to_string());
        }
        Ok(Self {
            key,
            samples,
            valid_frame_range,
        })
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
        if !self.valid_frame_range.contains(&relative_frame) {
            return None;
        }
        let channels = usize::from(self.key.source.format.channels);
        self.samples
            .get(relative_frame.checked_mul(channels)?.checked_add(channel)?)
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_format() -> Result<AudioDecodeFormat, std::io::Error> {
        AudioDecodeFormat::new(48_000, 2)
            .ok_or_else(|| std::io::Error::other("valid audio format was rejected"))
    }

    #[test]
    fn regular_local_file_becomes_an_audio_source_but_url_does_not()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let regular = directory.path().join("fixture.wav");
        std::fs::write(&regular, b"regular audio identity fixture")?;

        assert!(AudioSourceKey::read(&regular, None, test_format()?).is_ok());
        let error = AudioSourceKey::read(
            "https://example.invalid/document-controlled.wav",
            None,
            test_format()?,
        )
        .err()
        .ok_or_else(|| std::io::Error::other("URL became an automatic audio source"))?;
        assert!(error.to_string().contains("URL and URI-scheme"));
        Ok(())
    }

    #[test]
    fn decoded_silence_is_distinct_from_padding_beyond_source_eof()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("silence.wav");
        std::fs::write(&path, b"stable identity only")?;
        let key = AudioChunkKey {
            source: AudioSourceKey::read(&path, None, test_format()?)?,
            chunk_index: 0,
        };
        let chunk = AudioChunk::new_with_valid_frame_range(key, vec![0.0; 4], 0..1)?;

        assert_eq!(chunk.sample(0, 0), Some(0.0));
        assert_eq!(chunk.sample(0, 1), Some(0.0));
        assert_eq!(chunk.sample(1, 0), None);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symlink_never_becomes_an_audio_source_key() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir()?;
        let regular = directory.path().join("fixture.wav");
        let link = directory.path().join("linked.wav");
        std::fs::write(&regular, b"regular audio identity fixture")?;
        symlink(&regular, &link)?;

        let error = AudioSourceKey::read(&link, None, test_format()?)
            .err()
            .ok_or_else(|| std::io::Error::other("symlink became an automatic audio source"))?;
        assert!(error.to_string().contains("symbolic links"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn fifo_never_becomes_a_waveform_or_playback_source_key()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let directory = tempfile::tempdir()?;
        let fifo = directory.path().join("document-controlled.wav");
        let fifo_path = CString::new(fifo.as_os_str().as_bytes())?;
        // SAFETY: `fifo_path` is a live NUL-terminated path and `mkfifo` does
        // not retain the pointer after returning.
        let status = unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) };
        if status != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let format = test_format()?;

        let error = AudioSourceKey::read(&fifo, None, format)
            .err()
            .ok_or_else(|| std::io::Error::other("FIFO became an automatic audio source"))?;

        assert!(error.to_string().contains("FIFOs"));
        Ok(())
    }
}
