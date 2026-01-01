use super::action_handler::{ActionContext, PropertyTarget};
use super::properties::{render_property_rows, PropertyAction, PropertyRenderContext};
use crate::action::HistoryManager;
use crate::state::context::EditorContext;
use crate::ui::widgets::reorderable_list::ReorderableList;
use egui::collapsing_header::CollapsingState;
use egui::Ui;
use library::model::project::ensemble::{DecoratorInstance, EffectorInstance};
use library::model::project::property::PropertyMap;
use library::model::project::property::{PropertyDefinition, PropertyUiType};
use library::EditorService as ProjectService;
use uuid::Uuid;

pub fn render_ensemble_section(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    _editor_context: &EditorContext,
    comp_id: Uuid,
    track_id: Uuid,
    selected_entity_id: Uuid,
    current_time: f64,
    fps: f64,
    effectors: &Vec<EffectorInstance>,
    decorators: &Vec<DecoratorInstance>,
    needs_refresh: &mut bool,
    _properties: &PropertyMap,
    context: &PropertyRenderContext,
) -> Vec<PropertyAction> {
    let actions = vec![];

    ui.add_space(5.0);
    CollapsingHeader::new(egui::RichText::new("Ensemble").strong())
        .default_open(true)
        .show(ui, |ui| {
            // --- Effectors ---
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Effectors").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button("➕ Add", |ui| {
                        if ui.button("Transform").clicked() {
                            add_effector(
                                "transform",
                                project_service,
                                comp_id,
                                track_id,
                                selected_entity_id,
                                effectors,
                            );
                            ui.close();
                            *needs_refresh = true;
                        }
                        if ui.button("Step Delay").clicked() {
                            add_effector(
                                "step_delay",
                                project_service,
                                comp_id,
                                track_id,
                                selected_entity_id,
                                effectors,
                            );
                            ui.close();
                            *needs_refresh = true;
                        }
                        if ui.button("Randomize").clicked() {
                            add_effector(
                                "randomize",
                                project_service,
                                comp_id,
                                track_id,
                                selected_entity_id,
                                effectors,
                            );
                            ui.close();
                            *needs_refresh = true;
                        }
                    });
                });
            });

            let mut local_effectors = effectors.clone();
            let mut effectors_changed = false;
            let mut effector_delete_idx = None;
            let _effector_reorder_occurred = false;

            ReorderableList::new("ensemble_effectors_list", &mut local_effectors).show(
                ui,
                |ui, visual_index, effector, handle| {
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
                                egui::RichText::new(format_type_name(&effector.effector_type))
                                    .strong(),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("X").clicked() {
                                        remove_clicked = true;
                                    }
                                },
                            );
                        });
                    });

                    if remove_clicked {
                        effector_delete_idx = Some(visual_index);
                    }

                    header_res.body(|ui| {
                        let defs = get_effector_definitions(&effector.effector_type);

                        let item_actions = render_property_rows(
                            ui,
                            &defs,
                            |name| {
                                effector.properties.get(name).and_then(|p| {
                                    Some(project_service.evaluate_property_value(
                                        p,
                                        &effector.properties,
                                        current_time,
                                        fps,
                                    ))
                                })
                            },
                            |name| effector.properties.get(name).cloned(),
                            context,
                        );

                        // Use ActionContext to handle property updates
                        let effector_props = effector.properties.clone();
                        let mut ctx = ActionContext::new(
                            project_service,
                            history_manager,
                            comp_id,
                            track_id,
                            selected_entity_id,
                            current_time,
                        );

                        for action in item_actions {
                            match action {
                                PropertyAction::Update(key, value) => {
                                    ctx.handle_update(
                                        PropertyTarget::Effector(backend_index),
                                        &key,
                                        value,
                                        |n| effector_props.get(n).cloned(),
                                    );
                                    *needs_refresh = true;
                                }
                                PropertyAction::ToggleKeyframe(key, value) => {
                                    ctx.handle_toggle_keyframe(
                                        PropertyTarget::Effector(backend_index),
                                        &key,
                                        value,
                                        |n| effector_props.get(n).cloned(),
                                    );
                                    *needs_refresh = true;
                                }
                                PropertyAction::SetAttribute(key, attr, value) => {
                                    ctx.handle_set_attribute(
                                        PropertyTarget::Effector(backend_index),
                                        &key,
                                        &attr,
                                        value,
                                    );
                                    *needs_refresh = true;
                                }
                                PropertyAction::Commit => {
                                    ctx.handle_commit();
                                }
                            }
                        }
                    });
                },
            );

            if let Some(idx) = effector_delete_idx {
                local_effectors.remove(idx);
                effectors_changed = true;
            } else {
                // Check reordering by ID comparison
                let local_ids: Vec<Uuid> = local_effectors.iter().map(|e| e.id).collect();
                let current_ids: Vec<Uuid> = effectors.iter().map(|e| e.id).collect();
                if local_ids != current_ids {
                    effectors_changed = true;
                }
            }

            if effectors_changed {
                project_service
                    .update_track_clip_effectors(
                        comp_id,
                        track_id,
                        selected_entity_id,
                        local_effectors,
                    )
                    .ok();
                *needs_refresh = true;
            }

            ui.separator();

            // --- Decorators ---
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Decorators").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button("➕ Add", |ui| {
                        if ui.button("Backplate").clicked() {
                            add_decorator(
                                "backplate",
                                project_service,
                                comp_id,
                                track_id,
                                selected_entity_id,
                                decorators,
                            );
                            ui.close();
                            *needs_refresh = true;
                        }
                    });
                });
            });

            let mut local_decorators = decorators.clone();
            let mut decorators_changed = false;
            let mut decorator_delete_idx = None;

            ReorderableList::new("ensemble_decorators_list", &mut local_decorators).show(
                ui,
                |ui, visual_index, decorator, handle| {
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
                                egui::RichText::new(format_type_name(&decorator.decorator_type))
                                    .strong(),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("X").clicked() {
                                        remove_clicked = true;
                                    }
                                },
                            );
                        });
                    });

                    if remove_clicked {
                        decorator_delete_idx = Some(visual_index);
                    }

                    header_res.body(|ui| {
                        let defs = get_decorator_definitions(&decorator.decorator_type);

                        let item_actions = render_property_rows(
                            ui,
                            &defs,
                            |name| {
                                decorator.properties.get(name).and_then(|p| {
                                    Some(project_service.evaluate_property_value(
                                        p,
                                        &decorator.properties,
                                        current_time,
                                        fps,
                                    ))
                                })
                            },
                            |name| decorator.properties.get(name).cloned(),
                            context,
                        );

                        // Use ActionContext
                        let decorator_props = decorator.properties.clone();
                        let mut ctx = ActionContext::new(
                            project_service,
                            history_manager,
                            comp_id,
                            track_id,
                            selected_entity_id,
                            current_time,
                        );

                        for action in item_actions {
                            match action {
                                PropertyAction::Update(key, value) => {
                                    ctx.handle_update(
                                        PropertyTarget::Decorator(backend_index),
                                        &key,
                                        value,
                                        |n| decorator_props.get(n).cloned(),
                                    );
                                    *needs_refresh = true;
                                }
                                PropertyAction::ToggleKeyframe(key, value) => {
                                    ctx.handle_toggle_keyframe(
                                        PropertyTarget::Decorator(backend_index),
                                        &key,
                                        value,
                                        |n| decorator_props.get(n).cloned(),
                                    );
                                    *needs_refresh = true;
                                }
                                PropertyAction::SetAttribute(key, attr, value) => {
                                    ctx.handle_set_attribute(
                                        PropertyTarget::Decorator(backend_index),
                                        &key,
                                        &attr,
                                        value,
                                    );
                                    *needs_refresh = true;
                                }
                                PropertyAction::Commit => {
                                    ctx.handle_commit();
                                }
                            }
                        }
                    });
                },
            );

            if let Some(idx) = decorator_delete_idx {
                local_decorators.remove(idx);
                decorators_changed = true;
            } else {
                let local_ids: Vec<Uuid> = local_decorators.iter().map(|d| d.id).collect();
                let current_ids: Vec<Uuid> = decorators.iter().map(|d| d.id).collect();
                if local_ids != current_ids {
                    decorators_changed = true;
                }
            }

            if decorators_changed {
                project_service
                    .update_track_clip_decorators(
                        comp_id,
                        track_id,
                        selected_entity_id,
                        local_decorators,
                    )
                    .ok();
                *needs_refresh = true;
            }
        });

    actions
}

