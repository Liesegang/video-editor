use std::path::Path;

use egui_phosphor::regular as icons;
use library::editor::TimelineEditorService;
use library::model::asset::AssetKind;
use library::model::authoring::{AuthoringProject, ModuleDefinitionSharing, TimelineId};
use library::plugin::PluginManager;

use crate::state::authoring::{AuthoringLibraryDrag, AuthoringSelection, AuthoringUiState};

pub fn assets_panel(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
) {
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        import_toolbar(ui, state, service, plugins);
        ui.separator();
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("timeline_first_assets_scroll")
                .show(ui, |ui| {
                    ui.heading("Assets");
                    ui.add_space(4.0);
                    node_clip_source(ui, state);
                    ui.separator();
                    timeline_rows(ui, project, state);
                    module_rows(ui, project, state);
                    media_rows(ui, project, state);

                    let blank = ui.allocate_response(ui.available_size(), egui::Sense::click());
                    blank.context_menu(|ui| {
                        if ui
                            .button(format!("{} New Composition", icons::FILM_STRIP))
                            .clicked()
                        {
                            match service.add_timeline(
                                "Composition".to_string(),
                                1920,
                                1080,
                                library::model::authoring::RationalRate::new(30, 1)
                                    .unwrap_or_else(|_| default_rate_fallback()),
                                library::model::authoring::MediaTime::new(10, 1)
                                    .unwrap_or_else(|_| default_duration_fallback()),
                            ) {
                                Ok((timeline_id, track_id, _)) => {
                                    state.active_timeline_id = timeline_id;
                                    state.active_instance_path = None;
                                    state.timeline.expanded_tracks.insert(track_id);
                                    state
                                        .selection
                                        .replace(AuthoringSelection::Timeline(timeline_id));
                                    state.preview.auto_fit = true;
                                }
                                Err(error) => state.error = Some(error.to_string()),
                            }
                            ui.close();
                        }
                        if ui
                            .button(format!("{} New Node Clip Template", icons::SHARE_NETWORK))
                            .clicked()
                        {
                            let (definition, _) = super::project_module_definition("Node Clip");
                            if let Err(error) = service.add_module_definition(definition) {
                                state.error = Some(error.to_string());
                            }
                            ui.close();
                        }
                    });
                });
        });
    });
}

fn default_rate_fallback() -> library::model::authoring::RationalRate {
    // Constants are valid by construction; retaining a total fallback keeps
    // production UI free from panic-only paths under the strict lint policy.
    library::model::authoring::RationalRate::one()
}

fn default_duration_fallback() -> library::model::authoring::MediaTime {
    library::model::authoring::MediaTime::zero()
}

fn import_toolbar(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
) {
    ui.horizontal(|ui| {
        let import = ui
            .add(egui::Button::new(
                egui::RichText::new(icons::FILE_ARROW_DOWN).size(18.0),
            ))
            .on_hover_text("Import media");
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
            .on_hover_text("Import folder");
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
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.is_file() {
                files.push(path);
            }
        }
    }
    files
}

fn node_clip_source(ui: &mut egui::Ui, state: &mut AuthoringUiState) {
    let response = asset_row(ui, icons::SHARE_NETWORK, "Node Clip", "Procedural", false);
    crate::qa::register_component_with_metadata(
        "assets.node_clip_source",
        "asset_item",
        response.rect,
        true,
        Some(serde_json::json!({"draggable_to_timeline": true, "kind": "node_clip"})),
    );
    if response.drag_started() {
        state.timeline.library_drag = Some(AuthoringLibraryDrag::NewNodeClip);
    }
    response.on_hover_text("Drag to the Timeline to create a bounded Node graph");
}

fn timeline_rows(ui: &mut egui::Ui, project: &AuthoringProject, state: &mut AuthoringUiState) {
    ui.collapsing(format!("{} Compositions", icons::FILM_STRIP), |ui| {
        let mut timelines = project.timelines.values().collect::<Vec<_>>();
        timelines.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
        for timeline in timelines {
            let selected = state
                .selection
                .contains(AuthoringSelection::Timeline(timeline.id));
            let response = asset_row(
                ui,
                icons::FILM_STRIP,
                &timeline.name,
                &format!("{}×{}", timeline.width, timeline.height),
                selected,
            );
            crate::qa::register_component_with_metadata(
                format!("assets.timeline:{}", timeline.id),
                "asset_item",
                response.rect,
                true,
                Some(serde_json::json!({
                    "timeline_id": timeline.id,
                    "active": state.active_timeline_id == timeline.id,
                    "draggable_to_timeline": timeline.id != state.active_timeline_id,
                })),
            );
            if response.clicked() {
                state
                    .selection
                    .replace(AuthoringSelection::Timeline(timeline.id));
            }
            if response.double_clicked() {
                open_timeline(project, state, timeline.id);
            }
            if response.drag_started() && timeline.id != state.active_timeline_id {
                state.timeline.library_drag = Some(AuthoringLibraryDrag::Timeline(timeline.id));
            }
            response.context_menu(|ui| {
                if ui
                    .button(format!("{} Open Timeline", icons::ARROW_SQUARE_OUT))
                    .clicked()
                {
                    open_timeline(project, state, timeline.id);
                    ui.close();
                }
            });
        }
    });
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
    state.timeline.current_frame = 0;
    state.timeline.set_playing(false);
    state.preview.auto_fit = true;
}

