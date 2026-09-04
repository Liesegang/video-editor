use crate::core::audio::cache::{AudioChunk, AudioChunkKey, AudioFileIdentity, AudioSourceKey};
use crate::util::local_file::DirectRegularFile;
use lru::LruCache;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, Decoder, DecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader, Packet, SeekMode, SeekTo};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::{Time, TimeBase};

use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

pub struct AudioLoader;

// Predictive codecs (notably MP3) may need packets preceding the requested
// timestamp to rebuild their bit reservoir. Seeking exactly to a cache chunk
// boundary can otherwise decode the first few hundred frames as silence.
const SEEK_PREROLL_SECONDS: f64 = 0.25;
const MAX_DECODE_PREROLL_SECONDS: f64 = 2.0;
const MAX_PERSISTENT_DECODE_SESSIONS: usize = 8;

#[derive(Clone, Copy, Debug, Default)]
struct DecodeWork {
    packets_read: u64,
    decoded_source_frames: u64,
    seek_actual_seconds: Option<f64>,
}

fn probe_hint(path: &Path) -> Hint {
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        hint.with_extension(extension);
    }
    hint
}

struct OpenAudioTrack {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    time_base: TimeBase,
    pending_packet: Option<Packet>,
    next_target_frame: Option<u64>,
}

type SharedDecodeSession = Arc<Mutex<OpenAudioTrack>>;
type DecodeSessionCache = LruCache<AudioSourceKey, SharedDecodeSession>;

fn decode_sessions() -> &'static Mutex<DecodeSessionCache> {
    static SESSIONS: OnceLock<Mutex<DecodeSessionCache>> = OnceLock::new();
    SESSIONS.get_or_init(|| {
        let capacity =
            NonZeroUsize::new(MAX_PERSISTENT_DECODE_SESSIONS).unwrap_or(NonZeroUsize::MIN);
        Mutex::new(LruCache::new(capacity))
    })
}

impl AudioLoader {
    /// Decode exactly one bounded cache chunk.
    ///
    /// A requested stream is the zero-based global container stream index
    /// persisted by the FFmpeg metadata probe. Symphonia Track IDs are an
    /// implementation detail: the global ordinal is resolved first, then the
    /// selected Track ID is used for seeking and packet filtering.
    pub fn decode_chunk(key: &AudioChunkKey) -> Result<AudioChunk, anyhow::Error> {
        let current_identity = AudioFileIdentity::read(&key.source.identity.canonical_path)?;
        if current_identity != key.source.identity {
            anyhow::bail!("audio source changed before decode");
        }

        let session = Self::decode_session(&key.source)?;
        let mut work = DecodeWork::default();
        let chunk = {
            let mut opened = session.lock().unwrap_or_else(|poisoned| {
                log::error!("audio decode session lock was poisoned; recovering reader state");
                poisoned.into_inner()
            });
            Self::decode_chunk_with_opened(key, &mut opened, &mut work)
        };

        match AudioFileIdentity::read(&key.source.identity.canonical_path) {
            Ok(identity) if identity == key.source.identity => {}
            Ok(_) => {
                Self::evict_decode_session(&key.source, &session);
                anyhow::bail!("audio source changed during decode");
            }
            Err(error) => {
                Self::evict_decode_session(&key.source, &session);
                return Err(error.into());
            }
        }
        if chunk.is_err() {
            Self::evict_decode_session(&key.source, &session);
        }
        log::trace!(
            "audio chunk {} decoded with {} packets / {} source frames (seek {:?})",
            key.chunk_index,
            work.packets_read,
            work.decoded_source_frames,
            work.seek_actual_seconds
        );
        chunk
    }

