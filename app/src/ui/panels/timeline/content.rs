//! Source-appropriate content painted inside authoring Timeline clips.

use std::sync::Arc;

use egui::{Color32, Painter, Rect, Stroke, StrokeKind};
use library::editor::AuthoringWaveformService;
use library::model::asset::{Asset, AssetKind};
use library::model::authoring::{AuthoringProject, ShapeKind, SourceRef, TimelineItem};

use crate::ui::media_preview::{
    preview_request_size, AuthoringMediaPreviewService, MediaPreviewFrame,
};
use crate::ui::waveform::{paint_authoring_waveform, WaveformPaintReport, WaveformPaintRequest};

pub(super) struct ItemContentContext<'a> {
    pub ui: &'a egui::Ui,
    pub project: &'a Arc<AuthoringProject>,
    pub item: &'a TimelineItem,
    pub clip_rect: Rect,
    pub viewport_rect: Rect,
    pub evaluation_fps: f64,
    pub waveform: &'a AuthoringWaveformService,
    pub media_previews: &'a mut AuthoringMediaPreviewService,
}

#[derive(Default)]
struct ContentReport {
    visual: &'static str,
    frame_slots: usize,
    decoded_frames: usize,
    pending_frames: usize,
    fallback_frames: usize,
    failed_frames: usize,
    frame_hashes: Vec<String>,
    requested_sizes: Vec<[u32; 2]>,
    waveform: Option<WaveformPaintReport>,
    primitive_count: usize,
}

