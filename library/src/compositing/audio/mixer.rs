use crate::cache::CacheManager;
use crate::core::model::asset::Asset;
use crate::core::model::project::Composition;

pub fn mix_samples(
    assets: &[Asset],
    composition: &Composition,
    cache_manager: &CacheManager,
    start_sample: u64,
    frames_to_mix: usize,
    sample_rate: u32,
    channels: u32,
) -> Vec<f32> {
    let mut mix_buffer = vec![0.0; frames_to_mix * channels as usize];
    let fps = composition.fps;

    for track in &composition.tracks {
        for clip in &track.clips {
            if let Some(asset_id) = clip.reference_id {
                if let Some(asset) = assets.iter().find(|a| a.id == asset_id) {
                    if asset.kind != crate::core::model::asset::AssetKind::Audio {
                        continue;
                    }

                    if let Some(audio_data) = cache_manager.get_audio(asset_id) {
                        let clip_in_time = clip.in_frame as f64 / fps;
                        let clip_out_time = clip.out_frame as f64 / fps;

                        // Ensure sane FPS
                        let clip_fps = if clip.fps > 0.0 { clip.fps } else { fps };
                        let clip_source_offset = clip.source_begin_frame as f64 / clip_fps;

                        let start_time_s = start_sample as f64 / sample_rate as f64;
                        let end_time_s =
                            (start_sample + frames_to_mix as u64) as f64 / sample_rate as f64;

                        let overlap_start = start_time_s.max(clip_in_time);
                        let overlap_end = end_time_s.min(clip_out_time);

                        if overlap_start < overlap_end {
                            let render_offset_samples =
                                ((overlap_start - start_time_s) * sample_rate as f64).round()
                                    as usize;
                            let render_len_samples =
                                ((overlap_end - overlap_start) * sample_rate as f64).round()
                                    as usize;

                            let source_start_time =
                                (overlap_start - clip_in_time) + clip_source_offset;
                            let source_start_sample =
                                (source_start_time * sample_rate as f64).round() as usize;

                            // Optimized mixing loop using iterators/slices
                            let channels_usize = channels as usize;
                            let dest_start = render_offset_samples * channels_usize;
                            let len = render_len_samples * channels_usize;
                            let src_start = source_start_sample * channels_usize;

                            // Bounds check once
                            if dest_start + len <= mix_buffer.len()
                                && src_start + len <= audio_data.len()
                            {
                                let dest_slice = &mut mix_buffer[dest_start..dest_start + len];
                                let src_slice = &audio_data[src_start..src_start + len];

                                for (d, s) in dest_slice.iter_mut().zip(src_slice.iter()) {
                                    *d += s;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    mix_buffer
}