    fn decode_session(source: &AudioSourceKey) -> Result<SharedDecodeSession, anyhow::Error> {
        let mut sessions = decode_sessions().lock().unwrap_or_else(|poisoned| {
            log::error!("audio decode session cache lock was poisoned; recovering entries");
            poisoned.into_inner()
        });
        if let Some(session) = sessions.get(source) {
            return Ok(Arc::clone(session));
        }
        let opened = Self::open_track(
            &source.identity.canonical_path,
            source.stream_index,
            &source.identity,
        )?;
        let session = Arc::new(Mutex::new(opened));
        sessions.put(source.clone(), Arc::clone(&session));
        Ok(session)
    }

    fn evict_decode_session(source: &AudioSourceKey, failed: &SharedDecodeSession) {
        let mut sessions = decode_sessions().lock().unwrap_or_else(|poisoned| {
            log::error!("audio decode session cache lock was poisoned; recovering entries");
            poisoned.into_inner()
        });
        if sessions
            .peek(source)
            .is_some_and(|cached| Arc::ptr_eq(cached, failed))
        {
            sessions.pop(source);
        }
    }

    fn decode_chunk_with_opened(
        key: &AudioChunkKey,
        opened: &mut OpenAudioTrack,
        work: &mut DecodeWork,
    ) -> Result<AudioChunk, anyhow::Error> {
        let start_frame = key.start_frame();
        let target_rate = key.source.format.sample_rate;
        let target_channels = usize::from(key.source.format.channels);
        let frame_capacity = key.frame_capacity();
        let start_seconds = start_frame as f64 / f64::from(target_rate);
        let end_seconds = start_seconds + frame_capacity as f64 / f64::from(target_rate);

        let sequential = opened.next_target_frame == Some(start_frame);
        let untouched_start = start_frame == 0 && opened.next_target_frame.is_none();
        if !sequential && !untouched_start {
            let seek_seconds = (start_seconds - SEEK_PREROLL_SECONDS).max(0.0);
            let seeked = opened.format.seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: seconds_to_time(seek_seconds),
                    track_id: Some(opened.track_id),
                },
            )?;
            let actual = opened.time_base.calc_time(seeked.actual_ts);
            let actual_seconds = actual.seconds as f64 + actual.frac;
            work.seek_actual_seconds = Some(actual_seconds);
            if actual_seconds + MAX_DECODE_PREROLL_SECONDS < start_seconds {
                anyhow::bail!(
                    "audio seek landed at {actual_seconds:.3}s for {start_seconds:.3}s chunk; bounded decode preroll is {MAX_DECODE_PREROLL_SECONDS:.3}s"
                );
            }
            opened.decoder.reset();
            opened.pending_packet = None;
        }

        let mut output = vec![0.0; frame_capacity.saturating_mul(target_channels)];
        loop {
            let packet = if let Some(packet) = opened.pending_packet.take() {
                packet
            } else {
                match opened.format.next_packet() {
                    Ok(packet) => packet,
                    Err(Error::IoError(_)) => break,
                    Err(error) => return Err(error.into()),
                }
            };
            work.packets_read = work.packets_read.saturating_add(1);
            if packet.track_id() != opened.track_id {
                continue;
            }

            let packet_time = opened.time_base.calc_time(packet.ts());
            let packet_start_seconds = packet_time.seconds as f64 + packet_time.frac;
            if packet_start_seconds >= end_seconds {
                opened.pending_packet = Some(packet);
                break;
            }

            let decoded = match opened.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(Error::DecodeError(_)) => continue,
                Err(error) => return Err(error.into()),
            };
            let spec = *decoded.spec();
            let source_channels = spec.channels.count();
            if source_channels == 0 || spec.rate == 0 {
                continue;
            }
            let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
            samples.copy_interleaved_ref(decoded);
            let source = samples.samples();
            let source_frames = source.len() / source_channels;
            work.decoded_source_frames = work
                .decoded_source_frames
                .saturating_add(source_frames as u64);
            if source_frames == 0 {
                continue;
            }
            let packet_end_seconds =
                packet_start_seconds + source_frames as f64 / f64::from(spec.rate);
            if packet_end_seconds <= start_seconds {
                continue;
            }

            let first_target =
                ((packet_start_seconds * f64::from(target_rate)).ceil() as u64).max(start_frame);
            let final_target = ((packet_end_seconds * f64::from(target_rate)).ceil() as u64)
                .min(start_frame.saturating_add(frame_capacity as u64));
            for absolute_target in first_target..final_target {
                let target_seconds = absolute_target as f64 / f64::from(target_rate);
                let source_position =
                    ((target_seconds - packet_start_seconds) * f64::from(spec.rate)).max(0.0);
                let first_source_frame = (source_position.floor() as usize).min(source_frames - 1);
                let second_source_frame = (first_source_frame + 1).min(source_frames - 1);
                let fraction = (source_position - first_source_frame as f64) as f32;
                let target_frame = usize::try_from(absolute_target - start_frame)
                    .map_err(|_| anyhow::anyhow!("audio target frame does not fit usize"))?;
                for target_channel in 0..target_channels {
                    let first = mapped_sample(
                        source,
                        first_source_frame,
                        source_channels,
                        target_channel,
                        target_channels,
                    );
                    let second = mapped_sample(
                        source,
                        second_source_frame,
                        source_channels,
                        target_channel,
                        target_channels,
                    );
                    output[target_frame * target_channels + target_channel] =
                        first + (second - first) * fraction;
                }
            }
        }

        opened.next_target_frame = Some(start_frame.saturating_add(frame_capacity as u64));
        AudioChunk::new(key.clone(), output).map_err(anyhow::Error::msg)
    }

    fn open_track(
        path: &Path,
        stream_index: Option<usize>,
        expected_identity: &AudioFileIdentity,
    ) -> Result<OpenAudioTrack, anyhow::Error> {
        let source = DirectRegularFile::open(path)?;
        let opened_identity = AudioFileIdentity::from_opened(&source)?;
        if &opened_identity != expected_identity {
            anyhow::bail!("audio source changed before decode");
        }
        Self::open_track_from_source(path, stream_index, Box::new(source.into_file()))
    }

    fn open_track_from_source(
        path: &Path,
        stream_index: Option<usize>,
        source: Box<dyn MediaSource>,
    ) -> Result<OpenAudioTrack, anyhow::Error> {
        let media = MediaSourceStream::new(source, Default::default());
        let options = FormatOptions {
            enable_gapless: true,
            ..FormatOptions::default()
        };
        let probe = symphonia::default::get_probe().format(
            &probe_hint(path),
            media,
            &options,
            &MetadataOptions::default(),
        )?;
        let format = probe.format;

        let selected = if let Some(global_index) = stream_index {
            let track = format.tracks().get(global_index).ok_or_else(|| {
                anyhow::anyhow!("audio stream index {global_index} does not exist")
            })?;
            if track.codec_params.codec == CODEC_TYPE_NULL
                || track.codec_params.sample_rate.is_none()
            {
                anyhow::bail!("stream index {global_index} is not a decodable audio stream");
            }
            let decoder = symphonia::default::get_codecs()
                .make(&track.codec_params, &DecoderOptions::default())?;
            (track.id, track.codec_params.time_base, decoder)
        } else {
            let mut selected = None;
            for track in format.tracks() {
                if track.codec_params.codec == CODEC_TYPE_NULL
                    || track.codec_params.sample_rate.is_none()
                {
                    continue;
                }
                if let Ok(decoder) = symphonia::default::get_codecs()
                    .make(&track.codec_params, &DecoderOptions::default())
                {
                    selected = Some((track.id, track.codec_params.time_base, decoder));
                    break;
                }
            }
            selected.ok_or_else(|| anyhow::anyhow!("No supported audio stream found"))?
        };
        let (track_id, time_base, decoder) = selected;
        let time_base = time_base.ok_or_else(|| anyhow::anyhow!("Unknown audio time base"))?;

        Ok(OpenAudioTrack {
            format,
            decoder,
            track_id,
            time_base,
            pending_packet: None,
            next_target_frame: None,
        })
    }

    pub fn get_duration(path: &str, stream_index: Option<usize>) -> Result<f64, anyhow::Error> {
        let path = Path::new(path);
        let source = DirectRegularFile::open(path)?;
        let media = MediaSourceStream::new(Box::new(source.into_file()), Default::default());
        let probe = symphonia::default::get_probe().format(
            &probe_hint(path),
            media,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )?;
        let track = match stream_index {
            Some(index) => probe.format.tracks().get(index),
            None => probe.format.tracks().iter().find(|track| {
                track.codec_params.codec != CODEC_TYPE_NULL
                    && track.codec_params.sample_rate.is_some()
            }),
        }
        .ok_or_else(|| anyhow::anyhow!("No audio stream found"))?;

        if let (Some(time_base), Some(frames)) =
            (track.codec_params.time_base, track.codec_params.n_frames)
        {
            let time = time_base.calc_time(frames);
            Ok(time.seconds as f64 + time.frac)
        } else {
            Err(anyhow::anyhow!("Duration not available in header"))
        }
    }

    pub fn has_audio(path: &str) -> bool {
        let path = Path::new(path);
        let Ok(source) = DirectRegularFile::open(path) else {
            return false;
        };
        let media = MediaSourceStream::new(Box::new(source.into_file()), Default::default());
        symphonia::default::get_probe()
            .format(
                &probe_hint(path),
                media,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .is_ok_and(|probe| {
                probe.format.tracks().iter().any(|track| {
                    track.codec_params.codec != CODEC_TYPE_NULL
                        && track.codec_params.sample_rate.is_some()
                })
            })
    }
}

