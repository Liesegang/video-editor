use super::action_handler::{ActionContext, PropertyTarget};
use super::graph_items::{collect_graph_nodes, render_graph_node_item};
use super::properties::{render_inspector_properties_grid, PropertyRenderContext};
use crate::action::HistoryManager;
use crate::state::context::EditorContext;

use egui::collapsing_header::CollapsingState;
use egui::Ui;
use library::model::project::connection::PinId;
use library::model::project::graph_analysis;
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub(super) fn render_effects_section(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    selected_entity_id: Uuid,
    track_id: Uuid,
    current_time: f64,
    fps: f64,
    project: &Arc<RwLock<library::model::project::project::Project>>,
    needs_refresh: &mut bool,
) {
    ui.add_space(10.0);
    ui.heading("Effects");
    ui.separator();

    let graph_effects = collect_graph_nodes(
        project,
        project_service,
        selected_entity_id,
        |proj, clip_id| graph_analysis::get_effect_chain(proj, clip_id),
    );

    let has_graph_effects = !graph_effects.is_empty();

    // Collect embedded effects (legacy fallback)
    let embedded_effects = if !has_graph_effects {
        project_service
            .with_project(|proj| proj.get_clip(selected_entity_id).map(|c| c.effects.clone()))
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    // Add button (effects use searchable menu, not simple chain add)
    use super::properties::render_add_button;
    render_add_button(ui, |ui| {
        let available_effects = project_service.get_plugin_manager().get_available_effects();
        let items: Vec<(String, Option<String>, String)> = available_effects
            .into_iter()
            .map(|(id, name, category)| (name, Some(category), id))
            .collect();

        crate::ui::widgets::searchable_context_menu::show_searchable_menu(
            ui,
            "add_effect_menu",
            &items,
            |effect_id| {
                let type_id = format!("effect.{}", effect_id);
                match project_service.add_graph_node(track_id, &type_id) {
                    Ok(new_node_id) => {
                        let connect_from = if let Ok(proj) = project.read() {
                            let chain = graph_analysis::get_effect_chain(&proj, selected_entity_id);
                            if let Some(&last_effect) = chain.last() {
                                PinId::new(last_effect, "image_out")
                            } else {
                                PinId::new(selected_entity_id, "image_out")
                            }
                        } else {
                            PinId::new(selected_entity_id, "image_out")
                        };

                        let connect_to = PinId::new(new_node_id, "image_in");
                        if let Err(e) =
                            project_service.add_graph_connection(connect_from, connect_to)
                        {
                            log::error!("Failed to connect effect: {}", e);
                        }

                        drop(history_manager.begin_mutation(project));
                        *needs_refresh = true;
                    }
                    Err(e) => {
                        log::error!("Failed to add effect graph node: {}", e);
                    }
                }
            },
        );
    });

    let context = PropertyRenderContext {
        available_fonts: &editor_context.available_fonts,
        in_grid: true,
        current_time,
    };

    // Render graph-based effects
    if has_graph_effects {
        for effect in &graph_effects {
            render_graph_node_item(
                ui,
                project_service,
                history_manager,
                project,
                selected_entity_id,
                effect,
                current_time,
                fps,
                &context,
                needs_refresh,
                "graph_effect",
                false,
            );
        }
    } else if !embedded_effects.is_empty() {
        render_embedded_effects(
            ui,
            project_service,
            history_manager,
            editor_context,
            selected_entity_id,
            &embedded_effects,
            current_time,
            fps,
            needs_refresh,
        );
    }
}

/// Legacy: render embedded EffectConfig items (fallback when no graph effects exist)
fn render_embedded_effects(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    selected_entity_id: Uuid,
    effects: &[library::model::project::effect::EffectConfig],
    current_time: f64,
    fps: f64,
    needs_refresh: &mut bool,
) {
    let effects_owned = effects.to_vec();
    let mut local_effects = effects_owned.clone();
    let list_id = egui::Id::new(format!("effects_{}", selected_entity_id));

    crate::ui::widgets::collection_editor::CollectionEditor::new(
        list_id,
        &mut local_effects,
        |e| egui::Id::new(e.id),
        |ui, visual_index, effect, handle, history_manager, project_service, needs_refresh| {
            let effect_index = effects_owned
                .iter()
                .position(|e| e.id == effect.id)
                .unwrap_or(visual_index);
            let id = ui.make_persistent_id(format!("effect_{}", effect.id));
            let state = CollapsingState::load_with_default_open(ui.ctx(), id, false);

            let mut remove_clicked = false;
            let header_res = state.show_header(ui, |ui| {
                ui.horizontal(|ui| {
                    handle.ui(ui, |ui| {
                        ui.label("::");
                    });
                    ui.label(egui::RichText::new(&effect.effect_type).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("X").clicked() {
                            remove_clicked = true;
                        }
                    });
                });
            });

            header_res.body(|ui| {
                let defs = project_service
                    .get_plugin_manager()
                    .get_effect_properties(&effect.effect_type);

                let context = PropertyRenderContext {
                    available_fonts: &editor_context.available_fonts,
                    in_grid: true,
                    current_time,
                };

                let pending_actions = render_inspector_properties_grid(
                    ui,
                    format!("effect_grid_{}", effect.id),
                    &effect.properties,
                    &defs,
                    project_service,
                    &context,
                    fps,
                );
                let effect_props = effect.properties.clone();
                let mut ctx = ActionContext::new(
                    project_service,
                    history_manager,
                    selected_entity_id,
                    current_time,
                );
                if ctx.handle_actions(pending_actions, PropertyTarget::Effect(effect_index), |n| {
                    effect_props.get(n).cloned()
                }) {
                    *needs_refresh = true;
                }
            });

            remove_clicked
        },
        |new_effects, project_service| {
            project_service.update_track_clip_effects(selected_entity_id, new_effects)
        },
    )
    .show(ui, history_manager, project_service, needs_refresh);
}
