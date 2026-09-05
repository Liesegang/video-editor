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

#[derive(Clone, Copy)]
enum EnsembleOwner {
    Direct(library::model::authoring::TimelineItemId),
    NodeClip(library::model::authoring::TimelineItemId),
}

impl EnsembleOwner {
    fn item_id(self) -> library::model::authoring::TimelineItemId {
        match self {
            Self::Direct(item_id) | Self::NodeClip(item_id) => item_id,
        }
    }

    fn model_name(self) -> &'static str {
        match self {
            Self::Direct(_) => "timeline_text",
            Self::NodeClip(_) => "module_graph",
        }
    }
}

trait EnsembleEntryKind {
    fn ensemble_kind(&self) -> Option<TextEnsembleOperationKind>;
}

impl EnsembleEntryKind for TextEnsembleOperation {
    fn ensemble_kind(&self) -> Option<TextEnsembleOperationKind> {
        match self.operation.category.as_str() {
            EFFECTOR_CATEGORY => Some(TextEnsembleOperationKind::Effector),
            DECORATOR_CATEGORY => Some(TextEnsembleOperationKind::Decorator),
            _ => None,
        }
    }
}

impl EnsembleEntryKind for library::editor::NodeClipTextEnsembleEntry {
    fn ensemble_kind(&self) -> Option<TextEnsembleOperationKind> {
        Some(self.kind)
    }
}

