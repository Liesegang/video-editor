//! Shared controller for one published Module parameter.
//!
//! The Inspector and inline Node controls supply their own row presentation,
//! while this module owns effective-value resolution, Timeline automation,
//! transient Preview projection, and the single release-time commit.

use egui::Response;
use library::editor::{
    AuthoringPropertyValueTarget, ModuleParameterOwner, TimelineEditorService,
    TransitionAutomationOwner,
};
use library::model::authoring::{
    AuthoringProject, AutomationTrack, MediaTime, ModuleDefinition, ModuleInstance,
    PublishedParameter, PublishedParameterId,
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
    pub(crate) owner: ModuleParameterOwner,
    pub(crate) instance: &'a ModuleInstance,
    pub(crate) definition: &'a ModuleDefinition,
}

pub(crate) fn parameter_local_time(
    context: &ModuleParameterContext<'_>,
    state: &crate::state::authoring::AuthoringUiState,
) -> Result<MediaTime, String> {
    let timeline_id = context.owner.timeline_id(context.project)?;
    if timeline_id != state.active_timeline_id {
        return Err("Open the processor's Timeline to edit its animation".to_string());
    }
    if let ModuleParameterOwner::Transition(TransitionAutomationOwner::Instance {
        instance_path,
        ..
    }) = &context.owner
    {
        if !instance_path.composition_items.is_empty()
            && state.active_instance_path.as_ref() != Some(instance_path)
        {
            return Err("Open the processor's Composition placement to edit it".to_string());
        }
    }
    let timeline = context
        .project
        .timelines
        .get(&timeline_id)
        .ok_or_else(|| "The processor's Timeline is unavailable".to_string())?;
    let time = MediaTime::from_frame_index(state.timeline.current_frame, timeline.fps)?;
    let owner =
        crate::ui::automation_lanes::module_parameter_owner(context.project, &context.owner)
            .ok_or_else(|| "The processor has no animation time domain".to_string())?;
    crate::ui::automation_lanes::local_time_for_timeline(context.project, &owner, time)
        .ok_or_else(|| "The processor has no valid local time".to_string())
}

pub(crate) struct ModuleParameterRow<'a> {
    pub(crate) value: &'a mut PropertyValue,
    pub(crate) definition: Option<&'a PropertyDefinition>,
    pub(crate) mode_state: PropertyModeState,
    pub(crate) allow_keyframe: bool,
    pub(crate) keyframe_disabled_reason: Option<&'static str>,
    pub(crate) pending_keyframe: Option<PendingKeyframeMetadata>,
    pub(crate) has_resettable_override: bool,
    pub(crate) has_automation: bool,
    pub(crate) reset_label: &'static str,
}

pub(crate) struct ModuleParameterRowInteraction {
    pub(crate) response: Response,
    pub(crate) changed: bool,
    pub(crate) finished: bool,
    pub(crate) mode_action: Option<PropertyModeAction>,
    pub(crate) reset_to_default: bool,
}

pub(crate) struct ModuleParameterEditorOutcome {
    pub(crate) response: Response,
    pub(crate) mode_action: Option<PropertyModeAction>,
    pub(crate) error: Option<String>,
}

