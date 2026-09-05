use std::collections::HashMap;

use egui_phosphor::regular as icons;
use library::editor::{ModuleAttachmentPlacement, TimelineEditorService};
#[cfg(test)]
use library::model::authoring::SourceRef;
use library::model::authoring::{
    Attachment, AttachmentOwner, AttachmentProcessor, AttachmentStage, AuthoringProject,
    ModuleConnection, ModuleConnectionId, ModuleDefinition, ModuleDefinitionId,
    ModuleDefinitionSharing, ModuleOutputId, ModulePortAddress, PublishedMediaInput,
    PublishedMediaInputId, TimelineId,
};
use library::model::project::connection::{PortDataType, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT};
use library::model::property::PropertyDefinition;
use library::model::Node;
use library::plugin::PluginManager;

use crate::state::authoring::AuthoringUiState;
use crate::state::node_editor::{ModuleEditorHost, NodeEditorDocument};
use crate::ui::widgets::searchable_context_menu::{
    searchable_menu_button, show_searchable_items_with_qa, SearchableItem,
};

use super::{property_control, property_label, property_row, PropertyRowSpec};

mod drag_drop;
mod module_controls;
use drag_drop::{active_payload, drag_handle, drop_slot, paint_drag_preview, EffectDropTarget};
use module_controls::module_effect_controls;

struct EffectStackResources<'a> {
    project: &'a AuthoringProject,
    service: &'a TimelineEditorService,
    plugins: &'a PluginManager,
}

#[derive(Clone, Copy)]
struct StackPosition {
    index: usize,
    len: usize,
}

#[derive(Clone)]
enum AddEffect {
    Builtin(String),
    Custom,
    Template(ModuleDefinitionId, ModuleOutputId),
}

pub(super) fn effect_stack(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    owner: AttachmentOwner,
    stages: &[AttachmentStage],
) {
    ui.separator();
    ui.label(egui::RichText::new("Effects").strong());

    if stages.is_empty() {
        ui.weak("This source has no supported Effect stage");
        return;
    }

    let stack_start = ui.cursor().min;
    let active_drag = active_payload(ui.ctx());
    let previous_target = drag_drop::preview_target(ui.ctx());
    let mut hovered_target = None;
    for stage in stages {
        let authored = ordered_stage_attachments(project, &owner, *stage);
        let dragged_id = active_drag
            .as_ref()
            .filter(|payload| payload.owner == owner)
            .map(|payload| payload.attachment_id);
        let attachments = authored
            .iter()
            .copied()
            .filter(|attachment| Some(attachment.id) != dragged_id)
            .collect::<Vec<_>>();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{}  {}", stage_label(*stage), authored.len()))
                    .small()
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let add =
                    searchable_menu_button(ui, egui::RichText::new(icons::PLUS).size(15.0), |ui| {
                        add_stage_menu(ui, project, state, service, plugins, &owner, *stage)
                    });
                crate::qa::register_component_with_metadata(
                    format!("inspector.effects.add:{}", drag_drop::stage_id(*stage)),
                    "effect_stack_add",
                    add.response.rect,
                    true,
                    Some(serde_json::json!({
                        "owner": format!("{owner:?}"),
                        "stage": format!("{stage:?}"),
                    })),
                );
                add.response
                    .on_hover_text(format!("Add Effect at {}", stage_label(*stage)));
            });
        });

        let media_type = stage_media_type(*stage);
        let destination_empty = attachments.is_empty();
        for index in 0..=attachments.len() {
            let slot = drop_slot(
                ui,
                &owner,
                *stage,
                media_type,
                index,
                destination_empty,
                previous_target,
            );
            if slot.hovered {
                hovered_target = Some(EffectDropTarget {
                    stage: *stage,
                    index,
                });
            }
            if let Some(payload) = slot.dropped {
                match service.move_attachment(payload.attachment_id, *stage, index) {
                    Ok(_) => {
                        state.status =
                            format!("Moved {} to {}", payload.title, stage_label(*stage));
                    }
                    Err(error) => state.error = Some(error.to_string()),
                }
            }
            if let Some(attachment) = attachments.get(index) {
                let resources = EffectStackResources {
                    project,
                    service,
                    plugins,
                };
                effect_entry(
                    ui,
                    state,
                    &resources,
                    attachment,
                    StackPosition {
                        index,
                        len: attachments.len(),
                    },
                );
            }
        }
        if authored.is_empty() && active_drag.is_none() {
            ui.weak("No effects at this stage");
        }
        ui.add_space(4.0);
    }
    drag_drop::store_preview_target(ui.ctx(), active_drag.as_ref().and(hovered_target));
    if let Some(payload) = &active_drag {
        paint_drag_preview(ui, payload);
    }
    crate::qa::register_component_with_metadata(
        "inspector.effects.drag_state",
        "effect_stack_drag_state",
        egui::Rect::from_min_max(stack_start, ui.cursor().min),
        true,
        Some(serde_json::json!({
            "dragging": active_drag.is_some(),
            "attachment_id": active_drag.as_ref().map(|payload| payload.attachment_id),
            "source_stage": active_drag.as_ref().map(|payload| format!("{:?}", payload.source_stage)),
            "insertion_stage": hovered_target.map(|target| format!("{:?}", target.stage)),
            "insertion_index": hovered_target.map(|target| target.index),
        })),
    );
}

