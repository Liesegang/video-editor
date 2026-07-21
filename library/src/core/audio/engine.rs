use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Consumer, RingBuffer};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const OUTPUT_DECLICK_FRAMES: usize = 64;
const CLOCK_GENERATION_BITS: u32 = 16;
const CLOCK_SAMPLE_BITS: u32 = u64::BITS - CLOCK_GENERATION_BITS;
const CLOCK_SAMPLE_MASK: u64 = (1_u64 << CLOCK_SAMPLE_BITS) - 1;
const CLOCK_GENERATION_MASK: u64 = (1_u64 << CLOCK_GENERATION_BITS) - 1;

#[derive(Clone, Copy, Debug, Default)]
struct QueuedSample {
    generation: usize,
    value: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AudioOutputGeneration(usize);

fn packed_clock(generation: usize, sample: u64) -> u64 {
    let generation = u64::try_from(generation).unwrap_or(u64::MAX) & CLOCK_GENERATION_MASK;
    (generation << CLOCK_SAMPLE_BITS) | sample.min(CLOCK_SAMPLE_MASK)
}

fn clock_sample(clock: u64) -> u64 {
    clock & CLOCK_SAMPLE_MASK
}

fn clock_generation(clock: u64) -> u64 {
    clock >> CLOCK_SAMPLE_BITS
}

fn advance_clock(clock: &AtomicU64, generation: usize, frames: u64) -> bool {
    let expected_generation = u64::try_from(generation).unwrap_or(u64::MAX) & CLOCK_GENERATION_MASK;
    let mut current = clock.load(Ordering::Acquire);
    loop {
        if clock_generation(current) != expected_generation {
            return false;
        }
        let advanced = packed_clock(generation, clock_sample(current).saturating_add(frames));
        match clock.compare_exchange_weak(current, advanced, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return true,
            Err(observed) => current = observed,
        }
    }
}

fn rebase_clock(clock: &AtomicU64, generation: usize, target: Option<u64>) {
    if let Some(target) = target {
        clock.store(packed_clock(generation, target), Ordering::Release);
        return;
    }

    let mut current = clock.load(Ordering::Acquire);
    loop {
        let rebased = packed_clock(generation, clock_sample(current));
        match clock.compare_exchange_weak(current, rebased, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return,
            Err(observed) => current = observed,
        }
    }
}

pub struct AudioEngine {
    _stream: cpal::Stream, // Keep stream alive
    producer: Arc<Mutex<rtrb::Producer<QueuedSample>>>,
    playback_clock: Arc<AtomicU64>,
    playback_active: Arc<AtomicBool>,
    underrun_callbacks: Arc<AtomicU64>,
    flush_state: Arc<AudioFlushState>,
    sample_rate: u32,
    channels: u16,
}

#[derive(Clone)]
pub(crate) struct AudioFlushHandle {
    state: Arc<AudioFlushState>,
    playback_clock: Arc<AtomicU64>,
}

impl AudioFlushHandle {
    pub(crate) fn request(&self) {
        let generation = self.state.request();
        rebase_clock(&self.playback_clock, generation, None);
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self {
            state: Arc::new(AudioFlushState::default()),
            playback_clock: Arc::new(AtomicU64::new(0)),
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

    fn acknowledged(&self) -> usize {
        self.acknowledged.load(Ordering::Acquire)
    }

    fn acknowledge(&self, generation: usize) {
        self.acknowledged.store(generation, Ordering::Release);
    }

    fn pending(&self) -> bool {
        self.requested() != self.acknowledged()
    }

    fn ready(&self, generation: AudioOutputGeneration) -> bool {
        self.requested() == generation.0 && self.acknowledged() == generation.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CallbackWriteResult {
    consumed_frames: u64,
    discarded_stale_frames: u64,
    starved: bool,
}

fn record_callback_result(
    playback_active: bool,
    playback_clock: &AtomicU64,
    generation: usize,
    underrun_callbacks: &AtomicU64,
    result: CallbackWriteResult,
) {
    if !playback_active {
        return;
    }
    advance_clock(playback_clock, generation, result.consumed_frames);
    if result.starved {
        underrun_callbacks.fetch_add(1, Ordering::Relaxed);
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
        consumer: &mut Consumer<QueuedSample>,
        generation: usize,
    ) -> CallbackWriteResult {
        if self.channels == 0 {
            output.fill(0.0);
            return CallbackWriteResult::default();
        }

        let mut result = CallbackWriteResult::default();
        // Snapshot complete frames once. Samples produced during this callback
        // remain queued for the next device block; the real-time path avoids
        // an atomic tail refresh for every individual output frame.
        let mut available_frames = consumer.slots() / self.channels;
        let mut frames = output.chunks_exact_mut(self.channels);
        for frame in &mut frames {
            let mut has_current_source = false;
            while available_frames > 0 && !has_current_source {
                let mut frame_generation = None;
                let mut complete = true;
                for sample in frame.iter_mut() {
                    let Ok(queued) = consumer.pop() else {
                        complete = false;
                        break;
                    };
                    *sample = queued.value;
                    frame_generation.get_or_insert(queued.generation);
                    complete &= frame_generation == Some(queued.generation);
                }
                available_frames -= 1;
                has_current_source = complete && frame_generation == Some(generation);
                if !has_current_source {
                    result.discarded_stale_frames += 1;
                }
            }

            if has_current_source {
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
                    let value = *sample;
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
                result.consumed_frames += 1;
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
                result.starved = true;
            }
        }
        frames.into_remainder().fill(0.0);
        result
    }
}

fn push_complete_frames(
    producer: &mut rtrb::Producer<QueuedSample>,
    samples: &[f32],
    channels: usize,
    generation: AudioOutputGeneration,
) -> usize {
    if channels == 0 {
        return 0;
    }
    // Snapshot the writable capacity before starting. The consumer may only
    // increase it concurrently, so this bound cannot fail mid-frame.
    let writable = producer.slots().min(samples.len());
    let writable = writable - writable % channels;
    let Ok(mut chunk) = producer.write_chunk(writable) else {
        return 0;
    };
    let (first, second) = chunk.as_mut_slices();
    for (queued, value) in first
        .iter_mut()
        .chain(second)
        .zip(samples[..writable].iter().copied())
    {
        *queued = QueuedSample {
            generation: generation.0,
            value,
        };
    }
    chunk.commit_all();
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

        let playback_clock = Arc::new(AtomicU64::new(packed_clock(0, 0)));
        let callback_clock = Arc::clone(&playback_clock);
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
                let result = output_state.write(data, &mut consumer, local_generation);
                record_callback_result(
                    callback_playback_active.load(Ordering::Acquire),
                    &callback_clock,
                    local_generation,
                    &callback_underruns,
                    result,
                );
            },
            |err| log::error!("Audio stream error: {}", err),
            None,
        )?;

        stream.play()?;

        Ok(Self {
            _stream: stream,
            producer: Arc::new(Mutex::new(producer)),
            playback_clock,
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

    pub(crate) fn output_generation(&self) -> Option<AudioOutputGeneration> {
        let generation = AudioOutputGeneration(self.flush_state.requested());
        self.flush_state.ready(generation).then_some(generation)
    }

    // Main-thread API to feed a block mixed for one exact output generation.
    pub(crate) fn push_samples(&self, samples: &[f32], generation: AudioOutputGeneration) -> usize {
        if !self.flush_state.ready(generation) {
            return 0;
        }
        let mut producer = self.producer.lock().unwrap_or_else(|poisoned| {
            log::error!("audio producer lock was poisoned; recovering buffered samples");
            poisoned.into_inner()
        });
        if !self.flush_state.ready(generation) {
            return 0;
        }
        // Since Producer is SPSC, we need to lock if multiple writers, but we should have one AssetWorker.
        // We use Mutex here just for safety in 'library' context.

        let written = push_complete_frames(
            &mut producer,
            samples,
            usize::from(self.channels),
            generation,
        );
        // A concurrent seek after the pre-write check leaves only tagged stale
        // frames. The callback drops them without advancing the clock.
        if self.flush_state.ready(generation) {
            written
        } else {
            0
        }
    }

    pub fn get_current_time(&self) -> f64 {
        let samples = self.get_current_sample();
        samples as f64 / self.sample_rate as f64
    }

    pub fn get_current_sample(&self) -> u64 {
        clock_sample(self.playback_clock.load(Ordering::Acquire))
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
        // Publishing a new generation and target in the same atomic clock
        // prevents a callback from adding old-generation frames after a seek.
        let generation = self.flush_state.request();
        rebase_clock(&self.playback_clock, generation, Some(samples));
    }

    pub fn flush(&self) {
        let generation = self.flush_state.request();
        rebase_clock(&self.playback_clock, generation, None);
    }

    pub fn flush_pending(&self) -> bool {
        self.flush_state.pending()
    }

    pub(crate) fn flush_handle(&self) -> AudioFlushHandle {
        AudioFlushHandle {
            state: Arc::clone(&self.flush_state),
            playback_clock: Arc::clone(&self.playback_clock),
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
        let generation = AudioOutputGeneration(3);

        let first_write = push_complete_frames(&mut producer, &samples, 2, generation);
        assert_eq!(first_write, 4);
        assert_eq!(consumer.slots(), 4);
        let first_block = (0..4)
            .map(|_| consumer.pop().unwrap().value)
            .collect::<Vec<_>>();
        assert_eq!(first_block, samples[..4]);

        let second_write =
            push_complete_frames(&mut producer, &samples[first_write..], 2, generation);
        assert_eq!(second_write, 2);
        assert_eq!(consumer.pop().unwrap().value, 3.0);
        assert_eq!(consumer.pop().unwrap().value, 103.0);
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn playback_clock_holds_at_the_producer_cursor_during_underrun() {
        let (mut producer, mut consumer) = RingBuffer::new(8);
        let generation = AudioOutputGeneration(0);
        assert_eq!(
            push_complete_frames(&mut producer, &[1.0, -1.0, 2.0, -2.0], 2, generation,),
            4
        );
        let clock = AtomicU64::new(packed_clock(generation.0, 10));
        let underruns = AtomicU64::new(0);
        let mut state = OutputCallbackState::new(2);
        state.was_starved = false;
        state.fade_in_position = OUTPUT_DECLICK_FRAMES;

        let mut first_output = [0.0; 8];
        let result = state.write(&mut first_output, &mut consumer, generation.0);
        record_callback_result(true, &clock, generation.0, &underruns, result);
        assert_eq!(clock_sample(clock.load(Ordering::Acquire)), 12);
        assert_eq!(underruns.load(Ordering::Acquire), 1);

        let mut empty_output = [f32::NAN; 8];
        let result = state.write(&mut empty_output, &mut consumer, generation.0);
        record_callback_result(true, &clock, generation.0, &underruns, result);
        assert_eq!(clock_sample(clock.load(Ordering::Acquire)), 12);
        assert_eq!(underruns.load(Ordering::Acquire), 2);
        assert!(empty_output.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn paused_scrub_does_not_advance_or_report_realtime_playback() {
        let (mut producer, mut consumer) = RingBuffer::new(4);
        let generation = AudioOutputGeneration(2);
        assert_eq!(
            push_complete_frames(&mut producer, &[0.25, -0.25], 2, generation),
            2
        );
        let clock = AtomicU64::new(packed_clock(generation.0, 50));
        let underruns = AtomicU64::new(7);
        let mut state = OutputCallbackState::new(2);
        let mut output = [0.0; 4];
        let result = state.write(&mut output, &mut consumer, generation.0);
        record_callback_result(false, &clock, generation.0, &underruns, result);
        assert_eq!(clock_sample(clock.load(Ordering::Acquire)), 50);
        assert_eq!(underruns.load(Ordering::Acquire), 7);
    }

    #[test]
    fn seek_generation_rejects_an_in_flight_old_callback_clock_update() {
        let clock = AtomicU64::new(packed_clock(4, 1_000));
        let underruns = AtomicU64::new(0);
        let old_result = CallbackWriteResult {
            consumed_frames: 128,
            ..CallbackWriteResult::default()
        };

        // Deterministic interleaving: the old callback has consumed its block,
        // then a seek atomically publishes generation 5 at a new sample.
        rebase_clock(&clock, 5, Some(200));
        record_callback_result(true, &clock, 4, &underruns, old_result);
        assert_eq!(clock.load(Ordering::Acquire), packed_clock(5, 200));

        record_callback_result(
            true,
            &clock,
            5,
            &underruns,
            CallbackWriteResult {
                consumed_frames: 16,
                ..CallbackWriteResult::default()
            },
        );
        assert_eq!(clock.load(Ordering::Acquire), packed_clock(5, 216));
    }

    #[test]
    fn old_producer_write_after_flush_ack_is_discarded_by_generation() {
        let flush = AudioFlushState::default();
        let old = AudioOutputGeneration(flush.requested());
        let next = flush.request();
        flush.acknowledge(next);
        let current = AudioOutputGeneration(next);
        assert!(flush.ready(current));

        let (mut producer, mut consumer) = RingBuffer::new(8);
        // This is the exact race: an old producer reserved its block before
        // the callback drained and acknowledged, then published it afterward.
        assert_eq!(push_complete_frames(&mut producer, &[9.0, 9.0], 1, old), 2);
        assert_eq!(
            push_complete_frames(&mut producer, &[1.0, 2.0], 1, current),
            2
        );
        let mut state = OutputCallbackState::new(1);
        state.was_starved = false;
        state.fade_in_position = OUTPUT_DECLICK_FRAMES;
        let mut output = [0.0; 2];
        let result = state.write(&mut output, &mut consumer, current.0);
        assert_eq!(result.discarded_stale_frames, 2);
        assert_eq!(result.consumed_frames, 2);
        assert_eq!(output, [1.0, 2.0]);
    }

    #[test]
    fn normal_callback_blocks_are_sample_exact_and_cursor_exact() {
        let generation = AudioOutputGeneration(9);
        let source = [0.25, -0.5, 0.75, -1.0, 0.1, 0.2, 0.3, 0.4];
        let (mut producer, mut consumer) = RingBuffer::new(source.len());
        assert_eq!(
            push_complete_frames(&mut producer, &source, 2, generation),
            source.len()
        );
        let mut state = OutputCallbackState::new(2);
        state.was_starved = false;
        state.fade_in_position = OUTPUT_DECLICK_FRAMES;
        let clock = AtomicU64::new(packed_clock(generation.0, 100));
        let underruns = AtomicU64::new(0);
        let mut output = [0.0; 8];
        for block in output.chunks_exact_mut(4) {
            let result = state.write(block, &mut consumer, generation.0);
            assert!(!result.starved);
            record_callback_result(true, &clock, generation.0, &underruns, result);
        }
        assert_eq!(output, source);
        assert_eq!(clock_sample(clock.load(Ordering::Acquire)), 104);
        assert_eq!(underruns.load(Ordering::Acquire), 0);
    }

    #[test]
    fn underrun_recovery_is_declicked_without_touching_normal_blocks() {
        let (mut producer, mut consumer) = RingBuffer::new(256);
        let generation = AudioOutputGeneration(0);
        let clock = AtomicU64::new(packed_clock(generation.0, 0));
        let underruns = AtomicU64::new(0);
        let mut state = OutputCallbackState::new(1);
        state.was_starved = false;
        state.fade_in_position = OUTPUT_DECLICK_FRAMES;

        assert_eq!(
            push_complete_frames(&mut producer, &[1.0; 4], 1, generation),
            4
        );
        let mut first = [0.0; 4 + OUTPUT_DECLICK_FRAMES];
        let first_result = state.write(&mut first, &mut consumer, generation.0);
        record_callback_result(true, &clock, generation.0, &underruns, first_result);
        assert_eq!(&first[..4], &[1.0; 4]);
        assert!(underruns.load(Ordering::Acquire) > 0);

        assert_eq!(
            push_complete_frames(&mut producer, &[1.0; 80], 1, generation),
            80
        );
        let mut recovery = [0.0; 80];
        let recovery_result = state.write(&mut recovery, &mut consumer, generation.0);
        record_callback_result(true, &clock, generation.0, &underruns, recovery_result);

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
        // Every useful current-generation source frame is consumed exactly
        // once. Crossfade changes only what is heard, not source/cursor time.
        assert_eq!(
            first_result.consumed_frames + recovery_result.consumed_frames,
            84
        );
        assert_eq!(clock_sample(clock.load(Ordering::Acquire)), 84);
        assert!(consumer.is_empty());
    }
}
