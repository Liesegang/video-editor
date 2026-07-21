use crate::cache::CacheManager;
use crate::core::audio::cache::{AudioChunk, AudioChunkKey, AudioDecodeFormat, AudioSourceKey};
use crate::core::audio::loader::AudioLoader;
use crate::model::asset::{Asset, AssetKind};
use crate::model::project::{Composition, PortOwner, Project};
use crate::model::{Node, NodeContent};
use crate::plugin::{PluginManager, PropertyEvaluatorRegistry};
use lru::LruCache;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::Arc;

mod graph_evaluation;
mod property_evaluation;

use graph_evaluation::AudioGraphEvaluator;

/// Resolve enabled Media leaves through the canonical typed Audio routes.
/// The Timeline uses this for waveform source discovery while mixing retains
/// distinct route identities for repeated Composition Instance placements.
pub(crate) fn routed_audio_media_nodes(project: &Project, owner: PortOwner) -> Vec<uuid::Uuid> {
    graph_evaluation::routed_audio_media_nodes(project, owner)
}

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

/// Mixes the Media leaves that structurally reach the Composition's authored
/// Audio output. Image bindings and disconnected Media Nodes never contribute.
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
        plugin_manager,
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
    plugin_manager: &PluginManager,
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

    let evaluator =
        AudioGraphEvaluator::new(project, composition, plugin_manager, property_evaluators);
    let mut sources = evaluator
        .routes
        .iter()
        .map(|route| {
            project
                .get_node(route.node_id)
                .and_then(|node| audio_source_for_node(node, assets, sample_rate, channels))
                .map(|source_key| CachedAudioSource::new(source_key, cache_manager, decode_policy))
        })
        .collect::<Vec<_>>();
    let mut scope_path = HashSet::new();
    for frame in 0..frames_to_mix {
        let timeline_time =
            (start_sample.saturating_add(frame as u64)) as f64 / f64::from(sample_rate);
        for (route, source) in evaluator.routes.iter().zip(&mut sources) {
            let Some(source) = source else {
                continue;
            };
            let Some(leaf) = evaluator.evaluate_route(route, timeline_time, &mut scope_path) else {
                continue;
            };
            mix_source_frame(
                source,
                &mut mix_buffer,
                frame,
                leaf.source_time,
                leaf.gain,
                sample_rate,
                channels,
            );
        }
    }

    mix_buffer
}

fn audio_source_for_node(
    node: &Node,
    assets: &[Asset],
    sample_rate: u32,
    channels: usize,
) -> Option<AudioSourceKey> {
    let NodeContent::Media(media) = node.content() else {
        // Generators, Composition Instances, and Merge Nodes are not audio
        // decode leaves.
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

/// Resolve source-local windows through the same request-local graph/time
/// evaluation used by mixing. This keeps Composition Instance clip timing and
/// explicit Time remaps authoritative for both prefetch and sample reads.
pub fn audio_window_requests_for_composition(
    project: &Project,
    composition: &Composition,
    start_sample: u64,
    frames: usize,
    sample_rate: u32,
    plugin_manager: &PluginManager,
) -> Vec<AudioWindowRequest> {
    if frames == 0 || sample_rate == 0 {
        return Vec::new();
    }
    let frames = frames_inside_composition(composition, start_sample, frames, sample_rate);
    if frames == 0 {
        return Vec::new();
    }
    let property_evaluators = plugin_manager.get_property_evaluators();
    let evaluator = AudioGraphEvaluator::new(
        project,
        composition,
        plugin_manager,
        property_evaluators.as_ref(),
    );
    let sources = evaluator
        .routes
        .iter()
        .map(|route| {
            let node = project.get_node(route.node_id)?;
            let NodeContent::Media(media) = node.content() else {
                return None;
            };
            let asset = project.get_asset(media.asset_id)?;
            matches!(asset.kind, AssetKind::Audio | AssetKind::Video).then(|| AudioSourceSpec {
                path: asset.path.clone(),
                stream_index: audio_stream_index_for_media(asset, media),
            })
        })
        .collect::<Vec<_>>();
    // Keep request bounds aligned with the precomputed routes while sampling;
    // source identity strings are cloned only once per contributing route.
    let mut route_windows = vec![None::<(u64, u64)>; evaluator.routes.len()];
    let mut scope_path = HashSet::new();
    for frame in 0..frames {
        let timeline_time =
            (start_sample.saturating_add(frame as u64)) as f64 / f64::from(sample_rate);
        for ((route, source), bounds) in evaluator
            .routes
            .iter()
            .zip(&sources)
            .zip(&mut route_windows)
        {
            if source.is_none() {
                continue;
            }
            let Some(leaf) = evaluator.evaluate_route(route, timeline_time, &mut scope_path) else {
                continue;
            };
            if !leaf.source_time.is_finite() || leaf.source_time < 0.0 {
                continue;
            }
            let first = (leaf.source_time * f64::from(sample_rate)).floor() as u64;
            let last = first.saturating_add(1);
            if let Some(bounds) = bounds {
                bounds.0 = bounds.0.min(first);
                bounds.1 = bounds.1.max(last);
            } else {
                *bounds = Some((first, last));
            }
        }
    }
    let mut windows = HashMap::<AudioSourceSpec, (u64, u64)>::new();
    for (source, bounds) in sources.into_iter().zip(route_windows) {
        let (Some(source), Some((first, last))) = (source, bounds) else {
            continue;
        };
        windows
            .entry(source)
            .and_modify(|bounds| {
                bounds.0 = bounds.0.min(first);
                bounds.1 = bounds.1.max(last);
            })
            .or_insert((first, last));
    }
    windows
        .into_iter()
        .map(
            |(source, (first_source_frame, last_source_frame))| AudioWindowRequest {
                source,
                first_source_frame,
                last_source_frame,
            },
        )
        .collect()
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
        plugin_manager,
        DecodePolicy::DecodeMissing,
        property_evaluators.as_ref(),
    )
}

#[cfg(test)]
mod composition_instance_tests;
#[cfg(test)]
mod tests;
