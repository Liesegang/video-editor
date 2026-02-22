/// A chunk of interleaved audio samples.
#[derive(Clone, Debug)]
pub struct AudioChunk {
    /// Interleaved sample data (e.g. [L0, R0, L1, R1, ...] for stereo).
    pub samples: Vec<f32>,
    /// Sample rate in Hz (e.g. 44100, 48000).
    pub sample_rate: u32,
    /// Number of audio channels (1 = mono, 2 = stereo).
    pub channels: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_chunk_creation() {
        let chunk = AudioChunk {
            samples: vec![0.5, -0.5],
            sample_rate: 48000,
            channels: 1,
        };
        assert_eq!(chunk.samples, vec![0.5, -0.5]);
        assert_eq!(chunk.sample_rate, 48000);
        assert_eq!(chunk.channels, 1);
    }

    #[test]
    fn audio_chunk_clone() {
        let chunk = AudioChunk {
            samples: vec![1.0, 0.0, -1.0, 0.0],
            sample_rate: 44100,
            channels: 2,
        };
        let cloned = chunk.clone();
        assert_eq!(cloned.samples, chunk.samples);
        assert_eq!(cloned.sample_rate, chunk.sample_rate);
        assert_eq!(cloned.channels, chunk.channels);
    }

    #[test]
    fn audio_chunk_empty_samples() {
        let chunk = AudioChunk {
            samples: vec![],
            sample_rate: 44100,
            channels: 2,
        };
        assert!(chunk.samples.is_empty());
    }

    #[test]
    fn audio_chunk_debug_format() {
        let chunk = AudioChunk {
            samples: vec![0.1],
            sample_rate: 22050,
            channels: 1,
        };
        let dbg = format!("{:?}", chunk);
        assert!(dbg.contains("AudioChunk"));
        assert!(dbg.contains("22050"));
    }
}
