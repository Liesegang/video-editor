use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Consumer, RingBuffer};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const OUTPUT_DECLICK_FRAMES: usize = 64;

pub struct AudioEngine {
    _stream: cpal::Stream, // Keep stream alive
    producer: Arc<Mutex<rtrb::Producer<f32>>>,
    current_sample_count: Arc<AtomicU64>,
    playback_active: Arc<AtomicBool>,
    underrun_callbacks: Arc<AtomicU64>,
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

/// Callback-local output state. It owns no locks and never allocates after
/// stream construction, keeping underrun recovery safe for the device thread.
struct OutputCallbackState {
    channels: usize,
    last_frame: Vec<f32>,
    fade_origin: Vec<f32>,
    fade_out_remaining: usize,
    fade_in_position: usize,
    was_starved: bool,
}

impl OutputCallbackState {
    fn new(channels: usize) -> Self {
        Self {
            channels,
            last_frame: vec![0.0; channels],
            fade_origin: vec![0.0; channels],
            fade_out_remaining: 0,
            fade_in_position: 0,
            was_starved: true,
        }
    }

    fn reset(&mut self) {
        self.last_frame.fill(0.0);
        self.fade_origin.fill(0.0);
        self.fade_out_remaining = 0;
        self.fade_in_position = 0;
        self.was_starved = true;
    }

