//! Sparse placement storage for Timeline-owned Transition Module parameters.

use super::module_parameter_owner::transition_id;
use super::transition_module_controls::{
    require_editable_parameter, transition_module_context, transition_module_mut,
};
use super::*;

pub(super) fn set_transition_parameter_constant_in_project(
    project: &mut AuthoringProject,
    owner: &TransitionAutomationOwner,
    parameter_id: PublishedParameterId,
    value: PropertyValue,
) -> Result<(), String> {
    let transition_id = transition_id(owner);
    let (_, _, contract) =
        transition_module_context(project, transition_id).map_err(|error| error.to_string())?;
    require_editable_parameter(transition_id, &contract, parameter_id)
        .map_err(|error| error.to_string())?;
    match owner {
        TransitionAutomationOwner::Definition(_) => {
            let instance_id = transition_module_mut(project, transition_id)?.instance_id;
            transition_module_mut(project, transition_id)?
                .automation_tracks
                .remove(&parameter_id);
            project
                .module_instances
                .get_mut(&instance_id)
                .ok_or_else(|| format!("Missing Module instance {instance_id}"))?
                .parameter_overrides
                .insert(parameter_id, value);
        }
        TransitionAutomationOwner::Instance { instance_path, .. } => {
            let target =
                project.resolve_transition_module_instance_target(instance_path, transition_id)?;
            if instance_path.composition_items.is_empty() {
                transition_module_mut(project, transition_id)?
                    .automation_tracks
                    .remove(&parameter_id);
                project
                    .module_instances
                    .get_mut(&target.module_instance_id)
                    .ok_or_else(|| {
                        format!("Missing Module instance {}", target.module_instance_id)
                    })?
                    .parameter_overrides
                    .insert(parameter_id, value);
            } else {
                project.edit_transition_module_instance_overrides(&target, |controls| {
                    controls.parameter_overrides.insert(parameter_id, value);
                    controls.automation_tracks.insert(parameter_id, None);
                    Ok(())
                })?;
            }
        }
    }
    Ok(())
}

pub(super) fn clear_transition_parameter_override_in_project(
    project: &mut AuthoringProject,
    owner: &TransitionAutomationOwner,
    parameter_id: PublishedParameterId,
) -> Result<(), String> {
    let transition_id = transition_id(owner);
    let (_, _, contract) =
        transition_module_context(project, transition_id).map_err(|error| error.to_string())?;
    require_editable_parameter(transition_id, &contract, parameter_id)
        .map_err(|error| error.to_string())?;
    match owner {
        TransitionAutomationOwner::Definition(_) => {
            let instance_id = transition_module_mut(project, transition_id)?.instance_id;
            project
                .module_instances
                .get_mut(&instance_id)
                .ok_or_else(|| format!("Missing Module instance {instance_id}"))?
                .parameter_overrides
                .remove(&parameter_id)
                .map(|_| ())
                .ok_or_else(|| {
                    format!(
                        "Module instance {instance_id} has no override for Published parameter {parameter_id}"
                    )
                })
        }
        TransitionAutomationOwner::Instance { instance_path, .. } => {
            let target =
                project.resolve_transition_module_instance_target(instance_path, transition_id)?;
            if instance_path.composition_items.is_empty() {
                project
                    .module_instances
                    .get_mut(&target.module_instance_id)
                    .ok_or_else(|| {
                        format!("Missing Module instance {}", target.module_instance_id)
                    })?
                    .parameter_overrides
                    .remove(&parameter_id)
                    .map(|_| ())
                    .ok_or_else(|| {
                        format!(
                            "Module instance {} has no override for Published parameter {parameter_id}",
                            target.module_instance_id
                        )
                    })
            } else {
                project.edit_transition_module_instance_overrides(&target, |controls| {
                    let removed_value = controls.parameter_overrides.remove(&parameter_id).is_some();
                    let removed_automation =
                        controls.automation_tracks.remove(&parameter_id).is_some();
                    (removed_value || removed_automation).then_some(()).ok_or_else(|| {
                        format!(
                            "Transition {transition_id} concrete instance has no resettable override for Published parameter {parameter_id}"
                        )
                    })
                })
            }
        }
    }
}

pub(super) fn transition_owner_invalidations(
    project: &AuthoringProject,
    owner: &TransitionAutomationOwner,
) -> Result<Vec<ProjectInvalidation>, LibraryError> {
    let transition_id = transition_id(owner);
    let (timeline_id, interval, _) = transition_module_context(project, transition_id)?;

    if let TransitionAutomationOwner::Instance { instance_path, .. } = owner {
        project
            .resolve_transition_module_instance_target(instance_path, transition_id)
            .map_err(LibraryError::Validation)?;
        if !instance_path.composition_items.is_empty() {
            return Ok(vec![ProjectInvalidation::TimelineInstanceRange {
                instance_path: instance_path.clone(),
                timeline_id,
                transition_id,
                start: interval.start,
                duration: interval.duration,
            }]);
        }
    }

    Ok(vec![ProjectInvalidation::TimelineRange {
        timeline_id,
        start: interval.start,
        duration: interval.duration,
    }])
}

pub(super) fn edit_transition_parameter_track_in_project<T>(
    project: &mut AuthoringProject,
    owner: &TransitionAutomationOwner,
    parameter_id: PublishedParameterId,
    edit: impl FnOnce(&mut AutomationTrack) -> Result<T, String>,
) -> Result<T, String> {
    let transition_id = transition_id(owner);
    let target = match owner {
        TransitionAutomationOwner::Definition(_) => None,
        TransitionAutomationOwner::Instance { instance_path, .. } => Some((
            project.resolve_transition_module_instance_target(instance_path, transition_id)?,
            instance_path.composition_items.is_empty(),
        )),
    };
    let is_root = match &target {
        Some((_, is_root)) => *is_root,
        None => true,
    };
    let mut track = if is_root {
        transition_module_mut(project, transition_id)?
            .automation_tracks
            .get(&parameter_id)
            .cloned()
    } else {
        let (target, _) = target
            .as_ref()
            .ok_or_else(|| "Missing Transition instance target".to_string())?;
        project
            .effective_transition_module_controls(target)?
            .automation_tracks
            .get(&parameter_id)
            .cloned()
    }
    .unwrap_or(AutomationTrack {
        keyframes: Vec::new(),
    });
    let value = edit(&mut track)?;

    if is_root {
        let module = transition_module_mut(project, transition_id)?;
        if track.keyframes.is_empty() {
            module.automation_tracks.remove(&parameter_id);
        } else {
            module.automation_tracks.insert(parameter_id, track);
        }
    } else {
        let (target, _) = target
            .as_ref()
            .ok_or_else(|| "Missing Transition instance target".to_string())?;
        project.edit_transition_module_instance_overrides(target, |controls| {
            controls
                .automation_tracks
                .insert(parameter_id, (!track.keyframes.is_empty()).then_some(track));
            Ok(())
        })?;
    }
    Ok(value)
}