fn seconds_to_time(seconds: f64) -> Time {
    let seconds = seconds.max(0.0);
    Time::new(seconds.trunc() as u64, seconds.fract())
}

fn mapped_sample(
    samples: &[f32],
    frame: usize,
    source_channels: usize,
    target_channel: usize,
    target_channels: usize,
) -> f32 {
    let base = frame * source_channels;
    if target_channels == 1 && source_channels > 1 {
        return samples[base..base + source_channels].iter().sum::<f32>() / source_channels as f32;
    }
    let source_channel = if source_channels == 1 {
        0
    } else {
        target_channel.min(source_channels - 1)
    };
    samples[base + source_channel]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::audio::cache::{AudioChunkKey, AudioDecodeFormat, AudioSourceKey};
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Clone, Copy)]
    enum WaveEncoding {
        Pcm16,
        Float32,
    }

    struct TempWave(std::path::PathBuf);

    impl TempWave {
        fn continuous_sine(sample_rate: u32, seconds: u32) -> Self {
            Self::sine(
                sample_rate,
                2,
                sample_rate.saturating_mul(seconds),
                WaveEncoding::Pcm16,
            )
        }

        fn sine(sample_rate: u32, channels: u16, frames: u32, encoding: WaveEncoding) -> Self {
            let path = std::env::temp_dir().join(format!(
                "ruvie-audio-continuity-{}.wav",
                uuid::Uuid::new_v4()
            ));
            let (format_tag, bits_per_sample, bytes_per_sample) = match encoding {
                WaveEncoding::Pcm16 => (1_u16, 16_u16, 2_u32),
                WaveEncoding::Float32 => (3_u16, 32_u16, 4_u32),
            };
            let data_bytes = frames
                .saturating_mul(u32::from(channels))
                .saturating_mul(bytes_per_sample);
            let mut bytes = Vec::with_capacity(44 + data_bytes as usize);
            bytes.extend_from_slice(b"RIFF");
            bytes.extend_from_slice(&(36_u32 + data_bytes).to_le_bytes());
            bytes.extend_from_slice(b"WAVEfmt ");
            bytes.extend_from_slice(&16_u32.to_le_bytes());
            bytes.extend_from_slice(&format_tag.to_le_bytes());
            bytes.extend_from_slice(&channels.to_le_bytes());
            bytes.extend_from_slice(&sample_rate.to_le_bytes());
            bytes.extend_from_slice(
                &(sample_rate * u32::from(channels) * bytes_per_sample).to_le_bytes(),
            );
            bytes.extend_from_slice(
                &(channels * u16::try_from(bytes_per_sample).unwrap_or(u16::MAX)).to_le_bytes(),
            );
            bytes.extend_from_slice(&bits_per_sample.to_le_bytes());
            bytes.extend_from_slice(b"data");
            bytes.extend_from_slice(&data_bytes.to_le_bytes());
            for frame in 0..frames {
                let phase = std::f32::consts::TAU * 440.0 * frame as f32 / sample_rate as f32;
                let sample = phase.sin() * 0.75;
                for _ in 0..channels {
                    match encoding {
                        WaveEncoding::Pcm16 => bytes.extend_from_slice(
                            &((sample * f32::from(i16::MAX)) as i16).to_le_bytes(),
                        ),
                        WaveEncoding::Float32 => {
                            bytes.extend_from_slice(&sample.to_le_bytes());
                        }
                    }
                }
            }
            std::fs::write(&path, bytes).unwrap();
            Self(path)
        }
    }

    struct CountingFile {
        inner: File,
        length: u64,
        bytes_read: Arc<AtomicU64>,
    }

    impl CountingFile {
        fn open(path: &Path) -> std::io::Result<(Self, Arc<AtomicU64>)> {
            let inner = File::open(path)?;
            let length = inner.metadata()?.len();
            let bytes_read = Arc::new(AtomicU64::new(0));
            Ok((
                Self {
                    inner,
                    length,
                    bytes_read: Arc::clone(&bytes_read),
                },
                bytes_read,
            ))
        }
    }

    impl Read for CountingFile {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let read = self.inner.read(buffer)?;
            self.bytes_read.fetch_add(read as u64, Ordering::Relaxed);
            Ok(read)
        }
    }

    impl Seek for CountingFile {
        fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(position)
        }
    }

    impl MediaSource for CountingFile {
        fn is_seekable(&self) -> bool {
            true
        }

        fn byte_len(&self) -> Option<u64> {
            Some(self.length)
        }
    }

    impl Drop for TempWave {
        fn drop(&mut self) {
            drop(std::fs::remove_file(&self.0));
        }
    }

    #[test]
    fn independently_decoded_pcm_chunks_preserve_source_boundary_continuity() {
        let sample_rate = 8_000;
        let wave = TempWave::continuous_sine(sample_rate, 2);
        let format = AudioDecodeFormat::new(sample_rate, 2).unwrap();
        let source = AudioSourceKey::read(&wave.0, None, format).unwrap();
        let first = AudioLoader::decode_chunk(&AudioChunkKey {
            source: source.clone(),
            chunk_index: 0,
        })
        .unwrap();
        let second = AudioLoader::decode_chunk(&AudioChunkKey {
            source,
            chunk_index: 1,
        })
        .unwrap();
        assert_eq!(first.frame_count(), sample_rate as usize);
        assert_eq!(second.frame_count(), sample_rate as usize);

        let first_left = first
            .samples()
            .chunks_exact(2)
            .map(|frame| frame[0])
            .collect::<Vec<_>>();
        let second_left = second
            .samples()
            .chunks_exact(2)
            .map(|frame| frame[0])
            .collect::<Vec<_>>();
        let boundary_step = (second_left[0] - first_left[first_left.len() - 1]).abs();
        let largest_interior_step = first_left
            .windows(2)
            .chain(second_left.windows(2))
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            boundary_step <= largest_interior_step + 2.0 / f32::from(i16::MAX),
            "PCM decoder introduced boundary step {boundary_step}, interior max {largest_interior_step}"
        );
    }

    #[test]
    fn short_and_final_partial_mono_float_chunks_are_zero_padded_at_11khz() {
        let sample_rate = 11_025;
        let format = AudioDecodeFormat::new(sample_rate, 1).unwrap();

        let short_frames = sample_rate / 8;
        let short = TempWave::sine(sample_rate, 1, short_frames, WaveEncoding::Float32);
        let short_source = AudioSourceKey::read(&short.0, None, format).unwrap();
        let short_chunk = AudioLoader::decode_chunk(&AudioChunkKey {
            source: short_source,
            chunk_index: 0,
        })
        .unwrap();
        assert!(
            short_chunk.samples()[..short_frames as usize]
                .iter()
                .any(|sample| sample.abs() > 0.1)
        );
        assert!(
            short_chunk.samples()[short_frames as usize..]
                .iter()
                .all(|sample| *sample == 0.0)
        );

        let final_frames = sample_rate + sample_rate / 8;
        let partial = TempWave::sine(sample_rate, 1, final_frames, WaveEncoding::Float32);
        let partial_source = AudioSourceKey::read(&partial.0, None, format).unwrap();
        let final_chunk = AudioLoader::decode_chunk(&AudioChunkKey {
            source: partial_source,
            chunk_index: 1,
        })
        .unwrap();
        assert!(
            final_chunk.samples()[..short_frames as usize]
                .iter()
                .any(|sample| sample.abs() > 0.1)
        );
        assert!(
            final_chunk.samples()[short_frames as usize..]
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }

    fn decode_work_at(
        opened: &mut OpenAudioTrack,
        source: &AudioSourceKey,
        bytes_read: &AtomicU64,
        chunk_index: u64,
    ) -> Result<(DecodeWork, u64), anyhow::Error> {
        let key = AudioChunkKey {
            source: source.clone(),
            chunk_index,
        };
        bytes_read.store(0, Ordering::Relaxed);
        let mut work = DecodeWork::default();
        AudioLoader::decode_chunk_with_opened(&key, opened, &mut work)?;
        Ok((work, bytes_read.load(Ordering::Relaxed)))
    }

    #[test]
    fn late_mp3_chunk_seek_has_bounded_io_and_decode_work() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test_data/test_sound2.mp3");
        let format = AudioDecodeFormat::new(48_000, 2).unwrap();
        let source = AudioSourceKey::read(&path, None, format).unwrap();
        let (file, bytes_read) = CountingFile::open(&path).unwrap();
        let mut opened = AudioLoader::open_track_from_source(&path, None, Box::new(file)).unwrap();
        let (early, early_bytes) = decode_work_at(&mut opened, &source, &bytes_read, 1).unwrap();
        // The first distant jump extends the demuxer's progressive seek index.
        let (indexing, indexing_bytes) =
            decode_work_at(&mut opened, &source, &bytes_read, 60).unwrap();
        let (late, late_bytes) = decode_work_at(&mut opened, &source, &bytes_read, 61).unwrap();

        assert!(early.seek_actual_seconds.is_some());
        assert!(indexing.seek_actual_seconds.is_some());
        assert_eq!(
            late.seek_actual_seconds, None,
            "adjacent chunk must continue through the persistent decoder without another seek"
        );
        assert!(late.packets_read <= early.packets_read.saturating_mul(2));
        assert!(late.decoded_source_frames <= early.decoded_source_frames.saturating_mul(2));
        assert!(
            late_bytes <= early_bytes.saturating_mul(2),
            "late random access read {late_bytes} bytes versus {early_bytes} near the start"
        );
        assert!(late_bytes < 512 * 1_024);
        assert!(
            indexing_bytes.saturating_add(late_bytes) < 2 * 1_024 * 1_024,
            "progressively indexing and decoding adjacent late chunks repeated {indexing_bytes}+{late_bytes} bytes of work"
        );
    }
}
