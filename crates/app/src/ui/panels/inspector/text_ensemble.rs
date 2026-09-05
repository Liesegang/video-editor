use egui_phosphor::regular as icons;
use library::editor::{
    AuthoringPropertyOwner, AuthoringPropertyValueTarget, AuthoringPropertyValueUpdate,
    TextEnsembleOperationKind, TimelineEditorService,
};
use library::model::authoring::{
    text_ensemble_direct_contract_is_compatible, AuthoringProject, ProjectPalette,
    TextEnsembleOperation, TimelineItem,
};
use library::plugin::{
    OperationDescriptor, PluginManager, DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY,
    EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY,
};

use crate::state::authoring::{AuthoringUiState, TransientPropertyEdit};
use crate::ui::widgets::property_mode::PropertyModeState;
use crate::ui::widgets::searchable_context_menu::{
    searchable_menu_button, show_searchable_items_with_qa, SearchableItem,
};

#[derive(Clone)]
struct AddOperation {
    kind: TextEnsembleOperationKind,
    component_id: String,
    label: String,
}

pub(super) fn text_ensemble_section(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    item: &TimelineItem,
    operations: &[TextEnsembleOperation],
) {
    ui.separator();
    let response = egui::CollapsingHeader::new("Text Ensemble")
        .default_open(true)
        .show(ui, |ui| {
            add_menu(ui, state, service, plugins, item);
            if operations.is_empty() {
                ui.weak("Add an Effector or Decorator to animate grouped text elements.");
            }
            let local_time = super::item_local_time(project, state, item).ok();
            for (category, heading) in [
                (EFFECTOR_CATEGORY, "Effectors"),
                (DECORATOR_CATEGORY, "Decorators"),
            ] {
                let phase_indices = operations
                    .iter()
                    .enumerate()
                    .filter_map(|(index, operation)| {
                        (operation.operation.category == category).then_some(index)
                    })
                    .collect::<Vec<_>>();
                if !phase_indices.is_empty() {
                    ui.add_space(6.0);
                    ui.strong(heading);
                }
                for (phase_index, model_index) in phase_indices.iter().copied().enumerate() {
                    operation_entry(
                        ui,
                        &project.palette,
                        state,
                        service,
                        plugins,
                        item,
                        &operations[model_index],
                        model_index,
                        phase_index,
                        phase_indices.len(),
                        phase_index
                            .checked_sub(1)
                            .and_then(|index| phase_indices.get(index).copied()),
                        phase_indices.get(phase_index + 1).copied(),
                        local_time,
                    );
                }
            }
        });
    crate::qa::register_component_with_metadata(
        format!("inspector.text_ensemble:{}", item.id),
        "inspector_text_ensemble",
        response.header_response.rect,
        true,
        Some(serde_json::json!({
            "item_id": item.id,
            "operation_count": operations.len(),
            "operations": operations.iter().map(|operation| serde_json::json!({
                "id": operation.id,
                "category": operation.operation.category,
                "component_id": operation.operation.component_id,
                "operation": operation.operation.operation,
                "version": operation.operation.version,
            })).collect::<Vec<_>>(),
        })),
    );
}

