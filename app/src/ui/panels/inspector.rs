use egui::Ui;
use egui_phosphor::regular as icons;
use library::model::project::project::Project;
use library::model::project::property::{Property, PropertyValue};
use library::service::project_service::ProjectService;
use std::sync::{Arc, RwLock};

use crate::{action::HistoryManager, state::context::EditorContext};
use anyhow::Result;
use uuid::Uuid;

// Helper function to handle continuous property input via DragValue
#[allow(clippy::too_many_arguments)]
fn handle_drag_value_property(
    ui: &mut Ui,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
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

    if response.drag_started() {
        if editor_context.last_project_state_before_drag.is_none() {
            editor_context.last_project_state_before_drag =
                Some(project_service.get_project().read().unwrap().clone());
        }
    }

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
        if let Some(initial_state) = editor_context.last_project_state_before_drag.take() {
            let current_state = project_service.get_project().read().unwrap().clone();
            if initial_state != current_state {
                history_manager.push_project_state(initial_state);
            }
        }
    }
    response
}

// Helper function to handle continuous property input via Slider
#[allow(clippy::too_many_arguments)]
fn handle_slider_property(
    ui: &mut Ui,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
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

    if response.drag_started() {
        if editor_context.last_project_state_before_drag.is_none() {
            editor_context.last_project_state_before_drag =
                Some(project_service.get_project().read().unwrap().clone());
        }
    }

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
        if let Some(initial_state) = editor_context.last_project_state_before_drag.take() {
            let current_state = project_service.get_project().read().unwrap().clone();
            if initial_state != current_state {
                history_manager.push_project_state(initial_state);
            }
        }
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

    // Display tracks for selected composition
    if let Some(comp_id) = editor_context.selected_composition_id {
        ui.heading(format!("Tracks in Comp: {}", comp_id)); // Displaying ID for now
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
    // Use inspector_entity_cache
    if let Some(selected_entity_id) = editor_context.selected_entity_id {
        if let Some(comp_id) = editor_context.selected_composition_id {
            if let Some(track_id) = editor_context.selected_track_id {
                let cache = editor_context.inspector_entity_cache.take();

                match cache {
                    Some((
                        cached_entity_id,
                        mut cached_entity_type,
                        mut cached_properties,
                        cached_in_frame,           // u64
                        cached_out_frame,          // u64
                        cached_source_begin_frame, // u64
                        mut cached_duration_frame, // Option<u64>
                    )) => {
                        if cached_entity_id == selected_entity_id {
                            ui.heading("Entity Properties");
                            ui.separator();

                            let mut current_entity_type = cached_entity_type.clone();
                            ui.horizontal(|ui| {
                                ui.label("Type");
                                if ui.text_edit_singleline(&mut current_entity_type).changed() {
                                    let prev_project_state =
                                        project_service.get_project().read().unwrap().clone();
                                    cached_entity_type = current_entity_type.clone();
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
                                    history_manager.push_project_state(prev_project_state);
                                    needs_refresh = true;
                                }
                            });

                            let mut keep_cache = true;

                            egui::Grid::new("entity_props")
                                .striped(true)
                                .show(ui, |ui| {
                                    // position_x
                                    let mut pos_x =
                                        cached_properties.get_f32("position_x").unwrap_or(960.0);
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
                                      &mut needs_refresh // Pass needs_refresh
                                  );
                                    cached_properties.set(
                                        "position_x".to_string(),
                                        Property::constant(PropertyValue::Number(pos_x as f64)),
                                    );
                                    ui.end_row();

                                    // position_y
                                    let mut pos_y =
                                        cached_properties.get_f32("position_y").unwrap_or(540.0);
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
                                      &mut needs_refresh // Pass needs_refresh
                                  );
                                    cached_properties.set(
                                        "position_y".to_string(),
                                        Property::constant(PropertyValue::Number(pos_y as f64)),
                                    );

                                    // scale
                                    let mut scale =
                                        cached_properties.get_f32("scale").unwrap_or(100.0);
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
                                      &mut needs_refresh // Pass needs_refresh
                                  );
                                    cached_properties.set(
                                        "scale".to_string(),
                                        Property::constant(PropertyValue::Number(scale as f64)),
                                    );

                                    // opacity
                                    let mut opacity =
                                        cached_properties.get_f32("opacity").unwrap_or(100.0);
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
                                      &mut needs_refresh // Pass needs_refresh
                                  );
                                    cached_properties.set(
                                        "opacity".to_string(),
                                        Property::constant(PropertyValue::Number(opacity as f64)),
                                    );

                                    // rotation
                                    let mut rotation =
                                        cached_properties.get_f32("rotation").unwrap_or(0.0);
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
                                      &mut needs_refresh // Pass needs_refresh
                                  );
                                    cached_properties.set(
                                        "rotation".to_string(),
                                        Property::constant(PropertyValue::Number(rotation as f64)),
                                    );

                                    // In Frame
                                    let mut current_in_frame_f32 = cached_in_frame as f32; // Convert u64 to f32 for UI
                                    let response = handle_drag_value_property(
                                      ui,
                                      history_manager,
                                      editor_context,
                                      project_service,
                                      comp_id,
                                      track_id,
                                      selected_entity_id,
                                      "In Frame", // Renamed label
                                      "in_frame", // Not used in update_entity_time
                                      &mut current_in_frame_f32,
                                      1.0, // Speed for frames
                                      "fr", // Suffix for frames
                                      |service: &mut ProjectService,
                                       comp: Uuid,
                                       track: Uuid,
                                       entity: Uuid,
                                       _name: &str,
                                       value: PropertyValue| {
                                          if let PropertyValue::Number(new_val_f64) = value {
                                              let new_in_frame = new_val_f64 as u64; // Convert f64 back to u64
                                              // Call update_entity_time with new in_frame and current out_frame
                                              service.update_entity_time(comp, track, entity, new_in_frame, cached_out_frame).map_err(|e| anyhow::anyhow!(e))
                                          } else {
                                              Err(anyhow::anyhow!("Expected Number for In Frame"))
                                          }
                                      },
                                      &mut needs_refresh // Pass needs_refresh
                                  );

                                    // Out Frame
                                    let mut current_out_frame_f32 = cached_out_frame as f32; // Convert u64 to f32 for UI
                                    let response = handle_drag_value_property(
                                      ui,
                                      history_manager,
                                      editor_context,
                                      project_service,
                                      comp_id,
                                      track_id,
                                      selected_entity_id,
                                      "Out Frame", // Renamed label
                                      "out_frame", // Not used in update_entity_time
                                      &mut current_out_frame_f32,
                                      1.0, // Speed for frames
                                      "fr", // Suffix for frames
                                      |service: &mut ProjectService,
                                       comp: Uuid,
                                       track: Uuid,
                                       entity: Uuid,
                                       _name: &str,
                                       value: PropertyValue| {
                                          if let PropertyValue::Number(new_val_f64) = value {
                                              let new_out_frame = new_val_f64 as u64; // Convert f64 back to u64
                                              // Call update_entity_time with current in_frame and new out_frame
                                              service.update_entity_time(comp, track, entity, cached_in_frame, new_out_frame).map_err(|e| anyhow::anyhow!(e))
                                          } else {
                                              Err(anyhow::anyhow!("Expected Number for Out Frame"))
                                          }
                                      },
                                      &mut needs_refresh // Pass needs_refresh
                                  );

                                    // Source Begin Frame
                                    let mut current_source_begin_frame_f32 = cached_source_begin_frame as f32; // Convert u64 to f32 for UI
                                    let response = handle_drag_value_property(
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
                                      1.0, // Speed for frames
                                      "fr", // Suffix for frames
                                      |service: &mut ProjectService,
                                       comp: Uuid,
                                       track: Uuid,
                                       entity: Uuid,
                                       _name: &str,
                                       value: PropertyValue| {
                                          if let PropertyValue::Number(new_val_f64) = value {
                                              let new_source_begin_frame = new_val_f64 as u64; // Convert f64 back to u64
                                              // Call update_entity_source_frames
                                              service.update_entity_source_frames(comp, track, entity, new_source_begin_frame, cached_duration_frame).map_err(|e| anyhow::anyhow!(e))
                                          } else {
                                              Err(anyhow::anyhow!("Expected Number for Source Begin Frame"))
                                          }
                                      },
                                      &mut needs_refresh // Pass needs_refresh
                                  );

                                    // Duration Frame
                                    let mut current_duration_f32 = cached_duration_frame.unwrap_or(0) as f32; // Convert Option<u64> to f32 for UI
                                    let mut is_infinite_duration = cached_duration_frame.is_none();

                                    ui.horizontal(|ui| {
                                        ui.label("Duration Frame");
                                        let checkbox_response = ui.checkbox(&mut is_infinite_duration, "Infinite");
                                        if checkbox_response.changed() {
                                            let new_duration = if is_infinite_duration { None } else { Some(current_duration_f32 as u64) };
                                            if let Err(e) = project_service.update_entity_source_frames(comp_id, track_id, selected_entity_id, cached_source_begin_frame, new_duration) {
                                                eprintln!("Failed to update source duration: {:?}", e);
                                            } else {
                                                cached_duration_frame = new_duration;
                                                // needs_refresh = true; // Handled below
                                            }
                                        }

                                        let drag_value_response = ui.add_enabled_ui(!is_infinite_duration, |ui| {
                                            handle_drag_value_property( // This function call returns Response
                                                ui,
                                                history_manager,
                                                editor_context,
                                                project_service,
                                                comp_id,
                                                track_id,
                                                selected_entity_id,
                                                "", // No label here as it's part of horizontal layout
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
                                                        service.update_entity_source_frames(comp, track, entity, cached_source_begin_frame, new_duration).map_err(|e| anyhow::anyhow!(e))
                                                    } else {
                                                        Err(anyhow::anyhow!("Expected Number for Duration Frame"))
                                                    }
                                                },
                                                &mut needs_refresh // Pass needs_refresh
                                            )
                                        }).inner;

                                        needs_refresh |= checkbox_response.changed();
                                        needs_refresh |= drag_value_response.changed();
                                    });
                                     cached_duration_frame = if is_infinite_duration { None } else { Some(current_duration_f32 as u64) };
                                    ui.end_row();
                                });

                            if ui
                                .button(format!("{} Delete Entity", icons::TRASH))
                                .clicked()
                            {
                                let prev_project_state =
                                    project_service.get_project().read().unwrap().clone();
                                if let Err(e) = project_service.remove_entity_from_track(
                                    comp_id,
                                    track_id,
                                    selected_entity_id,
                                ) {
                                    eprintln!("Failed to remove entity: {:?}", e);
                                } else {
                                    editor_context.selected_entity_id = None;
                                    history_manager.push_project_state(prev_project_state);
                                    needs_refresh = true;
                                    keep_cache = false;
                                }
                            }

                            if keep_cache {
                                editor_context.inspector_entity_cache = Some((
                                    cached_entity_id,
                                    cached_entity_type,
                                    cached_properties,
                                    cached_in_frame,           // Renamed
                                    cached_out_frame,          // Renamed
                                    cached_source_begin_frame, // New
                                    cached_duration_frame,     // New
                                ));
                            } else {
                                editor_context.inspector_entity_cache = None;
                            }
                        } else {
                            ui.label(
                                "Inspector cache is stale or mismatched. Please re-select entity.",
                            );
                            editor_context.inspector_entity_cache = None;
                        }
                    }
                    None => {
                        ui.label("Inspector cache not populated for selected entity.");
                    }
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
