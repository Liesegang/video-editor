use egui::Ui;

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
        editor_context.selection.entity_id,
        editor_context.selection.composition_id,
        editor_context.selection.track_id,
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
                                e.effects.clone(),
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
            kind,
            properties,
            in_frame,
            out_frame,
            source_begin_frame,
            duration_frame,
            effects,
        )) = entity_data
        {
            ui.heading("Clip Properties");
            ui.separator();

            let current_kind = kind.clone();
            ui.horizontal(|ui| {
                ui.label("Type:");
                ui.label(current_kind.to_string());
            });

            let current_time = editor_context.timeline.current_time as f64;

            // --- Dynamic Properties ---
            let definitions = project_service.get_property_definitions(kind);
            // Group by category
            let mut grouped: std::collections::HashMap<
                String,
                Vec<library::plugin::PropertyDefinition>,
            > = std::collections::HashMap::new();
            for def in definitions {
                grouped.entry(def.category.clone()).or_default().push(def);
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

                if let Some(defs) = grouped.get(&category) {
                    // Split into chunks of (is_grid, vecs)
                    // But simpler: just iterate and flush grid when needed?
                    // No, Grid::new takes a closure. We can't interrupt it.
                    // So we must group them.

                    struct Chunk<'a> {
                        is_grid: bool,
                        defs: Vec<&'a library::plugin::PropertyDefinition>,
                    }

                    let mut chunks: Vec<Chunk> = Vec::new();
                    let mut current_grid_defs = Vec::new();

                    for def in defs {
                        let is_multiline = matches!(def.ui_type, PropertyUiType::MultilineText);
                        if is_multiline {
                            // Push existing grid chunk if any
                            if !current_grid_defs.is_empty() {
                                chunks.push(Chunk {
                                    is_grid: true,
                                    defs: current_grid_defs.clone(),
                                });
                                current_grid_defs.clear();
                            }
                            // Push this as full width chunk
                            chunks.push(Chunk {
                                is_grid: false,
                                defs: vec![def],
                            });
                        } else {
                            current_grid_defs.push(def);
                        }
                    }
                    if !current_grid_defs.is_empty() {
                        chunks.push(Chunk {
                            is_grid: true,
                            defs: current_grid_defs,
                        });
                    }

                    for (chunk_idx, chunk) in chunks.iter().enumerate() {
                        if chunk.is_grid {
                            egui::Grid::new(format!("cat_{}_{}", category, chunk_idx))
                                .striped(true)
                                .show(ui, |ui| {
                                    for def in &chunk.defs {
                                        // Copy pasting property handling logic (refactor ideally, but inline for now to save time/context)
                                        match &def.ui_type {
                                            PropertyUiType::Float { step, suffix, .. } => {
                                                let current_val = properties.get_f32(&def.name).unwrap_or(
                                                    def.default_value.get_as::<f32>().unwrap_or(0.0),
                                                );
                                                let mut val_mut = current_val;
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
                                                    move |service, c, t, e, n, v| {
                                                        Ok(service.update_property_or_keyframe(
                                                            c, t, e, n, current_time, v, None,
                                                        )?)
                                                    },
                                                    &mut needs_refresh,
                                                );
                                                ui.end_row();
                                            }
                                            PropertyUiType::Integer { .. } => {
                                                ui.label(&def.label);
                                                ui.label("Integer UI not impl");
                                                ui.end_row();
                                            }
                                            PropertyUiType::Text => {
                                                ui.label(&def.label);
                                                let current_val = properties.get_string(&def.name).unwrap_or(
                                                    def.default_value.get_as::<String>().unwrap_or_default(),
                                                );
                                                let mut buffer = current_val.clone();
                                                let response = ui.text_edit_singleline(&mut buffer);
                                                if response.changed() {
                                                    use library::model::project::property::PropertyValue;
                                                    project_service.update_property_or_keyframe(
                                                        comp_id, track_id, selected_entity_id, &def.name, current_time, PropertyValue::String(buffer), None,
                                                    ).ok();
                                                    needs_refresh = true;
                                                }
                                                if response.lost_focus() {
                                                    let current_state = project_service.get_project().read().unwrap().clone();
                                                    history_manager.push_project_state(current_state);
                                                }
                                                ui.end_row();
                                            }
                                            PropertyUiType::Color => {
                                                ui.label(&def.label);
                                                let current_val = properties.get_constant_value(&def.name)
                                                    .and_then(|v| v.get_as::<library::model::frame::color::Color>())
                                                    .unwrap_or(def.default_value.get_as::<library::model::frame::color::Color>().unwrap_or_default());
        
                                                let mut color32 = egui::Color32::from_rgba_premultiplied(
                                                    current_val.r, current_val.g, current_val.b, current_val.a,
                                                );
                                                let response = ui.color_edit_button_srgba(&mut color32);
                                                if response.changed() {
                                                    use library::model::project::property::PropertyValue;
                                                    let new_color = library::model::frame::color::Color {
                                                        r: color32.r(), g: color32.g(), b: color32.b(), a: color32.a(),
                                                    };
                                                    project_service.update_property_or_keyframe(
                                                        comp_id, track_id, selected_entity_id, &def.name, current_time, PropertyValue::Color(new_color), None,
                                                    ).ok();
                                                    needs_refresh = true;
                                                    let current_state = project_service.get_project().read().unwrap().clone();
                                                    history_manager.push_project_state(current_state);
                                                }
                                                ui.end_row();
                                            }
                                            _ => {
                                                ui.label(&def.label);
                                                ui.label("UI type not implemented");
                                                ui.end_row();
                                            }
                                        }
                                    }
                                });
                        } else {
                            // Full Width Render
                            for def in &chunk.defs {
                                // Assuming MultilineText
                                if let PropertyUiType::MultilineText = &def.ui_type {
                                    ui.add_space(5.0);
                                    ui.label(&def.label);
                                    let current_val = properties.get_string(&def.name).unwrap_or(
                                        def.default_value.get_as::<String>().unwrap_or_default(),
                                    );
                                    let mut buffer = current_val.clone();

                                    // Use a scroll area for code ideally, or just a large text edit
                                    let response = ui.add(
                                        egui::TextEdit::multiline(&mut buffer)
                                            .code_editor()
                                            .desired_rows(15)
                                            .desired_width(f32::INFINITY)
                                            .lock_focus(true), // prevent losing focus easily
                                    );
                                    if response.changed() {
                                        use library::model::project::property::PropertyValue;
                                        project_service
                                            .update_property_or_keyframe(
                                                comp_id,
                                                track_id,
                                                selected_entity_id,
                                                &def.name,
                                                current_time,
                                                PropertyValue::String(buffer),
                                                None,
                                            )
                                            .ok();
                                        needs_refresh = true;
                                    }
                                    if response.lost_focus() {
                                        let current_state =
                                            project_service.get_project().read().unwrap().clone();
                                        history_manager.push_project_state(current_state);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            ui.add_space(10.0);
            ui.heading("Effects");
            ui.separator();

            for (effect_index, effect) in effects.iter().enumerate() {
                ui.push_id(format!("effect_{}", effect_index), |ui| {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&effect.effect_type).strong());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("❌").clicked() {
                                        project_service
                                            .remove_effect_from_clip(
                                                comp_id,
                                                track_id,
                                                selected_entity_id,
                                                effect_index,
                                            )
                                            .ok();
                                        needs_refresh = true;
                                    }
                                },
                            );
                        });

                        let defs = project_service
                            .get_plugin_manager()
                            .get_effect_properties(&effect.effect_type);

                        egui::Grid::new(format!("effect_grid_{}", effect_index))
                            .striped(true)
                            .show(ui, |ui| {
                                for def in defs {
                                    match &def.ui_type {
                                        PropertyUiType::Float { step, suffix, .. } => {
                                            let current_val =
                                                effect.properties.get_f32(&def.name).unwrap_or(
                                                    def.default_value
                                                        .get_as::<f32>()
                                                        .unwrap_or(0.0),
                                                );
                                            let mut val_mut = current_val;
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
                                                move |service, c, t, e, n, v| {
                                                    Ok(service.update_effect_property_or_keyframe(
                                                        c,
                                                        t,
                                                        e,
                                                        effect_index,
                                                        n,
                                                        current_time,
                                                        v,
                                                        None,
                                                    )?)
                                                },
                                                &mut needs_refresh,
                                            );
                                            ui.end_row();
                                        }
                                        PropertyUiType::Text => {
                                            ui.label(&def.label);
                                            let current_val =
                                                effect.properties.get_string(&def.name).unwrap_or(
                                                    def.default_value
                                                        .get_as::<String>()
                                                        .unwrap_or_default(),
                                                );
                                            let mut buffer = current_val.clone();
                                            let response = ui.text_edit_singleline(&mut buffer);
                                            if response.changed() {
                                                project_service
                                                    .update_effect_property_or_keyframe(
                                                        comp_id,
                                                        track_id,
                                                        selected_entity_id,
                                                        effect_index,
                                                        &def.name,
                                                        current_time,
                                                        PropertyValue::String(buffer),
                                                        None,
                                                    )
                                                    .ok();
                                                needs_refresh = true;
                                            }
                                            if response.lost_focus() {
                                                let current_state = project_service
                                                    .get_project()
                                                    .read()
                                                    .unwrap()
                                                    .clone();
                                                history_manager.push_project_state(current_state);
                                            }
                                            ui.end_row();
                                        }
                                        PropertyUiType::Dropdown { options } => {
                                            ui.label(&def.label);
                                            let current_val =
                                                effect.properties.get_string(&def.name).unwrap_or(
                                                    def.default_value
                                                        .get_as::<String>()
                                                        .unwrap_or_default(),
                                                );
                                            let mut selected = current_val.clone();
                                            egui::ComboBox::from_id_source(format!(
                                                "combo_{}_{}",
                                                effect_index, def.name
                                            ))
                                            .selected_text(&selected)
                                            .show_ui(
                                                ui,
                                                |ui| {
                                                    for opt in options {
                                                        ui.selectable_value(
                                                            &mut selected,
                                                            opt.clone(),
                                                            opt,
                                                        );
                                                    }
                                                },
                                            );
                                            if selected != current_val {
                                                project_service
                                                    .update_effect_property_or_keyframe(
                                                        comp_id,
                                                        track_id,
                                                        selected_entity_id,
                                                        effect_index,
                                                        &def.name,
                                                        current_time,
                                                        PropertyValue::String(selected),
                                                        None,
                                                    )
                                                    .ok();
                                                needs_refresh = true;
                                                let current_state = project_service
                                                    .get_project()
                                                    .read()
                                                    .unwrap()
                                                    .clone();
                                                history_manager.push_project_state(current_state);
                                            }
                                            ui.end_row();
                                        }
                                        PropertyUiType::Color => {
                                            ui.label(&def.label);
                                            let current_val = effect.properties.get_constant_value(&def.name)
                                                .and_then(|v| v.get_as::<library::model::frame::color::Color>())
                                                .unwrap_or(def.default_value.get_as::<library::model::frame::color::Color>().unwrap_or_default());

                                            let mut color32 = egui::Color32::from_rgba_premultiplied(
                                                current_val.r, current_val.g, current_val.b, current_val.a,
                                            );
                                            let response = ui.color_edit_button_srgba(&mut color32);
                                            if response.changed() {
                                                use library::model::project::property::PropertyValue;
                                                let new_color = library::model::frame::color::Color {
                                                    r: color32.r(), g: color32.g(), b: color32.b(), a: color32.a(),
                                                };
                                                project_service.update_effect_property_or_keyframe(
                                                    comp_id, track_id, selected_entity_id, effect_index, &def.name, current_time, PropertyValue::Color(new_color), None,
                                                ).ok();
                                                needs_refresh = true;
                                                let current_state = project_service.get_project().read().unwrap().clone();
                                                history_manager.push_project_state(current_state);
                                            }
                                            ui.end_row();
                                        }
                                        PropertyUiType::Bool => {
                                            ui.label(&def.label);
                                            let current_val = effect.properties.get_bool(&def.name)
                                                .unwrap_or(def.default_value.get_as::<bool>().unwrap_or(false));
                                            
                                            let mut val_mut = current_val;
                                            let response = ui.checkbox(&mut val_mut, "");
                                            if response.changed() {
                                                 project_service.update_effect_property_or_keyframe(
                                                    comp_id, track_id, selected_entity_id, effect_index, &def.name, current_time, PropertyValue::Boolean(val_mut), None,
                                                ).ok();
                                                needs_refresh = true;
                                                let current_state = project_service.get_project().read().unwrap().clone();
                                                history_manager.push_project_state(current_state);
                                            }
                                            ui.end_row();
                                        }
                                        _ => {
                                            ui.label(&def.label);
                                            ui.label("UI type not implemented for effect");
                                            ui.end_row();
                                        }
                                    }
                                }
                            });
                    });
                });
                ui.add_space(5.0);
            }

            ui.horizontal(|ui| {
                ui.menu_button("Add Effect", |ui| {
                        let available_effects = project_service.get_plugin_manager().get_available_effects();
                        
                        // Group by category
                        let mut grouped_effects: std::collections::BTreeMap<String, Vec<(String, String)>> = std::collections::BTreeMap::new();
                        for (id, name, category) in available_effects {
                            grouped_effects.entry(category).or_default().push((id, name));
                        }

                        for (category, effects) in grouped_effects {
                            ui.menu_button(&category, |ui| {
                                for (effect_id, effect_name) in effects {
                                    if ui.button(&effect_name).clicked() {
                                        project_service.add_effect_to_clip(
                                            comp_id,
                                            track_id,
                                            selected_entity_id,
                                            &effect_id,
                                        ).ok();
                                        ui.close();
                                        needs_refresh = true;
                                    }
                                }
                            });
                        }
                    });
            });

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
            editor_context.selection.entity_id = None;
        }
    } else {
        if editor_context.selection.composition_id.is_none() {
            ui.label("No composition selected.");
        } else if editor_context.selection.track_id.is_none() {
            ui.label("No track selected.");
        } else {
            ui.label("Select a clip to edit");
        }
    }

    if needs_refresh {
        ui.ctx().request_repaint();
    }
}
