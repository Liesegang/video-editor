//! Published inputs use the same Timeline parameter controller as Inspector.

use library::editor::{ModuleAutomationOwner, ModuleParameterOwner};
use library::model::authoring::{
    AttachmentOwner, InstancePath, ModuleInstance, PublishedParameter, TimelineId,
};
use library::model::project::PortDefinition;

use super::*;
use crate::state::authoring::AuthoringInspectorView;
use crate::ui::module_parameter_editor::{
    edit_module_parameter, ModuleParameterContext, ModuleParameterRowInteraction,
};
use crate::ui::panels::node_editor::property_label;
use crate::ui::widgets::image_collection_editor::ImageCollectionEditorContext;
use crate::ui::widgets::property_mode::property_mode_control_for_state;
use crate::ui::widgets::property_value_editor::{property_value_editor, PropertyValueEditorSpec};

pub(super) struct NodeParameterHost<'a> {
    pub(super) project: &'a AuthoringProject,
    pub(super) service: &'a TimelineEditorService,
    pub(super) instance: &'a ModuleInstance,
    pub(super) owner: Result<ModuleParameterOwner, String>,
    pub(super) inspector: &'a mut AuthoringInspectorView,
    pub(super) status: &'a mut String,
    pub(super) error: &'a mut Option<String>,
}

/// A document retained while navigating elsewhere must not use that other
/// Timeline's playhead to write keys into its original Node Clip.
pub(super) fn module_parameter_owner(
    project: &AuthoringProject,
    active_timeline: TimelineId,
    active_path: Option<&InstancePath>,
    host: &ModuleEditorHost,
) -> Result<ModuleParameterOwner, String> {
    let (owner, instance_path) = match host {
        ModuleEditorHost::NodeClip {
            timeline_item_id,
            instance_path,
            ..
        } => (
            ModuleParameterOwner::Invocation(ModuleAutomationOwner::Item(*timeline_item_id)),
            instance_path,
        ),
        ModuleEditorHost::Attachment {
            attachment_id,
            instance_path,
            ..
        } => (
            ModuleParameterOwner::Invocation(ModuleAutomationOwner::Attachment(*attachment_id)),
            instance_path,
        ),
        ModuleEditorHost::Transition {
            transition_id,
            instance_path,
            ..
        } => {
            let owner = crate::ui::automation_lanes::transition_owner(
                *transition_id,
                instance_path.as_ref(),
            );
            let owner = crate::ui::automation_lanes::transition_service_owner(&owner)
                .ok_or_else(|| "The Transition has no parameter owner".to_string())?;
            (ModuleParameterOwner::Transition(owner), instance_path)
        }
    };
    if owner.timeline_id(project)? != active_timeline {
        return Err("Open the processor's Timeline to edit its input animation".to_string());
    }
    let nested_path = |path: Option<&InstancePath>| {
        path.filter(|path| !path.composition_items.is_empty())
            .cloned()
    };
    if nested_path(instance_path.as_ref()) != nested_path(active_path) {
        return Err(
            "Return to this Node document's Composition placement to edit its animation"
                .to_string(),
        );
    }
    if owner.instance_id(project)? != host.module_instance_id() {
        return Err("The processor's Module instance has changed".to_string());
    }
    Ok(owner)
}