fn ordered_stage_attachments<'a>(
    project: &'a AuthoringProject,
    owner: &AttachmentOwner,
    stage: AttachmentStage,
) -> Vec<&'a Attachment> {
    let mut attachments = project
        .attachments
        .values()
        .filter(|attachment| &attachment.owner == owner && attachment.stage == stage)
        .collect::<Vec<_>>();
    attachments.sort_by_key(|attachment| (attachment.order, attachment.id));
    attachments
}

fn add_stage_menu(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    owner: &AttachmentOwner,
    stage: AttachmentStage,
) {
    let Some(media_type) = stage.effect_media_type() else {
        ui.weak("This stage accepts Behaviors rather than Effects");
        return;
    };
    let mut effects = plugins
        .get_available_effects()
        .into_iter()
        .filter(|(effect_id, _, _)| {
            service
                .create_builtin_effect(plugins, effect_id)
                .is_ok_and(|effect| effect.contract.input_type == media_type)
        })
        .collect::<Vec<_>>();
    effects.sort_by(|left, right| left.2.cmp(&right.2).then(left.1.cmp(&right.1)));

    let mut choices = Vec::new();
    for (effect_id, name, category) in effects {
        let mut choice = SearchableItem::new(name, AddEffect::Builtin(effect_id.clone()));
        choice.category = Some(category);
        choice.keywords.push(effect_id.clone());
        choice.qa_id = Some(format!("inspector.effect.add_choice:{effect_id}"));
        choices.push(choice);
    }
    if media_type == PortDataType::Image {
        let mut choice = SearchableItem::new("New Custom Effect", AddEffect::Custom);
        choice.category = Some("Custom".to_string());
        choice.keywords = vec!["node".to_string(), "module".to_string()];
        choices.push(choice);
    }
    for (definition_id, output_id, label) in compatible_module_outputs(project, media_type) {
        let mut choice = SearchableItem::new(label, AddEffect::Template(definition_id, output_id));
        choice.category = Some("Module Templates".to_string());
        choices.push(choice);
    }
    if let Some(choice) = show_searchable_items_with_qa(
        ui,
        &format!("inspector.effect.add_menu:{owner:?}:{stage:?}"),
        Some("inspector.effect.add_search"),
        &choices,
    ) {
        match choice {
            AddEffect::Builtin(effect_id) => {
                match service.add_builtin_effect_by_id(plugins, owner.clone(), stage, &effect_id) {
                    Ok(_) => state.status = format!("Added Effect at {}", stage_label(stage)),
                    Err(error) => state.error = Some(error.to_string()),
                }
            }
            AddEffect::Custom => create_and_open_node_effect(state, service, owner.clone(), stage),
            AddEffect::Template(definition_id, output_id) => attach_and_open_module(
                state,
                service,
                owner.clone(),
                stage,
                definition_id,
                output_id,
            ),
        }
        ui.close();
    }
}

