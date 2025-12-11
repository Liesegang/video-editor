use egui::Ui;
use egui_phosphor::regular as icons;
use library::model::project::project::Project;
use library::model::project::property::PropertyValue;
use library::service::project_service::ProjectService;
use std::sync::{Arc, RwLock};

use crate::{action::HistoryManager, state::context::EditorContext};
use anyhow::Result;
use uuid::Uuid;

// Helper function to handle continuous property input via DragValue
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
fn handle_drag_value_property(
    ui: &mut Ui,
    history_manager: &mut HistoryManager,
    _editor_context: &mut EditorContext,
    project_service: &mut ProjectService,
    comp_id: Uuid,
    track_id: Uuid,
    selected_entity_id: Uuid,
    property_label: &str,
    property_name: &str,
    current_value: &mut f32,
    speed: f32,
    suffix: &str,
    update_fn: impl Fn(&mut ProjectService, Uuid, Uuid, Uuid, &str, PropertyValue) -> Result<()>,
    needs_refresh: &mut bool, // Added needs_refresh as mutable ref
) -> egui::Response {
    ui.label(property_label);
    let response = ui.add(
        egui::DragValue::new(current_value)
            .speed(speed)
            .suffix(suffix),
    );

    if response.changed() {
        update_fn(
            project_service,
            comp_id,
            track_id,
            selected_entity_id,
            property_name,
            PropertyValue::Number(*current_value as f64),
        )
        .ok();
        *needs_refresh = true; // Update needs_refresh internally
    }

    if response.drag_stopped() {
        // Post-Operation Push: Push the current state (which includes the change)
        let current_state = project_service.get_project().read().unwrap().clone();
        history_manager.push_project_state(current_state);
    }
    response
}

// Helper function to handle continuous property input via Slider
#[allow(clippy::too_many_arguments)]
fn handle_slider_property(
    ui: &mut Ui,
    history_manager: &mut HistoryManager,
    _editor_context: &mut EditorContext,
    project_service: &mut ProjectService,
    comp_id: Uuid,
    track_id: Uuid,
    selected_entity_id: Uuid,
    property_label: &str,
    property_name: &str,
    current_value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
    update_fn: impl Fn(&mut ProjectService, Uuid, Uuid, Uuid, &str, PropertyValue) -> Result<()>,
    needs_refresh: &mut bool, // Added needs_refresh as mutable ref
) -> egui::Response {
    ui.label(property_label);
    let response = ui.add(egui::Slider::new(current_value, range).suffix(suffix));

    if response.changed() {
        update_fn(
            project_service,
            comp_id,
            track_id,
            selected_entity_id,
            property_name,
            PropertyValue::Number(*current_value as f64),
        )
        .ok();
        *needs_refresh = true; // Update needs_refresh internally
    }

    if response.drag_stopped() {
        // Post-Operation Push
        let current_state = project_service.get_project().read().unwrap().clone();
        history_manager.push_project_state(current_state);
    }
    response
}

