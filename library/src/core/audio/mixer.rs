use crate::cache::CacheManager;
use crate::model::project::asset::Asset;
use crate::model::project::project::Composition;
use crate::model::project::{Track, TrackClip};

/// Recursively collect all clips from a track and its children
fn collect_all_clips(track: &Track) -> Vec<&TrackClip> {
    let mut clips: Vec<&TrackClip> = track.clips().collect();
    for child_item in &track.children {
        if let crate::model::project::TrackItem::SubTrack(child_track) = child_item {
            clips.extend(collect_all_clips(child_track));
        }
    }
    clips
}

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

    // Collect all clips from all tracks (including nested)
    let all_clips: Vec<&TrackClip> = composition
        .tracks
        .iter()
        .flat_map(|t| collect_all_clips(t))
        .collect();

    for clip in all_clips {
        if let Some(asset_id) = clip.reference_id {
            if let Some(asset) = assets.iter().find(|a| a.id == asset_id) {
                if asset.kind != crate::model::project::asset::AssetKind::Audio {
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
                            ((overlap_start - start_time_s) * sample_rate as f64).round() as usize;
                        let render_len_samples =
                            ((overlap_end - overlap_start) * sample_rate as f64).round() as usize;

                        let source_start_time = (overlap_start - clip_in_time) + clip_source_offset;

                        // Handle negative source start (Silence before media start)
                        let mut skip_samples = 0;
                        let fixed_source_start_time = if source_start_time < 0.0 {
                            let skip_seconds = -source_start_time;
                            skip_samples = (skip_seconds * sample_rate as f64).round() as usize;
                            0.0 // Effective start is 0.0 (Media Start)
                        } else {
                            source_start_time
                        };

                        // Check if we skipped the entire duration
                        if skip_samples >= render_len_samples {
                            continue;
                        }

                        let source_start_sample =
                            (fixed_source_start_time * sample_rate as f64).round() as usize;

                        // Optimized mixing loop using iterators/slices
                        let channels_usize = channels as usize;
                        // Adjust dest_start by render_offset + skipped silence
                        let dest_start = (render_offset_samples + skip_samples) * channels_usize;
                        let len = (render_len_samples - skip_samples) * channels_usize;
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
    mix_buffer
}