pub(super) fn paint_item_content(context: ItemContentContext<'_>) {
    let ItemContentContext {
        ui,
        project,
        item,
        clip_rect,
        viewport_rect,
        evaluation_fps,
        waveform,
        media_previews,
    } = context;
    let visible = clip_rect.intersect(viewport_rect);
    if !visible.is_positive() {
        return;
    }
    let painter = ui.painter().with_clip_rect(visible);
    let inner = clip_rect.shrink2(egui::vec2(5.0, 4.0));
    let mut report = match &item.source {
        SourceRef::Asset { asset_id } => project
            .assets
            .iter()
            .find(|asset| asset.id == *asset_id)
            .map_or_else(
                || ContentReport {
                    visual: "missing_asset",
                    primitive_count: paint_missing(&painter, inner),
                    ..ContentReport::default()
                },
                |asset| {
                    paint_asset(
                        ui,
                        project,
                        item,
                        asset,
                        inner,
                        visible,
                        evaluation_fps,
                        waveform,
                        media_previews,
                    )
                },
            ),
        SourceRef::Text { text, .. } => {
            paint_text(&painter, inner, text, ui.visuals().text_color())
        }
        SourceRef::Shape { shape } => paint_shape(&painter, inner, shape.shape_kind),
        SourceRef::Solid { color } => paint_solid(&painter, inner, color),
        SourceRef::Composition(instance) => {
            let name = project
                .timelines
                .get(&instance.timeline_id)
                .map_or("Composition", |timeline| timeline.name.as_str());
            paint_composition(&painter, inner, name)
        }
        SourceRef::Module(invocation) => {
            let (name, nodes) = project
                .module_instances
                .get(&invocation.instance_id)
                .and_then(|instance| project.module_definitions.get(&instance.definition_id))
                .map_or(("Node Clip", 0), |definition| {
                    (definition.name.as_str(), definition.graph.nodes.len())
                });
            paint_module(&painter, inner, name, nodes)
        }
    };
    if report.visual.is_empty() {
        report.visual = "unknown";
    }
    let waveform_report = report.waveform;
    let background_media = match &item.source {
        SourceRef::Asset { asset_id } => project.assets.iter().any(|asset| {
            asset.id == *asset_id
                && matches!(
                    asset.kind,
                    AssetKind::Audio | AssetKind::Image | AssetKind::Video
                )
        }),
        _ => false,
    };
    crate::qa::register_component_with_metadata(
        format!("timeline.content:{}", item.id),
        "timeline_clip_content",
        visible,
        true,
        Some(serde_json::json!({
            "item_id": item.id,
            "visual": report.visual,
            "frame_slots": report.frame_slots,
            "decoded_frames": report.decoded_frames,
            "pending_frames": report.pending_frames,
            "fallback_frames": report.fallback_frames,
            "failed_frames": report.failed_frames,
            "ready": report.frame_slots > 0 && report.decoded_frames == report.frame_slots,
            "frame_hashes": report.frame_hashes,
            "requested_sizes": report.requested_sizes,
            "primitive_count": report.primitive_count,
            "waveform_status": waveform_report.map(WaveformPaintReport::status),
            "waveform_segments": waveform_report.map(|waveform| waveform.segment_count),
            "uses_shared_media_cache": background_media,
            "decode_thread": background_media.then_some("background"),
        })),
    );
    if let Some(waveform_report) = waveform_report {
        crate::qa::register_component_with_metadata(
            format!("timeline.waveform:{}", item.id),
            "timeline_waveform",
            visible,
            true,
            Some(serde_json::json!({
                "item_id": item.id,
                "status": waveform_report.status(),
                "segment_count": waveform_report.segment_count,
                "requested_chunks": waveform_report.requested_chunks,
                "ready_chunks": waveform_report.ready_chunks,
                "failed_chunks": waveform_report.failed_chunks,
                "settled": waveform_report.settled,
                "ready": waveform_report.ready,
                "complete": waveform_report.complete,
                "truncated": waveform_report.truncated,
                "source_time_start": waveform_report.source_time_start,
                "source_time_end": waveform_report.source_time_end,
                "first_source_frame": waveform_report.first_source_frame,
                "end_source_frame_exclusive": waveform_report.end_source_frame_exclusive,
                "sample_rate": waveform.format().sample_rate,
                "decode_thread": "background",
                "shared_painter": "authoring_waveform",
                "cache_owner": "CacheManager.audio_waveform",
            })),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_asset(
    ui: &egui::Ui,
    project: &Arc<AuthoringProject>,
    item: &TimelineItem,
    asset: &Asset,
    clip_rect: Rect,
    visible: Rect,
    evaluation_fps: f64,
    waveform: &AuthoringWaveformService,
    media_previews: &mut AuthoringMediaPreviewService,
) -> ContentReport {
    match asset.kind {
        AssetKind::Audio => {
            let (source_left, source_right) = item_source_range(item);
            let painter = ui.painter().with_clip_rect(visible);
            painter.line_segment(
                [clip_rect.left_center(), clip_rect.right_center()],
                Stroke::new(1.0, Color32::from_gray(65)),
            );
            ContentReport {
                visual: "audio_waveform",
                waveform: Some(paint_authoring_waveform(WaveformPaintRequest {
                    service: waveform,
                    context: ui.ctx(),
                    painter: &painter,
                    rect: visible,
                    clip_rect,
                    path: &asset.path,
                    stream_index: asset.stream_index,
                    duration_seconds: asset.duration,
                    source_time_at_left: source_left,
                    source_time_at_right: source_right,
                    color: Color32::from_rgb(154, 236, 165),
                })),
                ..ContentReport::default()
            }
        }
        AssetKind::Image => {
            let requested_size = preview_request_size(ui.ctx(), clip_rect.size());
            let frame = media_previews.request(
                ui.ctx(),
                Arc::clone(project),
                asset,
                0.0,
                evaluation_fps,
                requested_size,
            );
            frame_report(
                "image_thumbnail",
                &ui.painter().with_clip_rect(visible),
                clip_rect,
                frame,
            )
        }
        AssetKind::Video => paint_video_strip(
            ui,
            project,
            item,
            asset,
            clip_rect,
            visible,
            evaluation_fps,
            media_previews,
        ),
        AssetKind::Model3D => icon_report(
            "model_representation",
            &ui.painter().with_clip_rect(visible),
            clip_rect,
            egui_phosphor::regular::CUBE,
        ),
        AssetKind::Other => icon_report(
            "file_representation",
            &ui.painter().with_clip_rect(visible),
            clip_rect,
            egui_phosphor::regular::FILE,
        ),
    }
}

fn paint_video_strip(
    ui: &egui::Ui,
    project: &Arc<AuthoringProject>,
    item: &TimelineItem,
    asset: &Asset,
    clip_rect: Rect,
    visible: Rect,
    evaluation_fps: f64,
    media_previews: &mut AuthoringMediaPreviewService,
) -> ContentReport {
    let painter = ui.painter().with_clip_rect(visible);
    let cell_width = (clip_rect.height() * 16.0 / 9.0).max(42.0);
    let first = ((visible.left() - clip_rect.left()) / cell_width)
        .floor()
        .max(0.0) as usize;
    let end = ((visible.right() - clip_rect.left()) / cell_width)
        .ceil()
        .max(first as f32 + 1.0) as usize;
    let (source_left, source_right) = item_source_range(item);
    let mut report = ContentReport {
        visual: "video_frame_strip",
        ..ContentReport::default()
    };
    for cell in first..end {
        let left = clip_rect.left() + cell as f32 * cell_width;
        let rect = Rect::from_min_max(
            egui::pos2(left, clip_rect.top()),
            egui::pos2(
                (left + cell_width).min(clip_rect.right()),
                clip_rect.bottom(),
            ),
        );
        if !rect.is_positive() || !rect.intersects(visible) {
            continue;
        }
        let ratio = f64::from((rect.center().x - clip_rect.left()) / clip_rect.width().max(1.0));
        let source_time = source_left + (source_right - source_left) * ratio;
        let frame = media_previews.request(
            ui.ctx(),
            Arc::clone(project),
            asset,
            source_time.max(0.0),
            evaluation_fps,
            preview_request_size(ui.ctx(), rect.size()),
        );
        report.frame_slots += 1;
        accumulate_frame(&mut report, &painter, rect, frame);
    }
    report
}

fn frame_report(
    visual: &'static str,
    painter: &Painter,
    rect: Rect,
    frame: MediaPreviewFrame,
) -> ContentReport {
    let mut report = ContentReport {
        visual,
        frame_slots: 1,
        ..ContentReport::default()
    };
    accumulate_frame(&mut report, painter, rect, frame);
    report
}

fn accumulate_frame(
    report: &mut ContentReport,
    painter: &Painter,
    rect: Rect,
    frame: MediaPreviewFrame,
) {
    if let Some(requested_size) = frame.requested_size {
        if !report.requested_sizes.contains(&requested_size) {
            report.requested_sizes.push(requested_size);
        }
    }
    if let Some(content_hash) = frame.content_hash.clone() {
        report.frame_hashes.push(content_hash);
    }
    if let (Some(texture), Some(size)) = (frame.texture, frame.texture_size) {
        paint_fitted_frame(painter, rect, &texture, size);
        report.decoded_frames += 1;
        report.pending_frames += usize::from(frame.pending);
        report.fallback_frames += usize::from(frame.fallback);
    } else if frame.pending {
        report.pending_frames += 1;
        paint_loading_hatch(painter, rect);
    } else {
        report.failed_frames += usize::from(frame.error.is_some());
        paint_missing(painter, rect);
    }
}

fn paint_fitted_frame(
    painter: &Painter,
    rect: Rect,
    texture: &egui::TextureHandle,
    size: [u32; 2],
) {
    let source = egui::vec2(size[0] as f32, size[1] as f32);
    if source.x <= 0.0 || source.y <= 0.0 {
        return;
    }
    let scale = (rect.width() / source.x).max(rect.height() / source.y);
    let fitted = Rect::from_center_size(rect.center(), source * scale);
    painter.with_clip_rect(rect).image(
        texture.id(),
        fitted,
        Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        Color32::from_white_alpha(190),
    );
}

fn paint_loading_hatch(painter: &Painter, rect: Rect) {
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        painter.line_segment(
            [
                egui::pos2(x, rect.bottom()),
                egui::pos2(x + rect.height(), rect.top()),
            ],
            Stroke::new(1.0, Color32::from_white_alpha(24)),
        );
        x += 12.0;
    }
}

fn paint_text(painter: &Painter, rect: Rect, text: &str, color: Color32) -> ContentReport {
    painter.text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        text.replace('\n', " "),
        egui::FontId::proportional((rect.height() * 0.45).clamp(9.0, 22.0)),
        color.gamma_multiply(0.42),
    );
    ContentReport {
        visual: "text_content",
        primitive_count: 1,
        ..ContentReport::default()
    }
}

fn paint_shape(painter: &Painter, rect: Rect, shape: ShapeKind) -> ContentReport {
    let bounds = Rect::from_center_size(rect.center(), rect.size() * egui::vec2(0.86, 0.66));
    let color = Color32::from_rgba_premultiplied(216, 241, 158, 90);
    match shape {
        ShapeKind::Rectangle => {
            painter.rect_stroke(bounds, 2.0, Stroke::new(1.5, color), StrokeKind::Inside)
        }
        ShapeKind::Ellipse => painter.circle_stroke(
            bounds.center(),
            bounds.width().min(bounds.height()) * 0.42,
            Stroke::new(1.5, color),
        ),
        ShapeKind::Path => painter.add(egui::Shape::line(
            vec![
                bounds.left_bottom(),
                bounds.center_top(),
                bounds.right_bottom(),
            ],
            Stroke::new(1.5, color),
        )),
    };
    ContentReport {
        visual: match shape {
            ShapeKind::Rectangle => "shape_rectangle",
            ShapeKind::Ellipse => "shape_ellipse",
            ShapeKind::Path => "shape_path",
        },
        primitive_count: 1,
        ..ContentReport::default()
    }
}

fn paint_solid(
    painter: &Painter,
    rect: Rect,
    color: &library::model::frame::color::Color,
) -> ContentReport {
    painter.rect_filled(
        rect,
        2.0,
        Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a).gamma_multiply(0.62),
    );
    ContentReport {
        visual: "solid_color",
        primitive_count: 1,
        ..ContentReport::default()
    }
}

