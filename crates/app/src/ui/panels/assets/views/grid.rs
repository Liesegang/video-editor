use super::*;

const GRID_CARD_MIN_WIDTH: f32 = 124.0;
const GRID_CARD_MAX_WIDTH: f32 = 184.0;
const GRID_CARD_HEIGHT: f32 = 132.0;
const GRID_GAP: f32 = 6.0;

#[allow(
    clippy::too_many_arguments,
    reason = "Asset grid painting keeps selection, drag handling, and shared preview services explicit at the immediate-mode UI boundary"
)]
pub(super) fn grid_entries(
    ui: &mut egui::Ui,
    entries: &[LibraryEntry<'_>],
    project: &Arc<AuthoringProject>,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    waveform: &AuthoringWaveformService,
    media_previews: &mut AuthoringMediaPreviewService,
) {
    let available = ui.available_width().max(GRID_CARD_MIN_WIDTH);
    let columns = ((available + GRID_GAP) / (GRID_CARD_MIN_WIDTH + GRID_GAP))
        .floor()
        .max(1.0) as usize;
    let width = ((available - GRID_GAP * (columns.saturating_sub(1) as f32)) / columns as f32)
        .clamp(GRID_CARD_MIN_WIDTH, GRID_CARD_MAX_WIDTH);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(GRID_GAP);
        for (index, entry) in entries.iter().copied().enumerate() {
            let response = grid_entry(
                ui,
                entry,
                index,
                width,
                project,
                state,
                waveform,
                media_previews,
            );
            handle_entry_response(ui, response, entry, project, state, service, index);
        }
    });
}

#[allow(
    clippy::too_many_arguments,
    reason = "A grid card needs its entry identity, layout, project context, and both shared preview services in one immediate-mode paint call"
)]
fn grid_entry(
    ui: &mut egui::Ui,
    entry: LibraryEntry<'_>,
    index: usize,
    width: f32,
    project: &Arc<AuthoringProject>,
    state: &AuthoringUiState,
    waveform: &AuthoringWaveformService,
    media_previews: &mut AuthoringMediaPreviewService,
) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, GRID_CARD_HEIGHT), Sense::click_and_drag());
    paint_entry_background(ui, rect, &response, entry.selected(state), index, 5.0);
    ui.painter().rect_stroke(
        rect,
        5.0,
        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        StrokeKind::Inside,
    );
    let preview_rect = Rect::from_min_max(
        rect.min + Vec2::splat(5.0),
        egui::pos2(rect.right() - 5.0, rect.top() + 88.0),
    );
    ui.painter()
        .rect_filled(preview_rect, 3.0, ui.visuals().extreme_bg_color);
    let preview = paint_grid_preview(ui, entry, preview_rect, project, waveform, media_previews);
    register_preview(entry, preview_rect, &preview);
    paint_single_line(
        ui,
        Rect::from_min_max(
            egui::pos2(rect.left() + 7.0, preview_rect.bottom() + 4.0),
            egui::pos2(rect.right() - 7.0, preview_rect.bottom() + 23.0),
        ),
        entry.name(),
        egui::FontId::proportional(12.0),
        ui.visuals().text_color(),
        0.0,
    );
    paint_single_line(
        ui,
        Rect::from_min_max(
            egui::pos2(rect.left() + 7.0, preview_rect.bottom() + 21.0),
            egui::pos2(rect.right() - 7.0, rect.bottom() - 3.0),
        ),
        &grid_subtitle(entry),
        egui::FontId::proportional(10.0),
        ui.visuals().weak_text_color(),
        0.0,
    );
    register_metadata(entry, rect, &entry.list_metadata(), rect, "grid");
    response
}

#[derive(Default)]
struct GridPreviewReport {
    visual: &'static str,
    ready: bool,
    pending: bool,
    fallback: bool,
    content_hash: Option<String>,
    requested_size: Option<[u32; 2]>,
    waveform_status: Option<&'static str>,
    uses_shared_media_cache: bool,
    uses_shared_waveform: bool,
}

