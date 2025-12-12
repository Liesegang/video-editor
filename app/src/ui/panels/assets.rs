use egui::Ui;
use egui_phosphor::regular as icons;
use library::model::project::asset::AssetKind;
use library::model::project::project::Project;
use library::service::project_service::ProjectService;
use std::sync::{Arc, RwLock};
use egui_extras::{TableBuilder, Column};

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
    project_service: &mut ProjectService,
    project: &Arc<RwLock<Project>>,
    composition_dialog: &mut CompositionDialog,
) {
    let mut needs_refresh = false;

    // Handle new composition dialog results
    if composition_dialog.confirmed && !composition_dialog.edit_mode {
        let new_comp_id = project_service
            .add_composition(
                &composition_dialog.name,
                composition_dialog.width,
                composition_dialog.height,
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
                composition_dialog.width,
                composition_dialog.height,
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
            if ui
                .add(egui::Button::new(
                    egui::RichText::new(icons::FILE_PLUS).size(18.0),
                ))
                .on_hover_text("Import Asset")
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    let path_str = path.to_string_lossy().to_string();
                    match project_service.import_file(&path_str) {
                        Ok(_) => {
                            let current_state =
                                project_service.get_project().read().unwrap().clone();
                            history_manager.push_project_state(current_state);
                            needs_refresh = true;
                        }
                        Err(e) => {
                            log::error!("Failed to import asset: {}", e);
                        }
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
                        .column(Column::initial(80.0).resizable(true))  // Resolution
                        .column(Column::initial(40.0).resizable(true))  // FPS
                        .column(Column::remainder())                    // Duration
                        .header(20.0, |mut header| {
                            header.col(|ui| { ui.strong("Name"); });
                            header.col(|ui| { ui.strong("Res"); });
                            header.col(|ui| { ui.strong("FPS"); });
                            header.col(|ui| { ui.strong("Dur"); });
                        })
                        .body(|mut body| {
                            for comp in &proj_read.compositions {
                                body.row(20.0, |mut row| {
                                    // Name Column
                                    row.col(|ui| {
                                        ui.push_id(comp.id, |ui| {
                                            let is_selected = editor_context.selection.composition_id == Some(comp.id);
                                            let response = ui.selectable_label(is_selected, &comp.name);
                                            
                                            // Interactions (same as before)
                                            response.context_menu(|ui| {
                                            if ui.button(format!("{} Edit Properties", icons::PENCIL_SIMPLE)).clicked() {
                                                composition_dialog.open_for_edit(comp);
                                                ui.close();
                                            }
                                                if ui.button(format!("{} Delete Composition", icons::TRASH)).clicked() {
                                                    if project_service.is_composition_used(comp.id) {
                                                        editor_context.interaction.comp_delete_candidate = Some(comp.id);
                                                    } else {
                                                        comp_to_remove = Some(comp.id);
                                                    }
                                                    ui.close();
                                                }
                                            });
    
                                            if response.clicked() {
                                                editor_context.selection.composition_id = Some(comp.id);
                                                editor_context.selection.track_id = None;
                                                editor_context.selection.entity_id = None;
                                            }
    
                                            if response.drag_started() {
                                                editor_context.interaction.dragged_item = Some(DraggedItem::Composition(comp.id));
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
                        .column(Column::auto())                         // Type Icon
                        .column(Column::initial(150.0).resizable(true)) // Name
                        .column(Column::initial(80.0).resizable(true))  // Type Text
                        .column(Column::initial(80.0).resizable(true))  // Duration
                        .column(Column::remainder())                    // Resolution
                        .header(20.0, |mut header| {
                            header.col(|_| {}); // Icon header empty
                            header.col(|ui| { ui.strong("Name"); });
                            header.col(|ui| { ui.strong("Type"); });
                            header.col(|ui| { ui.strong("Duration"); });
                            header.col(|ui| { ui.strong("Res"); });
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
                                        let icon_color = egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
                                        ui.label(egui::RichText::new(icon).color(icon_color).size(16.0));
                                    });
    
                                        // Name Column (Interactive)
                                        row.col(|ui| {
                                            ui.push_id(asset.id, |ui| {
                                                let _is_dragged = match editor_context.interaction.dragged_item {
                                                    Some(DraggedItem::Asset(id)) => id == asset.id,
                                                    _ => false,
                                                };
                                            
                                                // Use selectable_label logic or custom logic
                                                // We want it to be selectable? Maybe not strictly "selected" as current selection model is Comp/Track/Entity.
                                                // But we need context menu and drag.
                                                
                                                let response = ui.add(egui::Label::new(&asset.name).sense(egui::Sense::click().union(egui::Sense::drag())));
    
                                                // Context Menu
                                                response.context_menu(|ui| {
                                                    if ui.button(format!("{} Delete Asset", icons::TRASH)).clicked() {
                                                        if project_service.is_asset_used(asset.id) {
                                                            editor_context.interaction.asset_delete_candidate = Some(asset.id);
                                                        } else {
                                                            asset_to_remove = Some(asset.id);
                                                        }
                                                        ui.close();
                                                    }
                                                });
    
                                                // Drag
                                                if response.drag_started() {
                                                    editor_context.interaction.dragged_item = Some(DraggedItem::Asset(asset.id));
                                                }
                                                
                                                response.on_hover_text(format!("Asset ID: {}", asset.id));
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
                editor_context.selection.track_id = None;
                editor_context.selection.entity_id = None;
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

    // Confirmation Modal for Composition Deletion
    if let Some(comp_id) = editor_context.interaction.comp_delete_candidate {
        egui::Window::new("⚠ Confirm Composition Deletion")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ui.ctx(), |ui| {
                ui.label("This composition is used inside another timeline.");
                ui.label("Deleting it will remove all associated clips.");
                ui.label("Are you sure?");

                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        editor_context.interaction.comp_delete_candidate = None;
                    }
                    if ui
                        .button(egui::RichText::new("Delete").color(egui::Color32::RED))
                        .clicked()
                    {
                        match project_service.remove_composition_fully(comp_id) {
                            Ok(_) => {
                                // Clear selection if we just deleted the selected comp
                                if editor_context.selection.composition_id == Some(comp_id) {
                                    editor_context.selection.composition_id = None;
                                    editor_context.selection.track_id = None;
                                    editor_context.selection.entity_id = None;
                                }

                                let current_state =
                                    project_service.get_project().read().unwrap().clone();
                                history_manager.push_project_state(current_state);
                                needs_refresh = true;
                            }
                            Err(e) => {
                                log::error!("Failed to remove composition fully: {}", e);
                            }
                        }
                        editor_context.interaction.comp_delete_candidate = None;
                    }
                });
            });
    }

    // Confirmation Modal for Asset Deletion
    if let Some(asset_id) = editor_context.interaction.asset_delete_candidate {
        egui::Window::new("⚠ Confirm Deletion")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ui.ctx(), |ui| {
                ui.label("This asset is used in the timeline.");
                ui.label("Deleting it will remove all associated clips.");
                ui.label("Are you sure?");

                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        editor_context.interaction.asset_delete_candidate = None;
                    }
                    if ui
                        .button(egui::RichText::new("Delete").color(egui::Color32::RED))
                        .clicked()
                    {
                        match project_service.remove_asset_fully(asset_id) {
                            Ok(_) => {
                                let current_state =
                                    project_service.get_project().read().unwrap().clone();
                                history_manager.push_project_state(current_state);
                                needs_refresh = true;
                            }
                            Err(e) => {
                                log::error!("Failed to remove asset fully: {}", e);
                            }
                        }
                        editor_context.interaction.asset_delete_candidate = None;
                    }
                });
            });
    }

    if needs_refresh {
        ui.ctx().request_repaint();
    }
}
