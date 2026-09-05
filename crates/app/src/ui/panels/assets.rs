//! Project library for media, nested Compositions, and reusable Node Clips.
//!
//! Assets are sources. Placement is deliberately a drag from one of these
//! rows to the Timeline; this panel never grows a second placement command.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use egui::{Sense, UiBuilder, Vec2};
use egui_phosphor::regular as icons;
use library::editor::{AuthoringWaveformService, TimelineEditorService};
use library::model::authoring::{
    AuthoringProject, MediaTime, ModuleDefinition, RationalRate, TimelineId,
};
use library::plugin::PluginManager;

use crate::state::authoring::{AssetBrowserViewMode, AuthoringSelection, AuthoringUiState};
use crate::ui::media_preview::AuthoringMediaPreviewService;
use crate::ui::panel_layout::allocate_panel_with_footer;

const TOOLBAR_HEIGHT: f32 = 31.0;

mod views;

pub fn assets_panel(
    ui: &mut egui::Ui,
    project: &Arc<AuthoringProject>,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    waveform: &AuthoringWaveformService,
    media_previews: &mut AuthoringMediaPreviewService,
) {
    assets_header(ui, state);
    ui.separator();

    let regions = allocate_panel_with_footer(ui, TOOLBAR_HEIGHT);
    let scroll = ui
        .scope_builder(
            UiBuilder::new()
                .max_rect(regions.body)
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| {
                let scroll = if state.assets.view_mode == AssetBrowserViewMode::Table {
                    egui::ScrollArea::both()
                } else {
                    egui::ScrollArea::vertical()
                }
                .scroll_bar_visibility(
                    egui::containers::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                );
                scroll
                    .id_salt("assets.scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        views::project_library(
                            ui,
                            project,
                            state,
                            service,
                            waveform,
                            media_previews,
                        );
                        let blank = ui.allocate_response(
                            Vec2::new(ui.available_width(), 36.0),
                            Sense::click(),
                        );
                        blank.context_menu(|ui| creation_menu(ui, project, state, service));
                    })
            },
        )
        .inner;

    ui.scope_builder(
        UiBuilder::new()
            .max_rect(regions.footer)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| import_toolbar(ui, state, service, plugins),
    );

    crate::qa::register_component("assets.footer", "panel_footer", regions.footer);
    crate::qa::register_component_with_metadata(
        "assets.layout",
        "panel_layout",
        regions.body.union(regions.footer),
        true,
        Some(serde_json::json!({
            "body": rect_metadata(regions.body),
            "footer": rect_metadata(regions.footer),
            "footer_contained": regions.body.bottom() <= regions.footer.top(),
            "content_width": scroll.content_size.x,
            "content_height": scroll.content_size.y,
            "viewport_width": scroll.inner_rect.width(),
            "viewport_height": scroll.inner_rect.height(),
            "horizontal_overflow": scroll.content_size.x > scroll.inner_rect.width() + 0.1,
            "vertical_overflow": scroll.content_size.y > scroll.inner_rect.height() + 0.1,
            "offset_x": scroll.state.offset.x,
            "offset_y": scroll.state.offset.y,
            "scrollbars": "visible_when_needed",
        })),
    );
}

fn rect_metadata(rect: egui::Rect) -> serde_json::Value {
    serde_json::json!({
        "min_x": rect.min.x,
        "min_y": rect.min.y,
        "max_x": rect.max.x,
        "max_y": rect.max.y,
    })
}

fn assets_header(ui: &mut egui::Ui, state: &mut AuthoringUiState) {
    let header = ui.horizontal(|ui| {
        ui.heading(format!("{} Assets", icons::FOLDER));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (mode, icon, tooltip) in [
                (AssetBrowserViewMode::Grid, icons::SQUARES_FOUR, "Grid view"),
                (AssetBrowserViewMode::Table, icons::TABLE, "Table view"),
                (AssetBrowserViewMode::List, icons::LIST, "List view"),
            ] {
                let response = ui
                    .add(egui::Button::selectable(
                        state.assets.view_mode == mode,
                        egui::RichText::new(icon).size(16.0),
                    ))
                    .on_hover_text(tooltip);
                crate::qa::register_component_with_metadata(
                    format!("assets.view.{}", mode.qa_name()),
                    "asset_view_toggle",
                    response.rect,
                    response.enabled(),
                    Some(serde_json::json!({
                        "mode": mode.qa_name(),
                        "active": state.assets.view_mode == mode,
                        "icon_only": true,
                    })),
                );
                if response.clicked() {
                    state.assets.view_mode = mode;
                }
            }
        });
    });
    crate::qa::register_component_with_metadata(
        "assets.view_mode",
        "asset_view_mode",
        header.response.rect,
        true,
        Some(serde_json::json!({
            "mode": state.assets.view_mode.qa_name(),
            "available_modes": ["list", "table", "grid"],
        })),
    );
}

