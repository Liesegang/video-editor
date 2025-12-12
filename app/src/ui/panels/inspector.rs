use egui::Ui;

use library::model::project::project::Project;
use library::model::project::property::PropertyValue;
use library::model::project::TrackClipKind; // Added
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
            PropertyValue::Number(ordered_float::OrderedFloat(*current_value as f64)),
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


    pub fn inspector_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut ProjectService,
    project: &Arc<RwLock<Project>>,
) {
    let mut needs_refresh = false;

    // Display properties of selected entity
    if let (Some(selected_entity_id), Some(comp_id), Some(track_id)) = (
        editor_context.selected_entity_id,
        editor_context.selected_composition_id,
        editor_context.selected_track_id,
    ) {
        // Fetch entity data directly from project
        let entity_data = if let Ok(proj_read) = project.read() {
            if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
                if let Some(track) = comp.tracks.iter().find(|t| t.id == track_id) {
                    track
                        .clips
                        .iter()
                        .find(|e| e.id == selected_entity_id)
                        .map(|e| {
                            (
                                e.kind.clone(),
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

        if let Some((kind, properties, in_frame, out_frame, source_begin_frame, duration_frame)) =
            entity_data
        {
            ui.heading("Clip Properties");
            ui.separator();

            let mut current_kind = kind.clone();
            ui.horizontal(|ui| {
                ui.label("Type:");
                ui.label(current_kind.to_string());
            });

            // --- Dynamic Properties ---
            let definitions = project_service.get_property_definitions(kind);
            // Group by category
            let mut grouped: std::collections::HashMap<
                String,
                Vec<library::plugin::PropertyDefinition>,
            > = std::collections::HashMap::new();
            for def in definitions {
                grouped
                    .entry(def.category.clone())
                    .or_default()
                    .push(def);
            }

            // Sort categories? Transform first, then others.
            let mut categories: Vec<_> = grouped.keys().cloned().collect();
            categories.sort_by(|a, b| {
                if a == "Transform" {
                    std::cmp::Ordering::Less
                } else if b == "Transform" {
                    std::cmp::Ordering::Greater
                } else {
                    a.cmp(b)
                }
            });

            use library::plugin::PropertyUiType;

            for category in categories {
                ui.add_space(5.0);
                ui.heading(&category);
                egui::Grid::new(format!("cat_{}", category))
                    .striped(true)
                    .show(ui, |ui| {
                        if let Some(defs) = grouped.get(&category) {
                            for def in defs {
                                match &def.ui_type {
                                    PropertyUiType::Float {
                                        min,
                                        max,
                                        step,
                                        suffix,
                                    } => {
                                        // Get current value or default
                                        let current_val = properties
                                            .get_f32(&def.name)
                                            .unwrap_or(def.default_value.get_as::<f32>().unwrap_or(0.0));
                                        
                                        let mut val_mut = current_val;
                                        
                                        // use handle_drag_value_property
                                        handle_drag_value_property(
                                            ui,
                                            history_manager,
                                            editor_context,
                                            project_service,
                                            comp_id,
                                            track_id,
                                            selected_entity_id,
                                            &def.label,
                                            &def.name,
                                            &mut val_mut,
                                            *step as f32,
                                            suffix,
                                            |service, c, t, e, n, v| {
                                                Ok(service.update_clip_property(c, t, e, n, v)?)
                                            },
                                            &mut needs_refresh
                                        );
                                        ui.end_row();
                                    }
                                    PropertyUiType::Integer {
                                        min,
                                        max,
                                        suffix,
                                    } => {
                                         // Support Integer UI if needed, for now skip or treat as float
                                         ui.label(&def.label);
                                         ui.label("Integer UI not impl");
                                         ui.end_row();
                                    }
                                    _ => {
                                        ui.label(&def.label);
                                        ui.label("UI type not implemented");
                                        ui.end_row();
                                    }
                                }
                            }
                        }
                    });
            }


            ui.add_space(10.0);
            ui.heading("Timing");
            ui.separator();

            egui::Grid::new("entity_timing")
                .striped(true)
                .show(ui, |ui| {
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
                                let new_in_frame = new_val_f64.0 as u64;
                                service
                                    .update_clip_time(comp, track, entity, new_in_frame, out_frame)
                                    .map_err(|e| anyhow::anyhow!(e))
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
                                let new_out_frame = new_val_f64.0 as u64;
                                service
                                    .update_clip_time(comp, track, entity, in_frame, new_out_frame)
                                    .map_err(|e| anyhow::anyhow!(e))
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
                                let new_source_begin_frame = new_val_f64.0 as u64;
                                service
                                    .update_clip_source_frames(
                                        comp,
                                        track,
                                        entity,
                                        new_source_begin_frame,
                                        duration_frame,
                                    )
                                    .map_err(|e| anyhow::anyhow!(e))
                            } else {
                                Err(anyhow::anyhow!("Expected Number for Source Begin Frame"))
                            }
                        },
                        &mut needs_refresh,
                    );
                    ui.end_row();

                    // Duration Frame
                    let duration_text = if let Some(d) = duration_frame {
                        format!("{} fr", d)
                    } else {
                        "Infinite".to_string()
                    };

                    ui.horizontal(|ui| {
                        ui.label("Duration Frame");
                        ui.label(duration_text);
                    });
                    ui.end_row();
                });
        } else {
            ui.label("Clip not found (it may have been deleted).");
            // Deselect if not found
            editor_context.selected_entity_id = None;
        }
    } else {
        if editor_context.selected_composition_id.is_none() {
            ui.label("No composition selected.");
        } else if editor_context.selected_track_id.is_none() {
            ui.label("No track selected.");
        } else {
            ui.label("Select a clip to edit");
        }
    }

    if needs_refresh {
        ui.ctx().request_repaint();
    }
}
