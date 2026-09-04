//! Timeline waveform projection over the canonical typed Audio graph.
//!
//! Peak summaries are transient UI cache data. Clip timing and graph routing
//! remain authoritative in `Project`.

use std::time::Duration;

use egui::{Color32, Painter, Rect, Stroke};
use library::EditorService as ProjectService;
use library::audio::mixer::audio_stream_index_for_media;
use library::audio::waveform::AudioWaveformWindow;
use library::editor::audio_service::routed_audio_media_nodes_for_clip;
use library::model::{AssetKind, Clip, NodeContent, Project};
use serde_json::json;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaybackDirection {
    Forward,
    Freeze,
    Reverse,
}

impl PlaybackDirection {
    fn as_str(self) -> &'static str {
        match self {
            Self::Forward => "forward",
            Self::Freeze => "freeze",
            Self::Reverse => "reverse",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct VisibleWaveformRange {
    rect: Rect,
    timeline_offset_start: f64,
    timeline_offset_end: f64,
    source_time_start: f64,
    source_time_end: f64,
    first_source_frame: u64,
    end_source_frame_exclusive: u64,
    direction: PlaybackDirection,
}

struct WaveformMediaSource<'a> {
    node_id: Uuid,
    asset_id: Uuid,
    path: &'a str,
    stream_index: Option<usize>,
    duration: Option<f64>,
}

struct RequestedWaveformSource<'a> {
    source: WaveformMediaSource<'a>,
    requested_range: Option<(u64, u64)>,
    window: Option<AudioWaveformWindow>,
}

impl RequestedWaveformSource<'_> {
    fn is_outside_media(&self) -> bool {
        self.requested_range.is_none()
    }

    fn identity_failed(&self) -> bool {
        self.requested_range.is_some() && self.window.is_none()
    }

    fn is_settled(&self) -> bool {
        self.is_outside_media()
            || self.identity_failed()
            || self
                .window
                .as_ref()
                .is_some_and(AudioWaveformWindow::is_settled)
    }

    fn is_complete(&self) -> bool {
        self.is_outside_media()
            || self
                .window
                .as_ref()
                .is_some_and(AudioWaveformWindow::is_complete)
    }

    fn is_ready(&self) -> bool {
        self.is_outside_media()
            || self
                .window
                .as_ref()
                .is_some_and(AudioWaveformWindow::is_ready)
    }
}

fn interpolation_frame_bounds(
    first_source_time: f64,
    second_source_time: f64,
    sample_rate: u32,
) -> Option<(u64, u64)> {
    if !first_source_time.is_finite() || !second_source_time.is_finite() || sample_rate == 0 {
        return None;
    }
    let minimum_time = first_source_time.min(second_source_time);
    let maximum_time = first_source_time.max(second_source_time);
    if maximum_time < 0.0 {
        return None;
    }
    let sample_rate = f64::from(sample_rate);
    let first_frame = (minimum_time.max(0.0) * sample_rate).floor() as u64;
    // Playback linearly interpolates floor(position) with its following frame.
    // Include that neighbor even for a fractional freeze position.
    let final_interpolation_frame = (maximum_time.max(0.0) * sample_rate).floor() as u64;
    let final_interpolation_frame = final_interpolation_frame.saturating_add(1);
    Some((first_frame, final_interpolation_frame.saturating_add(1)))
}

fn clamp_source_frame_range(
    first_frame: u64,
    end_frame_exclusive: u64,
    duration: Option<f64>,
    sample_rate: u32,
) -> Option<(u64, u64)> {
    let end_frame_exclusive = if let Some(duration) = duration {
        if !duration.is_finite() || duration <= 0.0 || sample_rate == 0 {
            return None;
        }
        let media_end = (duration * f64::from(sample_rate)).ceil() as u64;
        end_frame_exclusive.min(media_end)
    } else {
        end_frame_exclusive
    };
    (first_frame < end_frame_exclusive).then_some((first_frame, end_frame_exclusive))
}