fn import_toolbar(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
) {
    let toolbar = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let import = ui
            .add(egui::Button::new(
                egui::RichText::new(icons::FILE_ARROW_DOWN).size(18.0),
            ))
            .on_hover_text("Import media files");
        crate::qa::register_component("assets.import", "asset_toolbar_button", import.rect);
        if import.clicked() {
            if let Some(paths) = rfd::FileDialog::new().pick_files() {
                import_paths(
                    paths.iter().map(std::path::PathBuf::as_path),
                    state,
                    service,
                    plugins,
                );
            }
        }

        let folder = ui
            .add(egui::Button::new(
                egui::RichText::new(icons::FOLDER_OPEN).size(18.0),
            ))
            .on_hover_text("Import a folder recursively");
        crate::qa::register_component("assets.import_folder", "asset_toolbar_button", folder.rect);
        if folder.clicked() {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                let paths = walk_files(&path);
                import_paths(
                    paths.iter().map(std::path::PathBuf::as_path),
                    state,
                    service,
                    plugins,
                );
            }
        }
    });
    crate::qa::register_component_with_metadata(
        "assets.toolbar",
        "asset_toolbar",
        toolbar.response.rect,
        true,
        Some(serde_json::json!({"drag_instruction_visible": false})),
    );
}

fn import_paths<'a>(
    paths: impl IntoIterator<Item = &'a Path>,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
) {
    let mut imported = 0_usize;
    let mut errors = Vec::new();
    for path in paths {
        match service.import_file(path, plugins) {
            Ok((ids, _)) => imported += ids.len(),
            Err(error) => errors.push(format!("{}: {error}", path.display())),
        }
    }
    state.status = format!("Imported {imported} asset(s)");
    if !errors.is_empty() {
        state.error = Some(errors.join("\n"));
    }
}

fn walk_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&path) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn creation_menu(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
) {
    if ui
        .button(format!("{} New Composition", icons::FILM_STRIP))
        .clicked()
    {
        create_composition(project, state, service);
        ui.close();
    }
    if ui
        .button(format!("{} New Node Clip Template", icons::SHARE_NETWORK))
        .clicked()
    {
        create_node_clip_template(project, state, service);
        ui.close();
    }
}

fn create_composition(
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
) {
    let template = project
        .timelines
        .get(&state.active_timeline_id)
        .or_else(|| project.timelines.get(&project.root_timeline_id));
    let (width, height, fps, duration) = template.map_or_else(
        || {
            (
                1920,
                1080,
                RationalRate::new(30, 1).unwrap_or_else(|_| RationalRate::one()),
                MediaTime::new(10, 1).unwrap_or_else(|_| MediaTime::zero()),
            )
        },
        |timeline| {
            (
                timeline.width,
                timeline.height,
                timeline.fps,
                timeline.duration,
            )
        },
    );
    let name = unique_name(
        "Composition",
        project
            .timelines
            .values()
            .map(|timeline| timeline.name.as_str()),
    );
    match service.add_timeline(name.clone(), width, height, fps, duration) {
        Ok((timeline_id, track_id, _)) => {
            state.active_timeline_id = timeline_id;
            state.active_instance_path = None;
            state.timeline.expanded_tracks.insert(track_id);
            state
                .selection
                .replace(AuthoringSelection::Timeline(timeline_id));
            state.timeline.seek_frame(0);
            state.timeline.set_playing(false);
            state.preview.auto_fit = true;
            state.status = format!("Created {name}");
        }
        Err(error) => state.error = Some(error.to_string()),
    }
}

fn create_node_clip_template(
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
) {
    let name = unique_name(
        "Node Clip",
        project
            .module_definitions
            .values()
            .map(|definition| definition.name.as_str()),
    );
    let (definition, _) = ModuleDefinition::new_project_image(name.clone());
    let definition_id = definition.id;
    match service.add_module_definition(definition) {
        Ok(_) => {
            state
                .selection
                .replace(AuthoringSelection::ModuleDefinition(definition_id));
            state.status = format!("Created {name} template");
        }
        Err(error) => state.error = Some(error.to_string()),
    }
}

fn open_timeline(
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    timeline_id: TimelineId,
) {
    state.active_timeline_id = timeline_id;
    if let Some(timeline) = project.timelines.get(&timeline_id) {
        state
            .timeline
            .expanded_tracks
            .extend(timeline.track_order.iter().copied());
    }
    state.active_instance_path = (timeline_id == project.root_timeline_id)
        .then(|| library::model::authoring::InstancePath::root(project.root_timeline_id));
    state
        .selection
        .replace(AuthoringSelection::Timeline(timeline_id));
    state.timeline.seek_frame(0);
    state.timeline.set_playing(false);
    state.preview.auto_fit = true;
}

fn unique_name<'a>(base: &str, existing: impl IntoIterator<Item = &'a str>) -> String {
    let existing = existing.into_iter().collect::<HashSet<_>>();
    if !existing.contains(base) {
        return base.to_string();
    }
    (2..)
        .map(|suffix| format!("{base} {suffix}"))
        .find(|candidate| !existing.contains(candidate.as_str()))
        .unwrap_or_else(|| format!("{base} Copy"))
}

#[cfg(test)]
mod tests {
    use super::unique_name;

    #[test]
    fn names_are_unique_without_discarding_the_readable_base() {
        assert_eq!(unique_name("Composition", ["Main", "Title"]), "Composition");
        assert_eq!(
            unique_name("Composition", ["Composition", "Composition 2"]),
            "Composition 3"
        );
    }
}
