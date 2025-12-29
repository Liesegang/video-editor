use egui::Ui;

use library::model::project::project::Project;
use library::model::project::TrackClipKind;

use library::EditorService;
use std::sync::{Arc, RwLock};

use crate::{action::HistoryManager, state::context::EditorContext};

use library::model::project::property::PropertyUiType;

pub mod action_handler;
pub mod effects;
pub mod ensemble;
pub mod properties;
pub mod styles;

use action_handler::{ActionContext, PropertyTarget};
use effects::render_effects_section;
use ensemble::render_ensemble_section;
use properties::{render_property_rows, PropertyAction, PropertyRenderContext};
use styles::render_styles_section;

/// Process PropertyAction events using ActionContext for unified handling.
fn process_property_actions(
    actions: Vec<PropertyAction>,
    ctx: &mut ActionContext,
    target: PropertyTarget,
    properties: &library::model::project::property::PropertyMap,
) -> bool {
    let mut needs_refresh = false;
    for action in actions {
        match action {
            PropertyAction::Update(name, val) => {
                ctx.handle_update(target, &name, val, |n| properties.get(n).cloned());
                needs_refresh = true;
            }
            PropertyAction::Commit => {
                ctx.handle_commit();
            }
            PropertyAction::ToggleKeyframe(name, val) => {
                ctx.handle_toggle_keyframe(target, &name, val, |n| properties.get(n).cloned());
                needs_refresh = true;
            }
            PropertyAction::SetAttribute(name, key, val) => {
                ctx.handle_set_attribute(target, &name, &key, val);
                needs_refresh = true;
            }
        }
    }
    needs_refresh
}

