use egui::collapsing_header::CollapsingState;
use egui::Ui;

use library::project::clip::TrackClipKind;

use crate::context::context::PanelContext;

use library::project::property::{PropertyMap, PropertyUiType};

mod action_handler;
mod effects;
mod ensemble;
mod graph_items;
mod properties;
mod styles;

use action_handler::{ActionContext, PropertyTarget};
use effects::render_effects_section;
use ensemble::render_ensemble_section;
use properties::{render_property_rows, PropertyRenderContext};
use styles::render_styles_section;

/// Transform property names that live on the graph node, not on the clip.
const TRANSFORM_PROPERTY_NAMES: &[&str] = &["position", "scale", "rotation", "anchor", "opacity"];

pub(crate) fn inspector_panel(ui: &mut Ui, ctx: &mut PanelContext) {
    let PanelContext {
        editor_context,
        history_manager,
        project_service,
        project,
    } = ctx;
    let mut needs_refresh = false;

    // Display properties of selected entity
    if let (Some(selected_entity_id), Some(comp_id), Some(track_id)) = (
        editor_context.selection.last_selected_entity_id,
        editor_context.selection.composition_id,
        editor_context.selection.last_selected_track_id,
    ) {
        // Fetch entity data + transform node properties from project
        let entity_data = if let Ok(proj_read) = project.read() {
            proj_read.get_clip(selected_entity_id).map(|e| {
                // Resolve the transform graph node for this clip
                let clip_ctx = library::project::graph_analysis::resolve_clip_context(
                    &proj_read,
                    selected_entity_id,
                );
                let transform_node_id = clip_ctx.transform_node;
                let transform_props = transform_node_id
                    .and_then(|tid| proj_read.get_graph_node(tid))
                    .map(|n| n.properties.clone())
                    .unwrap_or_default();

                (
                    e.kind.clone(),
                    e.properties.clone(),
                    e.in_frame,
                    e.out_frame,
                    e.source_begin_frame,
                    e.duration_frame,
                    transform_node_id,
                    transform_props,
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
            transform_node_id,
            transform_props,
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

            let current_kind = kind.clone();
            ui.horizontal(|ui| {
                ui.label("Type:");
                ui.strong(current_kind.to_string());
            });
            ui.separator();

            let current_time = editor_context.timeline.current_time as f64;

            // --- Dynamic Properties ---
            let definitions =
                project_service.get_property_definitions(comp_id, track_id, selected_entity_id);
            let fps = project_service
                .get_composition(comp_id)
                .map(|c| c.fps)
                .unwrap_or(60.0);

            // Split definitions into clip properties and transform properties
            let mut clip_defs = Vec::new();
            let mut transform_defs = Vec::new();
            for def in definitions {
                if TRANSFORM_PROPERTY_NAMES.contains(&def.name()) {
                    transform_defs.push(def);
                } else {
                    clip_defs.push(def);
                }
            }

            let context = PropertyRenderContext {
                available_fonts: &editor_context.available_fonts,
                in_grid: true,
                current_time,
            };

            // ===== Section 1: Clip Properties (source node in data-flow) =====
            if !clip_defs.is_empty() {
                let clip_section_id = ui.make_persistent_id("inspector_clip_props");
                let clip_state =
                    CollapsingState::load_with_default_open(ui.ctx(), clip_section_id, true);
                let clip_header = clip_state.show_header(ui, |ui| {
                    ui.label(egui::RichText::new(format!("Clip ({})", kind)).strong());
                });
                clip_header.body(|ui| {
                    render_property_section(
                        ui,
                        &clip_defs,
                        &properties,
                        PropertyTarget::Clip,
                        "clip_props",
                        project_service,
                        history_manager,
                        selected_entity_id,
                        current_time,
                        fps,
                        &context,
                        &mut needs_refresh,
                    );
                });
            }

            // ===== Section 2: Ensemble — Effectors (Text only, shape chain) =====
            if matches!(kind, TrackClipKind::Text) {
                render_ensemble_section(
                    ui,
                    project_service,
                    history_manager,
                    editor_context,
                    selected_entity_id,
                    track_id,
                    current_time,
                    fps,
                    &Vec::new(),
                    &Vec::new(),
                    &mut needs_refresh,
                    &properties,
                    &PropertyRenderContext {
                        available_fonts: &editor_context.available_fonts,
                        in_grid: false,
                        current_time,
                    },
                    project,
                );
            }

            // ===== Section 3: Styles (Text/Shape only, shape → image conversion) =====
            if matches!(kind, TrackClipKind::Text | TrackClipKind::Shape) {
                render_styles_section(
                    ui,
                    project_service,
                    history_manager,
                    editor_context,
                    selected_entity_id,
                    track_id,
                    current_time,
                    fps,
                    &Vec::new(),
                    project,
                    &mut needs_refresh,
                );
            }

            // ===== Section 4: Effects (image chain, between style/clip and transform) =====
            render_effects_section(
                ui,
                project_service,
                history_manager,
                editor_context,
                selected_entity_id,
                track_id,
                current_time,
                fps,
                project,
                &mut needs_refresh,
            );

            // ===== Section 5: Transform (final output, closest to render) =====
            if !transform_defs.is_empty() && transform_node_id.is_some() {
                ui.add_space(5.0);
                let transform_section_id = ui.make_persistent_id("inspector_transform_props");
                let transform_state =
                    CollapsingState::load_with_default_open(ui.ctx(), transform_section_id, true);
                let transform_header = transform_state.show_header(ui, |ui| {
                    ui.label(egui::RichText::new("Transform").strong());
                });
                transform_header.body(|ui| {
                    render_property_section(
                        ui,
                        &transform_defs,
                        &transform_props,
                        PropertyTarget::GraphNode(transform_node_id.unwrap()),
                        "transform_props",
                        project_service,
                        history_manager,
                        selected_entity_id,
                        current_time,
                        fps,
                        &PropertyRenderContext {
                            available_fonts: &editor_context.available_fonts,
                            in_grid: true,
                            current_time,
                        },
                        &mut needs_refresh,
                    );
                });
            }

            // ===== Timing Section =====
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

/// Render a section of property definitions with proper grid/full-width handling.
#[allow(clippy::too_many_arguments)]
fn render_property_section(
    ui: &mut Ui,
    defs: &[library::project::property::PropertyDefinition],
    prop_source: &PropertyMap,
    target: PropertyTarget,
    id_prefix: &str,
    project_service: &mut library::EditorService,
    history_manager: &mut crate::command::history::HistoryManager,
    entity_id: uuid::Uuid,
    current_time: f64,
    fps: f64,
    context: &PropertyRenderContext,
    needs_refresh: &mut bool,
) {
    struct Chunk {
        is_grid: bool,
        defs: Vec<library::project::property::PropertyDefinition>,
    }

    let mut chunks: Vec<Chunk> = Vec::new();
    let mut current_grid_defs = Vec::new();

    for def in defs {
        let is_multiline = matches!(def.ui_type(), PropertyUiType::MultilineText);
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
                defs: vec![def.clone()],
            });
        } else {
            current_grid_defs.push(def.clone());
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
            egui::Grid::new(format!("{}_{}", id_prefix, chunk_idx))
                .striped(true)
                .show(ui, |ui| {
                    let actions = render_property_rows(
                        ui,
                        &chunk.defs,
                        |name| {
                            prop_source.get(name).map(|p| {
                                project_service.evaluate_property_value(
                                    p,
                                    prop_source,
                                    current_time,
                                    fps,
                                )
                            })
                        },
                        |name| prop_source.get(name).cloned(),
                        context,
                    );
                    pending_actions = actions;
                });
            for action in pending_actions {
                let mut ctx =
                    ActionContext::new(project_service, history_manager, entity_id, current_time);
                if ctx.handle_actions(vec![action], target.clone(), |n| {
                    prop_source.get(n).cloned()
                }) {
                    *needs_refresh = true;
                }
            }
        } else {
            for def in &chunk.defs {
                ui.add_space(5.0);
                let actions = render_property_rows(
                    ui,
                    std::slice::from_ref(def),
                    |name| {
                        prop_source.get(name).map(|p| {
                            project_service.evaluate_property_value(
                                p,
                                prop_source,
                                current_time,
                                fps,
                            )
                        })
                    },
                    |name| prop_source.get(name).cloned(),
                    &PropertyRenderContext {
                        available_fonts: context.available_fonts,
                        in_grid: false,
                        current_time: context.current_time,
                    },
                );
                for action in actions {
                    let mut ctx = ActionContext::new(
                        project_service,
                        history_manager,
                        entity_id,
                        current_time,
                    );
                    if ctx.handle_actions(vec![action], target.clone(), |n| {
                        prop_source.get(n).cloned()
                    }) {
                        *needs_refresh = true;
                    }
                }
            }
        }
    }
}
