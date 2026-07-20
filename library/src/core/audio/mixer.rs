use crate::cache::CacheManager;
use crate::core::audio::cache::{AudioChunk, AudioChunkKey, AudioDecodeFormat, AudioSourceKey};
use crate::core::audio::loader::AudioLoader;
use crate::model::asset::{Asset, AssetKind};
use crate::model::project::{Composition, Project};
use crate::model::{Clip, Node, NodeContent, Track};
use crate::plugin::{PluginManager, PropertyEvaluatorRegistry};
use lru::LruCache;
use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::sync::Arc;

mod property_evaluation;

use property_evaluation::{AudioPropertyContext, volume_at};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct AudioSourceSpec {
    pub path: String,
    pub stream_index: Option<usize>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct AudioWindowRequest {
    pub source: AudioSourceSpec,
    pub first_source_frame: u64,
    pub last_source_frame: u64,
}

#[derive(Clone, Copy)]
enum DecodePolicy {
    CacheOnly,
    DecodeMissing,
}

/// Mixes the audio-producing leaf Nodes contained by a Composition.
///
/// Image graph outputs are deliberately not used as audio routing. Every Media
/// Node whose asset has audio contributes additively: Composition/Track Nodes
/// use global time, while Clip Nodes use the Clip's local time mapping.
#[allow(
    clippy::too_many_arguments,
    reason = "audio callback boundary requires project scope, cache, sample window, and device format as independent inputs"
)]
pub fn mix_samples(
    assets: &[Asset],
    project: &Project,
    composition: &Composition,
    cache_manager: &CacheManager,
    start_sample: u64,
    frames_to_mix: usize,
    sample_rate: u32,
    channels: u32,
    plugin_manager: &PluginManager,
) -> Vec<f32> {
    let property_evaluators = plugin_manager.get_property_evaluators();
    mix_samples_with_policy(
        assets,
        project,
        composition,
        cache_manager,
        start_sample,
        frames_to_mix,
        sample_rate,
        channels,
        DecodePolicy::CacheOnly,
        property_evaluators.as_ref(),
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "decode policy is an internal extension of the explicit audio callback boundary"
)]
fn mix_samples_with_policy(
    assets: &[Asset],
    project: &Project,
    composition: &Composition,
    cache_manager: &CacheManager,
    start_sample: u64,
    frames_to_mix: usize,
    sample_rate: u32,
    channels: u32,
    decode_policy: DecodePolicy,
    property_evaluators: &PropertyEvaluatorRegistry,
) -> Vec<f32> {
    let channels = channels as usize;
    let mut mix_buffer = vec![0.0; frames_to_mix.saturating_mul(channels)];
    if frames_to_mix == 0 || sample_rate == 0 || channels == 0 {
        return mix_buffer;
    }

    // Composition duration is a half-open output range. Direct Composition
    // and Track Nodes must become NoOutput at the same boundary as Clips;
    // limiting the work here also prevents cold export from decoding source
    // chunks that cannot contribute to the authoritative output.
    let frames_to_mix =
        frames_inside_composition(composition, start_sample, frames_to_mix, sample_rate);
    if frames_to_mix == 0 {
        return mix_buffer;
    }

    let start_time = start_sample as f64 / sample_rate as f64;
    let property_context = AudioPropertyContext::new(
        property_evaluators,
        composition.fps,
        (composition.width, composition.height),
    );

    // Direct Composition Nodes live in the global composition time scope.
    mix_global_nodes(
        project,
        &composition.node_ids,
        &mut mix_buffer,
        assets,
        cache_manager,
        start_time,
        frames_to_mix,
        sample_rate,
        channels,
        decode_policy,
        &property_context,
    );

    for track_id in &composition.track_ids {
        let Some(track) = project.get_track(*track_id) else {
            log::trace!("audio mixer skipped missing Track {track_id}");
            continue;
        };
        mix_track(
            project,
            track,
            &mut mix_buffer,
            assets,
            cache_manager,
            start_time,
            frames_to_mix,
            sample_rate,
            channels,
            decode_policy,
            &property_context,
        );
    }

    mix_buffer
}