/// Draw and author one published parameter without coupling the controller to
/// either the Inspector grid or the Node canvas layout.
pub(crate) fn edit_module_parameter(
    view: &mut AuthoringInspectorView,
    context: &ModuleParameterContext<'_>,
    parameter: &PublishedParameter,
    local_time: Result<MediaTime, String>,
    draw: impl FnOnce(ModuleParameterRow<'_>) -> ModuleParameterRowInteraction,
) -> ModuleParameterEditorOutcome {
    // A nested Timeline can expose the same Module instance in several
    // placements. Its scoped owner is part of the draft's identity as well.
    let key = format!(
        "module:{:?}:{}:{}",
        context.owner, context.instance.id, parameter.id
    );
    let local_seconds = local_time
        .as_ref()
        .map_or(0.0, |time| time.to_seconds_f64());
    let resolved = TimelineEditorService::resolve_module_parameter(
        context.project,
        &context.owner,
        parameter.id,
        local_time.as_ref().copied().unwrap_or(MediaTime::zero()),
    )
    .map_err(|error| error.to_string())
    .and_then(|resolved| {
        if resolved.instance_id == context.instance.id
            && resolved.definition_id == context.definition.id
        {
            Ok(resolved)
        } else {
            Err("The parameter editor's Module document is stale".to_string())
        }
    });
    let resolution_error = resolved
        .as_ref()
        .err()
        .cloned()
        .or_else(|| local_time.as_ref().err().cloned());
    let automation = resolved
        .as_ref()
        .ok()
        .and_then(|resolved| resolved.automation.as_ref());
    let initial = resolved.as_ref().map_or_else(
        |_| parameter.default_value.clone(),
        |resolved| resolved.value.clone(),
    );
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
        &context.owner,
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
            has_resettable_override: resolved
                .as_ref()
                .is_ok_and(|resolved| resolved.has_resettable_override),
            has_automation: automation.is_some(),
            reset_label: match &context.owner {
                ModuleParameterOwner::Transition(TransitionAutomationOwner::Instance {
                    instance_path,
                    ..
                }) if !instance_path.composition_items.is_empty() => {
                    "Inherit Timeline value and animation"
                }
                _ => "Reset base to Module default",
            },
        });
        (interaction, value.clone())
    };
    let validation_error = resolution_error.clone().or_else(|| {
        definition
            .as_ref()
            .and_then(|definition| definition.validate_value(&edited_value).err())
    });
    let mut error = (interaction.changed || interaction.finished)
        .then(|| validation_error.clone())
        .flatten();

    if interaction.changed && validation_error.is_none() {
        if let Some(edit) = module_parameter_edit(
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
            .is_some_and(|edit| edit.matches_module_parameter(&context.owner, parameter.id))
    {
        view.transient_property_edit.take()
    } else {
        None
    };
    if interaction.finished && edited_value != model_value && validation_error.is_none() {
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
    if interaction.reset_to_default {
        view.transient_property_edit = None;
        let result = resolution_error
            .clone()
            .map_or(Ok(()), Err)
            .and_then(|()| require_current_revision(context.service, view.synced_revision))
            .and_then(|()| {
                context
                    .service
                    .clear_module_parameter_override(&context.owner, parameter.id)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            });
        if let Err(reset_error) = result {
            error = Some(reset_error);
        }
    }
    if let Some(action) = interaction.mode_action {
        view.transient_property_edit = None;
        let result = resolution_error
            .map_or(Ok(()), Err)
            .and_then(|()| require_current_revision(context.service, view.synced_revision))
            .and_then(|()| match &local_time {
                Ok(time) => apply_module_parameter_mode_action(
                    context.service,
                    &context.owner,
                    parameter.id,
                    automation,
                    edited_value,
                    *time,
                    action,
                ),
                Err(time_error) => Err(time_error.clone()),
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
        context.owner.clone(),
        context.instance.id,
        parameter.id,
        value,
        target,
    ))
}

fn pending_module_keyframe(
    edit: Option<&TransientPropertyEdit>,
    owner: &ModuleParameterOwner,
    parameter_id: PublishedParameterId,
) -> Option<PendingKeyframeMetadata> {
    let edit = edit.filter(|edit| edit.matches_module_parameter(owner, parameter_id))?;
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
    owner: &ModuleParameterOwner,
    parameter_id: PublishedParameterId,
    automation: Option<&AutomationTrack>,
    value: PropertyValue,
    local_time: MediaTime,
    action: PropertyModeAction,
) -> Result<(), String> {
    match action {
        PropertyModeAction::SetMode(PropertyAuthoringMode::Constant) => service
            .set_module_parameter_constant(owner, parameter_id, value)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        PropertyModeAction::SetMode(PropertyAuthoringMode::Keyframe) => service
            .upsert_module_parameter_keyframe(owner, parameter_id, local_time, value, None)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        PropertyModeAction::SetMode(PropertyAuthoringMode::Expression) => {
            Err("Module parameter expressions belong inside the Node Module".to_string())
        }
        PropertyModeAction::ToggleKeyframe => {
            if let Some(keyframe_id) = keyframe_at(automation, local_time) {
                if automation.is_some_and(|track| track.keyframes.len() == 1) {
                    service
                        .set_module_parameter_constant(owner, parameter_id, value)
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                } else {
                    service
                        .remove_module_parameter_keyframe(owner, parameter_id, keyframe_id)
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                }
            } else {
                service
                    .upsert_module_parameter_keyframe(owner, parameter_id, local_time, value, None)
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
    track.and_then(|track| {
        track
            .keyframes
            .iter()
            .find(|keyframe| keyframe.time == local_time)
            .map(|keyframe| keyframe.id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::animation::EasingFunction;
    use library::model::authoring::AutomationKeyframe;

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

    #[test]
    fn current_key_lookup_uses_exact_media_time_at_fractional_rates() {
        let first_time = MediaTime::new(1000, 30000).unwrap();
        let adjacent_time = MediaTime::new(1001, 30000).unwrap();
        let first =
            AutomationKeyframe::new(first_time, PropertyValue::from(1.0), EasingFunction::Linear);
        let first_id = first.id;
        let adjacent = AutomationKeyframe::new(
            adjacent_time,
            PropertyValue::from(2.0),
            EasingFunction::Linear,
        );
        let adjacent_id = adjacent.id;
        let track = AutomationTrack {
            keyframes: vec![first, adjacent],
        };

        assert_eq!(keyframe_at(Some(&track), first_time), Some(first_id));
        assert_eq!(keyframe_at(Some(&track), adjacent_time), Some(adjacent_id));
        assert_eq!(
            keyframe_at(Some(&track), MediaTime::new(2001, 60000).unwrap()),
            None
        );
    }
}
