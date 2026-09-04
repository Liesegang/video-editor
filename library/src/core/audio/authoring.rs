//! Offline/realtime-window audio mixing for the Timeline-first authoring model.
//!
//! Ordinary Timeline placements are scheduled directly. This module never
//! synthesizes the pre-v1 `Project`, container Nodes, or structural sound
//! merges. Decode data remains in the bounded one-second audio cache.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::Arc;

use lru::LruCache;
use thiserror::Error;
use uuid::Uuid;

use crate::core::audio::cache::{AudioChunk, AudioChunkKey, AudioDecodeFormat, AudioSourceKey};
use crate::core::audio::loader::AudioLoader;
use crate::core::cache::CacheManager;
use crate::core::render_plan::map_composition_time;
use crate::model::authoring::{
    AuthoringProject, MediaTime, SourceRef, TimelineId, TimelineItem, TimelineItemId,
    TimelineTrackKind,
};
use crate::model::project::property::{PropertyMap, TryGetProperty};
use crate::model::{Asset, AssetKind};

pub const AUTHORING_AUDIO_SAMPLE_RATE: u32 = 48_000;
pub const AUTHORING_AUDIO_CHANNELS: u16 = 2;
/// Callers stream longer renders as consecutive windows. Keeping a single
/// request to one second bounds both its output allocation and working set.
pub const MAX_AUTHORING_AUDIO_WINDOW_FRAMES: usize = AUTHORING_AUDIO_SAMPLE_RATE as usize;

#[derive(Debug, Error)]
pub enum AuthoringAudioError {
    #[error("invalid authoring audio request: {0}")]
    InvalidRequest(String),
    #[error("invalid Timeline-first audio schedule: {0}")]
    InvalidSchedule(String),
    #[error("cannot evaluate {scope}.gain: {message}")]
    Gain { scope: String, message: String },
    #[error("cannot decode Audio Asset {asset_id} at '{path}': {message}")]
    Decode {
        asset_id: Uuid,
        path: String,
        message: String,
    },
}

/// A reusable schedule and source cache for one Timeline definition.
///
/// Only `AssetKind::Audio` is decoded in this first vertical slice. The
/// authoring Asset model does not yet persist an embedded-audio stream for a
/// `Video` Asset, so such placements are reported by
/// [`Self::unsupported_video_assets`] instead of guessing a stream index.
pub struct AuthoringAudioMixer<'a> {
    project: &'a AuthoringProject,
    cache: &'a CacheManager,
    timeline_id: TimelineId,
    routes: Vec<AudioRoute>,
    sources: HashMap<Uuid, CachedAudioSource<'a>>,
    unsupported_video_assets: Vec<Uuid>,
}

impl<'a> AuthoringAudioMixer<'a> {
    pub fn root(
        project: &'a AuthoringProject,
        cache: &'a CacheManager,
    ) -> Result<Self, AuthoringAudioError> {
        Self::new(project, cache, project.root_timeline_id)
    }

    pub fn new(
        project: &'a AuthoringProject,
        cache: &'a CacheManager,
        timeline_id: TimelineId,
    ) -> Result<Self, AuthoringAudioError> {
        let mut routes = Vec::new();
        let mut unsupported_video_assets = Vec::new();
        let mut composition_items = Vec::new();
        let mut timeline_stack = HashSet::new();
        collect_routes(
            project,
            timeline_id,
            &mut composition_items,
            &mut timeline_stack,
            &mut routes,
            &mut unsupported_video_assets,
        )?;
        unsupported_video_assets.sort_unstable();
        unsupported_video_assets.dedup();
        Ok(Self {
            project,
            cache,
            timeline_id,
            routes,
            sources: HashMap::new(),
            unsupported_video_assets,
        })
    }

    pub fn timeline_id(&self) -> TimelineId {
        self.timeline_id
    }

    pub fn has_audio_routes(&self) -> bool {
        !self.routes.is_empty()
    }

    /// Video placements which might contain audio but cannot yet be selected
    /// safely because authoring Assets persist only their primary video stream.
    pub fn unsupported_video_assets(&self) -> &[Uuid] {
        &self.unsupported_video_assets
    }

