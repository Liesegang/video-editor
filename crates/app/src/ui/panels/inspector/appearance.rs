//! Shared, descriptor-driven Appearance stack for direct Text and Shape clips.

use egui_phosphor::regular as icons;
use library::editor::{
    AuthoringPropertyOwner, AuthoringPropertyValueTarget, AuthoringPropertyValueUpdate,
    TimelineEditorService,
};
use library::model::authoring::{
    appearance_direct_contract_is_compatible, AppearanceOperation, AuthoringProject, TimelineItem,
};
use library::plugin::{OperationDescriptor, PluginManager, STYLE_APPLY_OPERATION, STYLE_CATEGORY};

use crate::state::authoring::{AuthoringUiState, TransientPropertyEdit};
use crate::ui::widgets::property_mode::PropertyModeState;
use crate::ui::widgets::searchable_context_menu::{
    searchable_menu_button, show_searchable_items_with_qa, SearchableItem,
};

#[derive(Clone, Copy)]
enum AppearanceOwner {
    Direct(library::model::authoring::TimelineItemId),
    NodeClip(library::model::authoring::TimelineItemId),
}

impl AppearanceOwner {
    fn item_id(self) -> library::model::authoring::TimelineItemId {
        match self {
            Self::Direct(item_id) | Self::NodeClip(item_id) => item_id,
        }
    }

    fn model_name(self) -> &'static str {
        match self {
            Self::Direct(_) => "timeline_source",
            Self::NodeClip(_) => "module_graph",
        }
    }
}