fn paint_grid_preview(
    ui: &egui::Ui,
    entry: LibraryEntry<'_>,
    rect: Rect,
    project: &Arc<AuthoringProject>,
    waveform: &AuthoringWaveformService,
    media_previews: &mut AuthoringMediaPreviewService,
) -> GridPreviewReport {
    if let LibraryEntry::Media(asset) = entry {
        match asset.kind {
            AssetKind::Image | AssetKind::Video => {
                let evaluation_fps = project
                    .timelines
                    .get(&project.root_timeline_id)
                    .map_or(30.0, |timeline| timeline.fps.to_f64());
                let frame = media_previews.request(
                    ui.ctx(),
                    Arc::clone(project),
                    asset,
                    representative_source_time(asset),
                    evaluation_fps,
                    preview_request_size(ui.ctx(), rect.size()),
                );
                let report = GridPreviewReport {
                    visual: if asset.kind == AssetKind::Video {
                        "video_thumbnail"
                    } else {
                        "image_thumbnail"
                    },
                    ready: frame.texture.is_some(),
                    pending: frame.pending,
                    fallback: frame.fallback,
                    content_hash: frame.content_hash.clone(),
                    requested_size: frame.requested_size,
                    uses_shared_media_cache: true,
                    ..GridPreviewReport::default()
                };
                paint_media_frame(ui, rect, frame);
                return report;
            }
            AssetKind::Audio => {
                let source_end = asset
                    .duration
                    .filter(|duration| duration.is_finite() && *duration > 0.0)
                    .unwrap_or(1.0);
                let report = paint_authoring_waveform(WaveformPaintRequest {
                    service: waveform,
                    context: ui.ctx(),
                    painter: &ui.painter().with_clip_rect(rect),
                    rect,
                    clip_rect: rect,
                    path: &asset.path,
                    stream_index: asset.stream_index,
                    duration_seconds: asset.duration,
                    source_time_at_left: 0.0,
                    source_time_at_right: source_end,
                    color: Color32::from_rgb(104, 220, 143),
                });
                if report.segment_count == 0 {
                    paint_preview_icon(ui, rect, icons::WAVEFORM, entry.icon().1);
                }
                return GridPreviewReport {
                    visual: "audio_waveform",
                    ready: report.ready,
                    pending: !report.settled,
                    waveform_status: Some(report.status()),
                    uses_shared_waveform: true,
                    ..GridPreviewReport::default()
                };
            }
            AssetKind::Model3D | AssetKind::Other => {}
        }
    }
    let (icon, color) = entry.icon();
    paint_preview_icon(ui, rect, icon, color);
    GridPreviewReport {
        visual: match entry {
            LibraryEntry::Composition(_) => "composition_icon",
            LibraryEntry::NewNodeClip | LibraryEntry::NodeClip(_) => "node_clip_icon",
            LibraryEntry::NewParticleNodeClip => "particle_node_clip_icon",
            LibraryEntry::Media(asset) if asset.kind == AssetKind::Model3D => "model_icon",
            LibraryEntry::Media(_) => "file_icon",
        },
        ready: true,
        ..GridPreviewReport::default()
    }
}

fn paint_media_frame(ui: &egui::Ui, rect: Rect, frame: MediaPreviewFrame) {
    if let (Some(texture), Some([width, height])) = (frame.texture, frame.texture_size) {
        let source = Vec2::new(width as f32, height as f32);
        let scale = (rect.width() / source.x.max(1.0)).min(rect.height() / source.y.max(1.0));
        let fitted = Rect::from_center_size(rect.center(), source * scale);
        ui.painter().image(
            texture.id(),
            fitted,
            Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else if frame.pending {
        paint_preview_icon(
            ui,
            rect,
            icons::CIRCLE_NOTCH,
            ui.visuals().weak_text_color(),
        );
    } else {
        paint_preview_icon(
            ui,
            rect,
            icons::IMAGE_BROKEN,
            ui.visuals().weak_text_color(),
        );
    }
}

fn paint_preview_icon(ui: &egui::Ui, rect: Rect, icon: &str, color: Color32) {
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(34.0),
        color,
    );
}

fn grid_subtitle(entry: LibraryEntry<'_>) -> String {
    let size = entry.size();
    let duration = entry.duration();
    if size != "--" {
        size
    } else if duration != "--" {
        duration
    } else {
        entry.kind().to_string()
    }
}

fn register_preview(entry: LibraryEntry<'_>, rect: Rect, report: &GridPreviewReport) {
    crate::qa::register_component_with_metadata(
        entry.preview_qa_id(),
        "asset_card_preview",
        rect,
        true,
        Some(serde_json::json!({
            "asset_id": match entry { LibraryEntry::Media(asset) => Some(asset.id), _ => None },
            "entry_id": entry.qa_id(),
            "visual": report.visual,
            "ready": report.ready,
            "pending": report.pending,
            "fallback": report.fallback,
            "content_hash": report.content_hash,
            "requested_size": report.requested_size,
            "waveform_status": report.waveform_status,
            "uses_shared_media_cache": report.uses_shared_media_cache,
            "uses_shared_waveform": report.uses_shared_waveform,
        })),
    );
}
