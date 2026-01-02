use crate::cache::CacheManager;
use crate::model::asset::Asset;
use crate::model::project::{Composite, Project};
use crate::model::{Layer, LayerContent, MediaContent, Node};
use uuid::Uuid;

pub fn mix_samples(
    assets: &[Asset],
    project: &Project,
    composition: &Composite,
    cache_manager: &CacheManager,
    start_sample: u64,
    frames_to_mix: usize,
    sample_rate: u32,
    channels: u32,
) -> Vec<f32> {
    let mut mix_buffer = vec![0.0; frames_to_mix * channels as usize];
    let start_time = start_sample as f64 / sample_rate as f64;

    mix_node_recursive(
        project,
        composition.root_track_id,
        &mut mix_buffer,
        assets,
        cache_manager,
        start_time,
        frames_to_mix,
        sample_rate,
        channels,
    );

    mix_buffer
}

#[allow(clippy::too_many_arguments)]
fn mix_node_recursive(
    project: &Project,
    node_id: Uuid,
    accum_buffer: &mut [f32],
    assets: &[Asset],
    cache_manager: &CacheManager,
    current_time: f64,
    frames: usize,
    sample_rate: u32,
    channels: u32,
) {
    let node = match project.get_node(node_id) {
        Some(n) => n,
        None => return,
    };

    match node {
        Node::Track(track) => {
            // Track Mixer Logic
            // 1. Create a buffer for this track
            let mut track_buffer = vec![0.0; accum_buffer.len()];

            // 2. Mix all children into track_buffer
            for child_id in &track.children {
                mix_node_recursive(
                    project,
                    *child_id,
                    &mut track_buffer,
                    assets,
                    cache_manager,
                    current_time,
                    frames,
                    sample_rate,
                    channels,
                );
            }

            // 3. Apply Track Volume
            // Evaluate volume property at current_time
            let volume = track
                .properties
                .get("volume")
                .map(|p| p.evaluate_at(current_time)) // Using the new evaluate_at
                .and_then(|pv| pv.get_as::<f64>())
                .unwrap_or(1.0); // Default volume 1.0

            // Apply volume to track_buffer
            if (volume - 1.0).abs() > f64::EPSILON || volume < 0.0 {
                for sample in track_buffer.iter_mut() {
                    *sample *= volume as f32;
                }
            }

            // 4. Accumulate into parent buffer (accum_buffer)
            // Audio mixing is additive.
            for (dst, src) in accum_buffer.iter_mut().zip(track_buffer.iter()) {
                *dst += src;
            }
        }
        Node::Layer(layer) => {
            // Layer Logic
            mix_layer(
                &layer,
                accum_buffer,
                assets,
                cache_manager,
                current_time,
                frames,
                sample_rate,
                channels,
            );
        }
    }
}

fn mix_layer(
    layer: &Layer,
    accum_buffer: &mut [f32],
    assets: &[Asset],
    cache_manager: &CacheManager,
    mix_start_time: f64,
    frames: usize,
    sample_rate: u32,
    channels: u32,
) {
    // 1. Check if media and has audio
    let asset_id = match &layer.content {
        LayerContent::Media(MediaContent { asset_id, .. }) => asset_id,
        _ => return, // Not media, no audio
    };

    let asset = match assets.iter().find(|a| a.id == *asset_id) {
        Some(a) => a,
        None => return,
    };

    if asset.kind != crate::model::asset::AssetKind::Audio
        && asset.kind != crate::model::asset::AssetKind::Video
    {
        return;
    }

    // 2. Get Audio Data
    let audio_data = match cache_manager.get_audio(*asset_id) {
        Some(data) => data,
        None => return, // Not cached/loaded
    };

    // 3. Time mapping
    let layer_start = layer.start_time.into_inner();
    let layer_duration = layer.duration.into_inner();
    let layer_end = layer_start + layer_duration;
    let trim_in = layer.trim_in.into_inner();
    let time_stretch = layer.time_stretch.into_inner();

    let mix_end_time = mix_start_time + (frames as f64 / sample_rate as f64);

    // Overlap
    let overlap_start = mix_start_time.max(layer_start);
    let overlap_end = mix_end_time.min(layer_end);

    if overlap_start >= overlap_end {
        return;
    }

    // Calculate buffer offsets
    let dest_offset_seconds = overlap_start - mix_start_time;
    let render_duration_seconds = overlap_end - overlap_start;

    let dest_start_sample = (dest_offset_seconds * sample_rate as f64).round() as usize;
    let len_samples = (render_duration_seconds * sample_rate as f64).round() as usize;

    if dest_start_sample >= frames {
        return;
    }

    // Safety clamp length
    let len_samples = len_samples.min(frames - dest_start_sample);

    // Source mapping
    // source_time = (timeline_time - layer_start) * time_stretch + trim_in
    let source_start_time = (overlap_start - layer_start) * time_stretch + trim_in;

    // Check source bounds (approximate)
    if source_start_time < 0.0 {
        // TODO: Handle start trimming if needed, for new complexity ignoring complex negative trim logic
    }

    let src_start_sample = (source_start_time * sample_rate as f64).round() as usize;

    // 4. Get Layer Volume
    let volume = layer
        .properties
        .get("volume")
        .map(|p| p.evaluate_at(mix_start_time))
        .and_then(|pv| pv.get_as::<f64>())
        .unwrap_or(1.0);

    // 5. Mix
    let channels_usize = channels as usize;
    let dest_idx_base = dest_start_sample * channels_usize;
    let src_idx_base = src_start_sample * channels_usize;
    let total_elements = len_samples * channels_usize;

    if dest_idx_base + total_elements <= accum_buffer.len()
        && src_idx_base + total_elements <= audio_data.len()
    {
        for i in 0..total_elements {
            let sample = audio_data[src_idx_base + i] * (volume as f32);
            accum_buffer[dest_idx_base + i] += sample;
        }
    }
}
