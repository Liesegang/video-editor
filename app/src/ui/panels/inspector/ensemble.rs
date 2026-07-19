use super::action_handler::{ActionContext, PropertyTarget};
use super::properties::{PropertyRenderContext, render_inspector_properties_grid};
use crate::action::HistoryManager;
use crate::state::context::EditorContext;
use egui::Ui;
use egui::collapsing_header::CollapsingState;
use library::EditorService as ProjectService;
use library::PropertyOwner;
use library::model::ensemble::{DecoratorInstance, EffectorInstance};
use library::plugin::{EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY, PluginManager};
use uuid::Uuid;

#[allow(
    clippy::too_many_arguments,
    reason = "ensemble rendering coordinates editor state, model services, timing, and refresh state"
)]
pub fn render_ensemble_section(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    editor_context: &EditorContext,
    node_id: Uuid,
    current_time: f64,
    fps: f64,
    effectors: &[EffectorInstance],
    decorators: &[DecoratorInstance],
    needs_refresh: &mut bool,
) {
    ui.add_space(10.0);
    ui.heading("Ensemble");
    ui.separator();

    render_effectors(
        ui,
        project_service,
        history_manager,
        editor_context,
        node_id,
        current_time,
        fps,
        effectors,
        needs_refresh,
    );
    ui.separator();
    render_decorators(
        ui,
        project_service,
        history_manager,
        editor_context,
        node_id,
        current_time,
        fps,
        decorators,
        needs_refresh,
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "effector rendering needs owner, model, timing, history, and refresh context"
)]
fn render_effectors(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    editor_context: &EditorContext,
    node_id: Uuid,
    current_time: f64,
    fps: f64,
    effectors: &[EffectorInstance],
    needs_refresh: &mut bool,
) {
    let plugin_manager = project_service.get_plugin_manager();
    let mut available = plugin_manager
        .get_available_effectors()
        .into_iter()
        .filter_map(|type_name| {
            match plugin_manager.operation_descriptor(
                EFFECTOR_CATEGORY,
                &type_name,
                EFFECTOR_APPLY_OPERATION,
            ) {
                Ok(descriptor) => Some((type_name, descriptor.label().to_string())),
                Err(error) => {
                    log::warn!("Cannot expose Effector {type_name} in Inspector: {error}");
                    None
                }
            }
        })
        .collect::<Vec<_>>();
    available.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Effectors").strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let menu = ui.menu_button("➕ Add", |ui| {
                for (type_name, label) in &available {
                    let response = ui.button(label);
                    crate::qa::register_component_with_metadata(
                        format!("inspector.ensemble.menu.add_effector.{type_name}"),
                        "inspector_menu_item",
                        response.rect,
                        response.enabled(),
                        Some(serde_json::json!({
                            "node_id": node_id,
                            "effector_type": type_name,
                            "label": label,
                            "category": EFFECTOR_CATEGORY,
                            "operation": EFFECTOR_APPLY_OPERATION,
                        })),
                    );
                    if response.clicked() {
                        match project_service.add_effector(node_id, type_name) {
                            Ok(()) => {
                                push_history(project_service, history_manager);
                                *needs_refresh = true;
                            }
                            Err(error) => log::error!("Failed to add Effector: {error}"),
                        }
                        ui.close();
                    }
                }
            });
            crate::qa::register_component(
                format!("inspector.ensemble.add_effector:{node_id}"),
                "inspector_add_button",
                menu.response.rect,
            );
        });
    });

    let original_effectors = effectors.to_vec();
    let mut local_effectors = original_effectors.clone();
    crate::ui::widgets::collection_editor::CollectionEditor::new(
        egui::Id::new(("ensemble_effectors", node_id)),
        &mut local_effectors,
        |effector| egui::Id::new(effector.id),
        |ui, visual_index, effector, handle, history_manager, project_service, needs_refresh| {
            let backend_index = original_effectors
                .iter()
                .position(|candidate| candidate.id == effector.id)
                .unwrap_or(visual_index);
            let state = CollapsingState::load_with_default_open(
                ui.ctx(),
                ui.make_persistent_id(("ensemble_effector", effector.id)),
                true,
            );
            let mut remove_clicked = false;
            let header = state.show_header(ui, |ui| {
                ui.horizontal(|ui| {
                    let handle_response = handle.ui(ui, |ui| {
                        ui.label("::");
                    });
                    crate::qa::register_component_with_metadata(
                        format!("inspector.ensemble.effector_handle:{}", effector.id),
                        "inspector_collection_drag_handle",
                        handle_response.rect,
                        handle_response.enabled(),
                        Some(serde_json::json!({
                            "node_id": node_id,
                            "instance_id": effector.id,
                            "index": backend_index,
                        })),
                    );
                    ui.label(
                        egui::RichText::new(effector_display_label(
                            project_service.get_plugin_manager().as_ref(),
                            &effector.effector_type,
                        ))
                        .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let response = ui.button("X");
                        crate::qa::register_component(
                            format!("inspector.ensemble.delete_effector:{}", effector.id),
                            "inspector_delete_button",
                            response.rect,
                        );
                        remove_clicked = response.clicked();
                    });
                });
            });

            let (_, header_response, _) = header.body(|ui| {
                let definitions = project_service
                    .get_plugin_manager()
                    .get_effector_properties(&effector.effector_type);
                let context = PropertyRenderContext {
                    available_fonts: &editor_context.available_fonts,
                    in_grid: true,
                    current_time,
                    qa_scope: format!("node:{node_id}.effector:{}", effector.id),
                };
                let actions = render_inspector_properties_grid(
                    ui,
                    ("effector_properties", effector.id),
                    &effector.properties,
                    &definitions,
                    project_service,
                    &context,
                    fps,
                );
                let properties = effector.properties.clone();
                let mut action_context = ActionContext::new(
                    project_service,
                    history_manager,
                    PropertyOwner::Node(node_id),
                    current_time,
                );
                if action_context.handle_actions(
                    actions,
                    PropertyTarget::Effector(effector.id),
                    |name| properties.get(name).cloned(),
                ) {
                    *needs_refresh = true;
                }
            });
            crate::qa::register_component_with_metadata(
                format!("inspector.ensemble.effector:{}", effector.id),
                "inspector_ensemble_item",
                header_response.response.rect,
                true,
                Some(serde_json::json!({
                    "node_id": node_id,
                    "instance_id": effector.id,
                    "index": backend_index,
                    "type": effector.effector_type,
                })),
            );
            remove_clicked
        },
        |effectors, project_service| project_service.update_node_effectors(node_id, effectors),
    )
    .show(ui, history_manager, project_service, needs_refresh);
}

