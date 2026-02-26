use super::action_handler::{ActionContext, PropertyTarget};
use super::graph_items::collect_graph_nodes;
use super::properties::{render_inspector_properties_grid, PropertyRenderContext};
use crate::command::history::HistoryManager;
use crate::context::context::EditorContext;

use egui::collapsing_header::CollapsingState;
use egui::Ui;
use library::project::connection::PinId;
use library::project::graph_analysis;
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
    project: &Arc<RwLock<library::project::project::Project>>,
    needs_refresh: &mut bool,
) {
    ui.add_space(10.0);
    ui.heading("Effects");
    ui.separator();

    let graph_effects = collect_graph_nodes(
        project,
        project_service,
        selected_entity_id,
        |proj, source_id| graph_analysis::get_effect_chain(proj, source_id),
    );

    let has_graph_effects = !graph_effects.is_empty();

    // Embedded effects have been removed; only graph-based effects are used
    let embedded_effects: Vec<library::project::effect::EffectConfig> = Vec::new();

    // Add button (effects use searchable menu, not simple chain add)
    use super::properties::render_add_button;
    render_add_button(ui, |ui| {
        use crate::widgets::context_menu::{show_searchable_context_menu, SearchableItem};

        let available_effects = project_service.get_plugin_manager().get_available_effects();
        let items: Vec<SearchableItem<String>> = available_effects
            .into_iter()
            .map(|(id, name, category)| SearchableItem {
                label: name,
                category: Some(category),
                icon: None,
                action: id,
                enabled: true,
                keywords: vec![],
            })
            .collect();

        if let Some(effect_id) = show_searchable_context_menu(ui, "add_effect_menu", &items) {
            let type_id = format!("effect.{}", effect_id);
            match project_service.add_graph_node(track_id, &type_id) {
                Ok(new_node_id) => {
                    // Resolve clip context to find insertion point and transform
                    let (connect_from_id, old_transform_conn, transform_id) = {
                        if let Ok(proj) = project.read() {
                            let ctx =
                                graph_analysis::resolve_source_context(&proj, selected_entity_id);

                            // Insert after last effect, or after style (for text/shape),
                            // or after clip itself
                            let from_id = ctx
                                .effect_chain
                                .last()
                                .copied()
                                .or_else(|| ctx.style_chain.last().copied())
                                .unwrap_or(selected_entity_id);

                            // Find existing connection to transform.image_in
                            let old_conn = ctx.transform_node.and_then(|t| {
                                proj.connections
                                    .iter()
                                    .find(|c| c.to == PinId::new(t, "image_in"))
                                    .map(|c| c.id)
                            });

                            (from_id, old_conn, ctx.transform_node)
                        } else {
                            (selected_entity_id, None, None)
                        }
                    };

                    // Remove old connection to transform (to make room for new chain)
                    if let Some(conn_id) = old_transform_conn {
                        let _ = project_service.remove_graph_connection(conn_id);
                    }

                    // Connect source → new_effect.image_in
                    let connect_from = PinId::new(connect_from_id, "image_out");
                    let connect_to = PinId::new(new_node_id, "image_in");
                    if let Err(e) = project_service.add_graph_connection(connect_from, connect_to) {
                        log::error!("Failed to connect effect input: {}", e);
                    }

                    // Connect new_effect.image_out → transform.image_in
                    if let Some(t_id) = transform_id {
                        if let Err(e) = project_service.add_graph_connection(
                            PinId::new(new_node_id, "image_out"),
                            PinId::new(t_id, "image_in"),
                        ) {
                            log::error!("Failed to connect effect to transform: {}", e);
                        }
                    }

                    drop(history_manager.begin_mutation(project));
                    *needs_refresh = true;
                }
                Err(e) => {
                    log::error!("Failed to add effect graph node: {}", e);
                }
            }
        }
    });

    let context = PropertyRenderContext {
        available_fonts: &editor_context.available_fonts,
        in_grid: true,
        current_time,
    };

    // Render graph-based effects with drag-and-drop reordering
    if has_graph_effects {
        render_reorderable_graph_effects(
            ui,
            project_service,
            history_manager,
            project,
            selected_entity_id,
            graph_effects,
            current_time,
            fps,
            &context,
            needs_refresh,
        );
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

/// Render graph-based effects with drag-and-drop reordering support.
#[allow(clippy::too_many_arguments)]
fn render_reorderable_graph_effects(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    project: &Arc<RwLock<library::project::project::Project>>,
    source_id: Uuid,
    graph_effects: Vec<super::graph_items::GraphNodeInfo>,
    current_time: f64,
    fps: f64,
    context: &PropertyRenderContext,
    needs_refresh: &mut bool,
) {
    use egui_dnd::dnd;

    // Build DnD wrapper items
    let mut dnd_items: Vec<(egui::Id, Uuid)> = graph_effects
        .iter()
        .map(|e| (egui::Id::new(e.node_id), e.node_id))
        .collect();
    let old_order: Vec<Uuid> = dnd_items.iter().map(|(_, id)| *id).collect();

    let response = dnd(ui, egui::Id::new("graph_effects_dnd")).show(
        dnd_items.iter_mut(),
        |ui, (_dnd_id, node_id), handle, _state| {
            // Find the corresponding graph effect info
            if let Some(effect) = graph_effects.iter().find(|e| e.node_id == *node_id) {
                let id = ui.make_persistent_id(format!("graph_effect_{}", effect.node_id));
                let state = CollapsingState::load_with_default_open(ui.ctx(), id, false);

                let mut remove_clicked = false;
                let header_res = state.show_header(ui, |ui| {
                    ui.horizontal(|ui| {
                        handle.ui(ui, |ui| {
                            ui.label("::");
                        });
                        ui.label(egui::RichText::new(&effect.display_name).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("X").clicked() {
                                remove_clicked = true;
                            }
                        });
                    });
                });

                if remove_clicked {
                    if let Err(e) = project_service.remove_graph_node(effect.node_id) {
                        log::error!("Failed to remove effect node: {}", e);
                    } else {
                        drop(history_manager.begin_mutation(project));
                        *needs_refresh = true;
                    }
                }

                header_res.body(|ui| {
                    let defs = project_service
                        .get_plugin_manager()
                        .get_node_type(&effect.type_id)
                        .map(|def| def.default_properties.clone())
                        .unwrap_or_default();

                    let item_actions = render_inspector_properties_grid(
                        ui,
                        format!("graph_effect_grid_{}", effect.node_id),
                        &effect.properties,
                        &defs,
                        project_service,
                        context,
                        fps,
                    );

                    let item_props = effect.properties.clone();
                    let mut ctx = ActionContext::new(
                        project_service,
                        history_manager,
                        source_id,
                        current_time,
                    );
                    if ctx.handle_actions(
                        item_actions,
                        PropertyTarget::GraphNode(effect.node_id),
                        |n| item_props.get(n).cloned(),
                    ) {
                        *needs_refresh = true;
                    }
                });
            }
        },
    );

    if response.final_update().is_some() {
        response.update_vec(&mut dnd_items);
        let new_order: Vec<Uuid> = dnd_items.iter().map(|(_, id)| *id).collect();

        if new_order != old_order {
            if let Err(e) = project_service.reorder_effect_chain(source_id, &new_order) {
                log::error!("Failed to reorder effect chain: {}", e);
            } else {
                let current_state = project_service.with_project(|p| p.clone());
                history_manager.push_project_state(current_state);
                *needs_refresh = true;
            }
        }
    }
}

/// Legacy: render embedded EffectConfig items (fallback when no graph effects exist)
fn render_embedded_effects(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    editor_context: &mut EditorContext,
    selected_entity_id: Uuid,
    effects: &[library::project::effect::EffectConfig],
    current_time: f64,
    fps: f64,
    needs_refresh: &mut bool,
) {
    let effects_owned = effects.to_vec();
    let mut local_effects = effects_owned.clone();
    let list_id = egui::Id::new(format!("effects_{}", selected_entity_id));

    crate::widgets::collection_editor::CollectionEditor::new(
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
        |_new_effects, _project_service| {
            // Embedded effects no longer exist; graph-based effects are managed via node editor
            Ok(())
        },
    )
    .show(ui, history_manager, project_service, needs_refresh);
}
