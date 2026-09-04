use std::sync::Arc;

use egui_phosphor::regular as icons;
use library::editor::AuthoringWaveformService;
use library::model::asset::{Asset, AssetKind};
use library::model::authoring::AuthoringProject;

use crate::ui::media_preview::{
    preview_request_size, representative_source_time, AuthoringMediaPreviewService,
    MediaPreviewFrame,
};
use crate::ui::waveform::{paint_authoring_waveform, WaveformPaintReport, WaveformPaintRequest};

const PREVIEW_MIN_HEIGHT: f32 = 116.0;
const PREVIEW_MAX_HEIGHT: f32 = 220.0;
const PREVIEW_PADDING: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AssetVisualKind {
    Image,
    Video,
    Audio,
    Model3D,
    Other,
}

impl AssetVisualKind {
    fn from_asset(kind: &AssetKind) -> Self {
        match kind {
            AssetKind::Image => Self::Image,
            AssetKind::Video => Self::Video,
            AssetKind::Audio => Self::Audio,
            AssetKind::Model3D => Self::Model3D,
            AssetKind::Other => Self::Other,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Image => "Image",
            Self::Video => "Video",
            Self::Audio => "Audio",
            Self::Model3D => "3D model",
            Self::Other => "File",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Image => icons::FILE_IMAGE,
            Self::Video => icons::FILE_VIDEO,
            Self::Audio => icons::FILE_AUDIO,
            Self::Model3D => icons::CUBE,
            Self::Other => icons::FILE,
        }
    }

    fn is_frame(self) -> bool {
        matches!(self, Self::Image | Self::Video)
    }
}

pub(super) fn asset_inspector(
    ui: &mut egui::Ui,
    project: &Arc<AuthoringProject>,
    asset: &Asset,
    waveform: &AuthoringWaveformService,
    media_previews: &mut AuthoringMediaPreviewService,
) {
    let kind = AssetVisualKind::from_asset(&asset.kind);
    super::section_title(ui, kind.icon(), kind.label(), &asset.name, None);
    ui.add_space(8.0);

    let source_time = representative_source_time(asset);
    let preview = if kind.is_frame() {
        let evaluation_fps = project
            .timelines
            .get(&project.root_timeline_id)
            .map_or(30.0, |timeline| timeline.fps.to_f64());
        let preview_width = ui.available_width().max(1.0);
        let preview_size = egui::vec2(
            preview_width,
            preview_height(preview_width, asset.width, asset.height),
        );
        media_previews.request(
            ui.ctx(),
            Arc::clone(project),
            asset,
            source_time,
            evaluation_fps,
            preview_request_size(ui.ctx(), preview_size),
        )
    } else {
        MediaPreviewFrame::default()
    };

    let (preview_rect, waveform_report) = paint_preview_card(ui, kind, asset, &preview, waveform);
    register_preview_qa(
        preview_rect,
        asset,
        kind,
        &preview,
        source_time,
        waveform_report,
        waveform.format().sample_rate,
    );

    ui.add_space(10.0);
    let info = egui::Frame::NONE.show(ui, |ui| {
        egui::Grid::new(("inspector.asset.info", asset.id))
            .num_columns(2)
            .spacing([12.0, 7.0])
            .show(ui, |ui| {
                info_row(ui, "Kind", kind.label());
                if let (Some(width), Some(height)) = (asset.width, asset.height) {
                    info_row(ui, "Frame size", &format!("{width} × {height}"));
                }
                if let Some(fps) = asset.fps.filter(|fps| fps.is_finite() && *fps > 0.0) {
                    info_row(ui, "Frame rate", &format!("{fps:.3} fps"));
                }
                if let Some(duration) = asset.duration.filter(|value| value.is_finite()) {
                    info_row(ui, "Duration", &format_duration(duration));
                }
                if let Some(stream_index) = asset.stream_index {
                    info_row(ui, "Stream", &stream_index.to_string());
                }
            });
        ui.add_space(8.0);
        ui.weak("Source");
        ui.add(
            egui::Label::new(egui::RichText::new(&asset.path).monospace().small())
                .wrap()
                .selectable(true),
        )
    });
    crate::qa::register_component_with_metadata(
        format!("inspector.asset_info:{}", asset.id),
        "inspector_asset_info",
        info.response.rect,
        true,
        Some(serde_json::json!({
            "asset_id": asset.id,
            "name": asset.name,
            "kind": kind.label().to_ascii_lowercase(),
            "path": asset.path,
            "duration_seconds": asset.duration,
            "width": asset.width,
            "height": asset.height,
            "fps": asset.fps,
            "stream_index": asset.stream_index,
        })),
    );
}