fn effect_entry(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    resources: &EffectStackResources<'_>,
    attachment: &Attachment,
    position: StackPosition,
) {
    let (icon, title) = attachment_title(resources.project, resources.plugins, attachment);
    let response = egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            drag_handle(ui, attachment, stage_media_type(attachment.stage), &title);
            ui.label(egui::RichText::new(icon).weak());
            ui.add_enabled(
                attachment.enabled && !attachment.bypassed,
                egui::Label::new(egui::RichText::new(&title).strong()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let remove = ui
                    .small_button(icons::TRASH)
                    .on_hover_text("Remove Effect (Undo is available)");
                crate::qa::register_component_with_metadata(
                    format!("inspector.effect_remove:{}", attachment.id),
                    "effect_remove",
                    remove.rect,
                    true,
                    Some(serde_json::json!({"attachment_id": attachment.id})),
                );
                if remove.clicked() {
                    remove_effect(state, resources.service, attachment.id);
                }
                if matches!(&attachment.processor, AttachmentProcessor::Module(_)) {
                    let edit = ui
                        .small_button(icons::SHARE_NETWORK)
                        .on_hover_text("Edit Effect in Node Editor");
                    crate::qa::register_component_with_metadata(
                        format!("inspector.effect_node:{}", attachment.id),
                        "effect_node_editor",
                        edit.rect,
                        true,
                        Some(serde_json::json!({"attachment_id": attachment.id})),
                    );
                    if edit.clicked() {
                        open_module_attachment(resources.project, state, attachment);
                    }
                }
                let enabled_icon = if attachment.enabled {
                    icons::EYE
                } else {
                    icons::EYE_SLASH
                };
                let enabled = ui
                    .small_button(enabled_icon)
                    .on_hover_text(if attachment.enabled {
                        "Disable Effect"
                    } else {
                        "Enable Effect"
                    });
                crate::qa::register_component_with_metadata(
                    format!("inspector.effect_enabled:{}", attachment.id),
                    "effect_enabled",
                    enabled.rect,
                    true,
                    Some(serde_json::json!({
                        "attachment_id": attachment.id,
                        "enabled": attachment.enabled,
                    })),
                );
                if enabled.clicked() {
                    update_attachment_state(
                        state,
                        resources.service,
                        attachment,
                        !attachment.enabled,
                        attachment.bypassed,
                    );
                }
            });
        });

        if attachment.bypassed {
            ui.weak("Bypassed");
        } else if !attachment.enabled {
            ui.weak("Disabled");
        }
        ui.add_enabled_ui(attachment.enabled && !attachment.bypassed, |ui| {
            effect_parameters(ui, state, resources, attachment);
        });
    });
    crate::qa::register_component_with_metadata(
        format!("inspector.effect:{}", attachment.id),
        "effect_stack_entry",
        response.response.rect,
        true,
        Some(serde_json::json!({
            "attachment_id": attachment.id,
            "stage": format!("{:?}", attachment.stage),
            "order": attachment.order,
            "enabled": attachment.enabled,
            "bypassed": attachment.bypassed,
            "kind": match &attachment.processor {
                AttachmentProcessor::BuiltinEffect(_) => "builtin",
                AttachmentProcessor::Module(_) => "module",
            },
            "component_id": match &attachment.processor {
                AttachmentProcessor::BuiltinEffect(effect) => Some(effect.operation.component_id.as_str()),
                AttachmentProcessor::Module(_) => None,
            },
        })),
    );
    response.response.context_menu(|ui| {
        effect_actions_menu(ui, state, resources, attachment, position);
    });
    ui.add_space(4.0);
}

fn remove_effect(
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    attachment_id: library::model::authoring::AttachmentId,
) {
    match service.remove_attachment(attachment_id) {
        Ok(_) => state.status = "Removed Effect".to_string(),
        Err(error) => state.error = Some(error.to_string()),
    }
}

