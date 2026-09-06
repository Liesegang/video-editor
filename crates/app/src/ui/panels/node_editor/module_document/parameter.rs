//! Published inputs use the same Timeline parameter controller as Inspector.

use library::model::authoring::{
    InstancePath, ModuleInstance, PublishedParameter, SourceRef, TimelineId, TimelineItemId,
};
use library::model::project::PortDefinition;

use super::*;
use crate::state::authoring::AuthoringInspectorView;
use crate::ui::module_parameter_editor::{
    edit_node_clip_parameter, ModuleParameterContext, ModuleParameterRowInteraction,
};
use crate::ui::panels::node_editor::property_label;
use crate::ui::widgets::property_mode::property_mode_control_for_state;
use crate::ui::widgets::property_value_editor::{property_value_editor, PropertyValueEditorSpec};

pub(super) struct NodeParameterHost<'a> {
    pub(super) project: &'a AuthoringProject,
    pub(super) service: &'a TimelineEditorService,
    pub(super) instance: &'a ModuleInstance,
    pub(super) item: Result<TimelineItemId, String>,
    pub(super) inspector: &'a mut AuthoringInspectorView,
    pub(super) status: &'a mut String,
    pub(super) error: &'a mut Option<String>,
}

/// A document retained while navigating elsewhere must not use that other
/// Timeline's playhead to write keys into its original Node Clip.
pub(super) fn node_clip_parameter_item(
    project: &AuthoringProject,
    active_timeline: TimelineId,
    active_path: Option<&InstancePath>,
    host: &ModuleEditorHost,
) -> Result<TimelineItemId, String> {
    let ModuleEditorHost::NodeClip {
        timeline_item_id,
        instance_path,
        module_instance_id,
    } = host
    else {
        return Err("Timeline input keyframes are currently available in Node Clips".to_string());
    };
    let item = project
        .items
        .get(timeline_item_id)
        .ok_or_else(|| "The Node Clip is no longer available".to_string())?;
    let track = project
        .tracks
        .get(&item.track_id)
        .ok_or_else(|| "The Node Clip's Track is no longer available".to_string())?;
    if track.timeline_id != active_timeline {
        return Err("Open the Node Clip's Timeline to edit its input animation".to_string());
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
    if !matches!(&item.source, SourceRef::Module(invocation) if invocation.instance_id == *module_instance_id)
    {
        return Err("The Node Clip's Module instance has changed".to_string());
    }
    Ok(*timeline_item_id)
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
) -> egui::Response {
    let item_id = match &host.item {
        Ok(id) => *id,
        Err(reason) => return ui.weak(&port.label).on_hover_text(reason),
    };
    let item = &host.project.items[&item_id];
    let SourceRef::Module(invocation) = &item.source else {
        return ui.weak(&port.label);
    };
    let context = ModuleParameterContext {
        project: host.project,
        service: host.service,
        plugins,
        item,
        invocation,
        instance: host.instance,
        definition,
    };
    let property_key = authored_property_key_for_port(node, &port.key).unwrap_or(&port.key);
    let qa_id = format!("node_editor.property.node:{}:{property_key}", node.id);
    let mut pending = None;
    let mut edited_value = None;
    let outcome = edit_node_clip_parameter(
        host.inspector,
        &context,
        parameter,
        Ok(clock.exact_time),
        |row| {
            pending = row.pending_keyframe;
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
                    ui, egui::Id::new(("module_parameter", host.instance.id, parameter.id)),
                    &qa_id, row.value,
                    PropertyValueEditorSpec {
                        definition: row.definition, fallback_suffix: "", fallback_speed: 0.05,
                        palette: &host.project.palette,
                    },
                )
            }).inner;
            edited_value = Some(row.value.clone());
            ModuleParameterRowInteraction {
                response: edit.response,
                changed: edit.changed,
                finished: edit.finished,
                mode_action,
            }
        },
    );
    if let Some(error) = outcome.error {
        *host.error = Some(error);
    }
    if outcome.mode_action.is_some() {
        *host.status = format!("Updated {} animation", parameter.name);
    }
    let mut metadata = serde_json::json!({
        "property_scope": "module_instance",
        "target": {"kind": "module_parameter", "id": parameter.id},
        "item_id": item_id, "instance_id": host.instance.id, "parameter_id": parameter.id,
        "node_id": node.id, "property": property_key, "current_time": clock.time,
        "timeline_owned": true,
        "value": edited_value,
        "evaluator": if invocation.automation_tracks.contains_key(&parameter.id) { "keyframe" } else { "constant" },
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
    outcome.response
}