fn paint_preview_card(
    ui: &mut egui::Ui,
    kind: AssetVisualKind,
    asset: &Asset,
    preview: &MediaPreviewFrame,
    waveform: &AuthoringWaveformService,
) -> (egui::Rect, Option<WaveformPaintReport>) {
    let width = ui.available_width().max(1.0);
    let height = preview_height(width, asset.width, asset.height);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 5.0, ui.visuals().extreme_bg_color);
    painter.rect_stroke(
        rect,
        5.0,
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(PREVIEW_PADDING);

    if let (Some(texture), Some([width, height])) = (&preview.texture, preview.texture_size) {
        let image_rect = fitted_rect(inner, egui::vec2(width as f32, height as f32));
        painter.image(
            texture.id(),
            image_rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        return (rect, None);
    }

    let icon_color = match kind {
        AssetVisualKind::Audio => {
            return (
                rect,
                Some(paint_audio_preview_hook(ui, rect, asset, waveform)),
            );
        }
        AssetVisualKind::Video => egui::Color32::from_rgb(85, 174, 255),
        AssetVisualKind::Image => egui::Color32::from_rgb(196, 135, 255),
        AssetVisualKind::Model3D => egui::Color32::from_rgb(255, 159, 86),
        AssetVisualKind::Other => ui.visuals().weak_text_color(),
    };
    painter.text(
        egui::pos2(rect.center().x, rect.center().y - 12.0),
        egui::Align2::CENTER_CENTER,
        kind.icon(),
        egui::FontId::proportional(42.0),
        icon_color,
    );
    painter.text(
        egui::pos2(rect.center().x, rect.center().y + 25.0),
        egui::Align2::CENTER_CENTER,
        if preview.error.is_some() {
            "Preview unavailable"
        } else {
            kind.label()
        },
        egui::FontId::proportional(12.0),
        ui.visuals().weak_text_color(),
    );
    (rect, None)
}

fn paint_audio_preview_hook(
    ui: &egui::Ui,
    rect: egui::Rect,
    asset: &Asset,
    waveform: &AuthoringWaveformService,
) -> WaveformPaintReport {
    let painter = ui.painter();
    let waveform_rect = rect.shrink2(egui::vec2(10.0, 18.0));
    painter.line_segment(
        [waveform_rect.left_center(), waveform_rect.right_center()],
        egui::Stroke::new(1.0, egui::Color32::from_gray(54)),
    );
    painter.text(
        waveform_rect.left_top(),
        egui::Align2::LEFT_TOP,
        icons::WAVEFORM,
        egui::FontId::proportional(15.0),
        egui::Color32::from_rgb(92, 214, 128),
    );
    let duration = asset
        .duration
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .unwrap_or(1.0);
    paint_authoring_waveform(WaveformPaintRequest {
        service: waveform,
        context: ui.ctx(),
        painter,
        rect: waveform_rect,
        clip_rect: waveform_rect,
        path: &asset.path,
        stream_index: asset.stream_index,
        duration_seconds: asset.duration,
        source_time_at_left: 0.0,
        source_time_at_right: duration,
        color: egui::Color32::from_rgb(92, 214, 128),
    })
}

fn register_preview_qa(
    rect: egui::Rect,
    asset: &Asset,
    kind: AssetVisualKind,
    preview: &MediaPreviewFrame,
    source_time: f64,
    waveform: Option<WaveformPaintReport>,
    waveform_sample_rate: u32,
) {
    let visual = if preview.texture.is_some() {
        "decoded_frame"
    } else {
        match kind {
            AssetVisualKind::Audio => "audio_waveform",
            AssetVisualKind::Model3D | AssetVisualKind::Other => "kind_representation",
            AssetVisualKind::Image | AssetVisualKind::Video => "unavailable",
        }
    };
    crate::qa::register_component_with_metadata(
        format!("inspector.asset_preview:{}", asset.id),
        "inspector_asset_preview",
        rect,
        true,
        Some(serde_json::json!({
            "asset_id": asset.id,
            "kind": kind.label().to_ascii_lowercase(),
            "visual": visual,
            "source_time_seconds": source_time,
            "texture_width": preview.texture_size.map(|size| size[0]),
            "texture_height": preview.texture_size.map(|size| size[1]),
            "requested_size": preview.requested_size,
            "content_hash": preview.content_hash,
            "error": preview.error,
            "pending": preview.pending,
            "fallback": preview.fallback,
            "ready": preview.texture.is_some(),
            "uses_shared_media_cache": matches!(kind, AssetVisualKind::Image | AssetVisualKind::Video | AssetVisualKind::Audio),
            "color_managed": preview.texture.is_some(),
            "waveform_status": waveform.map(WaveformPaintReport::status),
            "waveform_segments": waveform.map(|report| report.segment_count),
            "waveform_requested_chunks": waveform.map(|report| report.requested_chunks),
            "waveform_ready_chunks": waveform.map(|report| report.ready_chunks),
            "waveform_failed_chunks": waveform.map(|report| report.failed_chunks),
            "waveform_settled": waveform.map(|report| report.settled),
            "waveform_ready": waveform.map(|report| report.ready),
            "waveform_shared_painter": waveform.map(|_| "authoring_waveform"),
            "waveform_cache_owner": waveform.map(|_| "CacheManager.audio_waveform"),
            "waveform_sample_rate": waveform.map(|_| waveform_sample_rate),
            "waveform_source_time_start": waveform.map(|report| report.source_time_start),
            "waveform_source_time_end": waveform.map(|report| report.source_time_end),
            "waveform_first_source_frame": waveform.map(|report| report.first_source_frame),
            "waveform_end_source_frame_exclusive": waveform.map(|report| report.end_source_frame_exclusive),
            "waveform_decode_thread": waveform.map(|_| "background"),
        })),
    );
}

fn info_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.weak(label);
    ui.label(value);
    ui.end_row();
}

