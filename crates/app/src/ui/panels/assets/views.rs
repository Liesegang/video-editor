//! Presentation-only views for the Project library.
//!
//! Every view returns the same `Response` for the same semantic entry. Selection,
//! context menus, and Timeline drag intent are handled once below the painters.

use std::sync::Arc;

use egui::{Color32, CursorIcon, Rect, Response, Sense, Stroke, StrokeKind, Vec2};
use egui_phosphor::regular as icons;
use library::editor::{AuthoringWaveformService, TimelineEditorService};
use library::model::asset::{Asset, AssetKind};
use library::model::authoring::{
    AuthoringProject, ModuleDefinition, ModuleDefinitionSharing, Timeline,
};

use crate::state::authoring::{
    AssetBrowserViewMode, AuthoringLibraryDrag, AuthoringSelection, AuthoringUiState,
};
use crate::ui::media_preview::{
    preview_request_size, representative_source_time, AuthoringMediaPreviewService,
    MediaPreviewFrame,
};
use crate::ui::waveform::{paint_authoring_waveform, WaveformPaintRequest};

mod grid;
mod list_table;

#[derive(Clone, Copy)]
enum LibraryEntry<'a> {
    Composition(&'a Timeline),
    NewNodeClip,
    NodeClip(&'a ModuleDefinition),
    Media(&'a Asset),
}

impl<'a> LibraryEntry<'a> {
    fn qa_id(self) -> String {
        match self {
            Self::Composition(timeline) => format!("assets.composition:{}", timeline.id),
            Self::NewNodeClip => "assets.node_clip_source".to_string(),
            Self::NodeClip(definition) => format!("assets.module:{}", definition.id),
            Self::Media(asset) => format!("assets.asset:{}", asset.id),
        }
    }

    fn name(self) -> &'a str {
        match self {
            Self::Composition(timeline) => &timeline.name,
            Self::NewNodeClip => "New Node Clip",
            Self::NodeClip(definition) => &definition.name,
            Self::Media(asset) => &asset.name,
        }
    }

    fn icon(self) -> (&'static str, Color32) {
        match self {
            Self::Composition(_) => (icons::FILM_STRIP, Color32::from_rgb(242, 190, 72)),
            Self::NewNodeClip => (icons::PLUS_CIRCLE, Color32::from_rgb(202, 128, 255)),
            Self::NodeClip(_) => (icons::SHARE_NETWORK, Color32::from_rgb(202, 128, 255)),
            Self::Media(asset) => asset_kind_presentation(asset),
        }
    }

    fn kind(self) -> &'static str {
        match self {
            Self::Composition(_) => "Composition",
            Self::NewNodeClip => "Node Clip",
            Self::NodeClip(_) => "Node Clip",
            Self::Media(asset) => asset_kind_name(&asset.kind),
        }
    }

    fn size(self) -> String {
        match self {
            Self::Composition(timeline) => format!("{} x {}", timeline.width, timeline.height),
            Self::Media(asset) => asset.width.zip(asset.height).map_or_else(
                || "--".to_string(),
                |(width, height)| format!("{width} x {height}"),
            ),
            Self::NewNodeClip | Self::NodeClip(_) => "--".to_string(),
        }
    }

    fn fps(self) -> String {
        match self {
            Self::Composition(timeline) => format!("{:.3}", timeline.fps.to_f64()),
            Self::Media(asset) => asset
                .fps
                .filter(|fps| fps.is_finite() && *fps > 0.0)
                .map_or_else(|| "--".to_string(), |fps| format!("{fps:.3}")),
            Self::NewNodeClip | Self::NodeClip(_) => "--".to_string(),
        }
    }

    fn duration(self) -> String {
        match self {
            Self::Composition(timeline) => format_duration(timeline.duration.to_seconds_f64()),
            Self::Media(asset) => asset
                .duration
                .map_or_else(|| "--".to_string(), format_duration),
            Self::NewNodeClip | Self::NodeClip(_) => "--".to_string(),
        }
    }

    fn list_metadata(self) -> String {
        match self {
            Self::Composition(_) => {
                format!("{} | {} fps | {}", self.size(), self.fps(), self.duration())
            }
            Self::NewNodeClip => "Private logic clip".to_string(),
            Self::NodeClip(definition) => format!(
                "Template | {} node{}",
                definition.graph.nodes.len(),
                if definition.graph.nodes.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ),
            Self::Media(_) => {
                let mut fields = vec![self.kind().to_string()];
                let size = self.size();
                if size != "--" {
                    fields.push(size);
                }
                let fps = self.fps();
                if fps != "--" {
                    fields.push(format!("{fps} fps"));
                }
                let duration = self.duration();
                if duration != "--" {
                    fields.push(duration);
                }
                fields.join(" | ")
            }
        }
    }

    fn selected(self, state: &AuthoringUiState) -> bool {
        match self {
            Self::Composition(timeline) => state
                .selection
                .contains(AuthoringSelection::Timeline(timeline.id)),
            Self::NewNodeClip => false,
            Self::NodeClip(definition) => state
                .selection
                .contains(AuthoringSelection::ModuleDefinition(definition.id)),
            Self::Media(asset) => state
                .selection
                .contains(AuthoringSelection::Asset(asset.id)),
        }
    }

    fn draggable(self, state: &AuthoringUiState) -> bool {
        !matches!(self, Self::Composition(timeline) if timeline.id == state.active_timeline_id)
    }

    fn hover_text(self, state: &AuthoringUiState) -> String {
        let action = match self {
            Self::Composition(timeline) if timeline.id == state.active_timeline_id => {
                "Double-click to open. A Composition cannot contain itself.".to_string()
            }
            Self::Composition(_) => "Drag to the Timeline, or double-click to open".to_string(),
            Self::NewNodeClip => "Drag to create a private Node Clip".to_string(),
            Self::NodeClip(_) => "Drag this reusable Node Clip to the Timeline".to_string(),
            Self::Media(asset) => asset.path.clone(),
        };
        format!("{}\n{action}", self.list_metadata())
    }

    fn preview_qa_id(self) -> String {
        match self {
            Self::Composition(timeline) => format!("assets.preview.composition:{}", timeline.id),
            Self::NewNodeClip => "assets.preview.node_clip_source".to_string(),
            Self::NodeClip(definition) => format!("assets.preview.module:{}", definition.id),
            Self::Media(asset) => format!("assets.preview:{}", asset.id),
        }
    }
}