pub(super) fn appearance_section(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    item: &TimelineItem,
    operations: &[AppearanceOperation],
) {
    ui.separator();
    let response = egui::CollapsingHeader::new("Appearance")
        .default_open(true)
        .show(ui, |ui| {
            add_menu(
                ui,
                state,
                service,
                plugins,
                AppearanceOwner::Direct(item.id),
                operations.len(),
            );
            if operations.is_empty() {
                ui.weak("Add Fill, Stroke, or another style to draw this source.");
            }
            let local_time = super::item_local_time(project, state, item).ok();
            for (index, operation) in operations.iter().enumerate() {
                operation_entry(
                    ui,
                    project,
                    state,
                    service,
                    plugins,
                    item,
                    operation,
                    index,
                    operations.len(),
                    local_time,
                );
            }
        })
        .header_response;
    crate::qa::register_component_with_metadata(
        format!("inspector.appearance:{}", item.id),
        "inspector_appearance",
        response.rect,
        true,
        Some(serde_json::json!({
            "item_id": item.id,
            "owner_model": AppearanceOwner::Direct(item.id).model_name(),
            "operation_count": operations.len(),
            "operations": operations.iter().enumerate().map(|(index, operation)| serde_json::json!({
                "id": operation.id,
                "index": index,
                "component_id": operation.operation.component_id,
                "property_keys": operation.properties.iter()
                    .map(|(key, _)| key).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })),
    );
}

pub(super) fn node_clip_appearance_section(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    context: &super::module_clip::ModuleParameterContext<'_>,
    stack: &library::editor::NodeClipAppearanceStack,
) {
    ui.separator();
    let owner = AppearanceOwner::NodeClip(context.item.id);
    let response = egui::CollapsingHeader::new("Appearance")
        .default_open(true)
        .show(ui, |ui| {
            add_menu(
                ui,
                state,
                context.service,
                context.plugins,
                owner,
                stack.operations.len(),
            );
            for (index, operation) in stack.operations.iter().enumerate() {
                let descriptor = context
                    .plugins
                    .operation_descriptor(
                        STYLE_CATEGORY,
                        &operation.component_id,
                        STYLE_APPLY_OPERATION,
                    )
                    .ok();
                let title = descriptor
                    .as_ref()
                    .map_or(operation.component_id.as_str(), OperationDescriptor::label);
                operation_card(
                    ui,
                    state,
                    context.service,
                    OperationCardSpec {
                        owner,
                        operation_id: operation.node_id,
                        index,
                        operation_count: stack.operations.len(),
                        component_id: &operation.component_id,
                        title,
                        descriptor_available: descriptor.is_some(),
                        property_keys: descriptor.as_ref().map_or_else(Vec::new, |descriptor| {
                            descriptor
                                .properties()
                                .iter()
                                .map(|definition| definition.name().to_string())
                                .collect()
                        }),
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
                            super::module_clip::published_parameter_row(
                                ui, state, context, parameter,
                            );
                        }
                    },
                );
            }
        })
        .header_response;
    crate::qa::register_component_with_metadata(
        format!("inspector.appearance:{}", context.item.id),
        "inspector_appearance",
        response.rect,
        true,
        Some(serde_json::json!({
            "item_id": context.item.id,
            "owner_model": owner.model_name(),
            "operation_count": stack.operations.len(),
            "operations": stack.operations.iter().enumerate().map(|(index, operation)| serde_json::json!({
                "id": operation.node_id,
                "index": index,
                "component_id": operation.component_id,
                "parameter_ids": operation.parameter_ids,
            })).collect::<Vec<_>>(),
        })),
    );
}

fn add_menu(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    owner: AppearanceOwner,
    insertion_index: usize,
) {
    let item_id = owner.item_id();
    let mut items = plugins
        .get_available_styles()
        .into_iter()
        .filter_map(|component_id| {
            let descriptor = plugins
                .operation_descriptor(STYLE_CATEGORY, &component_id, STYLE_APPLY_OPERATION)
                .ok()?;
            appearance_direct_contract_is_compatible(descriptor.declared_ports()).then(|| {
                let label = descriptor.label().to_string();
                let mut item = SearchableItem::new(label.clone(), component_id.clone());
                item.category = Some("Style".to_string());
                item.keywords = vec![component_id.clone(), label];
                item.qa_id = Some(format!("inspector.appearance.add.{component_id}"));
                item.qa_metadata = Some(serde_json::json!({
                    "item_id": item_id,
                    "component_id": component_id,
                    "property_keys": descriptor.properties().iter()
                        .map(|definition| definition.name()).collect::<Vec<_>>(),
                    "descriptor_driven": true,
                }));
                item
            })
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| left.label.cmp(&right.label));
    let menu_id = format!("inspector.appearance.menu:{item_id}");
    let menu = searchable_menu_button(ui, format!("{} Add style", icons::PLUS), |ui| {
        ui.set_min_width(290.0);
        ui.set_min_height(220.0_f32.min(ui.available_height().max(0.0)));
        show_searchable_items_with_qa(ui, &menu_id, Some(&format!("{menu_id}.query")), &items)
    });
    crate::qa::register_component_with_metadata(
        format!("inspector.appearance.add_menu:{item_id}"),
        "inspector_add_button",
        menu.response.rect,
        menu.response.enabled(),
        Some(serde_json::json!({
            "item_id": item_id,
            "descriptor_count": items.len(),
            "component_ids": items.iter().map(|item| item.value.as_str()).collect::<Vec<_>>(),
            "descriptor_driven": true,
        })),
    );
    if let Some(Some(component_id)) = menu.inner {
        let label = plugins
            .operation_descriptor(STYLE_CATEGORY, &component_id, STYLE_APPLY_OPERATION)
            .map_or_else(
                |_| component_id.clone(),
                |descriptor| descriptor.label().to_string(),
            );
        let result = match owner {
            AppearanceOwner::Direct(item_id) => service
                .add_appearance_operation(plugins, item_id, &component_id, insertion_index)
                .map(|_| ()),
            AppearanceOwner::NodeClip(item_id) => service
                .add_node_clip_appearance_operation(
                    plugins,
                    item_id,
                    &component_id,
                    insertion_index,
                )
                .map(|_| ()),
        };
        match result {
            Ok(_) => state.status = format!("Added {label} style"),
            Err(error) => state.error = Some(error.to_string()),
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "an Appearance card needs its persisted operation, stack position, property registry, and authoring transaction"
)]
fn operation_entry(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    item: &TimelineItem,
    operation: &AppearanceOperation,
    index: usize,
    operation_count: usize,
    local_time: Option<library::model::authoring::MediaTime>,
) {
    let descriptor = plugins
        .operation_descriptor(
            &operation.operation.category,
            &operation.operation.component_id,
            &operation.operation.operation,
        )
        .ok();
    let title = descriptor.as_ref().map_or(
        operation.operation.component_id.as_str(),
        OperationDescriptor::label,
    );
    operation_card(
        ui,
        state,
        service,
        OperationCardSpec {
            owner: AppearanceOwner::Direct(item.id),
            operation_id: operation.id,
            index,
            operation_count,
            component_id: &operation.operation.component_id,
            title,
            descriptor_available: descriptor.is_some(),
            property_keys: descriptor.as_ref().map_or_else(Vec::new, |descriptor| {
                descriptor
                    .properties()
                    .iter()
                    .map(|definition| definition.name().to_string())
                    .collect()
            }),
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
                property_entry(
                    ui, project, state, service, plugins, item, operation, definition, property,
                    local_time,
                );
            }
        },
    );
}

struct OperationCardSpec<'a> {
    owner: AppearanceOwner,
    operation_id: uuid::Uuid,
    index: usize,
    operation_count: usize,
    component_id: &'a str,
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
            ui.label(egui::RichText::new(icons::PALETTE).weak());
            ui.label(egui::RichText::new(spec.title).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let actions = ui.menu_button(icons::DOTS_THREE, |ui| {
                    remove_action(ui, state, service, &spec);
                });
                crate::qa::register_component(
                    format!("inspector.appearance.actions:{}", spec.operation_id),
                    "inspector_overflow_button",
                    actions.response.rect,
                );
                move_button(
                    ui,
                    state,
                    service,
                    spec.owner,
                    spec.operation_id,
                    spec.index,
                    spec.index + 1 < spec.operation_count,
                    true,
                );
                move_button(
                    ui,
                    state,
                    service,
                    spec.owner,
                    spec.operation_id,
                    spec.index,
                    spec.index > 0,
                    false,
                );
            });
        });
        content(ui, state);
    });
    crate::qa::register_component_with_metadata(
        format!("inspector.appearance.operation:{}", spec.operation_id),
        "inspector_appearance_operation",
        frame.response.rect,
        spec.descriptor_available,
        Some(serde_json::json!({
            "item_id": spec.owner.item_id(),
            "operation_id": spec.operation_id,
            "index": spec.index,
            "component_id": spec.component_id,
            "label": spec.title,
            "descriptor_available": spec.descriptor_available,
            "owner_model": spec.owner.model_name(),
            "property_keys": spec.property_keys,
        })),
    );
    frame
        .response
        .context_menu(|ui| remove_action(ui, state, service, &spec));
    ui.add_space(4.0);
}