    /// Render one half-open 48 kHz stereo sample window.
    ///
    /// Decode and property failures are returned. They are never converted to
    /// a successful silent buffer. A window with no active Audio placement is
    /// legitimately silent.
    pub fn render_window(
        &mut self,
        start_frame: u64,
        frame_count: usize,
    ) -> Result<Vec<f32>, AuthoringAudioError> {
        if frame_count > MAX_AUTHORING_AUDIO_WINDOW_FRAMES {
            return Err(AuthoringAudioError::InvalidRequest(format!(
                "window has {frame_count} frames; maximum is {MAX_AUTHORING_AUDIO_WINDOW_FRAMES}"
            )));
        }
        let final_frame = start_frame
            .checked_add(frame_count as u64)
            .ok_or_else(|| AuthoringAudioError::InvalidRequest("sample range overflowed".into()))?;
        if final_frame > i64::MAX as u64 {
            return Err(AuthoringAudioError::InvalidRequest(
                "sample range exceeds exact MediaTime".into(),
            ));
        }

        let mut output =
            vec![0.0; frame_count.saturating_mul(usize::from(AUTHORING_AUDIO_CHANNELS))];
        for output_frame in 0..frame_count {
            let absolute_frame = start_frame + output_frame as u64;
            let timeline_time = MediaTime::new(absolute_frame as i64, AUTHORING_AUDIO_SAMPLE_RATE)
                .map_err(AuthoringAudioError::InvalidRequest)?;
            for route_index in 0..self.routes.len() {
                let Some(sample) = self.evaluate_route(route_index, timeline_time)? else {
                    continue;
                };
                let source = self.source_for(sample.asset_id)?;
                let stereo = source.sample(sample.source_time)?;
                let base = output_frame * usize::from(AUTHORING_AUDIO_CHANNELS);
                output[base] += stereo[0] * sample.gain;
                output[base + 1] += stereo[1] * sample.gain;
            }
        }
        Ok(output)
    }