pub fn inspector_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut EditorService,
    project: &Arc<RwLock<Project>>,
) {
    let mut needs_refresh = false;

    // Display properties of selected entity
    if let (Some(selected_entity_id), Some(comp_id), Some(track_id)) = (
        editor_context.selection.last_selected_entity_id,
        editor_context.selection.composition_id,
        editor_context.selection.last_selected_track_id,
    ) {
        // Fetch entity data directly from project using flat O(1) lookup
        let entity_data = if let Ok(proj_read) = project.read() {
            // Use direct project.get_clip() instead of nested traversal
            proj_read.get_clip(selected_entity_id).map(|e| {
                (
                    e.kind.clone(),
                    e.properties.clone(),
                    e.in_frame,
                    e.out_frame,
                    e.source_begin_frame,
                    e.duration_frame,
                    e.effects.clone(),
                    e.styles.clone(),
                    e.effectors.clone(),
                    e.decorators.clone(),
                )
            })
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
            _effects,
            styles,
            effectors,
            decorators,
        )) = entity_data
        {
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

            let current_kind = kind.clone();
            ui.horizontal(|ui| {
                ui.label("Type:");
                ui.label(current_kind.to_string());
            });

            let current_time = editor_context.timeline.current_time as f64;

            // --- Dynamic Properties ---
            let definitions =
                project_service.get_property_definitions(comp_id, track_id, selected_entity_id);
            let fps = project_service
                .get_composition(comp_id)
                .map(|c| c.fps)
                .unwrap_or(60.0);

            // Group by category
            let mut grouped: std::collections::HashMap<
                String,
                Vec<library::model::project::property::PropertyDefinition>,
            > = std::collections::HashMap::new();
            for def in definitions {
                grouped.entry(def.category.clone()).or_default().push(def);
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

            for category in categories {
                ui.add_space(5.0);
                ui.heading(&category);

                if let Some(defs) = grouped.remove(&category) {
                    struct Chunk {
                        is_grid: bool,
                        defs: Vec<library::model::project::property::PropertyDefinition>,
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
                                    defs: current_grid_defs,
                                });
                                current_grid_defs = Vec::new(); // Re-init
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
                                                    &properties,
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
                            // Process actions outside the Grid closure to avoid borrow conflicts
                            let mut ctx = ActionContext::new(
                                project_service,
                                history_manager,
                                comp_id,
                                track_id,
                                selected_entity_id,
                                current_time,
                            );
                            if process_property_actions(
                                pending_actions,
                                &mut ctx,
                                PropertyTarget::Clip,
                                &properties,
                            ) {
                                needs_refresh = true;
                            }
                        } else {
                            // Full Width Render
                            for def in &chunk.defs {
                                ui.add_space(5.0);
                                let actions = render_property_rows(
                                    ui,
                                    std::slice::from_ref(def),
                                    |name| {
                                        properties.get(name).and_then(|p| {
                                            Some(project_service.evaluate_property_value(
                                                p,
                                                &properties,
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
                                // Process actions using unified handler
                                let mut ctx = ActionContext::new(
                                    project_service,
                                    history_manager,
                                    comp_id,
                                    track_id,
                                    selected_entity_id,
                                    current_time,
                                );
                                if process_property_actions(
                                    actions,
                                    &mut ctx,
                                    PropertyTarget::Clip,
                                    &properties,
                                ) {
                                    needs_refresh = true;
                                }
                            }
                        }
                    }
                }
            }

            // --- Styles Section (Text and Shape only) ---
            if matches!(kind, TrackClipKind::Text | TrackClipKind::Shape) {
                render_styles_section(
                    ui,
                    project_service,
                    history_manager,
                    editor_context,
                    comp_id,
                    track_id,
                    selected_entity_id,
                    current_time,
                    fps,
                    &styles,
                    &mut needs_refresh,
                );
            }

            //--- Ensemble Section (Text only) ---
            if matches!(kind, TrackClipKind::Text) {
                ui.add_space(5.0);
                let ensemble_actions = render_ensemble_section(
                    ui,
                    project_service,    // Add project_service
                    history_manager,    // Add history_manager
                    editor_context,     // Add editor_context
                    comp_id,            // Add comp_id
                    track_id,           // Add track_id
                    selected_entity_id, // Add selected_entity_id
                    current_time,       // Add current_time
                    fps,                // Add fps
                    &effectors,         // Pass effectors
                    &decorators,        // Pass decorators
                    &mut needs_refresh, // Pass needs_refresh
                    &properties,
                    &PropertyRenderContext {
                        available_fonts: &editor_context.available_fonts,
                        in_grid: false,
                        current_time,
                    },
                );
                let mut ctx = ActionContext::new(
                    project_service,
                    history_manager,
                    comp_id,
                    track_id,
                    selected_entity_id,
                    current_time,
                );
                if process_property_actions(
                    ensemble_actions,
                    &mut ctx,
                    PropertyTarget::Clip,
                    &properties,
                ) {
                    needs_refresh = true;
                }
            }

            // --- Effects Section ---
            render_effects_section(
                ui,
                project_service,
                history_manager,
                editor_context,
                comp_id,
                track_id,
                selected_entity_id,
                current_time,
                fps,
                &mut needs_refresh,
            );

            ui.add_space(10.0);
            ui.heading("Timing");
            ui.separator();

            egui::Grid::new("entity_timing")
                .striped(true)
                .show(ui, |ui| {
                    // In Frame
                    ui.label("In Frame");
                    let mut current_in_frame_f32 = in_frame as f32;
                    let response = ui.add(
                        egui::DragValue::new(&mut current_in_frame_f32)
                            .speed(1.0)
                            .suffix("fr"),
                    );
                    if response.changed() {
                        project_service
                            .update_clip_time(
                                comp_id,
                                track_id,
                                selected_entity_id,
                                current_in_frame_f32 as u64,
                                out_frame,
                            )
                            .ok();
                        needs_refresh = true;
                    }
                    if response.drag_stopped() || response.lost_focus() {
                        let current_state = project.read().unwrap().clone();
                        history_manager.push_project_state(current_state);
                    }
                    ui.end_row();

                    // Out Frame
                    ui.label("Out Frame");
                    let mut current_out_frame_f32 = out_frame as f32;
                    let response = ui.add(
                        egui::DragValue::new(&mut current_out_frame_f32)
                            .speed(1.0)
                            .suffix("fr"),
                    );
                    if response.changed() {
                        project_service
                            .update_clip_time(
                                comp_id,
                                track_id,
                                selected_entity_id,
                                in_frame,
                                current_out_frame_f32 as u64,
                            )
                            .ok();
                        needs_refresh = true;
                    }
                    if response.drag_stopped() || response.lost_focus() {
                        let current_state = project.read().unwrap().clone();
                        history_manager.push_project_state(current_state);
                    }
                    ui.end_row();

                    // Source Begin Frame
                    ui.label("Source Begin Frame");
                    let mut current_source_begin_frame_f32 = source_begin_frame as f32;
                    let response = ui.add(
                        egui::DragValue::new(&mut current_source_begin_frame_f32)
                            .speed(1.0)
                            .suffix("fr"),
                    );
                    if response.changed() {
                        project_service
                            .update_clip_source_frames(
                                comp_id,
                                track_id,
                                selected_entity_id,
                                current_source_begin_frame_f32 as i64,
                            )
                            .ok();
                        needs_refresh = true;
                    }
                    if response.drag_stopped() || response.lost_focus() {
                        let current_state = project.read().unwrap().clone();
                        history_manager.push_project_state(current_state);
                    }
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
