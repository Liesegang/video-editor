use super::action_handler::{ActionContext, PropertyTarget};
use super::properties::{render_inspector_properties_grid, PropertyRenderContext};
use crate::action::HistoryManager;
use crate::state::context::EditorContext;

use egui::collapsing_header::CollapsingState;
use egui::Ui;
use library::model::project::ensemble::{DecoratorInstance, EffectorInstance};
use library::model::project::property::PropertyMap;
use library::EditorService as ProjectService;
use uuid::Uuid;

pub fn render_ensemble_section(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    _editor_context: &EditorContext,
    selected_entity_id: Uuid,
    current_time: f64,
    fps: f64,
    effectors: &Vec<EffectorInstance>,
    decorators: &Vec<DecoratorInstance>,
    needs_refresh: &mut bool,
    _properties: &PropertyMap,
    context: &PropertyRenderContext,
) {
    ui.add_space(10.0);
    ui.heading("Ensemble");
    ui.separator();

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
                        add_effector(
                            &type_name,
                            project_service,
                            history_manager,
                            selected_entity_id,
                            effectors,
                        );
                        ui.close();
                        *needs_refresh = true;
                    }
                }
            });
        });
    });

    let mut local_effectors = effectors.clone();

    crate::ui::widgets::collection_editor::CollectionEditor::new(
        "ensemble_effectors_list",
        &mut local_effectors,
        |e| egui::Id::new(e.id),
        |ui, visual_index, effector, handle, history_manager, project_service, needs_refresh| {
            let backend_index = effectors
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

                // Use ActionContext to handle property updates
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
                        add_decorator(
                            &type_name,
                            project_service,
                            history_manager,
                            selected_entity_id,
                            decorators,
                        );
                        ui.close();
                        *needs_refresh = true;
                    }
                }
            });
        });
    });

    let mut local_decorators = decorators.clone();

    crate::ui::widgets::collection_editor::CollectionEditor::new(
        "ensemble_decorators_list",
        &mut local_decorators,
        |d| egui::Id::new(d.id),
        |ui, visual_index, decorator, handle, history_manager, project_service, needs_refresh| {
            let backend_index = decorators
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

                // Use ActionContext
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

fn add_effector(
    type_name: &str,
    service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    clip_id: Uuid,
    _current_list: &Vec<EffectorInstance>,
) {
    if let Err(e) = service.add_effector(clip_id, type_name) {
        log::error!("Failed to add effector: {}", e);
        return;
    }

    let current_state = service.with_project(|p| p.clone());
    history_manager.push_project_state(current_state);
}

fn add_decorator(
    type_name: &str,
    service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    clip_id: Uuid,
    _current_list: &Vec<DecoratorInstance>,
) {
    if let Err(e) = service.add_decorator(clip_id, type_name) {
        log::error!("Failed to add decorator: {}", e);
        return;
    }

    let current_state = service.with_project(|p| p.clone());
    history_manager.push_project_state(current_state);
}
