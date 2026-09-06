//! Shared controller for one published Node Clip parameter.
//!
//! The Inspector and inline Node controls supply their own row presentation,
//! while this module owns effective-value resolution, Timeline automation,
//! transient Preview projection, and the single release-time commit.

use egui::Response;
use library::editor::{AuthoringPropertyValueTarget, TimelineEditorService};
use library::model::authoring::{
    AuthoringProject, AutomationTrack, MediaTime, ModuleDefinition, ModuleInstance,
    ModuleInvocation, PublishedParameter, PublishedParameterId, TimelineItem,
};
use library::model::property::{KeyframeId, PropertyDefinition, PropertyValue};
use library::plugin::PluginManager;

use crate::state::authoring::{AuthoringInspectorView, TransientPropertyEdit};
use crate::ui::panels::inspector::property_authoring::{
    update_transient_edit, PendingKeyframeMetadata,
};
use crate::ui::property_metadata::{
    node_property_definition, published_parameter_keyframe_capability,
};
use crate::ui::widgets::property_mode::{
    PropertyAuthoringMode, PropertyModeAction, PropertyModeState,
};

pub(crate) struct ModuleParameterContext<'a> {
    pub(crate) project: &'a AuthoringProject,
    pub(crate) service: &'a TimelineEditorService,
    pub(crate) plugins: &'a PluginManager,
    pub(crate) item: &'a TimelineItem,
    pub(crate) invocation: &'a ModuleInvocation,
    pub(crate) instance: &'a ModuleInstance,
    pub(crate) definition: &'a ModuleDefinition,
}

pub(crate) struct ModuleParameterRow<'a> {
    pub(crate) value: &'a mut PropertyValue,
    pub(crate) definition: Option<&'a PropertyDefinition>,
    pub(crate) mode_state: PropertyModeState,
    pub(crate) allow_keyframe: bool,
    pub(crate) keyframe_disabled_reason: Option<&'static str>,
    pub(crate) pending_keyframe: Option<PendingKeyframeMetadata>,
}

pub(crate) struct ModuleParameterRowInteraction {
    pub(crate) response: Response,
    pub(crate) changed: bool,
    pub(crate) finished: bool,
    pub(crate) mode_action: Option<PropertyModeAction>,
}

pub(crate) struct ModuleParameterEditorOutcome {
    pub(crate) response: Response,
    pub(crate) mode_action: Option<PropertyModeAction>,
    pub(crate) error: Option<String>,
}

