use egui::Ui;
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as icons;
use library::model::project::asset::AssetKind;
use library::model::project::project::Project;
use library::EditorService;
use std::sync::{Arc, RwLock};

use crate::ui::dialogs::composition_dialog::CompositionDialog;
use crate::{
    action::HistoryManager,
    model::ui_types::DraggedItem, // Added import
    state::context::EditorContext,
};

pub fn assets_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut EditorService,
    project: &Arc<RwLock<Project>>,
    composition_dialog: &mut CompositionDialog,
) {
    let mut needs_refresh = false;

    // Handle new composition dialog results
    if composition_dialog.confirmed && !composition_dialog.edit_mode {
        let new_comp_id = project_service
            .add_composition(
                &composition_dialog.name,
                composition_dialog.width as u32,
                composition_dialog.height as u32,
                composition_dialog.fps,
                composition_dialog.duration,
            )
            .expect("Failed to add composition");
        editor_context.selection.composition_id = Some(new_comp_id);

        // No need to add to assets list anymore, as Compositions are separate

        let current_state = project_service.get_project().read().unwrap().clone();
        history_manager.push_project_state(current_state);
        needs_refresh = true;
        composition_dialog.confirmed = false; // Reset confirmed state
        *composition_dialog = CompositionDialog::new(); // Reset dialog state
    }

    // Handle edit composition dialog results
    if composition_dialog.confirmed && composition_dialog.edit_mode {
        project_service
            .update_composition(
                composition_dialog.comp_id.unwrap(),
                &composition_dialog.name,
                composition_dialog.width as u32,
                composition_dialog.height as u32,
                composition_dialog.fps,
                composition_dialog.duration,
            )
            .expect("Failed to update composition");

        // No need to update assets list manually

        let current_state = project_service.get_project().read().unwrap().clone();
        history_manager.push_project_state(current_state);
        needs_refresh = true;
        composition_dialog.confirmed = false; // Reset confirmed state
        *composition_dialog = CompositionDialog::new(); // Reset dialog state
    }

    let mut comp_to_remove = None;
    let mut asset_to_remove = None;

    // Layout: Controls at the bottom, Content filling the rest
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.add_space(5.0);

        // Control Toolbar
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0; // Tighter spacing for toolbar

            // Add Asset Button
            // Add Asset Button (Files)
            if ui
                .add(egui::Button::new(
                    egui::RichText::new(icons::FILE_PLUS).size(18.0),
                ))
                .on_hover_text("Import Assets (Files)")
                .clicked()
            {
                if let Some(paths) = rfd::FileDialog::new().pick_files() {
                    let mut imported_any = false;
                    let mut report = crate::state::context_types::ImportReport::default();

                    for path in paths {
                        let path_str = path.to_string_lossy().to_string();

                        // 1. Check Duplicates
                        if project_service.has_asset_with_path(&path_str) {
                            report.duplicates.push(
                                path.file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string(),
                            );
                            continue;
                        }

                        // 2. Try Import
                        match project_service.import_file(&path_str) {
                            Ok(asset_ids) => {
                                imported_any = true;
                                report.successful_count += asset_ids.len();
                            }
                            Err(e) => {
                                report.errors.push((
                                    path.file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .to_string(),
                                    e.to_string(),
                                ));
                                log::error!("Failed to import asset {}: {}", path_str, e)
                            }
                        }
                    }

                    if imported_any {
                        let current_state = project_service.get_project().read().unwrap().clone();
                        history_manager.push_project_state(current_state);
                        needs_refresh = true;
                    }

                    if !report.duplicates.is_empty() || !report.errors.is_empty() {
                        editor_context.interaction.import_report = Some(report);
                    }
                }
            }

            // Import Folder Button
            if ui
                .add(egui::Button::new(
                    egui::RichText::new(icons::FOLDER_PLUS).size(18.0),
                ))
                .on_hover_text("Import Folder (Recursive)")
                .clicked()
            {
                if let Some(folder_path) = rfd::FileDialog::new().pick_folder() {
                    let mut imported_any = false;
                    let mut report = crate::state::context_types::ImportReport::default();

                    // Recursive directory scanner helper
                    fn visit_dirs(
                        dir: &std::path::Path,
                        cb: &mut dyn FnMut(&std::path::Path),
                    ) -> std::io::Result<()> {
                        if dir.is_dir() {
                            for entry in std::fs::read_dir(dir)? {
                                let entry = entry?;
                                let path = entry.path();
                                if path.is_dir() {
                                    visit_dirs(&path, cb)?;
                                } else {
                                    cb(&path);
                                }
                            }
                        }
                        Ok(())
                    }

                    let _ = visit_dirs(&folder_path, &mut |path| {
                        let path_str = path.to_string_lossy().to_string();

                        // 1. Check Duplicates
                        if project_service.has_asset_with_path(&path_str) {
                            report.duplicates.push(
                                path.file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string(),
                            );
                            return;
                        }

                        // 2. Try Import
                        match project_service.import_file(&path_str) {
                            Ok(asset_ids) => {
                                imported_any = true;
                                report.successful_count += asset_ids.len();
                            }
                            Err(e) => {
                                // Only log errors for supported file types to avoid spamming "unknown format" for random files
                                // However, import_file currently returns error for unsupported types.
                                // We can just format the error.
                                // Filter out "unsupported" if desired, but user might want to know.
                                report.errors.push((
                                    path.file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .to_string(),
                                    e.to_string(),
                                ));
                            }
                        }
                    });

                    if imported_any {
                        let current_state = project_service.get_project().read().unwrap().clone();
                        history_manager.push_project_state(current_state);
                        needs_refresh = true;
                    }

                    if !report.duplicates.is_empty() || !report.errors.is_empty() {
                        editor_context.interaction.import_report = Some(report);
                    }
                }
            }

            ui.separator();

            // Add Comp Button
            if ui
                .add(egui::Button::new(
                    egui::RichText::new(icons::FILM_STRIP).size(18.0),
                ))
                .on_hover_text("New Composition")
                .clicked()
            {
                composition_dialog.open_for_new();
            }
        });

        ui.separator();

        // Main Content (Top-Down)
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.heading("Assets");
            ui.add_space(5.0);

            // 1. Compositions
            ui.heading("Compositions");

            if let Ok(proj_read) = project.read() {
                ui.push_id("compositions_table_scope", |ui| {
                    let available_height = 150.0;
                    TableBuilder::new(ui)
                        .striped(true)
                        .vscroll(true) // Enable vertical scrolling
                        .min_scrolled_height(0.0)
                        .max_scroll_height(available_height)
                        .column(Column::initial(150.0).resizable(true)) // Name
                        .column(Column::initial(80.0).resizable(true)) // Resolution
                        .column(Column::initial(40.0).resizable(true)) // FPS
                        .column(Column::remainder()) // Duration
                        .header(20.0, |mut header| {
                            header.col(|ui| {
                                ui.strong("Name");
                            });
                            header.col(|ui| {
                                ui.strong("Res");
                            });
                            header.col(|ui| {
                                ui.strong("FPS");
                            });
                            header.col(|ui| {
                                ui.strong("Dur");
                            });
                        })
                        .body(|mut body| {
                            for comp in &proj_read.compositions {
                                body.row(20.0, |mut row| {
                                    // Name Column
                                    row.col(|ui| {
                                        ui.push_id(comp.id, |ui| {
                                            let is_selected =
                                                editor_context.selection.composition_id
                                                    == Some(comp.id);
                                            let response =
                                                ui.selectable_label(is_selected, &comp.name);

                                            // Interactions (same as before)
                                            response.context_menu(|ui| {
                                                if ui
                                                    .button(format!(
                                                        "{} Edit Properties",
                                                        icons::PENCIL_SIMPLE
                                                    ))
                                                    .clicked()
                                                {
                                                    composition_dialog.open_for_edit(comp);
                                                    ui.close();
                                                }
                                                if ui
                                                    .button(format!(
                                                        "{} Delete Composition",
                                                        icons::TRASH
                                                    ))
                                                    .clicked()
                                                {
                                                    if project_service.is_composition_used(comp.id) {
                                                        let mut dialog = crate::ui::dialogs::confirmation::ConfirmationDialog::new();
                                                        dialog.open(
                                                            "⚠ Confirm Composition Deletion",
                                                            "This composition is used inside another timeline.\nDeleting it will remove all associated clips.\nAre you sure?",
                                                            crate::ui::dialogs::confirmation::ConfirmationAction::DeleteComposition(comp.id)
                                                        );
                                                        editor_context.interaction.active_confirmation = Some(dialog);
                                                    } else {
                                                        comp_to_remove = Some(comp.id);
                                                    }
                                                    ui.close();
                                                }
                                            });

                                            if response.clicked() {
                                                editor_context.selection.composition_id =
                                                    Some(comp.id);
                                                editor_context.selection.last_selected_track_id =
                                                    None;
                                                editor_context.selection.last_selected_entity_id =
                                                    None;
                                                editor_context.selection.selected_entities.clear();
                                            }

                                            if response.drag_started() {
                                                editor_context.interaction.dragged_item =
                                                    Some(DraggedItem::Composition(comp.id));
                                            }
                                            response.on_hover_text(format!("Comp ID: {}", comp.id));
                                        });
                                    });

                                    // Resolution Column
                                    row.col(|ui| {
                                        ui.label(format!("{}x{}", comp.width, comp.height));
                                    });

                                    // FPS Column
                                    row.col(|ui| {
                                        ui.label(format!("{:.2}", comp.fps));
                                    });

                                    // Duration Column
                                    row.col(|ui| {
                                        ui.label(format!("{:.2}s", comp.duration));
                                    });
                                });
                            }
                        });
                });
            }

            ui.separator();
            ui.add_space(5.0);

            // 2. Other Assets
            ui.heading("Other Assets");

            if let Ok(proj_read) = project.read() {
                ui.push_id("assets_table_scope", |ui| {
                    TableBuilder::new(ui)
                        .striped(true)
                        .vscroll(true)
                        .column(Column::auto()) // Type Icon
                        .column(Column::initial(150.0).resizable(true)) // Name
                        .column(Column::initial(80.0).resizable(true)) // Type Text
                        .column(Column::initial(80.0).resizable(true)) // Duration
                        .column(Column::remainder()) // Resolution
                        .header(20.0, |mut header| {
                            header.col(|_| {}); // Icon header empty
                            header.col(|ui| {
                                ui.strong("Name");
                            });
                            header.col(|ui| {
                                ui.strong("Type");
                            });
                            header.col(|ui| {
                                ui.strong("Duration");
                            });
                            header.col(|ui| {
                                ui.strong("Res");
                            });
                        })
                        .body(|mut body| {
                            for asset in &proj_read.assets {
                                body.row(20.0, |mut row| {
                                    let icon = match asset.kind {
                                        AssetKind::Video => icons::FILE_VIDEO,
                                        AssetKind::Audio => icons::FILE_AUDIO,
                                        AssetKind::Image => icons::FILE_IMAGE,
                                        AssetKind::Model3D => icons::CUBE,
                                        AssetKind::Other => icons::FILE,
                                    };
                                    let type_text = format!("{:?}", asset.kind); // e.g. "Video", "Image"

                                    // Icon Column
                                    row.col(|ui| {
                                        let c = asset.color.clone();
                                        let icon_color = egui::Color32::from_rgba_unmultiplied(
                                            c.r, c.g, c.b, c.a,
                                        );
                                        ui.label(
                                            egui::RichText::new(icon).color(icon_color).size(16.0),
                                        );
                                    });

                                    // Name Column (Interactive)
                                    row.col(|ui| {
                                        ui.push_id(asset.id, |ui| {
                                            let _is_dragged =
                                                match editor_context.interaction.dragged_item {
                                                    Some(DraggedItem::Asset(id)) => id == asset.id,
                                                    _ => false,
                                                };

                                            // Use selectable_label logic or custom logic
                                            // We want it to be selectable? Maybe not strictly "selected" as current selection model is Comp/Track/Entity.
                                            // But we need context menu and drag.

                                            let response =
                                                ui.add(egui::Label::new(&asset.name).sense(
                                                    egui::Sense::click().union(egui::Sense::drag()),
                                                ));

                                            // Context Menu
                                            response.context_menu(|ui| {
                                                if ui
                                                    .button(format!(
                                                        "{} Delete Asset",
                                                        icons::TRASH
                                                    ))
                                                    .clicked()
                                                {
                                                    if project_service.is_asset_used(asset.id) {
                                                        let mut dialog = crate::ui::dialogs::confirmation::ConfirmationDialog::new();
                                                        dialog.open(
                                                            "⚠ Confirm Deletion",
                                                            "This asset is used in the timeline.\nDeleting it will remove all associated clips.\nAre you sure?",
                                                            crate::ui::dialogs::confirmation::ConfirmationAction::DeleteAsset(asset.id)
                                                        );
                                                        editor_context.interaction.active_confirmation = Some(dialog);
                                                    } else {
                                                        asset_to_remove = Some(asset.id);
                                                    }
                                                    ui.close();
                                                }
                                            });

                                            // Drag
                                            if response.drag_started() {
                                                editor_context.interaction.dragged_item =
                                                    Some(DraggedItem::Asset(asset.id));
                                            }

                                            response
                                                .on_hover_text(format!("Asset ID: {}", asset.id));
                                        });
                                    });

                                    // Type Text Column
                                    row.col(|ui| {
                                        ui.label(type_text);
                                    });

                                    // Duration Column
                                    row.col(|ui| {
                                        if let Some(d) = asset.duration {
                                            ui.label(format!("{:.1}s", d));
                                        }
                                    });

                                    // Resolution Column
                                    row.col(|ui| {
                                        if let (Some(w), Some(h)) = (asset.width, asset.height) {
                                            ui.label(format!("{}x{}", w, h));
                                        }
                                    });
                                });
                            }
                        });
                });
            }
        });
    });

    // Handle deferred deletions (to avoid deadlock)
    if let Some(comp_id) = comp_to_remove {
        if let Some(selected_id) = editor_context.selection.composition_id {
            if selected_id == comp_id {
                editor_context.selection.composition_id = None;
                editor_context.selection.last_selected_track_id = None;
                editor_context.selection.last_selected_entity_id = None;
                editor_context.selection.selected_entities.clear();
            }
        }

        project_service
            .remove_composition_fully(comp_id)
            .expect("Failed to remove composition");

        let current_state = project_service.get_project().read().unwrap().clone();
        history_manager.push_project_state(current_state);
        needs_refresh = true;
    }

    if let Some(asset_id) = asset_to_remove {
        project_service
            .remove_asset_fully(asset_id)
            .expect("Failed to remove asset");

        let current_state = project_service.get_project().read().unwrap().clone();
        history_manager.push_project_state(current_state);
        needs_refresh = true;
    }

    // Old modals removed.

    // Import Report Modal
    if let Some(report) = &editor_context.interaction.import_report {
        let mut open = true;
        let mut should_close = false;

        crate::ui::widgets::modal::Modal::new("Import Result")
            .collapsible(false)
            .resizable(true)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                ui.label(format!(
                    "Successfully imported {} assets.",
                    report.successful_count
                ));

                if !report.duplicates.is_empty() {
                    ui.add_space(5.0);
                    ui.heading(
                        egui::RichText::new("Duplicates (Skipped)").color(egui::Color32::YELLOW),
                    );
                    egui::ScrollArea::vertical()
                        .max_height(100.0)
                        .show(ui, |ui| {
                            for dup in &report.duplicates {
                                ui.label(format!("• {}", dup));
                            }
                        });
                }

                if !report.errors.is_empty() {
                    ui.add_space(5.0);
                    ui.heading(egui::RichText::new("Errors").color(egui::Color32::RED));
                    egui::ScrollArea::vertical()
                        .max_height(100.0)
                        .show(ui, |ui| {
                            for (file, err) in &report.errors {
                                ui.label(format!("• {}: {}", file, err));
                            }
                        });
                }

                ui.add_space(10.0);
                if ui.button("OK").clicked() {
                    should_close = true;
                }
            });

        if !open || should_close {
            editor_context.interaction.import_report = None;
        }
    }

    if needs_refresh {
        ui.ctx().request_repaint();
    }
}