    fn evaluate_route(
        &self,
        route_index: usize,
        mut timeline_time: MediaTime,
    ) -> Result<Option<ActiveAudioSample>, AuthoringAudioError> {
        let route = &self.routes[route_index];
        let mut gain = 1.0_f32;
        for composition_item_id in &route.composition_items {
            let item = self.item(*composition_item_id)?;
            let timeline_id = self.item_timeline_id(item)?;
            let timeline = self.project.timelines.get(&timeline_id).ok_or_else(|| {
                AuthoringAudioError::InvalidSchedule(format!("missing Timeline {timeline_id}"))
            })?;
            if timeline_time.is_negative() || timeline_time >= timeline.duration {
                return Ok(None);
            }
            let track = self.project.tracks.get(&item.track_id).ok_or_else(|| {
                AuthoringAudioError::InvalidSchedule(format!(
                    "item {} has no Track {}",
                    item.id, item.track_id
                ))
            })?;
            if !item
                .interval
                .contains(timeline_time)
                .map_err(schedule_error)?
            {
                return Ok(None);
            }
            let SourceRef::Composition(instance) = &item.source else {
                return Err(AuthoringAudioError::InvalidSchedule(format!(
                    "route item {} is no longer a Composition",
                    item.id
                )));
            };
            let nested = self
                .project
                .timelines
                .get(&instance.timeline_id)
                .ok_or_else(|| {
                    AuthoringAudioError::InvalidSchedule(format!(
                        "Composition item {} has no Timeline {}",
                        item.id, instance.timeline_id
                    ))
                })?;
            let Some(nested_time) = map_composition_time(
                item,
                nested.duration,
                &instance.duration_policy,
                timeline_time,
            )
            .map_err(schedule_error)?
            else {
                return Ok(None);
            };
            gain *= gain_at(
                &timeline.authored_properties,
                timeline_time,
                format!("Timeline {}", timeline.id),
            )?;
            gain *= gain_at(
                &track.authored_properties,
                timeline_time,
                format!("Track {}", track.id),
            )?;
            gain *= gain_at(
                &item.authored_properties,
                nested_time,
                format!("Timeline item {}", item.id),
            )?;
            timeline_time = nested_time;
            if gain == 0.0 {
                return Ok(None);
            }
        }

        let leaf = self.item(route.asset_item_id)?;
        let timeline_id = self.item_timeline_id(leaf)?;
        let timeline = self.project.timelines.get(&timeline_id).ok_or_else(|| {
            AuthoringAudioError::InvalidSchedule(format!("missing Timeline {timeline_id}"))
        })?;
        if timeline_time.is_negative() || timeline_time >= timeline.duration {
            return Ok(None);
        }
        if !leaf
            .interval
            .contains(timeline_time)
            .map_err(schedule_error)?
        {
            return Ok(None);
        }
        let track = self.project.tracks.get(&leaf.track_id).ok_or_else(|| {
            AuthoringAudioError::InvalidSchedule(format!(
                "item {} has no Track {}",
                leaf.id, leaf.track_id
            ))
        })?;
        let source_time = leaf
            .time_map
            .local_time(leaf.interval, timeline_time)
            .map_err(schedule_error)?;
        if source_time.is_negative() {
            return Ok(None);
        }
        let asset = self.asset(route.asset_id)?;
        if let Some(duration) = asset.duration {
            if !duration.is_finite() || duration < 0.0 {
                return Err(AuthoringAudioError::InvalidSchedule(format!(
                    "Audio Asset {} has invalid duration metadata",
                    asset.id
                )));
            }
            if source_time.to_seconds_f64() >= duration {
                return Ok(None);
            }
        }

        gain *= gain_at(
            &timeline.authored_properties,
            timeline_time,
            format!("Timeline {}", timeline.id),
        )?;
        gain *= gain_at(
            &track.authored_properties,
            timeline_time,
            format!("Track {}", track.id),
        )?;
        gain *= gain_at(
            &leaf.authored_properties,
            source_time,
            format!("Timeline item {}", leaf.id),
        )?;
        Ok(Some(ActiveAudioSample {
            asset_id: route.asset_id,
            source_time,
            gain,
        }))
    }

    fn source_for(
        &mut self,
        asset_id: Uuid,
    ) -> Result<&mut CachedAudioSource<'a>, AuthoringAudioError> {
        if !self.sources.contains_key(&asset_id) {
            let asset = self.asset(asset_id)?;
            let source = CachedAudioSource::new(asset, self.cache)?;
            self.sources.insert(asset_id, source);
        }
        self.sources.get_mut(&asset_id).ok_or_else(|| {
            AuthoringAudioError::InvalidSchedule(format!(
                "Audio Asset {asset_id} source cache disappeared"
            ))
        })
    }

    fn item(&self, item_id: TimelineItemId) -> Result<&TimelineItem, AuthoringAudioError> {
        self.project.items.get(&item_id).ok_or_else(|| {
            AuthoringAudioError::InvalidSchedule(format!("missing Timeline item {item_id}"))
        })
    }

    fn item_timeline_id(&self, item: &TimelineItem) -> Result<TimelineId, AuthoringAudioError> {
        self.project
            .tracks
            .get(&item.track_id)
            .map(|track| track.timeline_id)
            .ok_or_else(|| {
                AuthoringAudioError::InvalidSchedule(format!(
                    "item {} has no Track {}",
                    item.id, item.track_id
                ))
            })
    }

    fn asset(&self, asset_id: Uuid) -> Result<&Asset, AuthoringAudioError> {
        self.project
            .assets
            .iter()
            .find(|asset| asset.id == asset_id)
            .ok_or_else(|| {
                AuthoringAudioError::InvalidSchedule(format!("missing Audio Asset {asset_id}"))
            })
    }
}