fn ensemble_section<T: EnsembleEntryKind>(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    owner: EnsembleOwner,
    operations: &[T],
    mut render_entry: impl FnMut(
        &mut egui::Ui,
        &mut AuthoringUiState,
        usize,
        usize,
        usize,
        Option<usize>,
        Option<usize>,
    ),
) -> egui::Response {
    ui.separator();
    egui::CollapsingHeader::new("Text Ensemble")
        .default_open(true)
        .show(ui, |ui| {
            add_menu(ui, state, service, plugins, owner);
            if operations.is_empty() {
                ui.weak("Add an Effector or Decorator to animate grouped text elements.");
            }
            for (kind, heading) in [
                (TextEnsembleOperationKind::Effector, "Effectors"),
                (TextEnsembleOperationKind::Decorator, "Decorators"),
            ] {
                let phase_indices = operations
                    .iter()
                    .enumerate()
                    .filter_map(|(index, operation)| {
                        (operation.ensemble_kind() == Some(kind)).then_some(index)
                    })
                    .collect::<Vec<_>>();
                if !phase_indices.is_empty() {
                    ui.add_space(6.0);
                    ui.strong(heading);
                }
                for (phase_index, model_index) in phase_indices.iter().copied().enumerate() {
                    render_entry(
                        ui,
                        state,
                        model_index,
                        phase_index,
                        phase_indices.len(),
                        phase_index
                            .checked_sub(1)
                            .and_then(|index| phase_indices.get(index).copied()),
                        phase_indices.get(phase_index + 1).copied(),
                    );
                }
            }
        })
        .header_response
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
    let local_time = super::item_local_time(project, state, item).ok();
    let response = ensemble_section(
        ui,
        state,
        service,
        plugins,
        EnsembleOwner::Direct(item.id),
        operations,
        |ui, state, model_index, phase_index, phase_len, previous, next| {
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
                phase_len,
                previous,
                next,
                local_time,
            );
        },
    );
    crate::qa::register_component_with_metadata(
        format!("inspector.text_ensemble:{}", item.id),
        "inspector_text_ensemble",
        response.rect,
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

pub(super) fn node_clip_text_ensemble_section(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    context: &super::module_clip::ModuleParameterContext<'_>,
    stack: &library::editor::NodeClipTextEnsembleStack,
) {
    let owner = EnsembleOwner::NodeClip(context.item.id);
    let response = ensemble_section(
        ui,
        state,
        context.service,
        context.plugins,
        owner,
        &stack.operations,
        |ui, state, model_index, phase_index, phase_len, previous, next| {
            node_clip_operation_entry(
                ui,
                state,
                context,
                &stack.operations[model_index],
                model_index,
                phase_index,
                phase_len,
                previous,
                next,
            );
        },
    );
    crate::qa::register_component_with_metadata(
        format!("inspector.text_ensemble:{}", context.item.id),
        "inspector_text_ensemble",
        response.rect,
        true,
        Some(serde_json::json!({
            "item_id": context.item.id,
            "operation_count": stack.operations.len(),
            "owner_model": EnsembleOwner::NodeClip(context.item.id).model_name(),
            "operations": stack.operations.iter().map(|operation| serde_json::json!({
                "id": operation.node_id,
                "category": operation.category,
                "component_id": operation.component_id,
                "operation": operation.operation,
            })).collect::<Vec<_>>(),
        })),
    );
}

fn add_menu(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    owner: EnsembleOwner,
) {
    let item_id = owner.item_id();
    let mut items = operation_catalog(plugins, item_id);
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
    let menu_id = format!("inspector.text_ensemble.menu:{item_id}");
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
        format!("inspector.text_ensemble.add_menu:{item_id}"),
        "inspector_add_button",
        menu.response.rect,
        menu.response.enabled(),
        Some(serde_json::json!({
            "item_id": item_id,
            "owner_model": owner.model_name(),
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
        let result = match owner {
            EnsembleOwner::Direct(item_id) => service
                .add_text_ensemble_operation_by_id(
                    plugins,
                    item_id,
                    selected.kind,
                    &selected.component_id,
                )
                .map(|_| ()),
            EnsembleOwner::NodeClip(item_id) => service
                .add_node_clip_text_ensemble_operation_by_id(
                    plugins,
                    item_id,
                    selected.kind,
                    &selected.component_id,
                )
                .map(|_| ()),
        };
        match result {
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

struct OperationCardSpec<'a> {
    owner: EnsembleOwner,
    operation_id: uuid::Uuid,
    model_index: usize,
    phase_index: usize,
    phase_len: usize,
    previous_model_index: Option<usize>,
    next_model_index: Option<usize>,
    category: &'a str,
    component_id: &'a str,
    operation: &'a str,
    title: &'a str,
    descriptor_available: bool,
    property_keys: Vec<String>,
}

fn operation_card(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    spec: OperationCardSpec<'_>,
    content: impl FnOnce(&mut egui::Ui, &mut AuthoringUiState),
) {
    let frame = egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(operation_icon_for_category(spec.category)).weak());
            ui.label(egui::RichText::new(spec.title).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let overflow = ui.menu_button(icons::DOTS_THREE, |ui| {
                    operation_actions_menu(ui, state, service, spec.owner, spec.operation_id);
                });
                crate::qa::register_component_with_metadata(
                    format!("inspector.text_ensemble.actions:{}", spec.operation_id),
                    "inspector_overflow_button",
                    overflow.response.rect,
                    overflow.response.enabled(),
                    Some(serde_json::json!({"action": "open_operation_actions"})),
                );
                action_button_enabled(
                    ui,
                    state,
                    format!("inspector.text_ensemble.move_down:{}", spec.operation_id),
                    icons::ARROW_DOWN,
                    "Move later in this execution phase",
                    spec.next_model_index.is_some(),
                    || match spec.owner {
                        EnsembleOwner::Direct(item_id) => service.reorder_text_ensemble_operation(
                            item_id,
                            spec.operation_id,
                            spec.next_model_index.unwrap_or(spec.model_index),
                        ),
                        EnsembleOwner::NodeClip(item_id) => service
                            .reorder_node_clip_text_ensemble_operation(
                                item_id,
                                spec.operation_id,
                                spec.next_model_index.unwrap_or(spec.model_index),
                            ),
                    },
                );
                action_button_enabled(
                    ui,
                    state,
                    format!("inspector.text_ensemble.move_up:{}", spec.operation_id),
                    icons::ARROW_UP,
                    "Move earlier in this execution phase",
                    spec.previous_model_index.is_some(),
                    || match spec.owner {
                        EnsembleOwner::Direct(item_id) => service.reorder_text_ensemble_operation(
                            item_id,
                            spec.operation_id,
                            spec.previous_model_index.unwrap_or(spec.model_index),
                        ),
                        EnsembleOwner::NodeClip(item_id) => service
                            .reorder_node_clip_text_ensemble_operation(
                                item_id,
                                spec.operation_id,
                                spec.previous_model_index.unwrap_or(spec.model_index),
                            ),
                    },
                );
            });
        });
        content(ui, state);
    });
    crate::qa::register_component_with_metadata(
        format!("inspector.text_ensemble.operation:{}", spec.operation_id),
        "inspector_text_ensemble_operation",
        frame.response.rect,
        spec.descriptor_available,
        Some(serde_json::json!({
            "item_id": spec.owner.item_id(),
            "operation_id": spec.operation_id,
            "index": spec.model_index,
            "phase_index": spec.phase_index,
            "phase_length": spec.phase_len,
            "execution_phase": spec.category,
            "category": spec.category,
            "component_id": spec.component_id,
            "operation": spec.operation,
            "label": spec.title,
            "owner_model": spec.owner.model_name(),
            "descriptor_driven": spec.descriptor_available,
            "property_keys": spec.property_keys,
        })),
    );
    frame.response.context_menu(|ui| {
        operation_actions_menu(ui, state, service, spec.owner, spec.operation_id);
    });
    ui.add_space(4.0);
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
    let property_keys = descriptor.as_ref().map_or_else(Vec::new, |descriptor| {
        descriptor
            .properties()
            .iter()
            .map(|definition| definition.name().to_string())
            .collect()
    });
    operation_card(
        ui,
        state,
        service,
        OperationCardSpec {
            owner: EnsembleOwner::Direct(item.id),
            operation_id: operation.id,
            model_index,
            phase_index,
            phase_len,
            previous_model_index,
            next_model_index,
            category: &operation.operation.category,
            component_id: &operation.operation.component_id,
            operation: &operation.operation.operation,
            title,
            descriptor_available: descriptor.is_some(),
            property_keys,
        },
        |ui, state| {
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
                            allow_keyframe: true,
                            keyframe_disabled_reason: None,
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
                        state.error =
                            Some("Text Ensemble has no valid clip-local time".to_string());
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
        },
    );
}

