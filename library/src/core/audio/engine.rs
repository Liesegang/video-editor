use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Consumer, RingBuffer};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub struct AudioEngine {
    _stream: cpal::Stream, // Keep stream alive
    producer: Arc<Mutex<rtrb::Producer<f32>>>,
    current_sample_count: Arc<AtomicU64>,
    generation: Arc<std::sync::atomic::AtomicUsize>,
    sample_rate: u32,
    channels: u16,
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
        let buffer_size = (sample_rate as usize) * (channels as usize) * 1;
        let (producer, mut consumer) = RingBuffer::new(buffer_size);

        let current_sample_count = Arc::new(AtomicU64::new(0));
        let counter_clone = current_sample_count.clone();

        let generation = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let generation_clone = generation.clone();
        let mut local_generation = 0;

        // This closure runs on the high-priority audio thread.
        // No IO, no locking (mostly), no expensive ops.
        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let global_gen = generation_clone.load(Ordering::Relaxed);
                if global_gen > local_generation {
                    // Seek detected: Flush buffer
                    while consumer.pop().is_ok() {}
                    local_generation = global_gen;
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
            generation,
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
        let mut producer = self.producer.lock().unwrap();
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
        self.generation.fetch_add(1, Ordering::Relaxed);
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