fn add_menu(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    item: &TimelineItem,
) {
    let mut items = operation_catalog(plugins, item.id);
    let node_graph_decorators = plugins
        .get_available_decorators()
        .into_iter()
        .filter(|component_id| {
            plugins
                .operation_descriptor(DECORATOR_CATEGORY, component_id, DECORATOR_APPLY_OPERATION)
                .is_ok_and(|descriptor| {
                    !text_ensemble_direct_contract_is_compatible(descriptor.declared_ports())
                })
        })
        .count();
    items.sort_by(|left, right| {
        left.category
            .cmp(&right.category)
            .then_with(|| left.label.cmp(&right.label))
    });
    let menu_id = format!("inspector.text_ensemble.menu:{}", item.id);
    let menu = searchable_menu_button(ui, format!("{} Add operation", icons::PLUS), |ui| {
        ui.set_min_width(290.0);
        ui.set_min_height(240.0_f32.min(ui.available_height().max(0.0)));
        show_searchable_items_with_qa(ui, &menu_id, Some(&format!("{menu_id}.query")), &items)
    });
    if node_graph_decorators > 0 {
        menu.response.clone().on_hover_text(
            "Decorators that need additional Shape inputs are available in the Node Editor",
        );
    }
    crate::qa::register_component_with_metadata(
        format!("inspector.text_ensemble.add_menu:{}", item.id),
        "inspector_add_button",
        menu.response.rect,
        menu.response.enabled(),
        Some(serde_json::json!({
            "item_id": item.id,
            "descriptor_count": items.len(),
            "node_graph_decorator_count": node_graph_decorators,
            "descriptor_driven": true,
            "browse_mode": "hierarchical_accordion",
            "search_mode": "flat",
        })),
    );
    if let Some(Some(selected)) = menu.inner {
        let family = match selected.kind {
            TextEnsembleOperationKind::Effector => "Effector",
            TextEnsembleOperationKind::Decorator => "Decorator",
        };
        match service.add_text_ensemble_operation_by_id(
            plugins,
            item.id,
            selected.kind,
            &selected.component_id,
        ) {
            Ok(_) => state.status = format!("Added Text {family} {}", selected.label),
            Err(error) => state.error = Some(error.to_string()),
        }
    }
}

fn operation_catalog(
    plugins: &PluginManager,
    item_id: library::model::authoring::TimelineItemId,
) -> Vec<SearchableItem<AddOperation>> {
    [
        (
            TextEnsembleOperationKind::Effector,
            "Effector",
            EFFECTOR_CATEGORY,
            EFFECTOR_APPLY_OPERATION,
            plugins.get_available_effectors(),
        ),
        (
            TextEnsembleOperationKind::Decorator,
            "Decorator",
            DECORATOR_CATEGORY,
            DECORATOR_APPLY_OPERATION,
            plugins.get_available_decorators(),
        ),
    ]
    .into_iter()
    .flat_map(|(kind, family, category, operation, component_ids)| {
        component_ids.into_iter().filter_map(move |component_id| {
            let descriptor = plugins
                .text_ensemble_operation_descriptor(category, &component_id)
                .ok()?;
            if !text_ensemble_direct_contract_is_compatible(descriptor.declared_ports()) {
                return None;
            }
            let label = descriptor.label().to_string();
            let mut item = SearchableItem::new(
                label.clone(),
                AddOperation {
                    kind,
                    component_id: component_id.clone(),
                    label,
                },
            );
            item.category = Some(family.to_string());
            item.keywords = vec![
                family.to_string(),
                category.to_string(),
                component_id.clone(),
            ];
            item.qa_id = Some(format!(
                "inspector.text_ensemble.add.{category}:{component_id}"
            ));
            item.qa_metadata = Some(serde_json::json!({
                "item_id": item_id,
                "category": category,
                "component_id": component_id,
                "operation": operation,
                "property_keys": descriptor.properties().iter()
                    .map(|definition| definition.name()).collect::<Vec<_>>(),
                "descriptor_driven": true,
            }));
            Some(item)
        })
    })
    .collect()
}