/// Convenience wrapper for one independently bounded window.
pub fn render_authoring_audio_window(
    project: &AuthoringProject,
    cache: &CacheManager,
    timeline_id: TimelineId,
    start_frame: u64,
    frame_count: usize,
) -> Result<Vec<f32>, AuthoringAudioError> {
    AuthoringAudioMixer::new(project, cache, timeline_id)?.render_window(start_frame, frame_count)
}

#[derive(Clone, Debug)]
struct AudioRoute {
    composition_items: Vec<TimelineItemId>,
    asset_item_id: TimelineItemId,
    asset_id: Uuid,
}

#[derive(Clone, Copy, Debug)]
struct ActiveAudioSample {
    asset_id: Uuid,
    source_time: MediaTime,
    gain: f32,
}

fn collect_routes(
    project: &AuthoringProject,
    timeline_id: TimelineId,
    composition_items: &mut Vec<TimelineItemId>,
    timeline_stack: &mut HashSet<TimelineId>,
    routes: &mut Vec<AudioRoute>,
    unsupported_video_assets: &mut Vec<Uuid>,
) -> Result<(), AuthoringAudioError> {
    if !timeline_stack.insert(timeline_id) {
        return Err(AuthoringAudioError::InvalidSchedule(format!(
            "nested Timeline cycle reaches {timeline_id}"
        )));
    }
    let timeline = project.timelines.get(&timeline_id).ok_or_else(|| {
        AuthoringAudioError::InvalidSchedule(format!("missing Timeline {timeline_id}"))
    })?;
    for track_id in &timeline.track_order {
        let track = project.tracks.get(track_id).ok_or_else(|| {
            AuthoringAudioError::InvalidSchedule(format!(
                "Timeline {timeline_id} has no Track {track_id}"
            ))
        })?;
        if !matches!(
            track.kind,
            TimelineTrackKind::Audio | TimelineTrackKind::AudioVisual
        ) {
            continue;
        }
        let mut items = project
            .items
            .values()
            .filter(|item| item.track_id == *track_id)
            .collect::<Vec<_>>();
        items.sort_by_key(|item| (item.layer, item.id));
        for item in items {
            match &item.source {
                SourceRef::Asset { asset_id } => {
                    let asset = project
                        .assets
                        .iter()
                        .find(|asset| asset.id == *asset_id)
                        .ok_or_else(|| {
                            AuthoringAudioError::InvalidSchedule(format!(
                                "item {} has no Asset {asset_id}",
                                item.id
                            ))
                        })?;
                    match asset.kind {
                        AssetKind::Audio => routes.push(AudioRoute {
                            composition_items: composition_items.clone(),
                            asset_item_id: item.id,
                            asset_id: *asset_id,
                        }),
                        AssetKind::Video => unsupported_video_assets.push(*asset_id),
                        AssetKind::Image | AssetKind::Model3D | AssetKind::Other => {}
                    }
                }
                SourceRef::Composition(instance) => {
                    composition_items.push(item.id);
                    collect_routes(
                        project,
                        instance.timeline_id,
                        composition_items,
                        timeline_stack,
                        routes,
                        unsupported_video_assets,
                    )?;
                    composition_items.pop();
                }
                SourceRef::Text { .. }
                | SourceRef::Shape { .. }
                | SourceRef::Solid { .. }
                | SourceRef::Module(_) => {}
            }
        }
    }
    timeline_stack.remove(&timeline_id);
    Ok(())
}

fn gain_at(
    properties: &PropertyMap,
    time: MediaTime,
    scope: String,
) -> Result<f32, AuthoringAudioError> {
    let Some(property) = properties.get("gain") else {
        return Ok(1.0);
    };
    let value = property
        .evaluate_at(time.to_seconds_f64())
        .map_err(|error| AuthoringAudioError::Gain {
            scope: scope.clone(),
            message: error.to_string(),
        })?;
    let gain = f64::try_get(&value).ok_or_else(|| AuthoringAudioError::Gain {
        scope: scope.clone(),
        message: "authored value is not numeric".into(),
    })?;
    if !gain.is_finite() || gain < f32::MIN as f64 || gain > f32::MAX as f64 {
        return Err(AuthoringAudioError::Gain {
            scope,
            message: "authored value is not a finite f32".into(),
        });
    }
    Ok(gain as f32)
}