/// Draw and author one published parameter without coupling the controller to
/// either the Inspector grid or the Node canvas layout.
pub(crate) fn edit_node_clip_parameter(
    view: &mut AuthoringInspectorView,
    context: &ModuleParameterContext<'_>,
    parameter: &PublishedParameter,
    local_time: Result<MediaTime, String>,
    draw: impl FnOnce(ModuleParameterRow<'_>) -> ModuleParameterRowInteraction,
) -> ModuleParameterEditorOutcome {
    let key = format!("module:{}:{}", context.instance.id, parameter.id);
    let base_value = context
        .instance
        .parameter_overrides
        .get(&parameter.id)
        .cloned()
        .unwrap_or_else(|| parameter.default_value.clone());
    let local_seconds = local_time
        .as_ref()
        .map_or(0.0, |time| time.to_seconds_f64());
    let automation = context.invocation.automation_tracks.get(&parameter.id);
    let initial = automation
        .and_then(|track| {
            local_time
                .as_ref()
                .ok()
                .and_then(|time| track.evaluate_at(*time).ok())
        })
        .unwrap_or(base_value);
    let model_value = initial.clone();
    let mode_state = automation.map_or_else(
        || PropertyModeState::constant(local_seconds),
        |track| {
            PropertyModeState::from_keyframe_times(
                local_seconds,
                track
                    .keyframes
                    .iter()
                    .map(|keyframe| keyframe.time.to_seconds_f64()),
            )
        },
    );
    let definition = published_parameter_definition(context.plugins, context.definition, parameter);
    let (allow_keyframe, keyframe_disabled_reason) =
        published_parameter_keyframe_capability(context.definition, parameter.id);
    let pending_keyframe = pending_module_keyframe(
        view.transient_property_edit.as_ref(),
        context.item.id,
        parameter.id,
    );
    let (interaction, edited_value) = {
        let value = view.property_values.entry(key).or_insert(initial);
        let interaction = draw(ModuleParameterRow {
            value,
            definition: definition.as_ref(),
            mode_state,
            allow_keyframe,
            keyframe_disabled_reason,
            pending_keyframe,
        });
        (interaction, value.clone())
    };
    let mut error = None;

    if interaction.changed {
        let validation = definition.as_ref().map_or(Ok(()), |definition| {
            definition.validate_value(&edited_value)
        });
        if let Err(validation_error) = validation {
            error = Some(validation_error);
        } else if let Some(edit) = module_parameter_edit(
            view.synced_revision,
            context,
            parameter,
            automation,
            &local_time,
            edited_value.clone(),
        ) {
            update_transient_edit(&mut view.transient_property_edit, edit);
        }
    }

    let active_edit = if interaction.finished
        && view
            .transient_property_edit
            .as_ref()
            .is_some_and(|edit| edit.matches_module_parameter(context.item.id, parameter.id))
    {
        view.transient_property_edit.take()
    } else {
        None
    };
    if interaction.finished && edited_value != model_value {
        let result =
            active_edit
                .or_else(|| {
                    module_parameter_edit(
                        view.synced_revision,
                        context,
                        parameter,
                        automation,
                        &local_time,
                        edited_value.clone(),
                    )
                })
                .ok_or_else(|| {
                    local_time.as_ref().err().cloned().unwrap_or_else(|| {
                        "Inspector has no synchronized Project revision".to_string()
                    })
                })
                .and_then(|edit| {
                    edit.commit(context.service)
                        .map_err(|error| error.to_string())
                });
        if let Err(commit_error) = result {
            error = Some(commit_error);
        }
    }

    let mut applied_mode_action = None;
    if let Some(action) = interaction.mode_action {
        view.transient_property_edit = None;
        let result =
            require_current_revision(context.service, view.synced_revision).and_then(|()| {
                match &local_time {
                    Ok(time) => apply_module_parameter_mode_action(
                        context.service,
                        context.item.id,
                        parameter.id,
                        automation,
                        edited_value,
                        *time,
                        action,
                    ),
                    Err(time_error) => Err(time_error.clone()),
                }
            });
        match result {
            Ok(()) => applied_mode_action = Some(action),
            Err(mode_error) => error = Some(mode_error),
        }
    }

    ModuleParameterEditorOutcome {
        response: interaction.response,
        mode_action: applied_mode_action,
        error,
    }
}

fn require_current_revision(
    service: &TimelineEditorService,
    source_revision: Option<library::model::authoring::ProjectRevision>,
) -> Result<(), String> {
    let source_revision = source_revision
        .ok_or_else(|| "Parameter editor has no synchronized Project revision".to_string())?;
    let current_revision = service.revision().map_err(|error| error.to_string())?;
    if current_revision != source_revision {
        return Err(format!(
            "Parameter edit revision {} is stale; current revision is {}",
            source_revision.get(),
            current_revision.get()
        ));
    }
    Ok(())
}

fn module_parameter_edit(
    source_revision: Option<library::model::authoring::ProjectRevision>,
    context: &ModuleParameterContext<'_>,
    parameter: &PublishedParameter,
    automation: Option<&AutomationTrack>,
    local_time: &Result<MediaTime, String>,
    value: PropertyValue,
) -> Option<TransientPropertyEdit> {
    let source_revision = source_revision?;
    let local_time = *local_time.as_ref().ok()?;
    let target = if automation.is_some() {
        AuthoringPropertyValueTarget::Keyframe {
            local_time,
            insertion_id: KeyframeId::new(),
        }
    } else {
        AuthoringPropertyValueTarget::Constant
    };
    Some(TransientPropertyEdit::module_parameter(
        source_revision,
        context.item.id,
        context.instance.id,
        parameter.id,
        value,
        target,
    ))
}

fn pending_module_keyframe(
    edit: Option<&TransientPropertyEdit>,
    item_id: library::model::authoring::TimelineItemId,
    parameter_id: PublishedParameterId,
) -> Option<PendingKeyframeMetadata> {
    let edit = edit.filter(|edit| edit.matches_module_parameter(item_id, parameter_id))?;
    let (insertion_id, local_time) = edit.pending_keyframe()?;
    Some(PendingKeyframeMetadata {
        insertion_id,
        local_time,
    })
}

fn published_parameter_definition(
    plugins: &PluginManager,
    definition: &ModuleDefinition,
    parameter: &PublishedParameter,
) -> Option<PropertyDefinition> {
    let node = definition.graph.nodes.get(&parameter.target.node_id)?;
    let property_name = library::plugin::property_name_from_port(&parameter.target.port)
        .unwrap_or(parameter.target.port.as_str());
    node_property_definition(plugins, node, property_name)
}

fn apply_module_parameter_mode_action(
    service: &TimelineEditorService,
    item_id: library::model::authoring::TimelineItemId,
    parameter_id: PublishedParameterId,
    automation: Option<&AutomationTrack>,
    value: PropertyValue,
    local_time: MediaTime,
    action: PropertyModeAction,
) -> Result<(), String> {
    match action {
        PropertyModeAction::SetMode(PropertyAuthoringMode::Constant) => service
            .set_module_parameter_constant(item_id, parameter_id, value)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        PropertyModeAction::SetMode(PropertyAuthoringMode::Keyframe) => service
            .upsert_module_parameter_keyframe(item_id, parameter_id, local_time, value, None)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        PropertyModeAction::SetMode(PropertyAuthoringMode::Expression) => {
            Err("Module parameter expressions belong inside the Node Module".to_string())
        }
        PropertyModeAction::ToggleKeyframe => {
            if let Some(keyframe_id) = keyframe_at(automation, local_time) {
                if automation.is_some_and(|track| track.keyframes.len() == 1) {
                    service
                        .set_module_parameter_constant(item_id, parameter_id, value)
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                } else {
                    service
                        .remove_module_parameter_keyframe(item_id, parameter_id, keyframe_id)
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                }
            } else {
                service
                    .upsert_module_parameter_keyframe(
                        item_id,
                        parameter_id,
                        local_time,
                        value,
                        None,
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }
        }
    }
}

pub(crate) fn keyframe_at(
    track: Option<&AutomationTrack>,
    local_time: MediaTime,
) -> Option<KeyframeId> {
    let seconds = local_time.to_seconds_f64();
    track.and_then(|track| {
        track
            .keyframes
            .iter()
            .find(|keyframe| (keyframe.time.to_seconds_f64() - seconds).abs() < 0.001)
            .map(|keyframe| keyframe.id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_actions_reject_a_stale_paired_snapshot_revision() {
        let service = TimelineEditorService::create_default("stale mode action").unwrap();
        let source_revision = service.revision().unwrap();
        let project = service.snapshot().unwrap();
        let root = &project.timelines[&project.root_timeline_id];
        service
            .add_timeline(
                "Concurrent edit".to_string(),
                root.width,
                root.height,
                root.fps,
                root.duration,
            )
            .unwrap();
        let current_revision = service.revision().unwrap();

        let error = require_current_revision(&service, Some(source_revision))
            .expect_err("stale mode action must not reach its mutation command");
        assert!(error.contains("stale"), "{error}");
        assert_eq!(service.revision().unwrap(), current_revision);
    }
}
