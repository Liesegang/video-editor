use egui::Ui;
use egui_phosphor::regular as icons;
use library::model::project::project::Project;
use library::service::project_service::ProjectService;
use std::sync::{Arc, RwLock};

use crate::ui::dialogs::composition_dialog::CompositionDialog;
use crate::{
    action::HistoryManager,
    model::assets::{Asset, AssetKind},
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
        editor_context.selected_composition_id = Some(new_comp_id);
        editor_context.assets.push(Asset {
            name: format!("Comp: {}", composition_dialog.name),
            duration: composition_dialog.duration as f32,
            color: egui::Color32::from_rgb(255, 150, 255),
            kind: AssetKind::Composition(new_comp_id),
            composition_id: Some(new_comp_id),
        });
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

        // Update the asset name in editor_context
        if let Some(asset) = editor_context
            .assets
            .iter_mut()
            .find(|asset| matches!(asset.kind, AssetKind::Composition(id) if id == composition_dialog.comp_id.unwrap()))
        {
            asset.name = format!("Comp: {}", composition_dialog.name);
            asset.duration = composition_dialog.duration as f32;
        }

        let current_state = project_service.get_project().read().unwrap().clone();
        history_manager.push_project_state(current_state);
        needs_refresh = true;
        composition_dialog.confirmed = false; // Reset confirmed state
        *composition_dialog = CompositionDialog::new(); // Reset dialog state
    }

    ui.heading("Assets");
    ui.separator();

    // Compositions section
    ui.heading("Compositions");
    egui::ScrollArea::vertical()
        .id_salt("assets_compositions_scroll_area")
        .max_height(200.0)
        .show(ui, |ui| {
            if let Ok(proj_read) = project.read() {
                for comp in &proj_read.compositions {
                    ui.push_id(comp.id, |ui_in_scope| {
                        let is_selected = editor_context.selected_composition_id == Some(comp.id);
                        let response = ui_in_scope
                            .selectable_label(is_selected, &comp.name)
                            .on_hover_text(format!("Comp ID: {}", comp.id));

                        response.context_menu(|ui_context_menu| {
                            if ui_context_menu.button("Edit Properties").clicked() {
                                composition_dialog.open_for_edit(comp);
                                ui_context_menu.close();
                            }
                        });

                        if response.clicked() {
                            editor_context.selected_composition_id = Some(comp.id);
                            editor_context.selected_track_id = None; // Deselect track when composition changes
                            editor_context.selected_entity_id = None; // Deselect entity when composition changes
                        }
                    });
                }
            }
        });

    ui.horizontal(|ui| {
        if ui.button(format!("{} Add Comp", icons::PLUS)).clicked() {
            composition_dialog.open_for_new();
        }
        if ui.button(format!("{} Remove Comp", icons::MINUS)).clicked() {
            if let Some(comp_id) = editor_context.selected_composition_id {
                project_service
                    .remove_composition(comp_id)
                    .expect("Failed to remove composition");
                // Also remove the corresponding asset
                editor_context.assets.retain(
                    |asset| !matches!(asset.kind, AssetKind::Composition(id) if id == comp_id),
                );
                editor_context.selected_composition_id = None;
                editor_context.selected_track_id = None;
                editor_context.selected_entity_id = None;
                let current_state = project_service.get_project().read().unwrap().clone();
                history_manager.push_project_state(current_state);
                needs_refresh = true;
            }
        }
    });

    ui.add_space(10.0);
    ui.separator(); // Separate compositions from other assets

    // Other Assets section
    ui.heading("Other Assets");
    egui::ScrollArea::vertical()
        .id_salt("assets_other_scroll_area")
        .show(ui, |ui| {
            for (index, asset) in editor_context.assets.iter().enumerate() {
                // Only display non-composition assets here
                if !matches!(asset.kind, AssetKind::Composition(_)) {
                    ui.push_id(asset.id(), |ui_in_scope| {
                        let label_text = format!("{} ({:.1}s)", asset.name, asset.duration);
                        let icon = match asset.kind {
                            AssetKind::Video => icons::FILE_VIDEO,
                            AssetKind::Audio => icons::FILE_AUDIO,
                            AssetKind::Image => icons::FILE_IMAGE,
                            AssetKind::Composition(_) => unreachable!("Compositions handled above"),
                        };

                        let rich_text_label =
                            egui::RichText::new(format!("{} {}", icon, label_text))
                                .color(egui::Color32::BLACK)
                                .background_color(asset.color);

                        let item_response = ui_in_scope
                            .add(egui::Label::new(rich_text_label).sense(egui::Sense::drag()))
                            .on_hover_text(format!("Asset ID: {:?}", asset.id()));

                        if item_response.drag_started() {
                            editor_context.dragged_asset = Some(index);
                        }
                        ui_in_scope.add_space(5.0);
                    });
                }
            }
        });

    if needs_refresh {
        ui.ctx().request_repaint();
    }
}