fn effect_parameters(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    resources: &EffectStackResources<'_>,
    attachment: &Attachment,
) {
    match &attachment.processor {
        AttachmentProcessor::BuiltinEffect(effect) => {
            let definitions = resources
                .plugins
                .get_effect_properties(&effect.operation.component_id);
            for contract in &effect.contract.parameters {
                let Some(parameter) = effect.parameters.get(&contract.key) else {
                    continue;
                };
                let definition = definitions
                    .iter()
                    .find(|definition| definition.name() == contract.key);
                let label = definition.map_or(contract.key.as_str(), PropertyDefinition::label);
                let local_time = attachment_local_time(resources.project, state, &attachment.owner);
                let local_seconds = local_time
                    .as_ref()
                    .map_or(0.0, |time| time.to_seconds_f64());
                let initial = parameter
                    .automation
                    .as_ref()
                    .and_then(|track| {
                        local_time
                            .as_ref()
                            .ok()
                            .and_then(|time| track.evaluate_at(*time).ok())
                    })
                    .unwrap_or_else(|| parameter.value.clone());
                let mode_state = parameter.automation.as_ref().map_or_else(
                    || super::PropertyModeState::constant(local_seconds),
                    |track| {
                        super::PropertyModeState::from_keyframe_times(
                            local_seconds,
                            track
                                .keyframes
                                .iter()
                                .map(|keyframe| keyframe.time.to_seconds_f64()),
                        )
                    },
                );
                let (finished, mode_action, edited_value, model_value) = {
                    let draft = state
                        .inspector
                        .effect_values
                        .entry((attachment.id, contract.key.clone()))
                        .or_insert_with(|| initial.clone());
                    let result = property_row(
                        ui,
                        draft,
                        &resources.project.palette,
                        PropertyRowSpec {
                            control_id: &format!("attachment:{}:{}", attachment.id, contract.key),
                            label,
                            definition,
                            suffix: "",
                            speed: 0.1,
                            mode_state,
                            allow_keyframe: true,
                            keyframe_disabled_reason: None,
                            allow_expression: false,
                        },
                    );
                    (result.finished, result.mode_action, draft.clone(), initial)
                };
                let result = if finished && edited_value != model_value {
                    local_time.clone().and_then(|time| {
                        super::property_authoring::commit_builtin_effect_value(
                            resources.service,
                            attachment.id,
                            &contract.key,
                            parameter.automation.as_ref(),
                            edited_value.clone(),
                            time,
                        )
                    })
                } else {
                    Ok(())
                };
                if let Err(error) = result {
                    state.error = Some(error);
                }
                if let Some(action) = mode_action {
                    let result = local_time.clone().and_then(|time| {
                        super::property_authoring::apply_builtin_effect_mode_action(
                            resources.service,
                            attachment.id,
                            &contract.key,
                            parameter.automation.as_ref(),
                            edited_value,
                            time,
                            action,
                        )
                    });
                    if let Err(error) = result {
                        state.error = Some(error);
                    } else {
                        state.status = format!("{label}: {}", super::mode_action_label(action));
                    }
                }
                super::value_provenance(ui, parameter.automation.is_some(), false);
            }
        }
        AttachmentProcessor::Module(invocation) => {
            module_effect_controls(ui, state, resources, attachment, invocation)
        }
    }
}

fn attachment_local_time(
    project: &AuthoringProject,
    state: &AuthoringUiState,
    owner: &AttachmentOwner,
) -> Result<library::model::authoring::MediaTime, String> {
    match owner {
        AttachmentOwner::Item { item_id } => project
            .items
            .get(item_id)
            .ok_or_else(|| format!("Missing Timeline item {item_id}"))
            .and_then(|item| super::item_local_time(project, state, item)),
        AttachmentOwner::Track { track_id } => {
            let track = project
                .tracks
                .get(track_id)
                .ok_or_else(|| format!("Missing Timeline Track {track_id}"))?;
            if track.timeline_id != state.active_timeline_id {
                return Err("Effect Track is outside the active Timeline".to_string());
            }
            active_timeline_time(project, state)
        }
        AttachmentOwner::Timeline { timeline_id } => {
            if *timeline_id != state.active_timeline_id {
                return Err("Effect is outside the active Timeline".to_string());
            }
            active_timeline_time(project, state)
        }
    }
}

fn active_timeline_time(
    project: &AuthoringProject,
    state: &AuthoringUiState,
) -> Result<library::model::authoring::MediaTime, String> {
    let timeline = project
        .timelines
        .get(&state.active_timeline_id)
        .ok_or_else(|| "Missing active Timeline".to_string())?;
    library::model::authoring::MediaTime::from_frame_index(
        state.timeline.current_frame,
        timeline.fps,
    )
}

