use egui::Ui;
use egui_phosphor::regular as icons;
use library::model::project::project::Project;
use library::service::project_service::ProjectService;
use library::model::project::asset::AssetKind; // Updated import
use std::sync::{Arc, RwLock};

use crate::ui::dialogs::composition_dialog::CompositionDialog;
use crate::{
    action::HistoryManager,
    state::context::EditorContext,
    model::ui_types::DraggedItem, // Added import
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
        editor_context.selected_composition_id = Some(new_comp_id);
        
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
            if ui.add(egui::Button::new(egui::RichText::new(icons::FILE_PLUS).size(18.0)))
                .on_hover_text("Import Asset")
                .clicked() 
            {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    let path_str = path.to_string_lossy().to_string();
                    match project_service.import_file(&path_str) {
                        Ok(_) => {
                             let current_state = project_service.get_project().read().unwrap().clone();
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
            if ui.add(egui::Button::new(egui::RichText::new(icons::FILM_STRIP).size(18.0)))
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
             egui::ScrollArea::vertical()
                .id_salt("assets_compositions_scroll_area")
                .max_height(150.0)
                .show(ui, |ui| {
                    if let Ok(proj_read) = project.read() {
                        for comp in &proj_read.compositions {
                            ui.push_id(comp.id, |ui| {
                                let is_selected = editor_context.selected_composition_id == Some(comp.id);
                                let response = ui.selectable_label(is_selected, &comp.name);
                                
                                response.context_menu(|ui| {
                                    if ui.button("Edit Properties").clicked() {
                                        composition_dialog.open_for_edit(comp);
                                        ui.close();
                                    }
                                    if ui.button(format!("{} Delete Composition", icons::TRASH)).clicked() {
                                        if project_service.is_composition_used(comp.id) {
                                            editor_context.comp_delete_candidate = Some(comp.id);
                                        } else {
                                            comp_to_remove = Some(comp.id);
                                        }
                                        ui.close();
                                    }
                                });

                                if response.clicked() {
                                    editor_context.selected_composition_id = Some(comp.id);
                                    editor_context.selected_track_id = None;
                                    editor_context.selected_entity_id = None;
                                }
                                
                                if response.drag_started() {
                                    editor_context.dragged_item = Some(DraggedItem::Composition(comp.id));
                                }
                                response.on_hover_text(format!("Comp ID: {}", comp.id));
                            });
                        }
                    }
                });
            
            ui.separator();
            ui.add_space(5.0);

            // 2. Other Assets
            ui.heading("Other Assets");
            egui::ScrollArea::vertical()
                .id_salt("assets_other_scroll_area")
                .show(ui, |ui| {
                    if let Ok(proj_read) = project.read() {
                        for asset in &proj_read.assets {
                            let duration_text = if let Some(d) = asset.duration {
                                format!("({:.1}s)", d)
                            } else {
                                "".to_string()
                            };
                            let label_text = format!("{} {}", asset.name, duration_text);
                            let icon = match asset.kind {
                                AssetKind::Video => icons::FILE_VIDEO,
                                AssetKind::Audio => icons::FILE_AUDIO,
                                AssetKind::Image => icons::FILE_IMAGE,
                                AssetKind::Model3D => icons::CUBE,
                                AssetKind::Other => icons::FILE,
                            };

                            let is_dragged = match editor_context.dragged_item {
                                Some(DraggedItem::Asset(id)) => id == asset.id,
                                _ => false
                            };

                            // Custom rendering for Asset Item
                            // We allocate space and draw manually
                            let item_height = 24.0;
                            let width = ui.available_width();
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(width, item_height),
                                egui::Sense::click().union(egui::Sense::drag())
                            );

                            // Interactions
                            if response.drag_started() {
                                editor_context.dragged_item = Some(DraggedItem::Asset(asset.id));
                            }
                            
                            response.context_menu(|ui| {
                                if ui.button(format!("{} Delete Asset", icons::TRASH)).clicked() {
                                     if project_service.is_asset_used(asset.id) {
                                         editor_context.asset_delete_candidate = Some(asset.id);
                                     } else {
                                         asset_to_remove = Some(asset.id);
                                     }
                                     ui.close();
                                }
                            });

                            // Drawing
                            if ui.is_rect_visible(rect) {
                                let visuals = ui.style().interact(&response);
                                
                                // Background
                                if response.hovered() || is_dragged {
                                    ui.painter().rect(
                                        rect,
                                        2.0,
                                        visuals.bg_fill,
                                        egui::Stroke::NONE,
                                        egui::StrokeKind::Inside, 
                                    );
                                }

                                // Icon
                                let icon_rect = egui::Rect::from_min_size(
                                    rect.min + egui::vec2(4.0, 4.0),
                                    egui::vec2(16.0, 16.0)
                                );
                                let c = asset.color.clone();
                                let icon_color = egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
                                
                                ui.painter().text(
                                    icon_rect.min,
                                    egui::Align2::LEFT_TOP,
                                    icon,
                                    egui::FontId::proportional(16.0),
                                    icon_color,
                                );

                                // Text
                                let text_pos = rect.min + egui::vec2(24.0, 4.0);
                                ui.painter().text(
                                    text_pos,
                                    egui::Align2::LEFT_TOP,
                                    &label_text,
                                    egui::FontId::proportional(14.0),
                                    visuals.text_color(),
                                );
                            }
                            
                            response.on_hover_text(format!("Asset ID: {:?}", asset.id));
                        }
                    }
                });
        });
    });

    // Handle deferred deletions (to avoid deadlock)
    if let Some(comp_id) = comp_to_remove {
        if let Some(selected_id) = editor_context.selected_composition_id {
            if selected_id == comp_id {
                 editor_context.selected_composition_id = None;
                 editor_context.selected_track_id = None;
                 editor_context.selected_entity_id = None;
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
    if let Some(comp_id) = editor_context.comp_delete_candidate {
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
                        editor_context.comp_delete_candidate = None;
                    }
                    if ui.button(egui::RichText::new("Delete").color(egui::Color32::RED)).clicked() {
                         match project_service.remove_composition_fully(comp_id) {
                            Ok(_) => {
                                // Clear selection if we just deleted the selected comp
                                if editor_context.selected_composition_id == Some(comp_id) {
                                    editor_context.selected_composition_id = None;
                                    editor_context.selected_track_id = None;
                                    editor_context.selected_entity_id = None;
                                }

                                let current_state = project_service.get_project().read().unwrap().clone();
                                history_manager.push_project_state(current_state);
                                needs_refresh = true;
                            }
                            Err(e) => {
                                log::error!("Failed to remove composition fully: {}", e);
                            }
                        }
                        editor_context.comp_delete_candidate = None;
                    }
                });
            });
    }

    // Confirmation Modal for Asset Deletion
    if let Some(asset_id) = editor_context.asset_delete_candidate {
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
                        editor_context.asset_delete_candidate = None;
                    }
                    if ui.button(egui::RichText::new("Delete").color(egui::Color32::RED)).clicked() {
                         match project_service.remove_asset_fully(asset_id) {
                            Ok(_) => {
                                let current_state = project_service.get_project().read().unwrap().clone();
                                history_manager.push_project_state(current_state);
                                needs_refresh = true;
                            }
                            Err(e) => {
                                log::error!("Failed to remove asset fully: {}", e);
                            }
                        }
                        editor_context.asset_delete_candidate = None;
                    }
                });
            });
    }

    if needs_refresh {
        ui.ctx().request_repaint();
    }
}