fn paint_composition(painter: &Painter, rect: Rect, name: &str) -> ContentReport {
    let lane_height = (rect.height() / 5.0).max(2.0);
    for lane in 0..3 {
        let top = rect.center().y + (lane as f32 - 1.0) * (lane_height + 2.0);
        let width = rect.width() * (0.72 + lane as f32 * 0.08);
        painter.rect_filled(
            Rect::from_min_size(egui::pos2(rect.left(), top), egui::vec2(width, lane_height)),
            1.0,
            Color32::from_rgba_premultiplied(234, 168, 244, 72),
        );
    }
    painter.text(
        rect.right_center(),
        egui::Align2::RIGHT_CENTER,
        name,
        egui::FontId::proportional(9.0),
        Color32::from_white_alpha(90),
    );
    ContentReport {
        visual: "composition_lanes",
        primitive_count: 4,
        ..ContentReport::default()
    }
}

fn paint_module(painter: &Painter, rect: Rect, name: &str, node_count: usize) -> ContentReport {
    let left = egui::pos2(rect.left() + 8.0, rect.center().y);
    let middle = egui::pos2(
        rect.left() + rect.width() * 0.42,
        rect.top() + rect.height() * 0.3,
    );
    let right = egui::pos2(rect.left() + rect.width() * 0.72, rect.center().y);
    let stroke = Stroke::new(1.2, Color32::from_rgba_premultiplied(255, 202, 116, 105));
    painter.line_segment([left, middle], stroke);
    painter.line_segment([middle, right], stroke);
    for point in [left, middle, right] {
        painter.circle_filled(point, 2.5, stroke.color);
    }
    painter.text(
        rect.right_center(),
        egui::Align2::RIGHT_CENTER,
        format!("{name} · {node_count}"),
        egui::FontId::proportional(9.0),
        Color32::from_white_alpha(90),
    );
    ContentReport {
        visual: "node_topology",
        primitive_count: 6,
        ..ContentReport::default()
    }
}