#[allow(
    clippy::too_many_arguments,
    reason = "track mixing passes the same explicit render window and device format through the real-time audio path"
)]
fn mix_track(
    project: &Project,
    track: &Track,
    accum_buffer: &mut [f32],
    assets: &[Asset],
    cache_manager: &CacheManager,
    start_time: f64,
    frames: usize,
    sample_rate: u32,
    channels: usize,
    decode_policy: DecodePolicy,
    property_context: &AudioPropertyContext<'_>,
) {
    let mut track_buffer = vec![0.0; accum_buffer.len()];

    // Direct Track Nodes, like direct Composition Nodes, use global time.
    mix_global_nodes(
        project,
        &track.node_ids,
        &mut track_buffer,
        assets,
        cache_manager,
        start_time,
        frames,
        sample_rate,
        channels,
        decode_policy,
        property_context,
    );

    for clip_id in &track.clip_ids {
        let Some(clip) = project.get_clip(*clip_id) else {
            log::trace!("audio mixer skipped missing Clip {clip_id}");
            continue;
        };
        mix_clip(
            project,
            clip,
            &mut track_buffer,
            assets,
            cache_manager,
            start_time,
            frames,
            sample_rate,
            channels,
            decode_policy,
            property_context,
        );
    }

    // Track gain applies exactly once to the sum of direct and Clip audio.
    let scope = format!("track:{}", track.id);
    for frame in 0..frames {
        let global_time = start_time + frame as f64 / sample_rate as f64;
        let volume = volume_at(&track.properties, global_time, property_context, &scope);
        let base = frame * channels;
        for channel in 0..channels {
            accum_buffer[base + channel] += track_buffer[base + channel] * volume;
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "node mixing passes the same explicit render window and device format through the real-time audio path"
)]
fn mix_global_nodes(
    project: &Project,
    node_ids: &[uuid::Uuid],
    accum_buffer: &mut [f32],
    assets: &[Asset],
    cache_manager: &CacheManager,
    start_time: f64,
    frames: usize,
    sample_rate: u32,
    channels: usize,
    decode_policy: DecodePolicy,
    property_context: &AudioPropertyContext<'_>,
) {
    for node_id in node_ids {
        let Some(node) = project.get_node(*node_id) else {
            log::trace!("audio mixer skipped missing Node {node_id}");
            continue;
        };
        if !node.enabled {
            log::trace!("audio mixer skipped disabled Node {node_id}");
            continue;
        }
        let Some(source_key) = audio_source_for_node(node, assets, sample_rate, channels) else {
            continue;
        };
        let mut source = CachedAudioSource::new(source_key, cache_manager, decode_policy);
        let scope = format!("node:{}", node.id);

        for frame in 0..frames {
            let global_time = start_time + frame as f64 / sample_rate as f64;
            mix_source_frame(
                &mut source,
                accum_buffer,
                frame,
                global_time,
                volume_at(node.properties(), global_time, property_context, &scope),
                sample_rate,
                channels,
            );
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "clip mixing additionally needs clip-local time while preserving the explicit audio render window"
)]
fn mix_clip(
    project: &Project,
    clip: &Clip,
    accum_buffer: &mut [f32],
    assets: &[Asset],
    cache_manager: &CacheManager,
    start_time: f64,
    frames: usize,
    sample_rate: u32,
    channels: usize,
    decode_policy: DecodePolicy,
    property_context: &AudioPropertyContext<'_>,
) {
    if clip.duration.into_inner() <= 0.0 {
        return;
    }

    for node_id in &clip.node_ids {
        let Some(node) = project.get_node(*node_id) else {
            log::trace!(
                "audio mixer skipped missing Node {node_id} in Clip {}",
                clip.id
            );
            continue;
        };
        if !node.enabled {
            log::trace!(
                "audio mixer skipped disabled Node {node_id} in Clip {}",
                clip.id
            );
            continue;
        }
        let Some(source_key) = audio_source_for_node(node, assets, sample_rate, channels) else {
            continue;
        };
        let mut source = CachedAudioSource::new(source_key, cache_manager, decode_policy);
        let clip_scope = format!("clip:{}", clip.id);
        let node_scope = format!("{clip_scope}/node:{}", node.id);

        for frame in 0..frames {
            let global_time = start_time + frame as f64 / sample_rate as f64;
            if global_time < clip.start_time.into_inner() || global_time >= clip.end_time() {
                continue;
            }

            let local_time = clip.local_time(global_time);
            let gain = volume_at(&clip.properties, local_time, property_context, &clip_scope)
                * volume_at(node.properties(), local_time, property_context, &node_scope);
            mix_source_frame(
                &mut source,
                accum_buffer,
                frame,
                local_time,
                gain,
                sample_rate,
                channels,
            );
        }
    }
}

fn audio_source_for_node(
    node: &Node,
    assets: &[Asset],
    sample_rate: u32,
    channels: usize,
) -> Option<AudioSourceKey> {
    let NodeContent::Media(media) = node.content() else {
        // Generator, Reference, and Merge Nodes do not directly produce audio.
        return None;
    };
    let Some(asset) = assets.iter().find(|asset| asset.id == media.asset_id) else {
        log::trace!(
            "audio mixer skipped Media Node {} with missing asset {}",
            node.id,
            media.asset_id
        );
        return None;
    };
    if !matches!(asset.kind, AssetKind::Audio | AssetKind::Video) {
        log::trace!(
            "audio mixer skipped Media Node {} because asset {} is not audio/video",
            node.id,
            asset.id
        );
        return None;
    }
    let channels = u16::try_from(channels).ok()?;
    let format = AudioDecodeFormat::new(sample_rate, channels)?;
    let stream_index = audio_stream_index_for_media(asset, media);
    AudioSourceKey::read(&asset.path, stream_index, format)
        .inspect_err(|error| {
            log::trace!(
                "audio mixer skipped Media Node {} because source identity failed: {error}",
                node.id
            );
        })
        .ok()
}

pub fn audio_stream_index_for_media(
    asset: &Asset,
    media: &crate::model::MediaContent,
) -> Option<usize> {
    media.audio_stream_index.or_else(|| {
        // For an Audio Asset, its primary stream is itself an audio stream. A
        // Video Asset's primary stream is visual and must never be reused as
        // an embedded-audio selection.
        (asset.kind == AssetKind::Audio)
            .then_some(asset.stream_index)
            .flatten()
    })
}

fn mix_source_frame(
    source: &mut CachedAudioSource<'_>,
    destination: &mut [f32],
    destination_frame: usize,
    source_time: f64,
    gain: f32,
    sample_rate: u32,
    channels: usize,
) {
    if !source_time.is_finite() || source_time < 0.0 {
        return;
    }

    let source_position = source_time * sample_rate as f64;
    let first_frame = source_position.floor() as u64;
    let second_frame = first_frame.saturating_add(1);
    let fraction = (source_position - first_frame as f64) as f32;
    let destination_base = destination_frame * channels;

    for channel in 0..channels {
        let Some(first) = source.sample(first_frame, channel) else {
            continue;
        };
        let second = source.sample(second_frame, channel).unwrap_or(first);
        destination[destination_base + channel] += (first + (second - first) * fraction) * gain;
    }
}

struct CachedAudioSource<'a> {
    key: AudioSourceKey,
    cache: &'a CacheManager,
    chunks: LruCache<u64, Option<Arc<AudioChunk>>>,
    decode_policy: DecodePolicy,
}

impl<'a> CachedAudioSource<'a> {
    fn new(key: AudioSourceKey, cache: &'a CacheManager, decode_policy: DecodePolicy) -> Self {
        Self {
            key,
            cache,
            chunks: LruCache::new(NonZeroUsize::new(2).unwrap_or(NonZeroUsize::MIN)),
            decode_policy,
        }
    }

    fn sample(&mut self, frame: u64, channel: usize) -> Option<f32> {
        let chunk_frames = self.key.format.chunk_frames().max(1);
        let chunk_index = frame / chunk_frames;
        if !self.chunks.contains(&chunk_index) {
            let key = AudioChunkKey {
                source: self.key.clone(),
                chunk_index,
            };
            let mut chunk = self.cache.get_audio_chunk(&key);
            if chunk.is_none()
                && matches!(self.decode_policy, DecodePolicy::DecodeMissing)
                && !self.cache.audio_chunk_failed(&key)
            {
                match AudioLoader::decode_chunk(&key) {
                    Ok(decoded) => {
                        let decoded = Arc::new(decoded);
                        self.cache.put_audio_chunk_arc(Arc::clone(&decoded));
                        chunk = Some(decoded);
                    }
                    Err(error) => {
                        log::error!(
                            "Failed to decode offline audio {:?} stream {:?}, chunk {}: {error}",
                            key.source.identity.canonical_path,
                            key.source.stream_index,
                            key.chunk_index
                        );
                        self.cache.mark_audio_chunk_failed(key);
                    }
                }
            }
            self.chunks.put(chunk_index, chunk);
        }
        let chunk = self.chunks.get(&chunk_index)?;
        chunk.as_ref()?.sample(frame, channel)
    }
}

/// Resolve source-local windows for one authoritative Composition output
/// window. Direct Nodes use global time; Clip Nodes use the Clip's half-open
/// timeline intersection and canonical local-time mapping.
pub fn audio_window_requests_for_composition(
    project: &Project,
    composition: &Composition,
    start_sample: u64,
    frames: usize,
    sample_rate: u32,
) -> Vec<AudioWindowRequest> {
    if frames == 0 || sample_rate == 0 {
        return Vec::new();
    }
    let frames = frames_inside_composition(composition, start_sample, frames, sample_rate);
    if frames == 0 {
        return Vec::new();
    }
    let mut requests = HashSet::new();
    collect_node_windows(
        project,
        &composition.node_ids,
        None,
        start_sample,
        frames,
        sample_rate,
        &mut requests,
    );
    for track_id in &composition.track_ids {
        let Some(track) = project.get_track(*track_id) else {
            continue;
        };
        collect_node_windows(
            project,
            &track.node_ids,
            None,
            start_sample,
            frames,
            sample_rate,
            &mut requests,
        );
        for clip_id in &track.clip_ids {
            let Some(clip) = project.get_clip(*clip_id) else {
                continue;
            };
            collect_node_windows(
                project,
                &clip.node_ids,
                Some(clip),
                start_sample,
                frames,
                sample_rate,
                &mut requests,
            );
        }
    }
    requests.into_iter().collect()
}

fn frames_inside_composition(
    composition: &Composition,
    start_sample: u64,
    requested_frames: usize,
    sample_rate: u32,
) -> usize {
    let duration = composition.duration;
    if !duration.is_finite() || duration <= 0.0 || sample_rate == 0 {
        return 0;
    }
    let end_sample_exclusive = (duration * f64::from(sample_rate)).ceil() as u64;
    let available = end_sample_exclusive.saturating_sub(start_sample);
    requested_frames.min(usize::try_from(available).unwrap_or(usize::MAX))
}

#[allow(
    clippy::too_many_arguments,
    reason = "window planning keeps authoritative timeline and source-local bounds explicit"
)]
fn collect_node_windows(
    project: &Project,
    node_ids: &[uuid::Uuid],
    clip: Option<&Clip>,
    start_sample: u64,
    frames: usize,
    sample_rate: u32,
    requests: &mut HashSet<AudioWindowRequest>,
) {
    let Some((first_source_frame, last_source_frame)) =
        source_frame_bounds(clip, start_sample, frames, sample_rate)
    else {
        return;
    };
    for node_id in node_ids {
        let Some(node) = project.get_node(*node_id) else {
            continue;
        };
        if !node.enabled {
            continue;
        }
        let NodeContent::Media(media) = node.content() else {
            continue;
        };
        let Some(asset) = project.get_asset(media.asset_id) else {
            continue;
        };
        if !matches!(asset.kind, AssetKind::Audio | AssetKind::Video) {
            continue;
        }
        requests.insert(AudioWindowRequest {
            source: AudioSourceSpec {
                path: asset.path.clone(),
                stream_index: audio_stream_index_for_media(asset, media),
            },
            first_source_frame,
            last_source_frame,
        });
    }
}