fn schedule_error(message: String) -> AuthoringAudioError {
    AuthoringAudioError::InvalidSchedule(message)
}

struct CachedAudioSource<'a> {
    asset_id: Uuid,
    path: String,
    key: AudioSourceKey,
    cache: &'a CacheManager,
    chunks: LruCache<u64, Arc<AudioChunk>>,
}

impl<'a> CachedAudioSource<'a> {
    fn new(asset: &Asset, cache: &'a CacheManager) -> Result<Self, AuthoringAudioError> {
        let format = AudioDecodeFormat::new(AUTHORING_AUDIO_SAMPLE_RATE, AUTHORING_AUDIO_CHANNELS)
            .ok_or_else(|| {
                AuthoringAudioError::InvalidRequest("fixed audio format is invalid".into())
            })?;
        let key =
            AudioSourceKey::read(&asset.path, asset.stream_index, format).map_err(|error| {
                AuthoringAudioError::Decode {
                    asset_id: asset.id,
                    path: asset.path.clone(),
                    message: error.to_string(),
                }
            })?;
        Ok(Self {
            asset_id: asset.id,
            path: asset.path.clone(),
            key,
            cache,
            chunks: LruCache::new(NonZeroUsize::new(2).unwrap_or(NonZeroUsize::MIN)),
        })
    }

    fn sample(&mut self, source_time: MediaTime) -> Result<[f32; 2], AuthoringAudioError> {
        let numerator = i128::from(source_time.value())
            .checked_mul(i128::from(AUTHORING_AUDIO_SAMPLE_RATE))
            .ok_or_else(|| self.decode_error("source sample position overflowed"))?;
        if numerator < 0 {
            return Ok([0.0, 0.0]);
        }
        let denominator = i128::from(source_time.timescale());
        let first_frame = u64::try_from(numerator / denominator)
            .map_err(|_| self.decode_error("source sample position exceeds u64"))?;
        let remainder = numerator % denominator;
        let fraction = (remainder as f64 / denominator as f64) as f32;
        let second_frame = first_frame.saturating_add(1);
        let mut stereo = [0.0; 2];
        for (channel, output) in stereo.iter_mut().enumerate() {
            let first = self.sample_at(first_frame, channel)?.unwrap_or(0.0);
            let second = self.sample_at(second_frame, channel)?.unwrap_or(first);
            *output = first + (second - first) * fraction;
        }
        Ok(stereo)
    }

    fn sample_at(
        &mut self,
        frame: u64,
        channel: usize,
    ) -> Result<Option<f32>, AuthoringAudioError> {
        let chunk_frames = self.key.format.chunk_frames().max(1);
        let chunk_index = frame / chunk_frames;
        if !self.chunks.contains(&chunk_index) {
            let key = AudioChunkKey {
                source: self.key.clone(),
                chunk_index,
            };
            let chunk = if let Some(chunk) = self.cache.get_audio_chunk(&key) {
                chunk
            } else {
                if self.cache.audio_chunk_failed(&key) {
                    return Err(self.decode_error(format!(
                        "chunk {chunk_index} was previously marked failed"
                    )));
                }
                match AudioLoader::decode_chunk(&key) {
                    Ok(chunk) => {
                        let chunk = Arc::new(chunk);
                        self.cache.put_audio_chunk_arc(Arc::clone(&chunk));
                        chunk
                    }
                    Err(error) => {
                        self.cache.mark_audio_chunk_failed(key);
                        return Err(
                            self.decode_error(format!("chunk {chunk_index} failed: {error}"))
                        );
                    }
                }
            };
            self.chunks.put(chunk_index, chunk);
        }
        Ok(self
            .chunks
            .get(&chunk_index)
            .and_then(|chunk| chunk.sample(frame, channel)))
    }

    fn decode_error(&self, message: impl Into<String>) -> AuthoringAudioError {
        AuthoringAudioError::Decode {
            asset_id: self.asset_id,
            path: self.path.clone(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests;