fn preview_height(width: f32, source_width: Option<u32>, source_height: Option<u32>) -> f32 {
    let aspect = source_width
        .zip(source_height)
        .filter(|(width, height)| *width > 0 && *height > 0)
        .map(|(width, height)| width as f32 / height as f32)
        .unwrap_or(16.0 / 9.0);
    (width / aspect).clamp(PREVIEW_MIN_HEIGHT, PREVIEW_MAX_HEIGHT)
}

fn fitted_rect(bounds: egui::Rect, source_size: egui::Vec2) -> egui::Rect {
    if source_size.x <= 0.0 || source_size.y <= 0.0 || bounds.is_negative() {
        return egui::Rect::from_center_size(bounds.center(), egui::Vec2::ZERO);
    }
    let scale = (bounds.width() / source_size.x)
        .min(bounds.height() / source_size.y)
        .max(0.0);
    egui::Rect::from_center_size(bounds.center(), source_size * scale)
}

fn format_duration(seconds: f64) -> String {
    let total_millis = (seconds.max(0.0) * 1_000.0).round() as u64;
    let hours = total_millis / 3_600_000;
    let minutes = total_millis % 3_600_000 / 60_000;
    let whole_seconds = total_millis % 60_000 / 1_000;
    let millis = total_millis % 1_000;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{whole_seconds:02}.{millis:03}")
    } else {
        format!("{minutes}:{whole_seconds:02}.{millis:03}")
    }
}

#[cfg(test)]
mod tests {
    use super::{fitted_rect, format_duration, preview_height};

    #[test]
    fn image_fit_preserves_aspect_and_stays_inside_card() {
        let bounds = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(240.0, 130.0));
        let fitted = fitted_rect(bounds, egui::vec2(640.0, 360.0));
        assert!(bounds.contains(fitted.min) && bounds.contains(fitted.max));
        assert!((fitted.width() / fitted.height() - 16.0 / 9.0).abs() < 0.001);
    }

    #[test]
    fn preview_height_and_basic_time_format_are_stable() {
        assert_eq!(preview_height(240.0, Some(8), Some(6)), 180.0);
        assert_eq!(preview_height(240.0, None, None), 135.0);
        assert_eq!(format_duration(65.25), "1:05.250");
    }
}