fn icon_report(visual: &'static str, painter: &Painter, rect: Rect, icon: &str) -> ContentReport {
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional((rect.height() * 0.55).clamp(10.0, 24.0)),
        Color32::from_white_alpha(80),
    );
    ContentReport {
        visual,
        primitive_count: 1,
        ..ContentReport::default()
    }
}

fn paint_missing(painter: &Painter, rect: Rect) -> usize {
    painter.line_segment(
        [rect.left_top(), rect.right_bottom()],
        Stroke::new(1.0, Color32::from_rgb(220, 92, 92)),
    );
    1
}

fn item_source_range(item: &TimelineItem) -> (f64, f64) {
    let start = item.time_map.source_start.to_seconds_f64();
    let duration = item
        .interval
        .duration
        .checked_mul_rate(item.time_map.playback_rate)
        .map_or(0.0, |duration| duration.to_seconds_f64());
    (start, start + duration)
}

#[cfg(test)]
mod tests {
    use library::model::authoring::{
        MediaTime, RationalRate, SourceRef, TimeMap, TimelineInterval, TimelineItem,
        TimelineItemId, TimelineTrackId,
    };
    use library::model::property::PropertyMap;

    use super::item_source_range;

    #[test]
    fn waveform_and_frame_strip_share_the_item_time_map() {
        let item = TimelineItem {
            id: TimelineItemId::new(),
            track_id: TimelineTrackId::new(),
            name: "timed".into(),
            source: SourceRef::Solid {
                color: library::model::frame::color::Color::black(),
            },
            interval: TimelineInterval::new(
                MediaTime::new(4, 1).unwrap(),
                MediaTime::new(3, 1).unwrap(),
            )
            .unwrap(),
            time_map: TimeMap {
                source_start: MediaTime::new(1, 2).unwrap(),
                playback_rate: RationalRate::new(2, 1).unwrap(),
            },
            layer: 0,
            parent: None,
            blend_mode: library::model::BlendMode::Normal,
            authored_properties: PropertyMap::new(),
        };

        assert_eq!(item_source_range(&item), (0.5, 6.5));
    }
}