pub(super) fn project_library(
    ui: &mut egui::Ui,
    project: &Arc<AuthoringProject>,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    waveform: &AuthoringWaveformService,
    media_previews: &mut AuthoringMediaPreviewService,
) {
    let mut timelines = project.timelines.values().collect::<Vec<_>>();
    timelines.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    let compositions = timelines
        .into_iter()
        .map(LibraryEntry::Composition)
        .collect::<Vec<_>>();
    render_section(
        ui,
        "assets.section.compositions",
        icons::FILM_STRIP,
        "Compositions",
        &compositions,
        project,
        state,
        service,
        waveform,
        media_previews,
    );

    let mut definitions = project
        .module_definitions
        .values()
        .filter(|definition| is_node_clip_definition(definition))
        .collect::<Vec<_>>();
    definitions.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    let mut node_clips = vec![LibraryEntry::NewNodeClip];
    node_clips.extend(definitions.into_iter().map(LibraryEntry::NodeClip));
    render_section(
        ui,
        "assets.section.node_clips",
        icons::SHARE_NETWORK,
        "Node Clips",
        &node_clips,
        project,
        state,
        service,
        waveform,
        media_previews,
    );

    let mut assets = project.assets.iter().collect::<Vec<_>>();
    assets.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    let media = assets
        .into_iter()
        .map(LibraryEntry::Media)
        .collect::<Vec<_>>();
    render_section(
        ui,
        "assets.section.media",
        icons::FOLDER_OPEN,
        "Media",
        &media,
        project,
        state,
        service,
        waveform,
        media_previews,
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "Each collapsible asset section forwards the shared selection, drag, waveform, and media-preview state to the selected immediate-mode view"
)]
fn render_section(
    ui: &mut egui::Ui,
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    entries: &[LibraryEntry<'_>],
    project: &Arc<AuthoringProject>,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    waveform: &AuthoringWaveformService,
    media_previews: &mut AuthoringMediaPreviewService,
) {
    let heading = format!("{icon} {label}  {}", entries.len());
    egui::CollapsingHeader::new(heading)
        .id_salt(id)
        .default_open(true)
        .show(ui, |ui| {
            if entries.is_empty() {
                ui.weak("No media");
                return;
            }
            if state.assets.view_mode == AssetBrowserViewMode::Table {
                list_table::table_header(ui, id);
            }
            if state.assets.view_mode == AssetBrowserViewMode::Grid {
                grid::grid_entries(
                    ui,
                    entries,
                    project,
                    state,
                    service,
                    waveform,
                    media_previews,
                );
            } else {
                for (index, entry) in entries.iter().copied().enumerate() {
                    let response = match state.assets.view_mode {
                        AssetBrowserViewMode::List => {
                            list_table::list_entry(ui, entry, index, state)
                        }
                        AssetBrowserViewMode::Table => {
                            list_table::table_entry(ui, entry, index, state)
                        }
                        AssetBrowserViewMode::Grid => continue,
                    };
                    handle_entry_response(ui, response, entry, project, state, service, index);
                }
            }
        });
}

fn paint_entry_background(
    ui: &egui::Ui,
    rect: Rect,
    response: &Response,
    selected: bool,
    index: usize,
    rounding: f32,
) {
    let background = if selected {
        ui.visuals().selection.bg_fill
    } else if response.hovered() {
        ui.visuals().widgets.hovered.bg_fill
    } else if index % 2 == 1 {
        ui.visuals().faint_bg_color.gamma_multiply(0.45)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, rounding, background);
    if rounding == 0.0 {
        ui.painter().line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(
                1.0,
                ui.visuals()
                    .widgets
                    .noninteractive
                    .bg_stroke
                    .color
                    .gamma_multiply(0.35),
            ),
        );
    }
}

