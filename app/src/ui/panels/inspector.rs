use egui::Ui;
use std::sync::{Arc, RwLock};
use library::model::project::project::Project;
use library::model::project::property::{Property, PropertyValue};
use library::service::project_service::ProjectService;

use crate::{
    action::{HistoryManager},
    state::context::EditorContext,
    model::assets::{Asset, AssetKind},
};


pub fn inspector_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    _history_manager: &mut HistoryManager, // Not used directly in current implementation, but kept for consistency
    project_service: &ProjectService,
    project: &Arc<RwLock<Project>>,
) {
    let mut needs_refresh = false;

    // Display compositions
    ui.heading("Compositions");
    ui.separator();
    egui::ScrollArea::vertical()
        .id_source("compositions_scroll_area")
        .max_height(200.0)
        .show(ui, |ui| {
            if let Ok(proj_read) = project.read() {
                for comp in &proj_read.compositions {
                    ui.push_id(comp.id, |ui_in_scope| {
                        let is_selected = editor_context.selected_composition_id == Some(comp.id);
                        let response = ui_in_scope
                            .selectable_label(is_selected, &comp.name)
                            .on_hover_text(format!("Comp ID: {}", comp.id));
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
        if ui.button("Add Comp").clicked() {
            // TODO: Replace with a Command
            let new_comp_id = project_service
                .add_composition("New Composition", 1920, 1080, 30.0, 60.0)
                .expect("Failed to add composition");
            editor_context.selected_composition_id = Some(new_comp_id);
            // Also add a corresponding asset
            editor_context.assets.push(Asset {
                name: format!("Comp: New Composition"),
                duration: 60.0,
                color: egui::Color32::from_rgb(255, 150, 255),
                kind: AssetKind::Composition(new_comp_id),
                composition_id: Some(new_comp_id),
            });
            needs_refresh = true;
        }
        if ui.button("Remove Comp").clicked() {
            if let Some(comp_id) = editor_context.selected_composition_id {
                // TODO: Replace with a Command
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
                needs_refresh = true;
            }
        }
    });

    ui.add_space(10.0);

    // Display tracks for selected composition
    if let Some(comp_id) = editor_context.selected_composition_id {
        ui.heading(format!("Tracks in Comp: {}", comp_id)); // Displaying ID for now
        ui.separator();
        egui::ScrollArea::vertical()
            .id_source("tracks_scroll_area")
            .max_height(200.0)
            .show(ui, |ui| {
                if let Ok(proj_read) = project.read() {
                    if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id)
                    {
                        for track in &comp.tracks {
                            ui.push_id(track.id, |ui_in_scope| {
                                let is_selected = editor_context.selected_track_id == Some(track.id);
                                let response = ui_in_scope
                                    .selectable_label(is_selected, &track.name)
                                    .on_hover_text(format!("Track ID: {}", track.id));
                                if response.clicked() {
                                    editor_context.selected_track_id = Some(track.id);
                                    editor_context.selected_entity_id = None; // Deselect entity when track changes
                                }
                            });
                        }
                    }
                }
            });

        ui.add_space(10.0);
    }

    // Display properties of selected entity
    if let Some(selected_entity_id) = editor_context.selected_entity_id {
        if let Some(comp_id) = editor_context.selected_composition_id {
            if let Some(track_id) = editor_context.selected_track_id {
                // Use inspector_entity_cache
                if let Some((cached_entity_id, cached_entity_type, cached_properties, cached_start_time, cached_end_time)) =
                    editor_context.inspector_entity_cache.as_mut()
                {
                    // Ensure the cached entity matches the actually selected entity
                    if *cached_entity_id == selected_entity_id {
                        ui.heading("Entity Properties");
                        ui.separator();

                        let mut current_entity_type = cached_entity_type.clone();
                        ui.horizontal(|ui| {
                            ui.label("Type");
                            if ui.text_edit_singleline(&mut current_entity_type).changed() {
                                *cached_entity_type = current_entity_type.clone();
                                // TODO: Replace with a Command
                                project_service
                                    .with_track_mut(comp_id, track_id, |track_mut| {
                                        if let Some(entity_mut) = track_mut
                                            .entities
                                            .iter_mut()
                                            .find(|e| e.id == selected_entity_id)
                                        {
                                            entity_mut.entity_type = cached_entity_type.clone();
                                        }
                                    })
                                    .ok();
                                needs_refresh = true;
                            }
                        });

                        egui::Grid::new("entity_props")
                            .striped(true)
                            .show(ui, |ui| {
                                // position_x
                                let mut pos_x = cached_properties.get_f32("position_x").unwrap_or(960.0);
                                ui.label("Position X");
                                if ui
                                    .add(egui::DragValue::new(&mut pos_x).speed(1.0).suffix("px"))
                                    .changed()
                                {
                                    cached_properties.set("position_x".to_string(), Property::constant(PropertyValue::Number(pos_x as f64)));
                                    // TODO: Replace with a Command
                                    project_service
                                        .update_entity_property(
                                            comp_id,
                                            track_id,
                                            selected_entity_id,
                                            "position_x",
                                            PropertyValue::Number(pos_x as f64),
                                        )
                                        .ok();
                                    needs_refresh = true;
                                }
                                ui.end_row();

                                // position_y
                                let mut pos_y = cached_properties.get_f32("position_y").unwrap_or(540.0);
                                ui.label("Position Y");
                                if ui
                                    .add(egui::DragValue::new(&mut pos_y).speed(1.0).suffix("px"))
                                    .changed()
                                {
                                    cached_properties.set("position_y".to_string(), Property::constant(PropertyValue::Number(pos_y as f64)));
                                    // TODO: Replace with a Command
                                    project_service
                                        .update_entity_property(
                                            comp_id,
                                            track_id,
                                            selected_entity_id,
                                            "position_y",
                                            PropertyValue::Number(pos_y as f64),
                                        )
                                        .ok();
                                    needs_refresh = true;
                                }
                                ui.end_row();

                                // scale
                                let mut scale = cached_properties.get_f32("scale").unwrap_or(100.0);
                                ui.label("Scale");
                                if ui
                                    .add(egui::Slider::new(&mut scale, 0.0..=200.0).suffix("%"))
                                    .changed()
                                {
                                    cached_properties.set("scale".to_string(), Property::constant(PropertyValue::Number(scale as f64)));
                                    // TODO: Replace with a Command
                                    project_service
                                        .update_entity_property(
                                            comp_id,
                                            track_id,
                                            selected_entity_id,
                                            "scale",
                                            PropertyValue::Number(scale as f64),
                                        )
                                        .ok();
                                    needs_refresh = true;
                                }
                                ui.end_row();

                                // opacity
                                let mut opacity = cached_properties.get_f32("opacity").unwrap_or(100.0);
                                ui.label("Opacity");
                                if ui
                                    .add(egui::Slider::new(&mut opacity, 0.0..=100.0).suffix("%"))
                                    .changed()
                                {
                                    cached_properties.set("opacity".to_string(), Property::constant(PropertyValue::Number(opacity as f64)));
                                    // TODO: Replace with a Command
                                    project_service
                                        .update_entity_property(
                                            comp_id,
                                            track_id,
                                            selected_entity_id,
                                            "opacity",
                                            PropertyValue::Number(opacity as f64),
                                        )
                                        .ok();
                                    needs_refresh = true;
                                }
                                ui.end_row();

                                // rotation
                                let mut rotation = cached_properties.get_f32("rotation").unwrap_or(0.0);
                                ui.label("Rotation");
                                if ui
                                    .add(egui::DragValue::new(&mut rotation).speed(1.0).suffix("°"))
                                    .changed()
                                {
                                    cached_properties.set("rotation".to_string(), Property::constant(PropertyValue::Number(rotation as f64)));
                                    // TODO: Replace with a Command
                                    project_service
                                        .update_entity_property(
                                            comp_id,
                                            track_id,
                                            selected_entity_id,
                                            "rotation",
                                            PropertyValue::Number(rotation as f64),
                                        )
                                        .ok();
                                    needs_refresh = true;
                                }
                                ui.end_row();

                                // Start Time
                                let mut current_start_time = *cached_start_time as f32;
                                ui.label("Start Time");
                                if ui
                                    .add(egui::DragValue::new(&mut current_start_time).speed(0.1))
                                    .changed()
                                {
                                    *cached_start_time = current_start_time as f64;
                                    // TODO: Replace with a Command
                                    project_service
                                        .with_track_mut(comp_id, track_id, |track_mut| {
                                            if let Some(entity_mut) = track_mut
                                                .entities
                                                .iter_mut()
                                                .find(|e| e.id == selected_entity_id)
                                            {
                                                let duration =
                                                    entity_mut.end_time - entity_mut.start_time;
                                                entity_mut.start_time = *cached_start_time;
                                                entity_mut.end_time =
                                                    entity_mut.start_time + duration;
                                            }
                                        })
                                        .ok();
                                    needs_refresh = true;
                                }
                                ui.end_row();

                                // End Time
                                let mut current_end_time = *cached_end_time as f32;
                                ui.label("End Time");
                                if ui
                                    .add(egui::DragValue::new(&mut current_end_time).speed(0.1))
                                    .changed()
                                {
                                    *cached_end_time = current_end_time as f64;
                                    // TODO: Replace with a Command
                                    project_service
                                        .with_track_mut(comp_id, track_id, |track_mut| {
                                            if let Some(entity_mut) = track_mut
                                                .entities
                                                .iter_mut()
                                                .find(|e| e.id == selected_entity_id)
                                            {
                                                entity_mut.end_time = *cached_end_time;
                                            }
                                        })
                                        .ok();
                                    needs_refresh = true;
                                }
                                ui.end_row();
                            });

                        if ui.button("🗑 Delete Entity").clicked() {
                            // TODO: Replace with a Command
                            if let Err(e) = project_service.remove_entity_from_track(
                                comp_id,
                                track_id,
                                selected_entity_id,
                            ) {
                                eprintln!("Failed to remove entity: {:?}", e);
                            } else {
                                editor_context.selected_entity_id = None;
                                editor_context.inspector_entity_cache = None; // Clear cache on deletion
                                needs_refresh = true;
                            }
                        }
                    } else {
                        ui.label("Inspector cache is stale or mismatched. Please re-select entity.");
                        editor_context.inspector_entity_cache = None; // Invalidate cache
                    }
                } else {
                    ui.label("Inspector cache not populated for selected entity.");
                }
            } else {
                ui.label("No track selected for entity properties.");
            }
        } else {
            ui.label("Select an entity to edit");
        }
    }

    if needs_refresh {
        ui.ctx().request_repaint();
    }
}
