use crate::cache::CacheManager;
use crate::project::asset::Asset;
use crate::project::node::Node;
use crate::project::project::{Composition, Project};
use crate::project::source::SourceData;
use uuid::Uuid;

/// Recursively collect all sources from the project starting at a given node
fn collect_sources_recursive<'a>(project: &'a Project, node_id: Uuid) -> Vec<&'a SourceData> {
    let mut sources = Vec::new();
    match project.get_node(node_id) {
        Some(Node::Source(s)) => sources.push(s),
        Some(Node::Track(t)) => {
            for child_id in &t.child_ids {
                sources.extend(collect_sources_recursive(project, *child_id));
            }
        }
        Some(Node::Layer(l)) => {
            for child_id in &l.child_ids {
                sources.extend(collect_sources_recursive(project, *child_id));
            }
        }
        _ => {}
    }
    sources
}

pub fn mix_samples(
    assets: &[Asset],
    project: &Project,
    composition: &Composition,
    cache_manager: &CacheManager,
    start_sample: u64,
    frames_to_mix: usize,
    sample_rate: u32,
    channels: u32,
) -> Vec<f32> {
    let mut mix_buffer = vec![0.0; frames_to_mix * channels as usize];
    let fps = composition.fps;

    // Collect all sources from the composition's children
    let all_sources: Vec<&SourceData> = composition
        .child_ids
        .iter()
        .flat_map(|child_id| collect_sources_recursive(project, *child_id))
        .collect();

    for source in all_sources {
        if let Some(asset_id) = source.reference_id {
            if let Some(asset) = assets.iter().find(|a| a.id == asset_id) {
                if asset.kind != crate::project::asset::AssetKind::Audio {
                    continue;
                }

                if let Some(audio_data) = cache_manager.get_audio(asset_id) {
                    let source_in_time = source.in_frame as f64 / fps;
                    let source_out_time = source.out_frame as f64 / fps;

                    // Ensure sane FPS
                    let source_fps = if source.fps > 0.0 { source.fps } else { fps };
                    let source_offset = source.source_begin_frame as f64 / source_fps;

                    let start_time_s = start_sample as f64 / sample_rate as f64;
                    let end_time_s =
                        (start_sample + frames_to_mix as u64) as f64 / sample_rate as f64;

                    let overlap_start = start_time_s.max(source_in_time);
                    let overlap_end = end_time_s.min(source_out_time);

                    if overlap_start < overlap_end {
                        let render_offset_samples =
                            ((overlap_start - start_time_s) * sample_rate as f64).round() as usize;
                        let render_len_samples =
                            ((overlap_end - overlap_start) * sample_rate as f64).round() as usize;

                        let source_start_time = (overlap_start - source_in_time) + source_offset;

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
