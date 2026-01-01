use super::action_handler::{ActionContext, PropertyTarget};
use super::properties::{render_inspector_properties_grid, PropertyRenderContext};
use crate::action::HistoryManager;
use crate::state::context::EditorContext;

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
                if ui.button("Transform").clicked() {
                    add_effector(
                        "transform",
                        project_service,
                        history_manager,
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
                        history_manager,
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
                        history_manager,
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
                        egui::RichText::new(format_type_name(&effector.effector_type)).strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("X").clicked() {
                            remove_clicked = true;
                        }
                    });
                });
            });

            header_res.body(|ui| {
                let defs = get_effector_definitions(&effector.effector_type);

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
                if ui.button("Backplate").clicked() {
                    add_decorator(
                        "backplate",
                        project_service,
                        history_manager,
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
                        egui::RichText::new(format_type_name(&decorator.decorator_type)).strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("X").clicked() {
                            remove_clicked = true;
                        }
                    });
                });
            });

            header_res.body(|ui| {
                let defs = get_decorator_definitions(&decorator.decorator_type);

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
    history_manager: &mut HistoryManager,
    clip_id: Uuid,
    current_list: &Vec<EffectorInstance>,
) {
    let mut new_list = current_list.clone();
    new_list.push(EffectorInstance::default_of_type(type_name));
    service.update_track_clip_effectors(clip_id, new_list).ok();

    let current_state = service.get_project().read().unwrap().clone();
    history_manager.push_project_state(current_state);
}

fn add_decorator(
    type_name: &str,
    service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    clip_id: Uuid,
    current_list: &Vec<DecoratorInstance>,
) {
    let mut new_list = current_list.clone();
    new_list.push(DecoratorInstance::default_of_type(type_name));
    service.update_track_clip_decorators(clip_id, new_list).ok();

    let current_state = service.get_project().read().unwrap().clone();
    history_manager.push_project_state(current_state);
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
            PropertyDefinition::new(
                "from_opacity",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: "%".into(),
                },
                "From Opacity",
            ),
            PropertyDefinition::new(
                "to_opacity",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: "%".into(),
                },
                "To Opacity",
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
            PropertyDefinition::new(
                "translate_range",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 500.0,
                    step: 1.0,
                    suffix: "px".into(),
                },
                "Translate Range",
            ),
            PropertyDefinition::new(
                "rotate_range",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 360.0,
                    step: 1.0,
                    suffix: "deg".into(),
                },
                "Rotate Range",
            ),
        ],
        _ => vec![],
    }
}

fn get_decorator_definitions(type_name: &str) -> Vec<PropertyDefinition> {
    match type_name {
        "backplate" => vec![
            PropertyDefinition::new(
                "target",
                PropertyUiType::Dropdown {
                    options: vec!["Char".to_string(), "Line".to_string(), "Block".to_string()],
                },
                "Target",
            ),
            PropertyDefinition::new(
                "shape",
                PropertyUiType::Dropdown {
                    options: vec![
                        "Rect".to_string(),
                        "RoundRect".to_string(),
                        "Circle".to_string(),
                    ],
                },
                "Shape",
            ),
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
