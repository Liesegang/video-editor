use egui::Ui;

// use library::model::project::Project; // Removed duplicate
use library::model::{GeneratorContent, Layer, LayerContent}; // Updated imports

use library::EditorService;
use std::sync::{Arc, RwLock};

use crate::{action::HistoryManager, state::context::EditorContext};

use library::model::project::Project; // Cleaned up import block

pub mod action_handler;
pub mod effects;
pub mod ensemble;
pub mod properties;
pub mod styles;

use action_handler::{ActionContext, PropertyTarget};
use effects::render_effects_section;
use ensemble::render_ensemble_section;
use properties::{render_property_rows, PropertyRenderContext};
use styles::render_styles_section;

pub fn inspector_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut EditorService,
    project: &Arc<RwLock<Project>>,
) {
    let mut needs_refresh = false;

    // Display properties of selected entity
    fn get_layer_display_type(layer: &Layer) -> String {
        match &layer.content {
            LayerContent::Media(_) => "Media Clip".to_string(),
            LayerContent::Generator(g) => match g {
                GeneratorContent::Shape { .. } => "Shape".to_string(),
                GeneratorContent::Text { .. } => "Text".to_string(),
                GeneratorContent::Solid { .. } => "Solid".to_string(),
                GeneratorContent::SkSL { .. } => "SkSL Shader".to_string(),
            },
            LayerContent::Reference(_) => "Reference".to_string(),
        }
    }

    // Display properties of selected entity
    if let (Some(selected_entity_id), Some(comp_id), Some(track_id)) = (
        editor_context.selection.last_selected_entity_id,
        editor_context.selection.composition_id,
        editor_context.selection.last_selected_track_id,
    ) {
        // Fetch entity data directly
        let entity_data = if let Ok(proj_read) = project.read() {
            proj_read.get_layer(selected_entity_id).cloned()
        } else {
            None
        };

        if let Some(layer) = entity_data {
            // Get FPS for display conversion
            let fps = project_service
                .get_composition(comp_id)
                .map(|c| c.fps)
                .unwrap_or(60.0);

            if editor_context.selection.selected_entities.len() > 1 {
                ui.heading(format!(
                    "{} Items Selected",
                    editor_context.selection.selected_entities.len()
                ));
                ui.label(
                    egui::RichText::new("(Editing Primary Item)")
                        .italics()
                        .small(),
                );
                ui.separator();
            }
            ui.heading("Clip Properties");
            ui.separator();

            let display_type = get_layer_display_type(&layer);
            ui.horizontal(|ui| {
                ui.label("Type:");
                ui.label(display_type);
            });

            let current_time = editor_context.timeline.current_time as f64;

            // --- Dynamic Properties ---
            let definitions =
                project_service.get_property_definitions(comp_id, track_id, selected_entity_id);

            // Group by category
            let mut grouped: std::collections::HashMap<
                String,
                Vec<library::model::property::PropertyDefinition>,
            > = std::collections::HashMap::new();
            for def in definitions {
                grouped.entry("General".to_string()).or_default().push(def);
            }

            // Sort categories
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

            let properties = &layer.properties;

            for category in categories {
                ui.add_space(5.0);
                ui.heading(&category);

                if let Some(defs) = grouped.remove(&category) {
                    // (Grid rendering logic kept mostly same, adapted to layer properties)
                    struct Chunk {
                        is_grid: bool,
                        defs: Vec<library::model::property::PropertyDefinition>,
                    }

                    let mut chunks: Vec<Chunk> = Vec::new();
                    let mut current_grid_defs = Vec::new();

                    for def in defs {
                        let is_multiline = matches!(
                            def.ui_type(),
                            library::model::property::PropertyUiType::MultilineText
                        );
                        if is_multiline {
                            if !current_grid_defs.is_empty() {
                                chunks.push(Chunk {
                                    is_grid: true,
                                    defs: current_grid_defs,
                                });
                                current_grid_defs = Vec::new();
                            }
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
                            let mut pending_actions = Vec::new();
                            egui::Grid::new(format!("cat_{}_{}", category, chunk_idx))
                                .striped(true)
                                .show(ui, |ui| {
                                    let actions = render_property_rows(
                                        ui,
                                        &chunk.defs,
                                        |name| {
                                            properties.get(name).and_then(|p| {
                                                Some(project_service.evaluate_property_value(
                                                    p,
                                                    properties,
                                                    current_time,
                                                    fps,
                                                ))
                                            })
                                        },
                                        |name| properties.get(name).cloned(),
                                        &PropertyRenderContext {
                                            available_fonts: &editor_context.available_fonts,
                                            in_grid: true,
                                            current_time,
                                        },
                                    );
                                    pending_actions = actions;
                                });
                            let mut ctx = ActionContext::new(
                                project_service,
                                history_manager,
                                selected_entity_id,
                                current_time,
                            );
                            if ctx.handle_actions(pending_actions, PropertyTarget::Clip, |n| {
                                properties.get(n).cloned()
                            }) {
                                needs_refresh = true;
                            }
                        } else {
                            for def in &chunk.defs {
                                ui.add_space(5.0);
                                let actions = render_property_rows(
                                    ui,
                                    std::slice::from_ref(def),
                                    |name| {
                                        properties.get(name).and_then(|p| {
                                            Some(project_service.evaluate_property_value(
                                                p,
                                                properties,
                                                current_time,
                                                fps,
                                            ))
                                        })
                                    },
                                    |name| properties.get(name).cloned(),
                                    &PropertyRenderContext {
                                        available_fonts: &editor_context.available_fonts,
                                        in_grid: false,
                                        current_time,
                                    },
                                );
                                let mut ctx = ActionContext::new(
                                    project_service,
                                    history_manager,
                                    selected_entity_id,
                                    current_time,
                                );
                                if ctx.handle_actions(actions, PropertyTarget::Clip, |n| {
                                    properties.get(n).cloned()
                                }) {
                                    needs_refresh = true;
                                }
                            }
                        }
                    }
                }
            }

            // --- Type Specific Sections ---
            let is_text = matches!(
                layer.content,
                LayerContent::Generator(GeneratorContent::Text { .. })
            );
            let is_shape = matches!(
                layer.content,
                LayerContent::Generator(GeneratorContent::Shape { .. })
            );

            // Styles
            if is_text || is_shape {
                render_styles_section(
                    ui,
                    project_service,
                    history_manager,
                    editor_context,
                    selected_entity_id,
                    current_time,
                    fps,
                    &layer.styles,
                    &mut needs_refresh,
                );
            }

            // Ensemble (Text only)
            if is_text {
                ui.add_space(5.0);
                render_ensemble_section(
                    ui,
                    project_service,
                    history_manager,
                    editor_context,
                    selected_entity_id,
                    current_time,
                    fps,
                    &layer.effects,
                    &layer.styles,
                    &mut needs_refresh,
                    properties,
                    &PropertyRenderContext {
                        available_fonts: &editor_context.available_fonts,
                        in_grid: false,
                        current_time,
                    },
                );
            }

            // --- Effects Section ---
            render_effects_section(
                ui,
                project_service,
                history_manager,
                editor_context,
                selected_entity_id,
                current_time,
                fps,
                &mut needs_refresh,
            );

            // --- Timing Section (Converted to Frames for Display) ---
            ui.add_space(10.0);
            ui.heading("Timing");
            ui.separator();

            egui::Grid::new("entity_timing")
                .striped(true)
                .show(ui, |ui| {
                    let start_frame = (layer.start_time.into_inner() * fps).round();
                    let duration_frame = (layer.duration.into_inner() * fps).round();
                    let trim_in_frame = (layer.trim_in.into_inner() * fps).round();

                    // Start Frame (In Frame)
                    ui.label("In Frame");
                    let mut current_start_f32 = start_frame as f32;
                    let response = ui.add(
                        egui::DragValue::new(&mut current_start_f32)
                            .speed(1.0)
                            .suffix("fr"),
                    );
                    if response.changed() {
                        let new_start_sec = (current_start_f32 as f64) / fps;
                        project_service
                            .update_clip_timing(
                                selected_entity_id,
                                new_start_sec,
                                layer.duration.into_inner(),
                            )
                            .ok();
                        needs_refresh = true;
                    }
                    if response.drag_stopped() || response.lost_focus() {
                        let current_state = project.read().unwrap().clone();
                        history_manager.push_project_state(current_state);
                    }
                    ui.end_row();

                    // Out Frame (End Frame)
                    ui.label("Out Frame");
                    // End frame is (start + duration)
                    let mut current_end_f32 = (start_frame + duration_frame) as f32;
                    let response = ui.add(
                        egui::DragValue::new(&mut current_end_f32)
                            .speed(1.0)
                            .suffix("fr"),
                    );
                    // If user drags Out Frame, we usually change duration?
                    // Or move clip?
                    // Old logic: update_clip_time(id, in, out).
                    // Here we want to change duration, keeping start fixed?
                    // Verify old behavior: drag_and_drop handled move. Inspector out_frame should prob change duration.
                    if response.changed() {
                        let new_end_sec = (current_end_f32 as f64) / fps;
                        let new_duration_sec =
                            (new_end_sec - layer.start_time.into_inner()).max(0.001);
                        project_service
                            .update_clip_timing(
                                selected_entity_id,
                                layer.start_time.into_inner(),
                                new_duration_sec,
                            )
                            .ok();
                        needs_refresh = true;
                    }
                    if response.drag_stopped() || response.lost_focus() {
                        let current_state = project.read().unwrap().clone();
                        history_manager.push_project_state(current_state);
                    }
                    ui.end_row();

                    // Source Begin (Trim In)
                    ui.label("Source Start");
                    let mut current_trim_f32 = trim_in_frame as f32;
                    let response = ui.add(
                        egui::DragValue::new(&mut current_trim_f32)
                            .speed(1.0)
                            .suffix("fr"),
                    );
                    if response.changed() {
                        let new_trim_sec = (current_trim_f32 as f64) / fps;
                        project_service
                            .update_clip_source_start(selected_entity_id, new_trim_sec)
                            .ok();
                        needs_refresh = true;
                    }
                    if response.drag_stopped() || response.lost_focus() {
                        let current_state = project.read().unwrap().clone();
                        history_manager.push_project_state(current_state);
                    }
                    ui.end_row();

                    // Duration Frame
                    ui.horizontal(|ui| {
                        ui.label("Duration");
                        ui.label(format!("{} fr", duration_frame));
                    });
                    ui.end_row();
                });
        } else {
            ui.label("Clip not found (it may have been deleted).");
            // Deselect if not found
            editor_context.selection.last_selected_entity_id = None;
            editor_context
                .selection
                .selected_entities
                .remove(&selected_entity_id);
        }
    } else {
        if editor_context.selection.composition_id.is_none() {
            ui.label("No composition selected.");
        } else if editor_context.selection.last_selected_track_id.is_none() {
            ui.label("No track selected.");
        } else {
            ui.label("Select a clip to edit");
        }
    }

    if needs_refresh {
        ui.ctx().request_repaint();
    }
}