fn effect_actions_menu(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    resources: &EffectStackResources<'_>,
    attachment: &Attachment,
    position: StackPosition,
) {
    if matches!(&attachment.processor, AttachmentProcessor::Module(_))
        && ui
            .button(format!("{} Edit in Node Editor", icons::SHARE_NETWORK))
            .clicked()
    {
        open_module_attachment(resources.project, state, attachment);
        ui.close();
    }

    let mut enabled = attachment.enabled;
    if ui.checkbox(&mut enabled, "Enabled").changed() {
        update_attachment_state(
            state,
            resources.service,
            attachment,
            enabled,
            attachment.bypassed,
        );
        ui.close();
    }
    let mut bypassed = attachment.bypassed;
    if ui.checkbox(&mut bypassed, "Bypass").changed() {
        update_attachment_state(
            state,
            resources.service,
            attachment,
            attachment.enabled,
            bypassed,
        );
        ui.close();
    }
    ui.separator();
    if ui
        .add_enabled(
            position.index > 0,
            egui::Button::new(format!("{} Move up", icons::ARROW_UP)),
        )
        .clicked()
    {
        if let Err(error) = resources
            .service
            .reorder_attachment(attachment.id, position.index - 1)
        {
            state.error = Some(error.to_string());
        }
        ui.close();
    }
    if ui
        .add_enabled(
            position.index + 1 < position.len,
            egui::Button::new(format!("{} Move down", icons::ARROW_DOWN)),
        )
        .clicked()
    {
        if let Err(error) = resources
            .service
            .reorder_attachment(attachment.id, position.index + 1)
        {
            state.error = Some(error.to_string());
        }
        ui.close();
    }
    ui.separator();
    if ui
        .button(format!("{} Remove Effect", icons::TRASH))
        .clicked()
    {
        if let Err(error) = resources.service.remove_attachment(attachment.id) {
            state.error = Some(error.to_string());
        }
        ui.close();
    }
}

fn update_attachment_state(
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    attachment: &Attachment,
    enabled: bool,
    bypassed: bool,
) {
    match service.set_attachment_state(attachment.id, enabled, bypassed) {
        Ok(_) => {
            state.status = if !enabled {
                "Effect disabled".to_string()
            } else if bypassed {
                "Effect bypassed".to_string()
            } else {
                "Effect enabled".to_string()
            };
        }
        Err(error) => state.error = Some(error.to_string()),
    }
}

fn create_and_open_node_effect(
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    owner: AttachmentOwner,
    stage: AttachmentStage,
) {
    let (definition, output_id) = image_effect_module_definition("Custom Effect");
    let definition_id = definition.id;
    let placement = ModuleAttachmentPlacement {
        owner,
        stage,
        definition_id,
        output_id,
        parameter_overrides: HashMap::new(),
        input_bindings: HashMap::new(),
    };
    match service.create_private_module_attachment(definition, placement) {
        Ok((attachment_id, instance_id, _)) => {
            state
                .node_editor
                .request_document(NodeEditorDocument::ModuleDefinition {
                    definition_id,
                    host: ModuleEditorHost::Attachment {
                        attachment_id,
                        instance_path: state.active_instance_path.clone(),
                        module_instance_id: instance_id,
                    },
                });
            state.status = "Created Custom Effect".to_string();
        }
        Err(error) => state.error = Some(error.to_string()),
    }
}

fn attach_and_open_module(
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    owner: AttachmentOwner,
    stage: AttachmentStage,
    definition_id: ModuleDefinitionId,
    output_id: ModuleOutputId,
) {
    let placement = ModuleAttachmentPlacement {
        owner,
        stage,
        definition_id,
        output_id,
        parameter_overrides: HashMap::new(),
        input_bindings: HashMap::new(),
    };
    match service.attach_module(placement) {
        Ok((attachment_id, instance_id, _)) => {
            state
                .node_editor
                .request_document(NodeEditorDocument::ModuleDefinition {
                    definition_id,
                    host: ModuleEditorHost::Attachment {
                        attachment_id,
                        instance_path: state.active_instance_path.clone(),
                        module_instance_id: instance_id,
                    },
                });
            state.status = "Added Module as an Effect".to_string();
        }
        Err(error) => state.error = Some(error.to_string()),
    }
}

fn open_module_attachment(
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    attachment: &Attachment,
) {
    let AttachmentProcessor::Module(invocation) = &attachment.processor else {
        return;
    };
    let Some(instance) = project.module_instances.get(&invocation.instance_id) else {
        state.error = Some("The Custom Effect instance is missing".to_string());
        return;
    };
    state
        .node_editor
        .request_document(NodeEditorDocument::ModuleDefinition {
            definition_id: instance.definition_id,
            host: ModuleEditorHost::Attachment {
                attachment_id: attachment.id,
                instance_path: state.active_instance_path.clone(),
                module_instance_id: invocation.instance_id,
            },
        });
    state.status = "Opened Effect in Node Editor".to_string();
}

