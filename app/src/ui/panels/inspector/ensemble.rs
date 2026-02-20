use super::action_handler::{ActionContext, PropertyTarget};
use super::graph_items::{
    collect_graph_nodes, render_chain_add_button, render_graph_node_item, ChainConfig,
};
use super::properties::{render_inspector_properties_grid, PropertyRenderContext};
use crate::action::HistoryManager;
use crate::state::context::EditorContext;

use egui::collapsing_header::CollapsingState;
use egui::Ui;
use library::model::project::ensemble::{DecoratorInstance, EffectorInstance};
use library::model::project::graph_analysis;
use library::model::project::property::PropertyMap;
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub(super) fn render_ensemble_section(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    _editor_context: &EditorContext,
    selected_entity_id: Uuid,
    track_id: Uuid,
    current_time: f64,
    fps: f64,
    effectors: &Vec<EffectorInstance>,
    decorators: &Vec<DecoratorInstance>,
    needs_refresh: &mut bool,
    _properties: &PropertyMap,
    context: &PropertyRenderContext,
    project: &Arc<RwLock<library::model::project::project::Project>>,
) {
    ui.add_space(10.0);
    ui.heading("Ensemble");
    ui.separator();

    let graph_effectors = collect_graph_nodes(
        project,
        project_service,
        selected_entity_id,
        graph_analysis::get_associated_effectors,
    );
    let graph_decorators = collect_graph_nodes(
        project,
        project_service,
        selected_entity_id,
        graph_analysis::get_associated_decorators,
    );

    let has_graph_effectors = !graph_effectors.is_empty();
    let has_graph_decorators = !graph_decorators.is_empty();

    // --- Effectors ---
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Effectors").strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            render_chain_add_button(
                ui,
                project_service,
                history_manager,
                project,
                track_id,
                selected_entity_id,
                &ChainConfig::EFFECTOR,
                |pm| pm.get_available_effectors(),
                |pm, name| {
                    pm.get_effector_plugin(name)
                        .map(|p| p.name())
                        .unwrap_or_else(|| name.to_string())
                },
                needs_refresh,
            );
        });
    });

    if has_graph_effectors {
        for eff in &graph_effectors {
            render_graph_node_item(
                ui,
                project_service,
                history_manager,
                project,
                selected_entity_id,
                eff,
                current_time,
                fps,
                context,
                needs_refresh,
                "graph_ensemble",
                true,
            );
        }
    } else if !effectors.is_empty() {
        render_embedded_effectors(
            ui,
            project_service,
            history_manager,
            selected_entity_id,
            effectors,
            current_time,
            fps,
            context,
            needs_refresh,
        );
    }

    ui.separator();

    // --- Decorators ---
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Decorators").strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            render_chain_add_button(
                ui,
                project_service,
                history_manager,
                project,
                track_id,
                selected_entity_id,
                &ChainConfig::DECORATOR,
                |pm| pm.get_available_decorators(),
                |pm, name| {
                    pm.get_decorator_plugin(name)
                        .map(|p| p.name())
                        .unwrap_or_else(|| name.to_string())
                },
                needs_refresh,
            );
        });
    });

    if has_graph_decorators {
        for dec in &graph_decorators {
            render_graph_node_item(
                ui,
                project_service,
                history_manager,
                project,
                selected_entity_id,
                dec,
                current_time,
                fps,
                context,
                needs_refresh,
                "graph_ensemble",
                true,
            );
        }
    } else if !decorators.is_empty() {
        render_embedded_decorators(
            ui,
            project_service,
            history_manager,
            selected_entity_id,
            decorators,
            current_time,
            fps,
            context,
            needs_refresh,
        );
    }
}

// --- Legacy embedded renderers (kept as-is, unique CollectionEditor logic) ---