fn effector_display_label(plugin_manager: &PluginManager, component_id: &str) -> String {
    plugin_manager
        .operation_descriptor(EFFECTOR_CATEGORY, component_id, EFFECTOR_APPLY_OPERATION)
        .map(|descriptor| descriptor.label().to_string())
        // An unavailable plugin must not make an older/foreign Project
        // uninspectable. Its persisted identity remains a useful lossless
        // label until the matching runtime plugin is installed again.
        .unwrap_or_else(|_| component_id.to_string())
}

#[allow(
    clippy::too_many_arguments,
    reason = "decorator rendering needs owner, model, timing, history, and refresh context"
)]
fn render_decorators(
    ui: &mut Ui,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    editor_context: &EditorContext,
    node_id: Uuid,
    current_time: f64,
    fps: f64,
    decorators: &[DecoratorInstance],
    needs_refresh: &mut bool,
) {
    let available = project_service
        .get_plugin_manager()
        .get_available_decorators()
        .into_iter()
        .map(|type_name| {
            let label = project_service
                .get_plugin_manager()
                .get_decorator_plugin(&type_name)
                .map(|plugin| plugin.name())
                .unwrap_or_else(|| type_name.clone());
            (type_name, label)
        })
        .collect::<Vec<_>>();

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Decorators").strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let menu = ui.menu_button("➕ Add", |ui| {
                for (type_name, label) in &available {
                    let response = ui.button(label);
                    crate::qa::register_component_with_metadata(
                        format!("inspector.ensemble.menu.add_decorator.{type_name}"),
                        "inspector_menu_item",
                        response.rect,
                        response.enabled(),
                        Some(serde_json::json!({
                            "node_id": node_id,
                            "decorator_type": type_name,
                        })),
                    );
                    if response.clicked() {
                        match project_service.add_decorator(node_id, type_name) {
                            Ok(()) => {
                                push_history(project_service, history_manager);
                                *needs_refresh = true;
                            }
                            Err(error) => log::error!("Failed to add Decorator: {error}"),
                        }
                        ui.close();
                    }
                }
            });
            crate::qa::register_component(
                format!("inspector.ensemble.add_decorator:{node_id}"),
                "inspector_add_button",
                menu.response.rect,
            );
        });
    });

    let original_decorators = decorators.to_vec();
    let mut local_decorators = original_decorators.clone();
    crate::ui::widgets::collection_editor::CollectionEditor::new(
        egui::Id::new(("ensemble_decorators", node_id)),
        &mut local_decorators,
        |decorator| egui::Id::new(decorator.id),
        |ui, visual_index, decorator, handle, history_manager, project_service, needs_refresh| {
            let backend_index = original_decorators
                .iter()
                .position(|candidate| candidate.id == decorator.id)
                .unwrap_or(visual_index);
            let state = CollapsingState::load_with_default_open(
                ui.ctx(),
                ui.make_persistent_id(("ensemble_decorator", decorator.id)),
                true,
            );
            let mut remove_clicked = false;
            let header = state.show_header(ui, |ui| {
                ui.horizontal(|ui| {
                    let handle_response = handle.ui(ui, |ui| {
                        ui.label("::");
                    });
                    crate::qa::register_component_with_metadata(
                        format!("inspector.ensemble.decorator_handle:{}", decorator.id),
                        "inspector_collection_drag_handle",
                        handle_response.rect,
                        handle_response.enabled(),
                        Some(serde_json::json!({
                            "node_id": node_id,
                            "instance_id": decorator.id,
                            "index": backend_index,
                        })),
                    );
                    ui.label(
                        egui::RichText::new(
                            project_service
                                .get_plugin_manager()
                                .get_decorator_plugin(&decorator.decorator_type)
                                .map(|plugin| plugin.name())
                                .unwrap_or_else(|| decorator.decorator_type.clone()),
                        )
                        .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let response = ui.button("X");
                        crate::qa::register_component(
                            format!("inspector.ensemble.delete_decorator:{}", decorator.id),
                            "inspector_delete_button",
                            response.rect,
                        );
                        remove_clicked = response.clicked();
                    });
                });
            });

            let (_, header_response, _) = header.body(|ui| {
                let definitions = project_service
                    .get_plugin_manager()
                    .get_decorator_properties(&decorator.decorator_type);
                let context = PropertyRenderContext {
                    available_fonts: &editor_context.available_fonts,
                    in_grid: true,
                    current_time,
                    qa_scope: format!("node:{node_id}.decorator:{}", decorator.id),
                };
                let actions = render_inspector_properties_grid(
                    ui,
                    ("decorator_properties", decorator.id),
                    &decorator.properties,
                    &definitions,
                    project_service,
                    &context,
                    fps,
                );
                let properties = decorator.properties.clone();
                let mut action_context = ActionContext::new(
                    project_service,
                    history_manager,
                    PropertyOwner::Node(node_id),
                    current_time,
                );
                if action_context.handle_actions(
                    actions,
                    PropertyTarget::Decorator(decorator.id),
                    |name| properties.get(name).cloned(),
                ) {
                    *needs_refresh = true;
                }
            });
            crate::qa::register_component_with_metadata(
                format!("inspector.ensemble.decorator:{}", decorator.id),
                "inspector_ensemble_item",
                header_response.response.rect,
                true,
                Some(serde_json::json!({
                    "node_id": node_id,
                    "instance_id": decorator.id,
                    "index": backend_index,
                    "type": decorator.decorator_type,
                })),
            );
            remove_clicked
        },
        |decorators, project_service| project_service.update_node_decorators(node_id, decorators),
    )
    .show(ui, history_manager, project_service, needs_refresh);
}

fn push_history(project_service: &ProjectService, history_manager: &mut HistoryManager) {
    if let Ok(project) = project_service.get_project().read() {
        history_manager.push_project_state(project.clone());
    }
}
