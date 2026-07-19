use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Consumer, RingBuffer};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub struct AudioEngine {
    _stream: cpal::Stream, // Keep stream alive
    producer: Arc<Mutex<rtrb::Producer<f32>>>,
    current_sample_count: Arc<AtomicU64>,
    flush_state: Arc<AudioFlushState>,
    sample_rate: u32,
    channels: u16,
}

#[derive(Clone)]
pub(crate) struct AudioFlushHandle {
    state: Arc<AudioFlushState>,
}

impl AudioFlushHandle {
    pub(crate) fn request(&self) {
        self.state.request();
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self {
            state: Arc::new(AudioFlushState::default()),
        }
    }

    #[cfg(test)]
    pub(crate) fn pending(&self) -> bool {
        self.state.pending()
    }
}

#[derive(Default)]
struct AudioFlushState {
    requested: std::sync::atomic::AtomicUsize,
    acknowledged: std::sync::atomic::AtomicUsize,
}

impl AudioFlushState {
    fn request(&self) -> usize {
        self.requested.fetch_add(1, Ordering::AcqRel) + 1
    }

    fn requested(&self) -> usize {
        self.requested.load(Ordering::Acquire)
    }

    fn acknowledge(&self, generation: usize) {
        self.acknowledged.store(generation, Ordering::Release);
    }

    fn pending(&self) -> bool {
        self.requested.load(Ordering::Acquire) != self.acknowledged.load(Ordering::Acquire)
    }
}

impl AudioEngine {
    pub fn new() -> Result<Self, anyhow::Error> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("No default audio output device available"))?;

        let config: cpal::StreamConfig = device.default_output_config()?.into();

        let sample_rate = config.sample_rate.0;
        let channels = config.channels;

        // Create RingBuffer (Wait-free SPSC)
        // Capacity: 1 second buffer (approx)
        let buffer_size = (sample_rate as usize) * (channels as usize);
        let (producer, mut consumer) = RingBuffer::new(buffer_size);

        let current_sample_count = Arc::new(AtomicU64::new(0));
        let counter_clone = current_sample_count.clone();

        let flush_state = Arc::new(AudioFlushState::default());
        let callback_flush_state = Arc::clone(&flush_state);
        let mut local_generation = 0;

        // This closure runs on the high-priority audio thread.
        // No IO, no locking (mostly), no expensive ops.
        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let global_gen = callback_flush_state.requested();
                if global_gen > local_generation {
                    // Seek detected: Flush buffer
                    while consumer.pop().is_ok() {}
                    local_generation = global_gen;
                    callback_flush_state.acknowledge(global_gen);
                }
                Self::write_audio_data(data, channels as usize, &mut consumer, &counter_clone);
            },
            |err| log::error!("Audio stream error: {}", err),
            None,
        )?;

        stream.play()?;

        Ok(Self {
            _stream: stream,
            producer: Arc::new(Mutex::new(producer)),
            current_sample_count,
            flush_state,
            sample_rate,
            channels,
        })
    }

    fn write_audio_data(
        output: &mut [f32],
        channels: usize,
        consumer: &mut Consumer<f32>,
        counter: &AtomicU64,
    ) {
        // Fill the output buffer with data from the ring buffer
        // Or silence if empty
        let mut frames_written = 0;

        for frame in output.chunks_mut(channels) {
            for sample in frame.iter_mut() {
                if let Ok(value) = consumer.pop() {
                    *sample = value;
                } else {
                    *sample = 0.0;
                }
            }

            // Only advance time if we successfully popped a full frame?
            // Actually, audio device TIME advances regardless of whether we have data.
            // "Current Time" should be "how much we have played".
            // So we always increment.

            frames_written += 1;
        }

        counter.fetch_add(frames_written as u64, Ordering::Relaxed);
    }

    pub fn get_sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn get_channels(&self) -> u16 {
        self.channels
    }

    // "Main Thread" API to feed data
    pub fn push_samples(&self, samples: &[f32]) -> usize {
        if self.flush_state.pending() {
            return 0;
        }
        let mut producer = self.producer.lock().unwrap_or_else(|poisoned| {
            log::error!("audio producer lock was poisoned; recovering buffered samples");
            poisoned.into_inner()
        });
        if self.flush_state.pending() {
            return 0;
        }
        // Since Producer is SPSC, we need to lock if multiple writers, but we should have one AssetWorker.
        // We use Mutex here just for safety in 'library' context.

        let mut written = 0;
        for &sample in samples {
            if producer.push(sample).is_ok() {
                written += 1;
            } else {
                break;
            }
        }
        written
    }

    pub fn get_current_time(&self) -> f64 {
        let samples = self.current_sample_count.load(Ordering::Relaxed);
        samples as f64 / self.sample_rate as f64
    }

    pub fn get_current_sample(&self) -> u64 {
        self.current_sample_count.load(Ordering::Acquire)
    }

    // Playback control
    pub fn play(&self) -> Result<(), anyhow::Error> {
        // Stream remains active for scrubbing
        // self._stream.play()?;
        Ok(())
    }

    pub fn pause(&self) -> Result<(), anyhow::Error> {
        // Stream remains active for scrubbing
        // self._stream.pause()?;
        Ok(())
    }

    pub fn set_time(&self, time: f64) {
        let samples = (time * self.sample_rate as f64).round() as u64;
        self.current_sample_count.store(samples, Ordering::Relaxed);

        // Signal flush to clear old buffered audio
        self.flush();
    }

    pub fn flush(&self) {
        self.flush_state.request();
    }

    pub fn flush_pending(&self) -> bool {
        self.flush_state.pending()
    }

    pub(crate) fn flush_handle(&self) -> AudioFlushHandle {
        AudioFlushHandle {
            state: Arc::clone(&self.flush_state),
        }
    }

    pub fn free_capacity(&self) -> usize {
        if let Ok(producer) = self.producer.lock() {
            producer.slots()
        } else {
            0
        }
    }

    pub fn available_slots(&self) -> usize {
        if let Ok(producer) = self.producer.lock() {
            producer.slots()
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_remains_pending_until_the_audio_callback_acknowledges_it() {
        let state = AudioFlushState::default();
        assert!(!state.pending());

        let first = state.request();
        assert!(state.pending());
        state.acknowledge(first.saturating_sub(1));
        assert!(state.pending());
        state.acknowledge(first);
        assert!(!state.pending());

        let second = state.request();
        assert!(state.pending());
        assert!(second > first);
        state.acknowledge(second);
        assert!(!state.pending());
    }
}
