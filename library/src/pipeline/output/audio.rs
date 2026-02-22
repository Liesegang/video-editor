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
