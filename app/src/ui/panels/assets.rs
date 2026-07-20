use egui::Ui;
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as icons;
use library::model::asset::AssetKind;
use library::model::project::Project;
use library::EditorService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::ui::dialogs::composition_dialog::CompositionDialog;
use crate::utils::lock::read_or_recover;
use crate::{
    action::{activate_composition_with_history, commit_live_project_edits, HistoryManager},
    state::context::EditorContext,
    state::context_types::{DragStateItem, SelectionTarget},
};

fn push_project_history(project_service: &EditorService, history_manager: &mut HistoryManager) {
    let project = project_service.get_project();
    history_manager.push_project_state(read_or_recover(project.as_ref()).clone());
}

fn select_composition(
    editor_context: &mut EditorContext,
    composition_id: Uuid,
    history_manager: &mut HistoryManager,
    project: &Arc<RwLock<Project>>,
) {
    // Selecting the already active Composition still changes the typed owner
    // to Composition. Close edits owned by the previous Clip/Node first even
    // though navigation itself will be a no-op.
    commit_live_project_edits(editor_context, history_manager, project);
    activate_composition_with_history(
        editor_context,
        Some(composition_id),
        history_manager,
        project,
    );
    editor_context.select_target(SelectionTarget::Composition(composition_id));
}

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
        commit_live_project_edits(editor_context, history_manager, project);
        match project_service.add_composition(
            &composition_dialog.name,
            composition_dialog.width as u32,
            composition_dialog.height as u32,
            composition_dialog.fps,
            composition_dialog.duration,
        ) {
            Ok(new_comp_id) => {
                select_composition(editor_context, new_comp_id, history_manager, project);
                push_project_history(project_service, history_manager);
                needs_refresh = true;
            }
            Err(error) => {
                editor_context.interaction.active_modal_error =
                    Some(format!("Failed to add composition: {error}"));
            }
        }
        composition_dialog.confirmed = false; // Reset confirmed state
        *composition_dialog = CompositionDialog::new(); // Reset dialog state
    }

    // Handle edit composition dialog results
    if composition_dialog.confirmed && composition_dialog.edit_mode {
        let update_result = composition_dialog.comp_id.map_or_else(
            || Err("composition ID is missing".to_string()),
            |composition_id| {
                project_service
                    .update_composition(
                        composition_id,
                        &composition_dialog.name,
                        composition_dialog.width as u32,
                        composition_dialog.height as u32,
                        composition_dialog.fps,
                        composition_dialog.duration,
                    )
                    .map_err(|error| error.to_string())
            },
        );
        match update_result {
            Ok(()) => {
                push_project_history(project_service, history_manager);
                needs_refresh = true;
            }
            Err(error) => {
                editor_context.interaction.active_modal_error =
                    Some(format!("Failed to update composition: {error}"));
            }
        }
        composition_dialog.confirmed = false; // Reset confirmed state
        *composition_dialog = CompositionDialog::new(); // Reset dialog state
    }

    let mut comp_to_remove = None;
    let mut comp_to_activate = None;
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
                        push_project_history(project_service, history_manager);
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

                    let visit_result = visit_dirs(&folder_path, &mut |path| {
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
                    if let Err(error) = visit_result {
                        report.errors.push((
                            folder_path.to_string_lossy().into_owned(),
                            error.to_string(),
                        ));
                    }

                    if imported_any {
                        push_project_history(project_service, history_manager);
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
                                            let is_selected = editor_context.is_selected(
                                                SelectionTarget::Composition(comp.id),
                                            );
                                            // Composition rows are timeline sources as well as
                                            // navigation targets. A click-only selectable never
                                            // reports `drag_started`, which silently made nested
                                            // Composition placement impossible.
                                            let response = ui.add(
                                                egui::Button::selectable(is_selected, &comp.name)
                                                    .sense(egui::Sense::click_and_drag()),
                                            );

                                            crate::qa::register_component_with_metadata(
                                                format!("assets.composition:{}", comp.id),
                                                "assets_composition",
                                                response.rect,
                                                response.enabled(),
                                                Some(serde_json::json!({
                                                    "composition_id": comp.id,
                                                    "active": editor_context.active_composition_id
                                                        == Some(comp.id),
                                                    "draggable_to_timeline": true,
                                                })),
                                            );

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
                                                comp_to_activate = Some(comp.id);
                                            }

                                            if response.drag_started() {
                                                editor_context.interaction.dragged_item =
                                                    Some(DragStateItem::Composition {
                                                        id: comp.id,
                                                        pos: None,
                                                    });
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
                                                match &editor_context.interaction.dragged_item {
                                                    Some(DragStateItem::Asset { asset_id: id, .. }) => *id == asset.id,
                                                    _ => false,
                                                };

                                            // Use selectable_label logic or custom logic
                                            // We want it to be selectable? Maybe not strictly "selected" as current selection model is Comp/Track/Entity.
                                            // But we need context menu and drag.

                                            let response = ui.add(
                                                egui::Label::new(&asset.name)
                                                    .selectable(false)
                                                    .sense(
                                                        egui::Sense::click()
                                                            .union(egui::Sense::drag()),
                                                    ),
                                            );

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
                                                    Some(DragStateItem::Asset {
                                                        asset_id: asset.id,
                                                        pos: response.interact_pointer_pos(),
                                                    });
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

    if let Some(comp_id) = comp_to_activate {
        select_composition(editor_context, comp_id, history_manager, project);
    }

    // Handle deferred deletions (to avoid deadlock)
    if let Some(comp_id) = comp_to_remove {
        let removed_active_composition = editor_context.active_composition_id == Some(comp_id);
        commit_live_project_edits(editor_context, history_manager, project);
        match project_service.remove_composition_fully(comp_id) {
            Ok(()) => {
                if removed_active_composition {
                    activate_composition_with_history(
                        editor_context,
                        None,
                        history_manager,
                        project,
                    );
                }
                if let Ok(project) = project.read() {
                    editor_context.reconcile_selection(&project);
                }
                push_project_history(project_service, history_manager);
                needs_refresh = true;
            }
            Err(error) => {
                editor_context.interaction.active_modal_error =
                    Some(format!("Failed to remove composition: {error}"));
            }
        }
    }

    if let Some(asset_id) = asset_to_remove {
        commit_live_project_edits(editor_context, history_manager, project);
        match project_service.remove_asset_fully(asset_id) {
            Ok(()) => {
                let project = project_service.get_project();
                let current_state = read_or_recover(project.as_ref()).clone();
                editor_context.reconcile_selection(&current_state);
                history_manager.push_project_state(current_state);
                needs_refresh = true;
            }
            Err(error) => {
                editor_context.interaction.active_modal_error =
                    Some(format!("Failed to remove asset: {error}"));
            }
        }
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

#[cfg(test)]
mod tests {
    use super::select_composition;
    use crate::action::HistoryManager;
    use crate::state::context::EditorContext;
    use crate::state::context_types::{BodyDragState, SelectionTarget};
    use library::model::project::Project;
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};
    use uuid::Uuid;

    #[test]
    fn selecting_active_composition_commits_dirty_preview_and_timeline_once() {
        let composition_id = Uuid::new_v4();
        let node_id = Uuid::new_v4();
        let original = Project::new("before same-composition selection");
        let project = Arc::new(RwLock::new(original.clone()));
        project.write().unwrap().name = "after live edit".to_string();
        let edited = project.read().unwrap().clone();
        let mut context = EditorContext::new(composition_id);
        context.select_target(SelectionTarget::Node(node_id));
        context.interaction.dragged_entity_has_moved = true;
        context.interaction.is_moving_selected_entity = true;
        context.interaction.body_drag_state = Some(BodyDragState {
            start_mouse_pos: egui::Pos2::ZERO,
            original_positions: HashMap::new(),
            preview_targets: Vec::new(),
            has_changed: false,
        });
        let mut history = HistoryManager::new();
        history.push_project_state(original.clone());

        select_composition(&mut context, composition_id, &mut history, &project);

        assert_eq!(history.undo_depth(), 2);
        assert_eq!(history.undo(&edited), Some(original));
        assert!(!context.interaction.dragged_entity_has_moved);
        assert!(!context.interaction.is_moving_selected_entity);
        assert!(context.interaction.body_drag_state.is_none());
        assert_eq!(
            context.selection.primary(),
            Some(SelectionTarget::Composition(composition_id))
        );
    }
}
