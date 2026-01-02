use super::action_handler::{ActionContext, PropertyTarget};
use super::properties::{render_inspector_properties_grid, PropertyRenderContext};
use crate::action::HistoryManager;
use crate::state::context::EditorContext;

use egui::collapsing_header::CollapsingState;
use egui::Ui;
use library::model::property::PropertyMap;
use library::model::style::StyleInstance;
use library::model::EffectConfig;
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
    effects: &Vec<EffectConfig>,
    styles: &Vec<StyleInstance>,
    needs_refresh: &mut bool,
    _properties: &PropertyMap,
    context: &PropertyRenderContext,
) {
    ui.add_space(10.0);
    ui.heading("Ensemble");
    ui.separator();

    // --- Effectors ---
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Effects").strong());
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
                        add_effect(
                            &type_name,
                            project_service,
                            history_manager,
                            selected_entity_id,
                            effects,
                        );
                        ui.close();
                        *needs_refresh = true;
                    }
                }
            });
        });
    });

    let mut local_effects = effects.clone();

    crate::ui::widgets::collection_editor::CollectionEditor::new(
        "ensemble_effects_list",
        &mut local_effects,
        |e| egui::Id::new(e.id),
        |ui, visual_index, effect, handle, history_manager, project_service, needs_refresh| {
            let backend_index = effects
                .iter()
                .position(|e| e.id == effect.id)
                .unwrap_or(visual_index);

            let id = ui.make_persistent_id(format!("effect_{}", effect.id));
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
                                .get_effector_plugin(&effect.effect_type)
                                .map(|p| p.name())
                                .unwrap_or_else(|| effect.effect_type.clone()),
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
                    .get_effector_properties(&effect.effect_type);

                let item_actions = render_inspector_properties_grid(
                    ui,
                    format!("effect_grid_{}", effect.id),
                    &effect.properties,
                    &defs,
                    project_service,
                    context,
                    fps,
                );

                // Use ActionContext to handle property updates
                let effect_props = effect.properties.clone();
                let mut ctx = ActionContext::new(
                    project_service,
                    history_manager,
                    selected_entity_id,
                    current_time,
                );

                if ctx.handle_actions(item_actions, PropertyTarget::Effector(backend_index), |n| {
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

    ui.separator();

    // --- Styles ---
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Styles").strong());
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
                        add_style(
                            &type_name,
                            project_service,
                            history_manager,
                            selected_entity_id,
                            styles,
                        );
                        ui.close();
                        *needs_refresh = true;
                    }
                }
            });
        });
    });

    let mut local_styles = styles.clone();

    crate::ui::widgets::collection_editor::CollectionEditor::new(
        "ensemble_styles_list",
        &mut local_styles,
        |d| egui::Id::new(d.id),
        |ui, visual_index, style, handle, history_manager, project_service, needs_refresh| {
            let backend_index = styles
                .iter()
                .position(|d| d.id == style.id)
                .unwrap_or(visual_index);

            let id = ui.make_persistent_id(format!("style_{}", style.id));
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
                                .get_decorator_plugin(&style.style_type)
                                .map(|p| p.name())
                                .unwrap_or_else(|| style.style_type.clone()),
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
                    .get_decorator_properties(&style.style_type);

                let item_actions = render_inspector_properties_grid(
                    ui,
                    format!("style_grid_{}", style.id),
                    &style.properties,
                    &defs,
                    project_service,
                    context,
                    fps,
                );

                // Use ActionContext
                let style_props = style.properties.clone();
                let mut ctx = ActionContext::new(
                    project_service,
                    history_manager,
                    selected_entity_id,
                    current_time,
                );

                if ctx.handle_actions(
                    item_actions,
                    PropertyTarget::Decorator(backend_index),
                    |n| style_props.get(n).cloned(),
                ) {
                    *needs_refresh = true;
                }
            });

            remove_clicked
        },
        |new_styles, project_service| {
            project_service.update_track_clip_styles(selected_entity_id, new_styles)
        },
    )
    .show(ui, history_manager, project_service, needs_refresh);
}

fn add_effect(
    type_name: &str,
    service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    clip_id: Uuid,
    _current_list: &Vec<EffectConfig>,
) {
    if let Err(e) = service.add_effect_to_clip(clip_id, type_name) {
        log::error!("Failed to add effect: {}", e);
        return;
    }

    let current_state = service.get_project().read().unwrap().clone();
    history_manager.push_project_state(current_state);
}

fn add_style(
    type_name: &str,
    service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    clip_id: Uuid,
    _current_list: &Vec<StyleInstance>,
) {
    if let Err(e) = service.add_style(clip_id, type_name) {
        log::error!("Failed to add style: {}", e);
        return;
    }

    let current_state = service.get_project().read().unwrap().clone();
    history_manager.push_project_state(current_state);
}