    fn write(
        &mut self,
        output: &mut [f32],
        consumer: &mut Consumer<f32>,
        playback_active: bool,
        counter: &AtomicU64,
        underrun_callbacks: &AtomicU64,
    ) {
        if self.channels == 0 {
            output.fill(0.0);
            return;
        }

        let mut consumed_frames = 0_u64;
        let mut starved = false;
        // Snapshot complete frames once. Samples produced during this callback
        // remain queued for the next device block; the real-time path avoids
        // an atomic tail refresh for every individual output frame.
        let mut available_frames = consumer.slots() / self.channels;
        let mut frames = output.chunks_exact_mut(self.channels);
        for frame in &mut frames {
            if available_frames > 0 {
                let crossfade_gain = if self.was_starved && self.fade_out_remaining > 0 {
                    Some(self.fade_out_remaining as f32 / OUTPUT_DECLICK_FRAMES as f32)
                } else {
                    if self.was_starved {
                        self.fade_in_position = 0;
                        self.was_starved = false;
                    }
                    None
                };
                for (channel, sample) in frame.iter_mut().enumerate() {
                    // `slots()` proved that a complete interleaved frame is
                    // present. The producer can only add data concurrently.
                    let value = consumer.pop().unwrap_or(0.0);
                    *sample = if let Some(old_gain) = crossfade_gain {
                        self.fade_origin[channel] * old_gain + value * (1.0 - old_gain)
                    } else {
                        let gain =
                            (self.fade_in_position as f32 / OUTPUT_DECLICK_FRAMES as f32).min(1.0);
                        value * gain
                    };
                    self.last_frame[channel] = *sample;
                }
                if crossfade_gain.is_some() {
                    self.fade_out_remaining = self.fade_out_remaining.saturating_sub(1);
                    if self.fade_out_remaining == 0 {
                        self.was_starved = false;
                        self.fade_in_position = OUTPUT_DECLICK_FRAMES;
                    }
                } else {
                    self.fade_in_position = (self.fade_in_position + 1).min(OUTPUT_DECLICK_FRAMES);
                }
                consumed_frames += 1;
                available_frames -= 1;
            } else {
                if !self.was_starved {
                    self.fade_origin.copy_from_slice(&self.last_frame);
                    self.fade_out_remaining = OUTPUT_DECLICK_FRAMES;
                    self.was_starved = true;
                }
                let gain = self.fade_out_remaining as f32 / OUTPUT_DECLICK_FRAMES as f32;
                for (channel, sample) in frame.iter_mut().enumerate() {
                    *sample = self.fade_origin[channel] * gain;
                    self.last_frame[channel] = *sample;
                }
                self.fade_out_remaining = self.fade_out_remaining.saturating_sub(1);
                starved = true;
            }
        }
        frames.into_remainder().fill(0.0);

        if playback_active {
            counter.fetch_add(consumed_frames, Ordering::Relaxed);
            if starved {
                underrun_callbacks.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

fn push_complete_frames(
    producer: &mut rtrb::Producer<f32>,
    samples: &[f32],
    channels: usize,
) -> usize {
    if channels == 0 {
        return 0;
    }
    // Snapshot the writable capacity before starting. The consumer may only
    // increase it concurrently, so this bound cannot fail mid-frame.
    let writable = producer.slots().min(samples.len());
    let writable = writable - writable % channels;
    if producer.push_entire_slice(&samples[..writable]).is_err() {
        return 0;
    }
    writable
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

        // Two seconds decouples the real-time device from an expensive video
        // frame. The UI pump fills it in bounded one-second installments.
        let buffer_size = (sample_rate as usize) * (channels as usize) * 2;
        let (producer, mut consumer) = RingBuffer::new(buffer_size);

        let current_sample_count = Arc::new(AtomicU64::new(0));
        let counter_clone = current_sample_count.clone();
        let playback_active = Arc::new(AtomicBool::new(false));
        let callback_playback_active = Arc::clone(&playback_active);
        let underrun_callbacks = Arc::new(AtomicU64::new(0));
        let callback_underruns = Arc::clone(&underrun_callbacks);

        let flush_state = Arc::new(AudioFlushState::default());
        let callback_flush_state = Arc::clone(&flush_state);
        let mut local_generation = 0;
        let mut output_state = OutputCallbackState::new(channels as usize);

        // This closure runs on the high-priority audio thread.
        // No IO, no locking (mostly), no expensive ops.
        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let global_gen = callback_flush_state.requested();
                if global_gen > local_generation {
                    // Seek detected: Flush buffer
                    while consumer.pop().is_ok() {}
                    output_state.reset();
                    local_generation = global_gen;
                    callback_flush_state.acknowledge(global_gen);
                }
                output_state.write(
                    data,
                    &mut consumer,
                    callback_playback_active.load(Ordering::Acquire),
                    &counter_clone,
                    &callback_underruns,
                );
            },
            |err| log::error!("Audio stream error: {}", err),
            None,
        )?;

        stream.play()?;

        Ok(Self {
            _stream: stream,
            producer: Arc::new(Mutex::new(producer)),
            current_sample_count,
            playback_active,
            underrun_callbacks,
            flush_state,
            sample_rate,
            channels,
        })
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

        push_complete_frames(&mut producer, samples, usize::from(self.channels))
    }

    pub fn get_current_time(&self) -> f64 {
        let samples = self.current_sample_count.load(Ordering::Relaxed);
        samples as f64 / self.sample_rate as f64
    }

    pub fn get_current_sample(&self) -> u64 {
        self.current_sample_count.load(Ordering::Acquire)
    }

    /// Number of real-time callbacks that could not obtain a complete source
    /// buffer while playback was active. Decoder failures are reported by the
    /// audio cache separately and do not increment this counter.
    pub fn underrun_callbacks(&self) -> u64 {
        self.underrun_callbacks.load(Ordering::Acquire)
    }

    // Playback control
    pub fn play(&self) -> Result<(), anyhow::Error> {
        // Stream remains active for scrubbing
        // self._stream.play()?;
        self.playback_active.store(true, Ordering::Release);
        Ok(())
    }

    pub fn pause(&self) -> Result<(), anyhow::Error> {
        // Stream remains active for scrubbing
        // self._stream.pause()?;
        self.playback_active.store(false, Ordering::Release);
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

    #[test]
    fn producer_never_commits_half_an_interleaved_frame() {
        let (mut producer, mut consumer) = RingBuffer::new(5);
        let samples = [1.0, 101.0, 2.0, 102.0, 3.0, 103.0];

        let first_write = push_complete_frames(&mut producer, &samples, 2);
        assert_eq!(first_write, 4);
        assert_eq!(consumer.slots(), 4);
        let first_block = (0..4).map(|_| consumer.pop().unwrap()).collect::<Vec<_>>();
        assert_eq!(first_block, samples[..4]);

        let second_write = push_complete_frames(&mut producer, &samples[first_write..], 2);
        assert_eq!(second_write, 2);
        assert_eq!(consumer.pop().unwrap(), 3.0);
        assert_eq!(consumer.pop().unwrap(), 103.0);
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn playback_clock_holds_at_the_producer_cursor_during_underrun() {
        let (mut producer, mut consumer) = RingBuffer::new(8);
        assert_eq!(
            push_complete_frames(&mut producer, &[1.0, -1.0, 2.0, -2.0], 2),
            4
        );
        let counter = AtomicU64::new(10);
        let underruns = AtomicU64::new(0);
        let mut state = OutputCallbackState::new(2);
        state.was_starved = false;
        state.fade_in_position = OUTPUT_DECLICK_FRAMES;

        let mut first_output = [0.0; 8];
        state.write(&mut first_output, &mut consumer, true, &counter, &underruns);
        assert_eq!(counter.load(Ordering::Acquire), 12);
        assert_eq!(underruns.load(Ordering::Acquire), 1);

        let mut empty_output = [f32::NAN; 8];
        state.write(&mut empty_output, &mut consumer, true, &counter, &underruns);
        assert_eq!(counter.load(Ordering::Acquire), 12);
        assert_eq!(underruns.load(Ordering::Acquire), 2);
        assert!(empty_output.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn paused_scrub_does_not_advance_or_report_realtime_playback() {
        let (mut producer, mut consumer) = RingBuffer::new(4);
        assert_eq!(push_complete_frames(&mut producer, &[0.25, -0.25], 2), 2);
        let counter = AtomicU64::new(50);
        let underruns = AtomicU64::new(7);
        let mut state = OutputCallbackState::new(2);
        let mut output = [0.0; 4];
        state.write(&mut output, &mut consumer, false, &counter, &underruns);
        assert_eq!(counter.load(Ordering::Acquire), 50);
        assert_eq!(underruns.load(Ordering::Acquire), 7);
    }

    #[test]
    fn underrun_recovery_is_declicked_without_touching_normal_blocks() {
        let (mut producer, mut consumer) = RingBuffer::new(256);
        let counter = AtomicU64::new(0);
        let underruns = AtomicU64::new(0);
        let mut state = OutputCallbackState::new(1);
        state.was_starved = false;
        state.fade_in_position = OUTPUT_DECLICK_FRAMES;

        assert_eq!(push_complete_frames(&mut producer, &[1.0; 4], 1), 4);
        let mut first = [0.0; 4 + OUTPUT_DECLICK_FRAMES];
        state.write(&mut first, &mut consumer, true, &counter, &underruns);
        assert_eq!(&first[..4], &[1.0; 4]);
        assert!(underruns.load(Ordering::Acquire) > 0);

        assert_eq!(push_complete_frames(&mut producer, &[1.0; 80], 1), 80);
        let mut recovery = [0.0; 80];
        state.write(&mut recovery, &mut consumer, true, &counter, &underruns);

        let output = first.into_iter().chain(recovery).collect::<Vec<_>>();
        let largest_step = output
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            largest_step <= 1.0 / OUTPUT_DECLICK_FRAMES as f32 + f32::EPSILON,
            "underrun introduced a {largest_step} full-scale discontinuity"
        );
        assert_eq!(recovery[OUTPUT_DECLICK_FRAMES], 1.0);
    }
}