#[allow(
    clippy::too_many_arguments,
    reason = "Inline input presentation borrows the same parameter controller, port contract and canvas projection as the enclosing Node document"
)]
pub(super) fn show_published_input(
    ui: &mut egui::Ui,
    host: &mut NodeParameterHost<'_>,
    plugins: &PluginManager,
    definition: &ModuleDefinition,
    node: &Node,
    port: &PortDefinition,
    parameter: &PublishedParameter,
    clock: ModulePropertyContext,
    transform: egui::emath::TSTransform,
    image_collection: Option<ImageCollectionEditorContext<'_>>,
) -> (egui::Response, Vec<ModuleEditorAction>) {
    let owner = match &host.owner {
        Ok(id) => id.clone(),
        Err(reason) => {
            return (ui.weak(&port.label).on_hover_text(reason), Vec::new());
        }
    };
    let context = ModuleParameterContext {
        project: host.project,
        service: host.service,
        plugins,
        owner: owner.clone(),
        instance: host.instance,
        definition,
    };
    let property_key = authored_property_key_for_port(node, &port.key).unwrap_or(&port.key);
    let qa_id = format!("node_editor.property.node:{}:{property_key}", node.id);
    let mut pending = None;
    let mut edited_value = None;
    let mut has_automation = false;
    let mut interface_actions = Vec::new();
    let outcome = edit_module_parameter(
        host.inspector,
        &context,
        parameter,
        Ok(clock.exact_time),
        |row| {
            pending = row.pending_keyframe;
            has_automation = row.has_automation;
            let mut mode_action = None;
            let edit = ui.horizontal(|ui| {
                property_label(ui, &port.label).on_hover_text(
                    "Timeline-owned input animation; edit the same keys in Timeline or Curve Editor",
                );
                mode_action = property_mode_control_for_state(
                    ui,
                    &format!("node_editor.property_mode.node:{}:{property_key}", node.id),
                    row.mode_state, row.allow_keyframe, row.keyframe_disabled_reason, false,
                ).0;
                property_value_editor(
                    ui, egui::Id::new(("module_parameter", &owner, host.instance.id, parameter.id)),
                    &qa_id, row.value,
                    PropertyValueEditorSpec {
                        definition: row.definition, fallback_suffix: "", fallback_speed: 0.05,
                        palette: &host.project.palette,
                        image_collection,
                    },
                )
            }).inner;
            let mut reset_to_default = false;
            interface_actions.extend(super::interface::input_port_interface_actions(
                &edit.response,
                transform * edit.response.rect,
                definition,
                node.id,
                port,
                |ui| {
                    ui.separator();
                    let reset = ui.add_enabled(
                        row.has_resettable_override,
                        egui::Button::new(row.reset_label),
                    );
                    crate::qa::register_component_with_metadata(
                        super::interface::interface_action_qa_id(
                            node.id,
                            PortDirection::Input,
                            &port.key,
                            "reset_parameter",
                        ),
                        "node_editor_interface_action",
                        reset.rect,
                        reset.enabled(),
                        Some(serde_json::json!({
                            "action": "reset_parameter",
                            "node_id": node.id,
                            "port": port.key,
                            "parameter_id": parameter.id,
                            "instance_id": host.instance.id,
                            "label": row.reset_label,
                        })),
                    );
                    if reset.clicked() {
                        reset_to_default = true;
                        ui.close();
                    }
                },
            ));
            edited_value = Some(row.value.clone());
            ModuleParameterRowInteraction {
                response: edit.response,
                changed: edit.changed,
                finished: edit.finished,
                mode_action,
                reset_to_default,
            }
        },
    );
    if let Some(error) = outcome.error {
        *host.error = Some(error);
    }
    if outcome.mode_action.is_some() {
        *host.status = format!("Updated {} animation", parameter.name);
    }
    let (item_id, attachment_id, target) = match &owner {
        ModuleParameterOwner::Invocation(ModuleAutomationOwner::Item(item_id)) => (
            Some(*item_id),
            None,
            crate::state::authoring::AutomationTarget::ModuleParameter(parameter.id),
        ),
        ModuleParameterOwner::Invocation(ModuleAutomationOwner::Attachment(attachment_id)) => {
            let item_id = host
                .project
                .attachments
                .get(attachment_id)
                .and_then(|attachment| match attachment.owner {
                    AttachmentOwner::Item { item_id } => Some(item_id),
                    _ => None,
                });
            (
                item_id,
                Some(*attachment_id),
                crate::state::authoring::AutomationTarget::AttachmentModuleParameter {
                    attachment_id: *attachment_id,
                    parameter_id: parameter.id,
                },
            )
        }
        ModuleParameterOwner::Transition(_) => (
            None,
            None,
            crate::state::authoring::AutomationTarget::ModuleParameter(parameter.id),
        ),
    };
    let lane_owner = crate::ui::automation_lanes::module_parameter_owner(host.project, &owner);
    let mut metadata = serde_json::json!({
        "property_scope": "module_instance",
        "target": crate::ui::automation_lanes::target_metadata(&target),
        "owner": lane_owner.as_ref().map(crate::ui::automation_lanes::owner_metadata),
        "attachment_id": attachment_id,
        "item_id": item_id, "instance_id": host.instance.id, "parameter_id": parameter.id,
        "node_id": node.id, "property": property_key, "current_time": clock.time,
        "timeline_owned": true,
        "value": edited_value,
        "evaluator": if has_automation { "keyframe" } else { "constant" },
    });
    if let Some(pending) = pending {
        metadata["pending_keyframe_insertion_id"] = serde_json::json!(pending.insertion_id);
        metadata["pending_keyframe_time"] = serde_json::json!(pending.local_time.to_seconds_f64());
    }
    crate::qa::register_component_with_metadata(
        qa_id,
        "node_property_control",
        transform * outcome.response.rect,
        outcome.response.enabled(),
        Some(metadata),
    );
    (outcome.response, interface_actions)
}