#[expect(
    clippy::too_many_arguments,
    reason = "one operation row needs its model, service, descriptor registry, and stack position"
)]
fn operation_entry(
    ui: &mut egui::Ui,
    palette: &ProjectPalette,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    item: &TimelineItem,
    operation: &TextEnsembleOperation,
    model_index: usize,
    phase_index: usize,
    phase_len: usize,
    previous_model_index: Option<usize>,
    next_model_index: Option<usize>,
    local_time: Option<library::model::authoring::MediaTime>,
) {
    let descriptor = plugins
        .text_ensemble_operation_descriptor(
            &operation.operation.category,
            &operation.operation.component_id,
        )
        .ok();
    let title = descriptor.as_ref().map_or(
        operation.operation.component_id.as_str(),
        OperationDescriptor::label,
    );
    let frame = egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(operation_icon(operation)).weak());
            ui.label(egui::RichText::new(title).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let overflow = ui.menu_button(icons::DOTS_THREE, |ui| {
                    operation_actions_menu(ui, state, service, item.id, operation.id);
                });
                crate::qa::register_component_with_metadata(
                    format!("inspector.text_ensemble.actions:{}", operation.id),
                    "inspector_overflow_button",
                    overflow.response.rect,
                    overflow.response.enabled(),
                    Some(serde_json::json!({"action": "open_operation_actions"})),
                );
                action_button_enabled(
                    ui,
                    state,
                    format!("inspector.text_ensemble.move_down:{}", operation.id),
                    icons::ARROW_DOWN,
                    "Move later in this execution phase",
                    next_model_index.is_some(),
                    || {
                        service.reorder_text_ensemble_operation(
                            item.id,
                            operation.id,
                            next_model_index.unwrap_or(model_index),
                        )
                    },
                );
                action_button_enabled(
                    ui,
                    state,
                    format!("inspector.text_ensemble.move_up:{}", operation.id),
                    icons::ARROW_UP,
                    "Move earlier in this execution phase",
                    previous_model_index.is_some(),
                    || {
                        service.reorder_text_ensemble_operation(
                            item.id,
                            operation.id,
                            previous_model_index.unwrap_or(model_index),
                        )
                    },
                );
            });
        });
        let Some(descriptor) = descriptor.as_ref() else {
            ui.weak("Plugin unavailable; authored values are preserved.");
            return;
        };
        for definition in descriptor.properties() {
            let Some(property) = operation.properties.get(definition.name()) else {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Missing {}", definition.label()),
                );
                continue;
            };
            let draft_key = format!("ensemble:{}:{}", operation.id, definition.name());
            let control_id = format!(
                "text_ensemble:{}:{}:{}",
                item.id,
                operation.id,
                definition.name()
            );
            let initial = local_time
                .and_then(|time| property.evaluate_at(time.to_seconds_f64()).ok())
                .or_else(|| property.value().cloned())
                .unwrap_or_else(|| definition.default_value().clone());
            let model_value = initial.clone();
            let local_seconds = local_time.map_or(0.0, |time| time.to_seconds_f64());
            let owner = AuthoringPropertyOwner::TextEnsemble {
                item_id: item.id,
                operation_id: operation.id,
            };
            let (changed, finished, mode_action, edited_value) = {
                let draft = state
                    .inspector
                    .property_values
                    .entry(draft_key)
                    .or_insert(initial);
                let result = super::property_row(
                    ui,
                    draft,
                    palette,
                    super::PropertyRowSpec {
                        control_id: &control_id,
                        label: definition.label(),
                        definition: Some(definition),
                        suffix: "",
                        speed: 0.1,
                        mode_state: PropertyModeState::from_property(
                            Some(property),
                            local_seconds,
                            false,
                        ),
                        allow_expression: definition.default_value().supports_expression(),
                    },
                );
                (
                    result.changed,
                    result.finished,
                    result.mode_action,
                    draft.clone(),
                )
            };
            if changed {
                if let Err(error) = definition.validate_value(&edited_value) {
                    state.error = Some(error);
                } else if let (Some(source_revision), Some(local_time)) =
                    (state.inspector.synced_revision, local_time)
                {
                    if let Some(target) = direct_edit_target(property, local_time) {
                        state.inspector.transient_property_edit = Some(TransientPropertyEdit {
                            source_revision,
                            owner,
                            update: AuthoringPropertyValueUpdate {
                                key: definition.name().to_string(),
                                value: edited_value.clone(),
                                target,
                            },
                        });
                    }
                }
            }
            if finished {
                if state
                    .inspector
                    .transient_property_edit
                    .as_ref()
                    .is_some_and(|edit| edit.matches(owner, definition.name()))
                {
                    state.inspector.transient_property_edit = None;
                }
                let Some(local_time) = local_time else {
                    state.error = Some("Text Ensemble has no valid clip-local time".to_string());
                    continue;
                };
                if edited_value != model_value {
                    if let Err(error) = service.set_text_ensemble_property(
                        plugins,
                        item.id,
                        operation.id,
                        definition.name(),
                        local_time,
                        edited_value.clone(),
                    ) {
                        state.error = Some(error.to_string());
                    }
                }
            }
            if let Some(action) = mode_action {
                state.inspector.transient_property_edit = None;
                let result = local_time
                    .ok_or_else(|| "Text Ensemble has no valid clip-local time".to_string())
                    .and_then(|local_time| {
                        super::property_authoring::apply_authored_mode_action(
                            service,
                            owner,
                            definition.name(),
                            Some(property),
                            edited_value.clone(),
                            local_time,
                            action,
                        )
                    });
                if let Err(error) = result {
                    state.error = Some(error);
                } else {
                    state.status = format!(
                        "{}: {}",
                        definition.label(),
                        super::mode_action_label(action)
                    );
                }
            }
            if property.evaluator == "expression" {
                expression_source(
                    ui,
                    state,
                    service,
                    owner,
                    definition.name(),
                    property,
                    &control_id,
                );
            }
            super::value_provenance(ui, property.evaluator == "keyframe", false);
        }
    });
    crate::qa::register_component_with_metadata(
        format!("inspector.text_ensemble.operation:{}", operation.id),
        "inspector_text_ensemble_operation",
        frame.response.rect,
        descriptor.is_some(),
        Some(serde_json::json!({
            "item_id": item.id,
            "operation_id": operation.id,
            "index": model_index,
            "phase_index": phase_index,
            "phase_length": phase_len,
            "execution_phase": operation.operation.category,
            "category": operation.operation.category,
            "component_id": operation.operation.component_id,
            "operation": operation.operation.operation,
            "label": title,
            "descriptor_driven": descriptor.is_some(),
            "property_keys": descriptor.as_ref().map(|descriptor| {
                descriptor.properties().iter().map(|definition| definition.name()).collect::<Vec<_>>()
            }).unwrap_or_default(),
        })),
    );
    frame.response.context_menu(|ui| {
        operation_actions_menu(ui, state, service, item.id, operation.id);
    });
    ui.add_space(4.0);
}

