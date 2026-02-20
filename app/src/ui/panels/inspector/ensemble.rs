use super::action_handler::{ActionContext, PropertyTarget};
use super::properties::{render_inspector_properties_grid, PropertyRenderContext};
use crate::action::HistoryManager;
use crate::state::context::EditorContext;

use egui::collapsing_header::CollapsingState;
use egui::Ui;
use library::model::project::connection::PinId;
use library::model::project::ensemble::{DecoratorInstance, EffectorInstance};
use library::model::project::graph_analysis;
use library::model::project::property::PropertyMap;
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Lightweight info about a graph-based ensemble node for UI display.
struct GraphEnsembleInfo {
    node_id: Uuid,
    type_id: String,
    display_name: String,
    properties: PropertyMap,
}

pub fn render_ensemble_section(
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

    // Collect graph-based effectors and decorators
    let (graph_effectors, graph_decorators) = if let Ok(proj) = project.read() {
        let eff_ids = graph_analysis::get_associated_effectors(&proj, selected_entity_id);
        let dec_ids = graph_analysis::get_associated_decorators(&proj, selected_entity_id);

        let collect_info = |ids: Vec<Uuid>| -> Vec<GraphEnsembleInfo> {
            ids.into_iter()
                .filter_map(|node_id| {
                    let node = proj.get_graph_node(node_id)?;
                    let type_id = node.type_id.clone();
                    let display_name = project_service
                        .get_plugin_manager()
                        .get_node_type(&type_id)
                        .map(|def| def.display_name.clone())
                        .unwrap_or_else(|| type_id.clone());
                    Some(GraphEnsembleInfo {
                        node_id,
                        type_id,
                        display_name,
                        properties: node.properties.clone(),
                    })
                })
                .collect()
        };

        (collect_info(eff_ids), collect_info(dec_ids))
    } else {
        (Vec::new(), Vec::new())
    };

    let has_graph_effectors = !graph_effectors.is_empty();
    let has_graph_decorators = !graph_decorators.is_empty();

    // --- Effectors ---
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Effectors").strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            use super::properties::render_add_button;
            render_add_button(ui, |ui| {
                let plugin_manager = project_service.get_plugin_manager();
                for type_name in plugin_manager.get_available_effectors() {
                    let label = plugin_manager
                        .get_effector_plugin(&type_name)
                        .map(|p| p.name())
                        .unwrap_or_else(|| type_name.clone());
                    if ui.button(label).clicked() {
                        let graph_type_id = format!("effector.{}", type_name);
                        match project_service.add_graph_node(track_id, &graph_type_id) {
                            Ok(new_node_id) => {
                                let from = PinId::new(new_node_id, "effector_out");
                                let to = PinId::new(selected_entity_id, "effector_in");
                                if let Err(e) = project_service.add_graph_connection(from, to) {
                                    log::error!("Failed to connect effector: {}", e);
                                }
                                let current_state = project_service.with_project(|p| p.clone());
                                history_manager.push_project_state(current_state);
                                *needs_refresh = true;
                            }
                            Err(e) => {
                                log::error!("Failed to add effector graph node: {}", e);
                            }
                        }
                        ui.close();
                    }
                }
            });
        });
    });

    if has_graph_effectors {
        for eff in &graph_effectors {
            render_graph_ensemble_item(
                ui,
                project_service,
                history_manager,
                selected_entity_id,
                eff,
                current_time,
                fps,
                context,
                needs_refresh,
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
            use super::properties::render_add_button;
            render_add_button(ui, |ui| {
                let plugin_manager = project_service.get_plugin_manager();
                for type_name in plugin_manager.get_available_decorators() {
                    let label = plugin_manager
                        .get_decorator_plugin(&type_name)
                        .map(|p| p.name())
                        .unwrap_or_else(|| type_name.clone());
                    if ui.button(label).clicked() {
                        let graph_type_id = format!("decorator.{}", type_name);
                        match project_service.add_graph_node(track_id, &graph_type_id) {
                            Ok(new_node_id) => {
                                let from = PinId::new(new_node_id, "decorator_out");
                                let to = PinId::new(selected_entity_id, "decorator_in");
                                if let Err(e) = project_service.add_graph_connection(from, to) {
                                    log::error!("Failed to connect decorator: {}", e);
                                }
                                let current_state = project_service.with_project(|p| p.clone());
                                history_manager.push_project_state(current_state);
                                *needs_refresh = true;
                            }
                            Err(e) => {
                                log::error!("Failed to add decorator graph node: {}", e);
                            }
                        }
                        ui.close();
                    }
                }
            });
        });
    });

    if has_graph_decorators {
        for dec in &graph_decorators {
            render_graph_ensemble_item(
                ui,
                project_service,
                history_manager,
                selected_entity_id,
                dec,
                current_time,
                fps,
                context,
                needs_refresh,
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

fn render_graph_ensemble_item(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    clip_id: Uuid,
    item: &GraphEnsembleInfo,
    current_time: f64,
    fps: f64,
    context: &PropertyRenderContext,
    needs_refresh: &mut bool,
) {
    let id = ui.make_persistent_id(format!("graph_ensemble_{}", item.node_id));
    let state = CollapsingState::load_with_default_open(ui.ctx(), id, true);

    let mut remove_clicked = false;
    let header_res = state.show_header(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&item.display_name).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("X").clicked() {
                    remove_clicked = true;
                }
            });
        });
    });

    if remove_clicked {
        if let Err(e) = project_service.remove_graph_node(item.node_id) {
            log::error!("Failed to remove ensemble node: {}", e);
        } else {
            let current_state = project_service.with_project(|p| p.clone());
            history_manager.push_project_state(current_state);
            *needs_refresh = true;
        }
    }

    header_res.body(|ui| {
        let defs = project_service
            .get_plugin_manager()
            .get_node_type(&item.type_id)
            .map(|def| def.default_properties.clone())
            .unwrap_or_default();

        let item_actions = render_inspector_properties_grid(
            ui,
            format!("graph_ensemble_grid_{}", item.node_id),
            &item.properties,
            &defs,
            project_service,
            context,
            fps,
        );

        let item_props = item.properties.clone();
        let mut ctx = ActionContext::new(project_service, history_manager, clip_id, current_time);
        if ctx.handle_actions(item_actions, PropertyTarget::GraphNode(item.node_id), |n| {
            item_props.get(n).cloned()
        }) {
            *needs_refresh = true;
        }
    });
}

// --- Legacy embedded renderers ---

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