fn render_embedded_effectors(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    selected_entity_id: Uuid,
    effectors: &[EffectorInstance],
    current_time: f64,
    fps: f64,
    context: &PropertyRenderContext,
    needs_refresh: &mut bool,
) {
    let effectors_owned = effectors.to_vec();
    let mut local_effectors = effectors_owned.clone();

    crate::ui::widgets::collection_editor::CollectionEditor::new(
        "ensemble_effectors_list",
        &mut local_effectors,
        |e| egui::Id::new(e.id),
        |ui, visual_index, effector, handle, history_manager, project_service, needs_refresh| {
            let backend_index = effectors_owned
                .iter()
                .position(|e| e.id == effector.id)
                .unwrap_or(visual_index);

            let id = ui.make_persistent_id(format!("effector_{}", effector.id));
            let state = CollapsingState::load_with_default_open(ui.ctx(), id, true);

            let mut remove_clicked = false;
            let header_res = state.show_header(ui, |ui| {
                ui.horizontal(|ui| {
                    handle.ui(ui, |ui| {
                        ui.label("::");
                    });
                    ui.label(
                        egui::RichText::new(
                            project_service
                                .get_plugin_manager()
                                .get_effector_plugin(&effector.effector_type)
                                .map(|p| p.name())
                                .unwrap_or_else(|| effector.effector_type.clone()),
                        )
                        .strong(),
                    );
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
                    .get_effector_properties(&effector.effector_type);

                let item_actions = render_inspector_properties_grid(
                    ui,
                    format!("effector_grid_{}", effector.id),
                    &effector.properties,
                    &defs,
                    project_service,
                    context,
                    fps,
                );

                let effector_props = effector.properties.clone();
                let mut ctx = ActionContext::new(
                    project_service,
                    history_manager,
                    selected_entity_id,
                    current_time,
                );
                if ctx.handle_actions(item_actions, PropertyTarget::Effector(backend_index), |n| {
                    effector_props.get(n).cloned()
                }) {
                    *needs_refresh = true;
                }
            });

            remove_clicked
        },
        |new_effectors, project_service| {
            project_service.update_track_clip_effectors(selected_entity_id, new_effectors)
        },
    )
    .show(ui, history_manager, project_service, needs_refresh);
}

fn render_embedded_decorators(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    selected_entity_id: Uuid,
    decorators: &[DecoratorInstance],
    current_time: f64,
    fps: f64,
    context: &PropertyRenderContext,
    needs_refresh: &mut bool,
) {
    let decorators_owned = decorators.to_vec();
    let mut local_decorators = decorators_owned.clone();

    crate::ui::widgets::collection_editor::CollectionEditor::new(
        "ensemble_decorators_list",
        &mut local_decorators,
        |d| egui::Id::new(d.id),
        |ui, visual_index, decorator, handle, history_manager, project_service, needs_refresh| {
            let backend_index = decorators_owned
                .iter()
                .position(|d| d.id == decorator.id)
                .unwrap_or(visual_index);

            let id = ui.make_persistent_id(format!("decorator_{}", decorator.id));
            let state = CollapsingState::load_with_default_open(ui.ctx(), id, true);

            let mut remove_clicked = false;
            let header_res = state.show_header(ui, |ui| {
                ui.horizontal(|ui| {
                    handle.ui(ui, |ui| {
                        ui.label("::");
                    });
                    ui.label(
                        egui::RichText::new(
                            project_service
                                .get_plugin_manager()
                                .get_decorator_plugin(&decorator.decorator_type)
                                .map(|p| p.name())
                                .unwrap_or_else(|| decorator.decorator_type.clone()),
                        )
                        .strong(),
                    );
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
                    .get_decorator_properties(&decorator.decorator_type);

                let item_actions = render_inspector_properties_grid(
                    ui,
                    format!("decorator_grid_{}", decorator.id),
                    &decorator.properties,
                    &defs,
                    project_service,
                    context,
                    fps,
                );

                let decorator_props = decorator.properties.clone();
                let mut ctx = ActionContext::new(
                    project_service,
                    history_manager,
                    selected_entity_id,
                    current_time,
                );
                if ctx.handle_actions(
                    item_actions,
                    PropertyTarget::Decorator(backend_index),
                    |n| decorator_props.get(n).cloned(),
                ) {
                    *needs_refresh = true;
                }
            });

            remove_clicked
        },
        |new_decorators, project_service| {
            project_service.update_track_clip_decorators(selected_entity_id, new_decorators)
        },
    )
    .show(ui, history_manager, project_service, needs_refresh);
}