fn operation_actions_menu(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item_id: library::model::authoring::TimelineItemId,
    operation_id: uuid::Uuid,
) {
    let remove = ui.button(format!("{} Remove operation", icons::TRASH));
    crate::qa::register_component_with_metadata(
        format!("inspector.text_ensemble.remove:{operation_id}"),
        "inspector_menu_item",
        remove.rect,
        remove.enabled(),
        Some(serde_json::json!({"action": "remove_operation"})),
    );
    if remove.clicked() {
        if let Err(error) = service.remove_text_ensemble_operation(item_id, operation_id) {
            state.error = Some(error.to_string());
        }
        ui.close();
    }
}

fn action_button_enabled(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    id: String,
    icon: &str,
    tooltip: &str,
    enabled: bool,
    action: impl FnOnce() -> Result<library::model::authoring::ChangeSet, library::LibraryError>,
) {
    let button = ui
        .add_enabled(enabled, egui::Button::new(icon))
        .on_hover_text(tooltip);
    crate::qa::register_component_with_metadata(
        id,
        "inspector_action_button",
        button.rect,
        button.enabled(),
        Some(serde_json::json!({"action": tooltip})),
    );
    if button.clicked() {
        if let Err(error) = action() {
            state.error = Some(error.to_string());
        }
    }
}

fn operation_icon(operation: &TextEnsembleOperation) -> &'static str {
    match operation.operation.category.as_str() {
        EFFECTOR_CATEGORY => icons::SPARKLE,
        DECORATOR_CATEGORY => icons::MAGIC_WAND,
        _ => icons::QUESTION,
    }
}

fn direct_edit_target(
    property: &library::model::property::Property,
    local_time: library::model::authoring::MediaTime,
) -> Option<AuthoringPropertyValueTarget> {
    match property.evaluator.as_str() {
        "constant" => Some(AuthoringPropertyValueTarget::Constant),
        "keyframe" => Some(AuthoringPropertyValueTarget::Keyframe { local_time }),
        _ => None,
    }
}

fn expression_source(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    owner: AuthoringPropertyOwner,
    key: &str,
    property: &library::model::property::Property,
    control_id: &str,
) {
    let model_source = property.expression_text().unwrap_or_default();
    let committed = {
        let source = state
            .inspector
            .expression_sources
            .entry(control_id.to_string())
            .or_insert_with(|| model_source.to_string());
        super::property_authoring::expression_source_editor(ui, control_id, source, model_source)
            .then(|| source.clone())
    };
    if let Some(source) = committed {
        if let Err(error) = super::property_authoring::commit_expression_source(
            service,
            owner,
            key,
            Some(property),
            source,
        ) {
            state.error = Some(error);
        }
    }
}