fn source_frame_bounds(
    clip: Option<&Clip>,
    start_sample: u64,
    frames: usize,
    sample_rate: u32,
) -> Option<(u64, u64)> {
    if let Some(clip) = clip {
        if clip.duration.into_inner() <= 0.0 {
            return None;
        }
        let window_last = start_sample.saturating_add(frames.saturating_sub(1) as u64);
        let clip_first =
            (clip.start_time.into_inner().max(0.0) * f64::from(sample_rate)).ceil() as u64;
        let clip_end_exclusive = (clip.end_time().max(0.0) * f64::from(sample_rate)).ceil() as u64;
        let first_global = start_sample.max(clip_first);
        let last_global = window_last.min(clip_end_exclusive.saturating_sub(1));
        if first_global > last_global || clip_end_exclusive == 0 {
            return None;
        }
        let first_time = first_global as f64 / f64::from(sample_rate);
        let last_time = last_global as f64 / f64::from(sample_rate);
        let first_position = clip.local_time(first_time).max(0.0) * f64::from(sample_rate);
        let last_position = clip.local_time(last_time).max(0.0) * f64::from(sample_rate);
        let minimum = first_position.min(last_position).floor() as u64;
        // The mixer linearly interpolates with the following source frame.
        let maximum = first_position.max(last_position).floor() as u64;
        return Some((minimum, maximum.saturating_add(1)));
    }

    Some((start_sample, start_sample.saturating_add(frames as u64)))
}

/// Synchronous cold-cache path used by export/offline rendering. Decoding is
/// chunk-bounded and each output block is mixed before its chunks may be
/// evicted by a later block.
#[allow(
    clippy::too_many_arguments,
    reason = "offline audio rendering mirrors the explicit real-time mix boundary"
)]
pub fn render_samples(
    assets: &[Asset],
    project: &Project,
    composition: &Composition,
    cache_manager: &CacheManager,
    start_sample: u64,
    frames_to_render: usize,
    sample_rate: u32,
    channels: u32,
    plugin_manager: &PluginManager,
) -> Vec<f32> {
    let property_evaluators = plugin_manager.get_property_evaluators();
    mix_samples_with_policy(
        assets,
        project,
        composition,
        cache_manager,
        start_sample,
        frames_to_render,
        sample_rate,
        channels,
        DecodePolicy::DecodeMissing,
        property_evaluators.as_ref(),
    )
}

#[cfg(test)]
mod tests;
