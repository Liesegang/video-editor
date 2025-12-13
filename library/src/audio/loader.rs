use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use std::fs::File;
use std::path::Path;

pub struct AudioLoader;

impl AudioLoader {
    // Decodes the entire file into a float vector (Interleaved Stereo)
    // Resamples to 44100 or 48000 (target_sample_rate)
    pub fn load_entire_file(path: &str, target_sample_rate: u32) -> Result<Vec<f32>, anyhow::Error> {
        let src = File::open(Path::new(path))?;
        let mss = MediaSourceStream::new(Box::new(src), Default::default());

        let hint = symphonia::default::get_probe().format(
            &Default::default(),
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )?;

        let mut format = hint.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| anyhow::anyhow!("No supported audio track found"))?;

        let mut decoder = symphonia::default::get_codecs().make(
            &track.codec_params,
            &DecoderOptions::default(),
        )?;

        let track_id = track.id;
        let source_sample_rate = track.codec_params.sample_rate.ok_or_else(|| anyhow::anyhow!("Unknown sample rate"))?;
        
        // Setup Resampler if needed
        // Simpler implementation: Just collect all samples first, then resample if needed? 
        // For large files this is bad, but for "Step 1" it's fine.
        
        let mut audio_data: Vec<f32> = Vec::new();
        
        // Decode loop
        loop {
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(Error::IoError(_)) => break, // EOF
                Err(Error::ResetRequired) => {
                    // unexpected, but handle
                    continue;
                }
                Err(err) => return Err(err.into()),
            };

            if packet.track_id() != track_id {
                continue;
            }

            match decoder.decode(&packet) {
                Ok(decoded) => {
                    // Convert to f32 interleaved
                     let spec = *decoded.spec();
                     let mut sample_buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                     sample_buf.copy_interleaved_ref(decoded);
                     
                     // Force Stereo
                     // If source is Mono (1 channel), duplicate to Stereo
                     // If source is Stereo (2 channels), keep
                     // If > 2, take first 2
                     
                     let samples = sample_buf.samples();
                     let channels = spec.channels.count();
                     
                     if channels == 1 {
                         // Mono -> Stereo
                         for sample in samples {
                             audio_data.push(*sample); // L
                             audio_data.push(*sample); // R
                         }
                     } else if channels == 2 {
                         // Stereo -> Stereo
                         audio_data.extend_from_slice(samples);
                     } else {
                         // Multi -> Stereo (Truncate)
                         for chunk in samples.chunks(channels) {
                             if chunk.len() >= 2 {
                                 audio_data.push(chunk[0]);
                                 audio_data.push(chunk[1]);
                             }
                         }
                     }
                }
                Err(Error::DecodeError(_)) => (),
                Err(err) => return Err(err.into()),
            }
        }
        
        // Resample if needed using Linear Interpolation
        if source_sample_rate != target_sample_rate && target_sample_rate > 0 {
            let ratio = source_sample_rate as f32 / target_sample_rate as f32;
            let num_channels = 2; // We forced Stereo above
            
            // Calculate new length
            // input frames = audio_data.len() / 2
            let input_frames = audio_data.len() / num_channels;
            let output_frames = (input_frames as f32 / ratio).ceil() as usize;
            
            let mut resampled_data = Vec::with_capacity(output_frames * num_channels);
            
            for i in 0..output_frames {
                let src_idx_float = i as f32 * ratio;
                let src_idx_floor = src_idx_float.floor() as usize;
                let t = src_idx_float - src_idx_float.floor(); // Fractional part for interpolation (0.0..1.0)
                
                // Clamp to valid range
                let idx0 = src_idx_floor.min(input_frames - 1);
                let idx1 = (src_idx_floor + 1).min(input_frames - 1);
                
                // Interleaved access: [L0, R0, L1, R1, ...]
                let l0 = audio_data[idx0 * 2];
                let r0 = audio_data[idx0 * 2 + 1];
                let l1 = audio_data[idx1 * 2];
                let r1 = audio_data[idx1 * 2 + 1];
                
                // Linear Interpolation
                let l_out = l0 + (l1 - l0) * t;
                let r_out = r0 + (r1 - r0) * t;
                
                resampled_data.push(l_out);
                resampled_data.push(r_out);
            }
            
            audio_data = resampled_data;
        }

        // Add proper handling for Stereo/Mono conversion
        // If source is mono, duplicate to stereo.
        // track.codec_params.channels
        
        Ok(audio_data)
    }

    pub fn get_duration(path: &str) -> Result<f64, anyhow::Error> {
        let src = File::open(Path::new(path))?;
        let mss = MediaSourceStream::new(Box::new(src), Default::default());
        let probe = symphonia::default::get_probe().format(
            &Default::default(),
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default()
        )?;

        let track = probe.format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| anyhow::anyhow!("No audio track found"))?;

        if let (Some(tb), Some(frames)) = (track.codec_params.time_base, track.codec_params.n_frames) {
             let time = tb.calc_time(frames);
             Ok(time.seconds as f64 + time.frac)
        } else {
             // Fallback: If duration is unknown from header.
             // We can return 0.0 or try to estimate?
             // For now, return error or 0.0?
             // Returning 10.0s as dummy is bad.
             Err(anyhow::anyhow!("Duration not available in header"))
        } 
    }
}