pub fn inspector_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut ProjectService,
    project: &Arc<RwLock<Project>>,
) {
    let mut needs_refresh = false;

    // Display tracks for selected composition
    if let Some(comp_id) = editor_context.selected_composition_id {
        ui.heading(format!("Tracks in Comp: {}", comp_id));
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("tracks_scroll_area")
            .max_height(200.0)
            .show(ui, |ui| {
                if let Ok(proj_read) = project.read() {
                    if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
                        for track in &comp.tracks {
                            ui.push_id(track.id, |ui_in_scope| {
                                let is_selected =
                                    editor_context.selected_track_id == Some(track.id);
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
    if let (Some(selected_entity_id), Some(comp_id), Some(track_id)) = (
        editor_context.selected_entity_id,
        editor_context.selected_composition_id,
        editor_context.selected_track_id,
    ) {
        // Fetch entity data directly from project
        // We clone the necessary data to avoid holding the read lock while drawing UI (which might need to acquire write lock for updates)
        let entity_data = if let Ok(proj_read) = project.read() {
            if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
                if let Some(track) = comp.tracks.iter().find(|t| t.id == track_id) {
                    track.entities.iter().find(|e| e.id == selected_entity_id).map(|e| {
                        (
                            e.entity_type.clone(),
                            e.properties.clone(),
                            e.in_frame,
                            e.out_frame,
                            e.source_begin_frame,
                            e.duration_frame,
                        )
                    })
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        if let Some((
            entity_type,
            properties,
            in_frame,
            out_frame,
            source_begin_frame,
            duration_frame,
        )) = entity_data
        {
            ui.heading("Entity Properties");
            ui.separator();

             let mut current_entity_type = entity_type.clone();
            ui.horizontal(|ui| {
                ui.label("Type");
                if ui.text_edit_singleline(&mut current_entity_type).changed() {
                    project_service
                        .with_track_mut(comp_id, track_id, |track_mut| {
                            if let Some(entity_mut) = track_mut
                                .entities
                                .iter_mut()
                                .find(|e| e.id == selected_entity_id)
                            {
                                entity_mut.entity_type = current_entity_type.clone();
                            }
                        })
                        .ok();
                    let current_state = project_service.get_project().read().unwrap().clone();
                    history_manager.push_project_state(current_state);
                    needs_refresh = true;
                }
            });

            egui::Grid::new("entity_props")
                .striped(true)
                .show(ui, |ui| {
                    // position_x
                    let mut pos_x = properties.get_f32("position_x").unwrap_or(960.0);
                    let _response = handle_drag_value_property(
                        ui,
                        history_manager,
                        editor_context,
                        project_service,
                        comp_id,
                        track_id,
                        selected_entity_id,
                        "Position X",
                        "position_x",
                        &mut pos_x,
                        1.0,
                        "px",
                        |service: &mut ProjectService,
                         comp: Uuid,
                         track: Uuid,
                         entity: Uuid,
                         name: &str,
                         value: PropertyValue| {
                            Ok(service.update_entity_property(
                                comp, track, entity, name, value,
                            )?)
                        },
                        &mut needs_refresh,
                    );
                    ui.end_row();

                    // position_y
                    let mut pos_y = properties.get_f32("position_y").unwrap_or(540.0);
                    let _response = handle_drag_value_property(
                        ui,
                        history_manager,
                        editor_context,
                        project_service,
                        comp_id,
                        track_id,
                        selected_entity_id,
                        "Position Y",
                        "position_y",
                        &mut pos_y,
                        1.0,
                        "px",
                        |service: &mut ProjectService,
                         comp: Uuid,
                         track: Uuid,
                         entity: Uuid,
                         name: &str,
                         value: PropertyValue| {
                            Ok(service.update_entity_property(
                                comp, track, entity, name, value,
                            )?)
                        },
                        &mut needs_refresh,
                    );
                    ui.end_row();

                    // scale
                    let mut scale = properties.get_f32("scale").unwrap_or(100.0);
                    let _response = handle_slider_property(
                        ui,
                        history_manager,
                        editor_context,
                        project_service,
                        comp_id,
                        track_id,
                        selected_entity_id,
                        "Scale",
                        "scale",
                        &mut scale,
                        0.0..=200.0,
                        "%",
                        |service: &mut ProjectService,
                         comp: Uuid,
                         track: Uuid,
                         entity: Uuid,
                         name: &str,
                         value: PropertyValue| {
                            Ok(service.update_entity_property(
                                comp, track, entity, name, value,
                            )?)
                        },
                        &mut needs_refresh,
                    );
                    ui.end_row();

                    // opacity
                    let mut opacity = properties.get_f32("opacity").unwrap_or(100.0);
                    let _response = handle_slider_property(
                        ui,
                        history_manager,
                        editor_context,
                        project_service,
                        comp_id,
                        track_id,
                        selected_entity_id,
                        "Opacity",
                        "opacity",
                        &mut opacity,
                        0.0..=100.0,
                        "%",
                        |service: &mut ProjectService,
                         comp: Uuid,
                         track: Uuid,
                         entity: Uuid,
                         name: &str,
                         value: PropertyValue| {
                            Ok(service.update_entity_property(
                                comp, track, entity, name, value,
                            )?)
                        },
                        &mut needs_refresh,
                    );
                    ui.end_row();

                    // rotation
                    let mut rotation = properties.get_f32("rotation").unwrap_or(0.0);
                    let _response = handle_drag_value_property(
                        ui,
                        history_manager,
                        editor_context,
                        project_service,
                        comp_id,
                        track_id,
                        selected_entity_id,
                        "Rotation",
                        "rotation",
                        &mut rotation,
                        1.0,
                        "°",
                        |service: &mut ProjectService,
                         comp: Uuid,
                         track: Uuid,
                         entity: Uuid,
                         name: &str,
                         value: PropertyValue| {
                            Ok(service.update_entity_property(
                                comp, track, entity, name, value,
                            )?)
                        },
                        &mut needs_refresh,
                    );
                    ui.end_row();

                    // In Frame
                    let mut current_in_frame_f32 = in_frame as f32;
                    let _response = handle_drag_value_property(
                        ui,
                        history_manager,
                        editor_context,
                        project_service,
                        comp_id,
                        track_id,
                        selected_entity_id,
                        "In Frame",
                        "in_frame",
                        &mut current_in_frame_f32,
                        1.0,
                        "fr",
                        |service: &mut ProjectService,
                         comp: Uuid,
                         track: Uuid,
                         entity: Uuid,
                         _name: &str,
                         value: PropertyValue| {
                            if let PropertyValue::Number(new_val_f64) = value {
                                let new_in_frame = new_val_f64 as u64;
                                service.update_entity_time(comp, track, entity, new_in_frame, out_frame).map_err(|e| anyhow::anyhow!(e))
                            } else {
                                Err(anyhow::anyhow!("Expected Number for In Frame"))
                            }
                        },
                        &mut needs_refresh,
                    );
                    ui.end_row();

                    // Out Frame
                    let mut current_out_frame_f32 = out_frame as f32;
                    let _response = handle_drag_value_property(
                        ui,
                        history_manager,
                        editor_context,
                        project_service,
                        comp_id,
                        track_id,
                        selected_entity_id,
                        "Out Frame",
                        "out_frame",
                        &mut current_out_frame_f32,
                        1.0,
                        "fr",
                        |service: &mut ProjectService,
                         comp: Uuid,
                         track: Uuid,
                         entity: Uuid,
                         _name: &str,
                         value: PropertyValue| {
                            if let PropertyValue::Number(new_val_f64) = value {
                                let new_out_frame = new_val_f64 as u64;
                                service.update_entity_time(comp, track, entity, in_frame, new_out_frame).map_err(|e| anyhow::anyhow!(e))
                            } else {
                                Err(anyhow::anyhow!("Expected Number for Out Frame"))
                            }
                        },
                        &mut needs_refresh,
                    );
                    ui.end_row();

                    // Source Begin Frame
                    let mut current_source_begin_frame_f32 = source_begin_frame as f32;
                    let _response = handle_drag_value_property(
                        ui,
                        history_manager,
                        editor_context,
                        project_service,
                        comp_id,
                        track_id,
                        selected_entity_id,
                        "Source Begin Frame",
                        "source_begin_frame",
                        &mut current_source_begin_frame_f32,
                        1.0,
                        "fr",
                        |service: &mut ProjectService,
                         comp: Uuid,
                         track: Uuid,
                         entity: Uuid,
                         _name: &str,
                         value: PropertyValue| {
                            if let PropertyValue::Number(new_val_f64) = value {
                                let new_source_begin_frame = new_val_f64 as u64;
                                service.update_entity_source_frames(comp, track, entity, new_source_begin_frame, duration_frame).map_err(|e| anyhow::anyhow!(e))
                            } else {
                                Err(anyhow::anyhow!("Expected Number for Source Begin Frame"))
                            }
                        },
                        &mut needs_refresh,
                    );
                    ui.end_row();

                    // Duration Frame
                    let mut current_duration_f32 = duration_frame.unwrap_or(0) as f32;
                    let mut is_infinite_duration = duration_frame.is_none();

                    ui.horizontal(|ui| {
                        ui.label("Duration Frame");
                        let checkbox_response = ui.checkbox(&mut is_infinite_duration, "Infinite");
                        if checkbox_response.changed() {
                            let new_duration = if is_infinite_duration { None } else { Some(current_duration_f32 as u64) };
                            if let Err(e) = project_service.update_entity_source_frames(comp_id, track_id, selected_entity_id, source_begin_frame, new_duration) {
                                eprintln!("Failed to update source duration: {:?}", e);
                            } else {
                                needs_refresh = true;
                            }
                        }

                        let drag_value_response = ui.add_enabled_ui(!is_infinite_duration, |ui| {
                            handle_drag_value_property(
                                ui,
                                history_manager,
                                editor_context,
                                project_service,
                                comp_id,
                                track_id,
                                selected_entity_id,
                                "",
                                "duration_frame",
                                &mut current_duration_f32,
                                1.0,
                                "fr",
                                |service: &mut ProjectService,
                                 comp: Uuid,
                                 track: Uuid,
                                 entity: Uuid,
                                 _name: &str,
                                 value: PropertyValue| {
                                    if let PropertyValue::Number(new_val_f64) = value {
                                        let new_duration = Some(new_val_f64 as u64);
                                        service.update_entity_source_frames(comp, track, entity, source_begin_frame, new_duration).map_err(|e| anyhow::anyhow!(e))
                                    } else {
                                        Err(anyhow::anyhow!("Expected Number for Duration Frame"))
                                    }
                                },
                                &mut needs_refresh
                            )
                        }).inner;

                        needs_refresh |= checkbox_response.changed();
                        needs_refresh |= drag_value_response.changed();
                    });
                    ui.end_row();
                });

            if ui
                .button(format!("{} Delete Entity", icons::TRASH))
                .clicked()
            {

                if let Err(e) = project_service.remove_entity_from_track(
                    comp_id,
                    track_id,
                    selected_entity_id,
                ) {
                    eprintln!("Failed to remove entity: {:?}", e);
                } else {
                    editor_context.selected_entity_id = None;
                    editor_context.selected_entity_id = None;
                    let current_state = project_service.get_project().read().unwrap().clone();
                    history_manager.push_project_state(current_state);
                    needs_refresh = true;
                }
            }
        } else {
             ui.label("Entity not found (it may have been deleted).");
             // Deselect if not found
             editor_context.selected_entity_id = None;
        }
    } else {
        if editor_context.selected_composition_id.is_none() {
             ui.label("No composition selected.");
        } else if editor_context.selected_track_id.is_none() {
             ui.label("No track selected.");
        } else {
             ui.label("Select an entity to edit");
        }
    }

    if needs_refresh {
        ui.ctx().request_repaint();
    }
}
