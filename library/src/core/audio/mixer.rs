use crate::cache::CacheManager;
use crate::core::audio::cache::{AudioChunk, AudioChunkKey, AudioDecodeFormat, AudioSourceKey};
use crate::core::audio::loader::AudioLoader;
use crate::model::asset::{Asset, AssetKind};
use crate::model::project::{Composition, Project};
use crate::model::property::PropertyMap;
use crate::model::{Clip, Node, NodeContent, Track};
use lru::LruCache;
use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::sync::Arc;

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
) -> Vec<f32> {
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
        );
    }

    // Track gain applies exactly once to the sum of direct and Clip audio.
    for frame in 0..frames {
        let global_time = start_time + frame as f64 / sample_rate as f64;
        let volume = volume_at(&track.properties, global_time);
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

        for frame in 0..frames {
            let global_time = start_time + frame as f64 / sample_rate as f64;
            mix_source_frame(
                &mut source,
                accum_buffer,
                frame,
                global_time,
                volume_at(&node.properties, global_time),
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

        for frame in 0..frames {
            let global_time = start_time + frame as f64 / sample_rate as f64;
            if global_time < clip.start_time.into_inner() || global_time >= clip.end_time() {
                continue;
            }

            let local_time = clip.local_time(global_time);
            let gain =
                volume_at(&clip.properties, local_time) * volume_at(&node.properties, local_time);
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
    let NodeContent::Media(media) = &node.content else {
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

fn volume_at(properties: &PropertyMap, time: f64) -> f32 {
    properties
        .get("volume")
        .map(|property| property.evaluate_at(time))
        .and_then(|value| value.get_as::<f64>())
        .unwrap_or(1.0) as f32
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
        let NodeContent::Media(media) = &node.content else {
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
) -> Vec<f32> {
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
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::property::{Property, PropertyValue};
    use crate::model::{MediaContent, NodeContainer};
    use ordered_float::OrderedFloat;
    use std::path::PathBuf;

    #[derive(Default)]
    struct TestAudioFiles(Vec<PathBuf>);

    impl TestAudioFiles {
        fn create(&mut self) -> String {
            let path = std::env::temp_dir()
                .join(format!("ruvie-audio-mixer-{}.source", uuid::Uuid::new_v4()));
            std::fs::write(&path, b"identity-only audio test source").unwrap();
            self.0.push(path.clone());
            path.to_string_lossy().into_owned()
        }
    }

    impl Drop for TestAudioFiles {
        fn drop(&mut self) {
            for path in &self.0 {
                if let Err(error) = std::fs::remove_file(path) {
                    log::warn!("failed to remove audio mixer test source: {error}");
                }
            }
        }
    }

    fn add_audio_node(
        project: &mut Project,
        cache_manager: &CacheManager,
        files: &mut TestAudioFiles,
        samples: Vec<f32>,
    ) -> uuid::Uuid {
        add_media_node(
            project,
            cache_manager,
            files,
            samples,
            AssetKind::Audio,
            None,
            true,
        )
        .0
    }

    fn add_media_node(
        project: &mut Project,
        cache_manager: &CacheManager,
        files: &mut TestAudioFiles,
        samples: Vec<f32>,
        kind: AssetKind,
        audio_stream_index: Option<usize>,
        enabled: bool,
    ) -> (uuid::Uuid, String) {
        let is_video = matches!(&kind, AssetKind::Video);
        let mut asset = Asset::new("audio", &files.create(), kind);
        if is_video {
            // Preserve a distinct visual stream while the Media Node selects
            // its embedded audio stream explicitly.
            asset.stream_index = Some(0);
        }
        let format = AudioDecodeFormat::new(4, 1).unwrap();
        let source = AudioSourceKey::read(&asset.path, audio_stream_index, format).unwrap();
        let samples_per_chunk = format.chunk_frames() as usize * usize::from(format.channels);
        for (chunk_index, chunk_samples) in samples.chunks(samples_per_chunk).enumerate() {
            cache_manager.put_audio_chunk(
                AudioChunk::new(
                    AudioChunkKey {
                        source: source.clone(),
                        chunk_index: chunk_index as u64,
                    },
                    chunk_samples.to_vec(),
                )
                .unwrap(),
            );
        }
        let mut node = Node::new(
            "audio",
            NodeContent::Media(MediaContent {
                asset_id: asset.id,
                stream_index: is_video.then_some(0),
                audio_stream_index,
            }),
        );
        node.enabled = enabled;
        let node_id = node.id;
        let path = asset.path.clone();
        project.assets.push(asset);
        project.add_node(node);
        (node_id, path)
    }

    fn set_volume(properties: &mut PropertyMap, volume: f64) {
        properties.set(
            "volume".to_string(),
            Property::constant(PropertyValue::Number(OrderedFloat(volume))),
        );
    }

    #[derive(Clone, Copy, Debug)]
    enum TestNodeScope {
        Composition,
        Track,
        Clip,
    }

    fn assert_disabled_media_contract(scope: TestNodeScope) {
        let mut project = Project::new("disabled audio contract");
        let (composition, mut track) = Composition::new("main", 16, 16, 4.0, 1.0);
        let composition_id = composition.id;
        let track_id = track.id;
        if !matches!(scope, TestNodeScope::Composition) {
            set_volume(&mut track.properties, 0.5);
        }
        project.add_track(track);
        project.add_composition(composition);

        let container = match scope {
            TestNodeScope::Composition => NodeContainer::Composition(composition_id),
            TestNodeScope::Track => NodeContainer::Track(track_id),
            TestNodeScope::Clip => {
                // At 4 Hz, only output frames 1 and 2 are inside [0.25, 0.75).
                let mut clip = Clip::new("half-open", 0.25, 0.5);
                set_volume(&mut clip.properties, 0.5);
                let clip_id = clip.id;
                project.add_clip(clip);
                project.attach_clip_to_track(track_id, clip_id).unwrap();
                NodeContainer::Clip(clip_id)
            }
        };

        let cache = CacheManager::new();
        let mut files = TestAudioFiles::default();
        let mut enabled_sources = HashSet::new();
        let mut disabled_sources = HashSet::new();
        for (sample, kind, stream_index, enabled) in [
            (1.0, AssetKind::Audio, None, true),
            (10.0, AssetKind::Audio, None, false),
            (2.0, AssetKind::Video, Some(2), true),
            (20.0, AssetKind::Video, Some(3), false),
        ] {
            let (node_id, path) = add_media_node(
                &mut project,
                &cache,
                &mut files,
                vec![sample; 8],
                kind,
                stream_index,
                enabled,
            );
            project
                .attach_node_to_container(container, node_id)
                .unwrap();
            let source = (path, stream_index);
            if enabled {
                enabled_sources.insert(source);
            } else {
                disabled_sources.insert(source);
            }
        }

        let composition = project.get_composition(composition_id).unwrap();
        let expected = match scope {
            // Enabled Audio (1) + enabled embedded Video audio (2).
            TestNodeScope::Composition => vec![3.0; 4],
            // Track volume is applied once to direct Track audio.
            TestNodeScope::Track => vec![1.5; 4],
            // Clip and Track gains are both 0.5, and the Clip end is excluded.
            TestNodeScope::Clip => vec![0.0, 0.75, 0.75, 0.0],
        };
        assert_eq!(
            mix_samples(&project.assets, &project, composition, &cache, 0, 4, 4, 1),
            expected
        );
        assert_eq!(
            render_samples(&project.assets, &project, composition, &cache, 0, 4, 4, 1),
            expected
        );

        let requests = audio_window_requests_for_composition(&project, composition, 0, 4, 4);
        let requested_sources = requests
            .iter()
            .map(|request| (request.source.path.clone(), request.source.stream_index))
            .collect::<HashSet<_>>();
        assert_eq!(requested_sources, enabled_sources);
        assert!(requested_sources.is_disjoint(&disabled_sources));
        assert_eq!(requests.len(), 2);
        assert!(
            requests
                .iter()
                .any(|request| request.source.stream_index == Some(2)),
            "enabled Video embedded-audio stream was not planned"
        );
        let expected_bounds = match scope {
            TestNodeScope::Composition | TestNodeScope::Track => (0, 4),
            TestNodeScope::Clip => (0, 2),
        };
        assert!(requests.iter().all(|request| {
            (request.first_source_frame, request.last_source_frame) == expected_bounds
        }));
    }

    #[test]
    fn disabled_composition_media_is_no_output_for_mix_render_and_prefetch() {
        assert_disabled_media_contract(TestNodeScope::Composition);
    }

    #[test]
    fn disabled_track_media_is_no_output_for_mix_render_and_prefetch() {
        assert_disabled_media_contract(TestNodeScope::Track);
    }

    #[test]
    fn disabled_clip_media_is_no_output_for_mix_render_and_prefetch() {
        assert_disabled_media_contract(TestNodeScope::Clip);
    }

    #[test]
    fn mixes_every_top_level_track_and_all_audio_nodes() {
        let mut project = Project::new("audio test");
        let (composition, first_track) = Composition::new("main", 1920, 1080, 30.0, 1.0);
        let composition_id = composition.id;
        let first_track_id = first_track.id;
        project.add_track(first_track);
        project.add_composition(composition);

        let second_track = Track::new("second");
        let second_track_id = second_track.id;
        project.add_track(second_track);
        project
            .attach_track_to_composition(composition_id, second_track_id)
            .unwrap();

        let cache_manager = CacheManager::new();
        let mut files = TestAudioFiles::default();
        for (track_id, sample) in [(first_track_id, 0.25), (second_track_id, 0.5)] {
            let clip = Clip::new("audio clip", 0.0, 1.0);
            let clip_id = clip.id;
            project.add_clip(clip);
            project.attach_clip_to_track(track_id, clip_id).unwrap();

            // Multiple Nodes in a Clip are additive; neither output_node_id nor
            // image graph shape decides which Nodes contribute audio.
            for node_sample in [sample / 2.0, sample / 2.0] {
                let node_id = add_audio_node(
                    &mut project,
                    &cache_manager,
                    &mut files,
                    vec![node_sample; 4],
                );
                project
                    .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
                    .unwrap();
            }
        }

        let composition = project.get_composition(composition_id).unwrap();
        let mixed = mix_samples(
            &project.assets,
            &project,
            composition,
            &cache_manager,
            0,
            4,
            4,
            1,
        );

        assert_eq!(mixed, vec![0.75; 4]);
    }

    #[test]
    fn applies_clip_timing_trim_and_stretch_per_output_frame() {
        let mut project = Project::new("audio timing test");
        let (composition, track) = Composition::new("main", 1920, 1080, 30.0, 2.0);
        let composition_id = composition.id;
        let track_id = track.id;
        project.add_track(track);
        project.add_composition(composition);

        let mut clip = Clip::new("retimed", 0.5, 1.0);
        clip.trim_in = OrderedFloat(0.25);
        clip.time_stretch = OrderedFloat(2.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();

        let cache_manager = CacheManager::new();
        let mut files = TestAudioFiles::default();
        let node_id = add_audio_node(
            &mut project,
            &cache_manager,
            &mut files,
            (0..12).map(|sample| sample as f32).collect(),
        );
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
            .unwrap();

        let composition = project.get_composition(composition_id).unwrap();
        let mixed = mix_samples(
            &project.assets,
            &project,
            composition,
            &cache_manager,
            0,
            6,
            4,
            1,
        );

        // t=0.5 maps to source 0.25 (sample 1), then advances two source
        // samples for each output sample because time_stretch is 2.
        assert_eq!(mixed, vec![0.0, 0.0, 1.0, 3.0, 5.0, 7.0]);
    }

    #[test]
    fn includes_direct_nodes_and_applies_track_volume_once() {
        let mut project = Project::new("audio direct-node test");
        let (composition, mut track) = Composition::new("main", 1920, 1080, 30.0, 1.0);
        let composition_id = composition.id;
        let track_id = track.id;
        set_volume(&mut track.properties, 0.5);
        project.add_track(track);
        project.add_composition(composition);

        let cache_manager = CacheManager::new();
        let mut files = TestAudioFiles::default();
        let composition_node =
            add_audio_node(&mut project, &cache_manager, &mut files, vec![1.0; 4]);
        project
            .attach_node_to_container(NodeContainer::Composition(composition_id), composition_node)
            .unwrap();

        let track_node = add_audio_node(&mut project, &cache_manager, &mut files, vec![2.0; 4]);
        project
            .attach_node_to_container(NodeContainer::Track(track_id), track_node)
            .unwrap();

        let clip = Clip::new("audio clip", 0.0, 1.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();
        let clip_node = add_audio_node(&mut project, &cache_manager, &mut files, vec![4.0; 4]);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), clip_node)
            .unwrap();

        let composition = project.get_composition(composition_id).unwrap();
        let mixed = mix_samples(
            &project.assets,
            &project,
            composition,
            &cache_manager,
            0,
            4,
            4,
            1,
        );

        // Composition direct: 1. Track direct + Clip: (2 + 4) * 0.5.
        assert_eq!(mixed, vec![4.0; 4]);
    }

    #[test]
    fn source_window_uses_clip_local_time_and_explicit_audio_stream() {
        let mut project = Project::new("late clip audio window");
        let (composition, track) = Composition::new("main", 16, 16, 4.0, 200.0);
        let composition_id = composition.id;
        let track_id = track.id;
        project.add_track(track);
        project.add_composition(composition);

        let mut clip = Clip::new("late retimed clip", 100.0, 2.0);
        clip.trim_in = OrderedFloat(0.25);
        clip.time_stretch = OrderedFloat(2.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();

        let mut asset = Asset::new("multi stream", "/fixture/multi_audio.mkv", AssetKind::Video);
        asset.stream_index = Some(0);
        let asset_id = asset.id;
        project.assets.push(asset);
        let node = Node::new(
            "audio stream two",
            NodeContent::Media(MediaContent {
                asset_id,
                stream_index: Some(0),
                audio_stream_index: Some(2),
            }),
        );
        let node_id = node.id;
        project.add_node(node);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
            .unwrap();

        // The global window starts before the Clip (99.75s). Its contributing
        // samples at 100.0..100.75 map to source frames 1,3,5,7; frame 8 is
        // additionally required for interpolation.
        let requests = audio_window_requests_for_composition(
            &project,
            project.get_composition(composition_id).unwrap(),
            399,
            5,
            4,
        );
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].source.stream_index, Some(2));
        assert_eq!(requests[0].first_source_frame, 1);
        assert_eq!(requests[0].last_source_frame, 8);
    }

    #[test]
    fn visual_stream_is_never_reused_as_embedded_audio_selection() {
        let mut video = Asset::new("video", "/fixture/video.mkv", AssetKind::Video);
        video.stream_index = Some(0);
        let mut audio = Asset::new("audio", "/fixture/audio.mka", AssetKind::Audio);
        audio.stream_index = Some(2);
        let mut media = MediaContent {
            asset_id: video.id,
            stream_index: Some(0),
            audio_stream_index: None,
        };

        assert_eq!(audio_stream_index_for_media(&video, &media), None);
        media.audio_stream_index = Some(1);
        assert_eq!(audio_stream_index_for_media(&video, &media), Some(1));
        media.asset_id = audio.id;
        media.audio_stream_index = None;
        assert_eq!(audio_stream_index_for_media(&audio, &media), Some(2));
    }

    #[test]
    fn composition_range_is_half_open_for_direct_and_scheduled_audio() {
        let mut project = Project::new("composition audio range");
        let (composition, track) = Composition::new("main", 16, 16, 4.0, 1.0);
        let composition_id = composition.id;
        project.add_track(track);
        project.add_composition(composition);
        let cache = CacheManager::new();
        let mut files = TestAudioFiles::default();
        let node_id = add_audio_node(&mut project, &cache, &mut files, vec![1.0; 8]);
        project
            .attach_node_to_container(NodeContainer::Composition(composition_id), node_id)
            .unwrap();
        let composition = project.get_composition(composition_id).unwrap();

        let mixed = mix_samples(&project.assets, &project, composition, &cache, 2, 4, 4, 1);
        assert_eq!(mixed, vec![1.0, 1.0, 0.0, 0.0]);
        assert!(audio_window_requests_for_composition(&project, composition, 4, 4, 4).is_empty());
    }
}
