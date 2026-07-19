use crate::core::audio::cache::{AudioChunk, AudioChunkKey, AudioFileIdentity};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, Decoder, DecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::{Time, TimeBase};

use std::fs::File;
use std::path::Path;

pub struct AudioLoader;

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

        let mut opened =
            Self::open_track(&key.source.identity.canonical_path, key.source.stream_index)?;
        let start_frame = key.start_frame();
        let target_rate = key.source.format.sample_rate;
        let target_channels = usize::from(key.source.format.channels);
        let frame_capacity = key.frame_capacity();
        let start_seconds = start_frame as f64 / f64::from(target_rate);
        let end_seconds = start_seconds + frame_capacity as f64 / f64::from(target_rate);

        if start_frame > 0 {
            opened.format.seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: seconds_to_time(start_seconds),
                    track_id: Some(opened.track_id),
                },
            )?;
            opened.decoder.reset();
        }

        let mut output = vec![0.0; frame_capacity.saturating_mul(target_channels)];
        loop {
            let packet = match opened.format.next_packet() {
                Ok(packet) => packet,
                Err(Error::IoError(_)) => break,
                Err(error) => return Err(error.into()),
            };
            if packet.track_id() != opened.track_id {
                continue;
            }

            let packet_time = opened.time_base.calc_time(packet.ts());
            let packet_start_seconds = packet_time.seconds as f64 + packet_time.frac;
            if packet_start_seconds >= end_seconds {
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

        if AudioFileIdentity::read(&key.source.identity.canonical_path)? != key.source.identity {
            anyhow::bail!("audio source changed during decode");
        }
        AudioChunk::new(key.clone(), output).map_err(anyhow::Error::msg)
    }

    fn open_track(
        path: &Path,
        stream_index: Option<usize>,
    ) -> Result<OpenAudioTrack, anyhow::Error> {
        let source = File::open(path)?;
        let media = MediaSourceStream::new(Box::new(source), Default::default());
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
        })
    }

    pub fn get_duration(path: &str, stream_index: Option<usize>) -> Result<f64, anyhow::Error> {
        let path = Path::new(path);
        let source = File::open(path)?;
        let media = MediaSourceStream::new(Box::new(source), Default::default());
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
        let Ok(source) = File::open(path) else {
            return false;
        };
        let media = MediaSourceStream::new(Box::new(source), Default::default());
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