#[expect(
    clippy::too_many_arguments,
    reason = "one structured Node Clip operation row needs its stack position and Module context"
)]
fn node_clip_operation_entry(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    context: &super::module_clip::ModuleParameterContext<'_>,
    operation: &library::editor::NodeClipTextEnsembleEntry,
    model_index: usize,
    phase_index: usize,
    phase_len: usize,
    previous_model_index: Option<usize>,
    next_model_index: Option<usize>,
) {
    let descriptor = context
        .plugins
        .text_ensemble_operation_descriptor(&operation.category, &operation.component_id)
        .ok();
    let title = descriptor
        .as_ref()
        .map_or(operation.component_id.as_str(), OperationDescriptor::label);
    let owner = EnsembleOwner::NodeClip(context.item.id);
    let property_keys = descriptor.as_ref().map_or_else(Vec::new, |descriptor| {
        descriptor
            .properties()
            .iter()
            .map(|definition| definition.name().to_string())
            .collect()
    });
    operation_card(
        ui,
        state,
        context.service,
        OperationCardSpec {
            owner,
            operation_id: operation.node_id,
            model_index,
            phase_index,
            phase_len,
            previous_model_index,
            next_model_index,
            category: &operation.category,
            component_id: &operation.component_id,
            operation: &operation.operation,
            title,
            descriptor_available: descriptor.is_some(),
            property_keys,
        },
        |ui, state| {
            if descriptor.is_none() {
                ui.weak("Plugin unavailable; published values are preserved.");
                return;
            }
            for parameter_id in &operation.parameter_ids {
                let Some(parameter) = context
                    .definition
                    .interface
                    .parameters
                    .iter()
                    .find(|parameter| parameter.id == *parameter_id)
                else {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("Missing published parameter {parameter_id}"),
                    );
                    continue;
                };
                super::module_clip::published_parameter_row(ui, state, context, parameter);
            }
        },
    );
}

fn operation_actions_menu(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    owner: EnsembleOwner,
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
        let result = match owner {
            EnsembleOwner::Direct(item_id) => service
                .remove_text_ensemble_operation(item_id, operation_id)
                .map(|_| ()),
            EnsembleOwner::NodeClip(item_id) => service
                .remove_node_clip_text_ensemble_operation(item_id, operation_id)
                .map(|_| ()),
        };
        if let Err(error) = result {
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

fn operation_icon_for_category(category: &str) -> &'static str {
    match category {
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