fn compatible_module_outputs(
    project: &AuthoringProject,
    media_type: PortDataType,
) -> Vec<(ModuleDefinitionId, ModuleOutputId, String)> {
    let mut compatible = project
        .module_definitions
        .values()
        .filter(|definition| {
            matches!(
                &definition.sharing,
                ModuleDefinitionSharing::ReusableTemplate(_)
            ) && definition
                .interface
                .media_inputs
                .iter()
                .any(|input| input.primary && input.data_type == media_type)
        })
        .flat_map(|definition| {
            definition
                .outputs()
                .filter(move |output| output.supports(media_type))
                .map(move |output| {
                    (
                        definition.id,
                        output.id,
                        format!("{} / {}", definition.name, output.name),
                    )
                })
        })
        .collect::<Vec<_>>();
    compatible.sort_by(|left, right| left.2.cmp(&right.2));
    compatible
}

fn attachment_title(
    project: &AuthoringProject,
    plugins: &PluginManager,
    attachment: &Attachment,
) -> (&'static str, String) {
    match &attachment.processor {
        AttachmentProcessor::BuiltinEffect(effect) => (
            icons::MAGIC_WAND,
            plugins
                .get_available_effects()
                .into_iter()
                .find(|(id, _, _)| *id == effect.operation.component_id)
                .map_or_else(
                    || effect.operation.component_id.clone(),
                    |(_, name, _)| name,
                ),
        ),
        AttachmentProcessor::Module(invocation) => (
            icons::SHARE_NETWORK,
            project
                .module_instances
                .get(&invocation.instance_id)
                .and_then(|instance| project.module_definitions.get(&instance.definition_id))
                .map_or_else(
                    || "Custom Effect".to_string(),
                    |definition| definition.name.clone(),
                ),
        ),
    }
}

fn image_effect_module_definition(name: impl Into<String>) -> (ModuleDefinition, ModuleOutputId) {
    let (mut definition, output_id) =
        ModuleDefinition::new_image(name, ModuleDefinitionSharing::Private);
    let mut input = Node::new_merge("Effect Input");
    input.ui_position = [80.0, 120.0];
    let input_node_id = input.id;
    let Some(output) = definition.output(output_id) else {
        return (definition, output_id);
    };
    let Some(output_target) = output.target(PortDataType::Image) else {
        return (definition, output_id);
    };
    let Some(output_node) = definition.graph.nodes.get_mut(&output.node_id) else {
        return (definition, output_id);
    };
    output_node.ui_position = [520.0, 120.0];
    let input_id = PublishedMediaInputId::new();
    definition.graph.nodes.insert(input_node_id, input);
    definition.graph.connections.push(ModuleConnection {
        id: ModuleConnectionId::new(),
        from: ModulePortAddress {
            node_id: input_node_id,
            port: IMAGE_OUTPUT_PORT.to_string(),
        },
        to: output_target,
        order: 0,
        blend_mode: library::model::BlendMode::Normal,
    });
    definition.interface.media_inputs.push(PublishedMediaInput {
        id: input_id,
        name: "Input".to_string(),
        data_type: PortDataType::Image,
        target: ModulePortAddress {
            node_id: input_node_id,
            port: MERGE_IMAGES_PORT.to_string(),
        },
        required: true,
        primary: true,
    });
    definition.topology_revision = 2;
    (definition, output_id)
}

const fn stage_media_type(stage: AttachmentStage) -> PortDataType {
    match stage.effect_media_type() {
        Some(media_type) => media_type,
        None => PortDataType::Number,
    }
}

const fn stage_label(stage: AttachmentStage) -> &'static str {
    match stage {
        AttachmentStage::ItemTimeMap => "Time Map",
        AttachmentStage::ItemPreTransform => "Before Transform",
        AttachmentStage::ItemPostTransform => "After Transform",
        AttachmentStage::TrackPostComposite => "Track Composite",
        AttachmentStage::TimelinePostComposite => "Composition Output",
        AttachmentStage::AudioPreFader => "Before Fader",
        AttachmentStage::AudioPostFader => "After Fader",
        AttachmentStage::TrackPostMix => "Track Mix",
        AttachmentStage::TimelinePostMix => "Composition Mix",
    }
}

#[cfg(test)]
mod tests;