fn paint_single_line(
    ui: &egui::Ui,
    rect: Rect,
    text: &str,
    font: egui::FontId,
    color: Color32,
    left_padding: f32,
) {
    let mut job = egui::text::LayoutJob::simple(
        text.to_string(),
        font,
        color,
        (rect.width() - left_padding * 2.0).max(1.0),
    );
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(
            rect.left() + left_padding,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        color,
    );
}

fn register_metadata(
    entry: LibraryEntry<'_>,
    metadata_rect: Rect,
    text: &str,
    containing_rect: Rect,
    mode: &'static str,
) {
    let id = match entry {
        LibraryEntry::Composition(timeline) => {
            format!("assets.composition_metadata:{}", timeline.id)
        }
        LibraryEntry::NewNodeClip => "assets.node_clip_source_metadata".to_string(),
        LibraryEntry::NodeClip(definition) => format!("assets.module_metadata:{}", definition.id),
        LibraryEntry::Media(asset) => format!("assets.asset_metadata:{}", asset.id),
    };
    crate::qa::register_component_with_metadata(
        id,
        "asset_metadata",
        metadata_rect,
        true,
        Some(serde_json::json!({
            "text": text,
            "fully_visible": containing_rect.contains(metadata_rect.min)
                && containing_rect.contains(metadata_rect.max),
            "row_width": containing_rect.width(),
            "row_height": containing_rect.height(),
            "mode": mode,
        })),
    );
}

fn handle_entry_response(
    ui: &egui::Ui,
    response: Response,
    entry: LibraryEntry<'_>,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    row_index: usize,
) {
    let draggable = entry.draggable(state);
    let visible_rect = response.rect.intersect(ui.clip_rect());
    crate::qa::register_component_with_metadata(
        entry.qa_id(),
        "asset_item",
        visible_rect,
        response.enabled(),
        Some(entry_qa_metadata(entry, state, draggable, row_index)),
    );
    if response.clicked() {
        match entry {
            LibraryEntry::Composition(timeline) => state
                .selection
                .replace(AuthoringSelection::Timeline(timeline.id)),
            LibraryEntry::NewNodeClip => {}
            LibraryEntry::NodeClip(definition) => state
                .selection
                .replace(AuthoringSelection::ModuleDefinition(definition.id)),
            LibraryEntry::Media(asset) => {
                state.selection.replace(AuthoringSelection::Asset(asset.id))
            }
        }
    }
    if response.double_clicked() {
        if let LibraryEntry::Composition(timeline) = entry {
            super::open_timeline(project, state, timeline.id);
        }
    }
    if draggable && response.drag_started() {
        state.timeline.library_drag = Some(match entry {
            LibraryEntry::Composition(timeline) => AuthoringLibraryDrag::Timeline(timeline.id),
            LibraryEntry::NewNodeClip => AuthoringLibraryDrag::NewNodeClip,
            LibraryEntry::NodeClip(definition) => {
                AuthoringLibraryDrag::ModuleDefinition(definition.id)
            }
            LibraryEntry::Media(asset) => AuthoringLibraryDrag::Asset(asset.id),
        });
    }
    response.context_menu(|ui| {
        match entry {
            LibraryEntry::Composition(timeline) => {
                if ui
                    .button(format!("{} Open Composition", icons::ARROW_SQUARE_OUT))
                    .clicked()
                {
                    super::open_timeline(project, state, timeline.id);
                    ui.close();
                }
                ui.separator();
            }
            LibraryEntry::Media(asset) => {
                if ui
                    .button(format!("{} Copy File Path", icons::COPY))
                    .clicked()
                {
                    ui.ctx().copy_text(asset.path.clone());
                    state.status = "Copied Asset path".to_string();
                    ui.close();
                }
                ui.separator();
            }
            LibraryEntry::NewNodeClip | LibraryEntry::NodeClip(_) => {}
        }
        super::creation_menu(ui, project, state, service);
    });
    let response = response.on_hover_text(entry.hover_text(state));
    if draggable {
        response.on_hover_cursor(CursorIcon::Grab);
    }
}