use egui::collapsing_header::CollapsingHeader;

fn format_type_name(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn add_effector(
    type_name: &str,
    service: &mut ProjectService,
    comp_id: Uuid,
    track_id: Uuid,
    clip_id: Uuid,
    current_list: &Vec<EffectorInstance>,
) {
    let mut new_list = current_list.clone();
    new_list.push(EffectorInstance::default_of_type(type_name));
    service
        .update_track_clip_effectors(comp_id, track_id, clip_id, new_list)
        .ok();
}

fn add_decorator(
    type_name: &str,
    service: &mut ProjectService,
    comp_id: Uuid,
    track_id: Uuid,
    clip_id: Uuid,
    current_list: &Vec<DecoratorInstance>,
) {
    let mut new_list = current_list.clone();
    new_list.push(DecoratorInstance::default_of_type(type_name));
    service
        .update_track_clip_decorators(comp_id, track_id, clip_id, new_list)
        .ok();
}

fn get_effector_definitions(type_name: &str) -> Vec<PropertyDefinition> {
    match type_name {
        "transform" => vec![
            PropertyDefinition::new(
                "tx",
                PropertyUiType::Float {
                    min: -1000.0,
                    max: 1000.0,
                    step: 1.0,
                    suffix: "px".into(),
                },
                "Translate X",
            ),
            PropertyDefinition::new(
                "ty",
                PropertyUiType::Float {
                    min: -1000.0,
                    max: 1000.0,
                    step: 1.0,
                    suffix: "px".into(),
                },
                "Translate Y",
            ),
            PropertyDefinition::new(
                "scale_x",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 10.0,
                    step: 0.1,
                    suffix: "".into(),
                },
                "Scale X",
            ),
            PropertyDefinition::new(
                "scale_y",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 10.0,
                    step: 0.1,
                    suffix: "".into(),
                },
                "Scale Y",
            ),
            PropertyDefinition::new(
                "rotation",
                PropertyUiType::Float {
                    min: -360.0,
                    max: 360.0,
                    step: 1.0,
                    suffix: "°".into(),
                },
                "Rotation",
            ),
        ],
        "step_delay" => vec![
            PropertyDefinition::new(
                "delay",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 5.0,
                    step: 0.05,
                    suffix: "s".into(),
                },
                "Delay per Char",
            ),
            PropertyDefinition::new(
                "duration",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 5.0,
                    step: 0.05,
                    suffix: "s".into(),
                },
                "Duration",
            ),
        ],
        "randomize" => vec![
            PropertyDefinition::new(
                "seed",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: "".into(),
                },
                "Seed",
            ),
            PropertyDefinition::new(
                "amount",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 1.0,
                    step: 0.01,
                    suffix: "".into(),
                },
                "Amount",
            ),
        ],
        _ => vec![],
    }
}

fn get_decorator_definitions(type_name: &str) -> Vec<PropertyDefinition> {
    match type_name {
        "backplate" => vec![
            PropertyDefinition::new("color", PropertyUiType::Color, "Color"),
            PropertyDefinition::new(
                "padding",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: "px".into(),
                },
                "Padding",
            ),
            PropertyDefinition::new(
                "radius",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 50.0,
                    step: 1.0,
                    suffix: "px".into(),
                },
                "Corner Radius",
            ),
        ],
        _ => vec![],
    }
}