fn visible_waveform_range(
    clip: &Clip,
    clip_rect: Rect,
    viewport_rect: Rect,
    pixels_per_second: f32,
    sample_rate: u32,
) -> Option<VisibleWaveformRange> {
    if !pixels_per_second.is_finite()
        || pixels_per_second <= 0.0
        || sample_rate == 0
        || !clip_rect.is_positive()
    {
        return None;
    }
    let rect = clip_rect.intersect(viewport_rect);
    if !rect.is_positive() {
        return None;
    }

    let offset_at = |x: f32| {
        (f64::from(x - clip_rect.min.x) / f64::from(pixels_per_second))
            .clamp(0.0, clip.duration.into_inner())
    };
    let timeline_offset_start = offset_at(rect.min.x);
    let timeline_offset_end = offset_at(rect.max.x);
    let clip_start = clip.start_time.into_inner();
    let source_time_start = clip.local_time(clip_start + timeline_offset_start);
    let source_time_end = clip.local_time(clip_start + timeline_offset_end);
    let direction = match clip.time_stretch.into_inner().total_cmp(&0.0) {
        std::cmp::Ordering::Greater => PlaybackDirection::Forward,
        std::cmp::Ordering::Equal => PlaybackDirection::Freeze,
        std::cmp::Ordering::Less => PlaybackDirection::Reverse,
    };
    let (first_source_frame, end_source_frame_exclusive) =
        interpolation_frame_bounds(source_time_start, source_time_end, sample_rate)?;

    Some(VisibleWaveformRange {
        rect,
        timeline_offset_start,
        timeline_offset_end,
        source_time_start,
        source_time_end,
        first_source_frame,
        end_source_frame_exclusive,
        direction,
    })
}

fn resolve_waveform_sources<'a>(project: &'a Project, clip: &Clip) -> Vec<WaveformMediaSource<'a>> {
    routed_audio_media_nodes_for_clip(project, clip.id)
        .into_iter()
        .filter_map(|node_id| {
            let node = project.get_node(node_id)?;
            let NodeContent::Media(media) = node.content() else {
                return None;
            };
            let asset = project.get_asset(media.asset_id)?;
            matches!(asset.kind, AssetKind::Audio | AssetKind::Video).then(|| WaveformMediaSource {
                node_id,
                asset_id: asset.id,
                path: &asset.path,
                stream_index: audio_stream_index_for_media(asset, media),
                duration: asset.duration,
            })
        })
        .collect()
}

fn source_frames_for_segment(
    clip: &Clip,
    clip_rect: Rect,
    x_start: f32,
    x_end: f32,
    pixels_per_second: f32,
    sample_rate: u32,
) -> Option<(u64, u64)> {
    let offset_at = |x: f32| f64::from(x - clip_rect.min.x) / f64::from(pixels_per_second);
    let clip_start = clip.start_time.into_inner();
    let first_time = clip.local_time(clip_start + offset_at(x_start));
    let second_time = clip.local_time(clip_start + offset_at(x_end));
    interpolation_frame_bounds(first_time, second_time, sample_rate)
}

/// Multiple routed sources use an envelope preview: the highest source peak
/// is shown without pretending that a cheap UI summary is the mixed waveform.
fn maximum_source_peak(peaks: impl Iterator<Item = f32>) -> f32 {
    peaks.fold(0.0_f32, f32::max)
}

pub(super) struct WaveformDrawContext<'a> {
    pub(super) ctx: &'a egui::Context,
    pub(super) painter: &'a Painter,
    pub(super) clip_rect: Rect,
    pub(super) viewport_rect: Rect,
    pub(super) pixels_per_second: f32,
    pub(super) clip: &'a Clip,
    pub(super) project: &'a Project,
    pub(super) project_service: &'a ProjectService,
}