fn entry_qa_metadata(
    entry: LibraryEntry<'_>,
    state: &AuthoringUiState,
    draggable: bool,
    row_index: usize,
) -> serde_json::Value {
    let mut object = serde_json::Map::from_iter([
        (
            "kind".to_string(),
            serde_json::json!(entry.kind().to_ascii_lowercase().replace(' ', "_")),
        ),
        (
            "view_mode".to_string(),
            serde_json::json!(state.assets.view_mode.qa_name()),
        ),
        (
            "draggable_to_timeline".to_string(),
            serde_json::json!(draggable),
        ),
        ("row_index".to_string(), serde_json::json!(row_index)),
    ]);
    match entry {
        LibraryEntry::Composition(timeline) => {
            object.insert("timeline_id".to_string(), serde_json::json!(timeline.id));
            object.insert(
                "active".to_string(),
                serde_json::json!(state.active_timeline_id == timeline.id),
            );
        }
        LibraryEntry::NewNodeClip => {}
        LibraryEntry::NodeClip(definition) => {
            object.insert(
                "module_definition_id".to_string(),
                serde_json::json!(definition.id),
            );
        }
        LibraryEntry::Media(asset) => {
            object.insert("asset_id".to_string(), serde_json::json!(asset.id));
            object.insert(
                "duration_seconds".to_string(),
                serde_json::json!(asset.duration),
            );
            object.insert("width".to_string(), serde_json::json!(asset.width));
            object.insert("height".to_string(), serde_json::json!(asset.height));
            object.insert("fps".to_string(), serde_json::json!(asset.fps));
        }
    }
    serde_json::Value::Object(object)
}

fn is_node_clip_definition(definition: &ModuleDefinition) -> bool {
    matches!(
        &definition.sharing,
        ModuleDefinitionSharing::ReusableTemplate(_)
    ) && definition.host_contract.transition().is_none()
}

fn asset_kind_name(kind: &AssetKind) -> &'static str {
    match kind {
        AssetKind::Video => "Video",
        AssetKind::Audio => "Audio",
        AssetKind::Image => "Image",
        AssetKind::Model3D => "3D",
        AssetKind::Other => "File",
    }
}

fn asset_kind_presentation(asset: &Asset) -> (&'static str, Color32) {
    match asset.kind {
        AssetKind::Video => (icons::FILE_VIDEO, Color32::from_rgb(85, 174, 255)),
        AssetKind::Audio => (icons::FILE_AUDIO, Color32::from_rgb(82, 205, 135)),
        AssetKind::Image => (icons::FILE_IMAGE, Color32::from_rgb(196, 135, 255)),
        AssetKind::Model3D => (icons::CUBE, Color32::from_rgb(255, 159, 86)),
        AssetKind::Other if asset.path.to_ascii_lowercase().ends_with(".svg") => {
            (icons::BEZIER_CURVE, Color32::from_rgb(196, 135, 255))
        }
        AssetKind::Other
            if [".txt", ".srt", ".vtt", ".ass"]
                .iter()
                .any(|extension| asset.path.to_ascii_lowercase().ends_with(extension)) =>
        {
            (icons::FILE_TEXT, Color32::from_rgb(242, 190, 72))
        }
        AssetKind::Other => (icons::FILE, Color32::from_rgb(155, 163, 177)),
    }
}

fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "--".to_string();
    }
    let total_seconds = seconds.floor() as u64;
    let hours = total_seconds / 3_600;
    let minutes = total_seconds % 3_600 / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests;