fn module_rows(ui: &mut egui::Ui, project: &AuthoringProject, state: &mut AuthoringUiState) {
    ui.collapsing(
        format!("{} Node Clip Templates", icons::SHARE_NETWORK),
        |ui| {
            let mut definitions = project
                .module_definitions
                .values()
                .filter(|definition| {
                    matches!(
                        &definition.sharing,
                        ModuleDefinitionSharing::ReusableTemplate(_)
                    )
                })
                .collect::<Vec<_>>();
            definitions
                .sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
            if definitions.is_empty() {
                ui.weak("Right-click empty space to create a template");
            }
            for definition in definitions {
                let selected = state
                    .selection
                    .contains(AuthoringSelection::ModuleDefinition(definition.id));
                let response = asset_row(
                    ui,
                    icons::SHARE_NETWORK,
                    &definition.name,
                    "Module",
                    selected,
                );
                crate::qa::register_component_with_metadata(
                    format!("assets.module:{}", definition.id),
                    "asset_item",
                    response.rect,
                    true,
                    Some(serde_json::json!({
                        "module_definition_id": definition.id,
                        "draggable_to_timeline": true,
                    })),
                );
                if response.clicked() {
                    state
                        .selection
                        .replace(AuthoringSelection::ModuleDefinition(definition.id));
                }
                if response.drag_started() {
                    state.timeline.library_drag =
                        Some(AuthoringLibraryDrag::ModuleDefinition(definition.id));
                }
            }
        },
    );
}

fn media_rows(ui: &mut egui::Ui, project: &AuthoringProject, state: &mut AuthoringUiState) {
    ui.collapsing(format!("{} Media", icons::FOLDER), |ui| {
        if project.assets.is_empty() {
            ui.weak("Import media with the toolbar below");
        }
        for asset in &project.assets {
            let (icon, kind) = match asset.kind {
                AssetKind::Video => (icons::FILE_VIDEO, "Video"),
                AssetKind::Audio => (icons::FILE_AUDIO, "Audio"),
                AssetKind::Image => (icons::FILE_IMAGE, "Image"),
                AssetKind::Model3D => (icons::CUBE, "3D"),
                AssetKind::Other => (icons::FILE, "File"),
            };
            let selected = state
                .selection
                .contains(AuthoringSelection::Asset(asset.id));
            let response = asset_row(ui, icon, &asset.name, kind, selected);
            crate::qa::register_component_with_metadata(
                format!("assets.asset:{}", asset.id),
                "asset_item",
                response.rect,
                true,
                Some(serde_json::json!({
                    "asset_id": asset.id,
                    "kind": kind.to_ascii_lowercase(),
                    "draggable_to_timeline": true,
                })),
            );
            if response.clicked() {
                state.selection.replace(AuthoringSelection::Asset(asset.id));
            }
            if response.drag_started() {
                state.timeline.library_drag = Some(AuthoringLibraryDrag::Asset(asset.id));
            }
            response.on_hover_text(&asset.path);
        }
    });
}

fn asset_row(
    ui: &mut egui::Ui,
    icon: &str,
    name: &str,
    detail: &str,
    selected: bool,
) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::click_and_drag());
    let visuals = ui.style().interact_selectable(&response, selected);
    if selected || response.hovered() {
        ui.painter().rect_filled(rect, 3.0, visuals.bg_fill);
    }
    ui.painter().text(
        rect.left_center() + egui::vec2(6.0, 0.0),
        egui::Align2::LEFT_CENTER,
        icon,
        egui::FontId::proportional(17.0),
        visuals.text_color(),
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(30.0, 0.0),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(13.0),
        visuals.text_color(),
    );
    ui.painter().text(
        rect.right_center() - egui::vec2(6.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        detail,
        egui::FontId::proportional(11.0),
        ui.visuals().weak_text_color(),
    );
    response
}