pub(super) fn draw_clip_waveform(context: WaveformDrawContext<'_>) {
    let WaveformDrawContext {
        ctx,
        painter,
        clip_rect,
        viewport_rect,
        pixels_per_second,
        clip,
        project,
        project_service,
    } = context;
    let audio_service = project_service.get_audio_service();
    let sample_rate = audio_service.get_audio_engine().get_sample_rate();
    let Some(range) = visible_waveform_range(
        clip,
        clip_rect,
        viewport_rect,
        pixels_per_second,
        sample_rate,
    ) else {
        return;
    };
    let sources = resolve_waveform_sources(project, clip);
    if sources.is_empty() {
        return;
    }

    let mut requested_sources = Vec::with_capacity(sources.len());
    for source in sources {
        let requested_range = clamp_source_frame_range(
            range.first_source_frame,
            range.end_source_frame_exclusive,
            source.duration,
            sample_rate,
        );
        let window = requested_range.and_then(|(first_frame, end_frame_exclusive)| {
            audio_service.request_waveform_window(
                source.path,
                source.stream_index,
                first_frame,
                end_frame_exclusive.saturating_sub(1),
            )
        });
        requested_sources.push(RequestedWaveformSource {
            source,
            requested_range,
            window,
        });
    }

    let samples_per_pixel = f64::from(sample_rate) * clip.time_stretch.into_inner().abs()
        / f64::from(pixels_per_second);
    let step_width = if samples_per_pixel > 1_000.0 {
        2.0
    } else {
        1.0
    };
    let center_y = clip_rect.center().y;
    let max_amplitude_height = clip_rect.height() * 0.4;
    let mut segment_count = 0_usize;
    let mut x = range.rect.min.x;
    while x < range.rect.max.x {
        let end_x = (x + step_width).min(range.rect.max.x);
        let Some((first_frame, end_frame_exclusive)) =
            source_frames_for_segment(clip, clip_rect, x, end_x, pixels_per_second, sample_rate)
        else {
            x += step_width;
            continue;
        };
        let peak = maximum_source_peak(requested_sources.iter().filter_map(|source| {
            let (first_frame, end_frame_exclusive) = clamp_source_frame_range(
                first_frame,
                end_frame_exclusive,
                source.source.duration,
                sample_rate,
            )?;
            source
                .window
                .as_ref()?
                .peak_between(first_frame, end_frame_exclusive)
        }))
        .clamp(0.0, 1.0);
        if peak > 0.0 {
            let height = (peak * max_amplitude_height).max(1.0);
            painter.line_segment(
                [
                    egui::pos2(x, center_y - height),
                    egui::pos2(x, center_y + height),
                ],
                Stroke::new(1.0, Color32::from_rgba_premultiplied(0, 0, 0, 130)),
            );
            segment_count += 1;
        }
        x += step_width;
    }

    let requested_chunks = requested_sources
        .iter()
        .filter_map(|source| source.window.as_ref())
        .map(AudioWaveformWindow::requested_chunks)
        .sum::<usize>();
    let ready_chunks = requested_sources
        .iter()
        .filter_map(|source| source.window.as_ref())
        .map(AudioWaveformWindow::ready_chunks)
        .sum::<usize>();
    let failed_chunks = requested_sources
        .iter()
        .filter_map(|source| source.window.as_ref())
        .map(AudioWaveformWindow::failed_chunks)
        .sum::<usize>();
    let source_identity_failures = requested_sources
        .iter()
        .filter(|source| source.identity_failed())
        .count();
    let outside_media_sources = requested_sources
        .iter()
        .filter(|source| source.is_outside_media())
        .count();
    let settled = requested_sources
        .iter()
        .all(RequestedWaveformSource::is_settled);
    let complete = requested_sources
        .iter()
        .all(RequestedWaveformSource::is_complete);
    let has_requested_source = outside_media_sources < requested_sources.len();
    let ready = has_requested_source
        && requested_sources
            .iter()
            .all(RequestedWaveformSource::is_ready);
    let truncated = requested_sources
        .iter()
        .filter_map(|source| source.window.as_ref())
        .any(AudioWaveformWindow::is_truncated);
    let pending = !settled;
    let no_output = !has_requested_source;
    let status = if no_output {
        "no_output"
    } else if pending {
        "pending"
    } else if ready {
        "ready"
    } else if truncated {
        "truncated"
    } else {
        "failed"
    };
    let source_metadata = requested_sources
        .iter()
        .map(|source| {
            let window = source.window.as_ref();
            let status = if source.is_outside_media() {
                "no_output"
            } else if source.identity_failed() {
                "identity_failure"
            } else if window.is_some_and(AudioWaveformWindow::has_pending_chunks) {
                "pending"
            } else if window.is_some_and(|window| window.failed_chunks() > 0) {
                "decode_failure"
            } else if window.is_some_and(AudioWaveformWindow::is_truncated) {
                "truncated"
            } else {
                "ready"
            };
            json!({
                "node_id": source.source.node_id,
                "asset_id": source.source.asset_id,
                "path": source.source.path,
                "stream_index": source.source.stream_index,
                "duration": source.source.duration,
                "requested_first_source_frame": source.requested_range.map(|range| range.0),
                "requested_end_source_frame_exclusive": source.requested_range.map(|range| range.1),
                "identity_ready": window.is_some(),
                "sample_rate": window.map(|window| window.source().format.sample_rate),
                "channels": window.map(|window| window.source().format.channels),
                "ready_chunks": window.map_or(0, AudioWaveformWindow::ready_chunks),
                "requested_chunks": window.map_or(0, AudioWaveformWindow::requested_chunks),
                "failed_chunks": window.map_or(0, AudioWaveformWindow::failed_chunks),
                "settled": source.is_settled(),
                "ready": source.is_ready() && !source.is_outside_media(),
                "complete": source.is_complete(),
                "truncated": window.is_some_and(AudioWaveformWindow::is_truncated),
                "status": status,
            })
        })
        .collect::<Vec<_>>();
    crate::qa::register_component_with_metadata(
        format!("timeline.waveform:{}", clip.id),
        "timeline_waveform",
        range.rect,
        true,
        Some(json!({
            "clip_id": clip.id,
            "segment_count": segment_count,
            "source_count": requested_sources.len(),
            "source_identity_failures": source_identity_failures,
            "outside_media_sources": outside_media_sources,
            "sources": source_metadata,
            "requested_chunks": requested_chunks,
            "ready_chunks": ready_chunks,
            "failed_chunks": failed_chunks,
            "settled": settled,
            "ready": ready,
            "complete": complete,
            "no_output": no_output,
            "status": status,
            "truncated": truncated,
            "timeline_offset_start": range.timeline_offset_start,
            "timeline_offset_end": range.timeline_offset_end,
            "source_time_start": range.source_time_start,
            "source_time_end": range.source_time_end,
            "first_source_frame": range.first_source_frame,
            "end_source_frame_exclusive": range.end_source_frame_exclusive,
            "sample_rate": sample_rate,
            "trim_in": clip.trim_in.into_inner(),
            "time_stretch": clip.time_stretch.into_inner(),
            "direction": range.direction.as_str(),
            "decode_thread": "background",
        })),
    );
    if pending {
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::media_node_for_canvas;
    use library::editor::project_service::MediaNodeRequest;
    use library::model::{Asset, NodeContainer};

    fn rect(min_x: f32, max_x: f32) -> Rect {
        Rect::from_min_max(egui::pos2(min_x, 0.0), egui::pos2(max_x, 40.0))
    }

    #[test]
    fn visible_range_maps_trim_stretch_and_viewport_clipping() {
        let mut clip = Clip::new("audio", 10.0, 4.0);
        clip.trim_in = 0.5.into();
        clip.time_stretch = 2.0.into();
        let range =
            visible_waveform_range(&clip, rect(100.0, 500.0), rect(200.0, 400.0), 100.0, 10)
                .unwrap();

        assert_eq!(range.timeline_offset_start, 1.0);
        assert_eq!(range.timeline_offset_end, 3.0);
        assert_eq!(range.source_time_start, 2.5);
        assert_eq!(range.source_time_end, 6.5);
        assert_eq!(range.first_source_frame, 25);
        assert_eq!(range.end_source_frame_exclusive, 67);
        assert_eq!(range.direction, PlaybackDirection::Forward);
    }

    #[test]
    fn visible_range_handles_reverse_and_freeze_explicitly() {
        let mut reverse = Clip::new("reverse", 0.0, 4.0);
        reverse.trim_in = 5.0.into();
        reverse.time_stretch = (-1.0).into();
        let range =
            visible_waveform_range(&reverse, rect(100.0, 500.0), rect(200.0, 400.0), 100.0, 10)
                .unwrap();
        assert_eq!(
            (range.first_source_frame, range.end_source_frame_exclusive),
            (20, 42)
        );
        assert_eq!(range.direction, PlaybackDirection::Reverse);

        let mut freeze = Clip::new("freeze", 0.0, 4.0);
        freeze.trim_in = 1.25.into();
        freeze.time_stretch = 0.0.into();
        let range =
            visible_waveform_range(&freeze, rect(100.0, 500.0), rect(200.0, 400.0), 100.0, 10)
                .unwrap();
        assert_eq!(
            (range.first_source_frame, range.end_source_frame_exclusive),
            (12, 14)
        );
        assert_eq!(range.direction, PlaybackDirection::Freeze);
        assert_eq!(
            source_frames_for_segment(&freeze, rect(100.0, 500.0), 200.0, 201.0, 100.0, 10),
            Some((12, 14))
        );
        assert_eq!(maximum_source_peak([0.65].into_iter()), 0.65);
    }

    #[test]
    fn multiple_sources_display_the_maximum_envelope_not_a_sum() {
        assert_eq!(maximum_source_peak([0.2, 0.8, 0.4].into_iter()), 0.8);
    }

    #[test]
    fn known_media_duration_clamps_half_open_source_ranges() {
        assert_eq!(
            clamp_source_frame_range(8, 14, Some(1.0), 10),
            Some((8, 10))
        );
        assert_eq!(clamp_source_frame_range(10, 14, Some(1.0), 10), None);
        assert_eq!(clamp_source_frame_range(10, 14, None, 10), Some((10, 14)));
    }

    #[test]
    fn zoom_changes_pixels_without_changing_the_visible_timeline_range() {
        let clip = Clip::new("audio", 0.0, 10.0);
        let zoomed_out =
            visible_waveform_range(&clip, rect(0.0, 500.0), rect(100.0, 200.0), 50.0, 10).unwrap();
        let zoomed_in =
            visible_waveform_range(&clip, rect(0.0, 1_000.0), rect(200.0, 400.0), 100.0, 10)
                .unwrap();
        assert_eq!(
            (
                zoomed_out.timeline_offset_start,
                zoomed_out.timeline_offset_end
            ),
            (
                zoomed_in.timeline_offset_start,
                zoomed_in.timeline_offset_end
            )
        );
        assert_eq!(
            (
                zoomed_out.first_source_frame,
                zoomed_out.end_source_frame_exclusive
            ),
            (
                zoomed_in.first_source_frame,
                zoomed_in.end_source_frame_exclusive
            )
        );
    }

    fn project_with_audio_media(kind: AssetKind) -> Option<(Project, Clip, Uuid)> {
        let mut project = Project::new("waveform routing");
        let mut asset = Asset::new("source", "source.media", kind.clone());
        asset.duration = Some(2.5);
        let asset_id = asset.id;
        project.assets.push(asset);
        let request = match kind {
            AssetKind::Audio => MediaNodeRequest::Audio {
                asset_id,
                file_path: "source.media".to_string(),
                audio_stream_index: Some(3),
            },
            AssetKind::Video => MediaNodeRequest::Video {
                asset_id,
                file_path: "source.media".to_string(),
                stream_index: Some(1),
                audio_stream_index: Some(3),
            },
            _ => return None,
        };
        let node = media_node_for_canvas("source", request, 16, 9, 16, 9);
        let node_id = node.id;
        let clip = Clip::new("clip", 0.0, 1.0);
        let clip_id = clip.id;
        project.add_clip(clip.clone());
        project.add_node(node);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
            .unwrap();
        project
            .set_audio_output_node(NodeContainer::Clip(clip_id), Some(node_id))
            .unwrap();
        Some((project, clip, node_id))
    }

    #[test]
    fn canonical_audio_routing_finds_audio_and_video_media() {
        for kind in [AssetKind::Audio, AssetKind::Video] {
            let (project, clip, node_id) = project_with_audio_media(kind).unwrap();
            let sources = resolve_waveform_sources(&project, &clip);
            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].node_id, node_id);
            assert_eq!(sources[0].stream_index, Some(3));
            assert_eq!(sources[0].duration, Some(2.5));
        }
    }

    #[test]
    fn disabled_audio_media_fails_closed() {
        let (mut project, clip, node_id) = project_with_audio_media(AssetKind::Audio).unwrap();
        project.get_node_mut(node_id).unwrap().enabled = false;
        assert!(resolve_waveform_sources(&project, &clip).is_empty());
    }
}