fn remove_action(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    spec: &OperationCardSpec<'_>,
) {
    let can_remove =
        !matches!(spec.owner, AppearanceOwner::NodeClip(_)) || spec.operation_count > 1;
    let remove = ui.add_enabled(
        can_remove,
        egui::Button::new(format!("{} Remove style", icons::TRASH)),
    );
    crate::qa::register_component_with_metadata(
        format!("inspector.appearance.remove:{}", spec.operation_id),
        "inspector_menu_item",
        remove.rect,
        remove.enabled(),
        Some(serde_json::json!({"action": "remove_style"})),
    );
    if remove.clicked() {
        let result = match spec.owner {
            AppearanceOwner::Direct(item_id) => service
                .remove_appearance_operation(item_id, spec.operation_id)
                .map(|_| ()),
            AppearanceOwner::NodeClip(item_id) => service
                .remove_node_clip_appearance_operation(item_id, spec.operation_id)
                .map(|_| ()),
        };
        if let Err(error) = result {
            state.error = Some(error.to_string());
        }
        ui.close();
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the shared property row needs model context plus the Appearance operation owner"
)]
fn property_entry(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    item: &TimelineItem,
    operation: &AppearanceOperation,
    definition: &library::model::property::PropertyDefinition,
    property: &library::model::property::Property,
    local_time: Option<library::model::authoring::MediaTime>,
) {
    let local_seconds = local_time.map_or(0.0, |time| time.to_seconds_f64());
    let initial = property
        .evaluate_at(local_seconds)
        .ok()
        .or_else(|| property.value().cloned())
        .unwrap_or_else(|| definition.default_value().clone());
    let model_value = initial.clone();
    let draft_key = format!("appearance:{}:{}", operation.id, definition.name());
    let control_id = format!(
        "appearance:{}:{}:{}",
        item.id,
        operation.id,
        definition.name()
    );
    let owner = AuthoringPropertyOwner::Appearance {
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
            &project.palette,
            super::PropertyRowSpec {
                control_id: &control_id,
                label: definition.label(),
                definition: Some(definition),
                suffix: "",
                speed: 0.1,
                mode_state: PropertyModeState::from_property(Some(property), local_seconds, false),
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
        match local_time {
            Some(local_time) if edited_value != model_value => {
                if let Err(error) = service.set_appearance_property(
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
            None => state.error = Some("Appearance has no valid clip-local time".to_string()),
            _ => {}
        }
    }
    if let Some(action) = mode_action {
        state.inspector.transient_property_edit = None;
        let result = local_time
            .ok_or_else(|| "Appearance has no valid clip-local time".to_string())
            .and_then(|local_time| {
                super::property_authoring::apply_authored_mode_action(
                    service,
                    owner,
                    definition.name(),
                    Some(property),
                    edited_value,
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
        let model_source = property.expression_text().unwrap_or_default();
        let committed = {
            let source = state
                .inspector
                .expression_sources
                .entry(control_id.clone())
                .or_insert_with(|| model_source.to_string());
            super::property_authoring::expression_source_editor(
                ui,
                &control_id,
                source,
                model_source,
            )
            .then(|| source.clone())
        };
        if let Some(source) = committed {
            if let Err(error) = super::property_authoring::commit_expression_source(
                service,
                owner,
                definition.name(),
                Some(property),
                source,
            ) {
                state.error = Some(error);
            }
        }
    }
    super::value_provenance(ui, property.evaluator == "keyframe", false);
}

#[expect(
    clippy::too_many_arguments,
    reason = "one compact stack action needs its UI state, operation identity, direction, and authoritative service"
)]
fn move_button(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    owner: AppearanceOwner,
    operation_id: uuid::Uuid,
    index: usize,
    enabled: bool,
    down: bool,
) {
    let target = if down {
        index.saturating_add(1)
    } else {
        index.saturating_sub(1)
    };
    let icon = if down {
        icons::ARROW_DOWN
    } else {
        icons::ARROW_UP
    };
    let label = if down { "Move later" } else { "Move earlier" };
    let button = ui
        .add_enabled(enabled, egui::Button::new(icon))
        .on_hover_text(label);
    crate::qa::register_component_with_metadata(
        format!(
            "inspector.appearance.move_{}:{operation_id}",
            if down { "down" } else { "up" }
        ),
        "inspector_action_button",
        button.rect,
        button.enabled(),
        Some(serde_json::json!({"action": label, "target_index": target})),
    );
    if button.clicked() {
        let result = match owner {
            AppearanceOwner::Direct(item_id) => service
                .reorder_appearance_operation(item_id, operation_id, target)
                .map(|_| ()),
            AppearanceOwner::NodeClip(item_id) => service
                .reorder_node_clip_appearance_operation(item_id, operation_id, target)
                .map(|_| ()),
        };
        if let Err(error) = result {
            state.error = Some(error.to_string());
        }
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
