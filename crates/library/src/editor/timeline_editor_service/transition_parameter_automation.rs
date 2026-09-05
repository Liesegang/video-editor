//! Atomic keyframe commands for Timeline-owned Transition Module parameters.

use super::transition_module_controls::{
    require_editable_parameter, require_transition_parameter_automation, transition_module_context,
    transition_module_mut,
};
use super::*;

impl TimelineEditorService {
    pub fn upsert_transition_parameter_keyframe(
        &self,
        owner: &TransitionAutomationOwner,
        parameter_id: PublishedParameterId,
        local_time: MediaTime,
        value: PropertyValue,
        easing: Option<EasingFunction>,
    ) -> Result<(KeyframeId, ChangeSet), LibraryError> {
        self.edit_transition_parameter_track(owner, parameter_id, move |track| {
            track.upsert(local_time, value, easing)
        })
    }

    pub fn remove_transition_parameter_keyframe(
        &self,
        owner: &TransitionAutomationOwner,
        parameter_id: PublishedParameterId,
        keyframe_id: KeyframeId,
    ) -> Result<ChangeSet, LibraryError> {
        let (_, changes) =
            self.edit_transition_parameter_track(owner, parameter_id, move |track| {
                if track.remove_keyframe(keyframe_id) {
                    Ok(track.keyframes.is_empty())
                } else {
                    Err(format!("Missing Automation Keyframe {keyframe_id}"))
                }
            })?;
        Ok(changes)
    }

    pub fn set_transition_parameter_constant(
        &self,
        owner: &TransitionAutomationOwner,
        parameter_id: PublishedParameterId,
        value: PropertyValue,
    ) -> Result<ChangeSet, LibraryError> {
        match owner {
            TransitionAutomationOwner::Definition(transition_id) => {
                let mut session = self.write_session()?;
                let (timeline_id, interval, contract) =
                    transition_module_context(session.project(), *transition_id)?;
                require_editable_parameter(*transition_id, &contract, parameter_id)?;
                session
                    .transact(
                        vec![ProjectInvalidation::TimelineRange {
                            timeline_id,
                            start: interval.start,
                            duration: interval.duration,
                        }],
                        |project| {
                            let instance_id =
                                transition_module_mut(project, *transition_id)?.instance_id;
                            transition_module_mut(project, *transition_id)?
                                .automation_tracks
                                .remove(&parameter_id);
                            project
                                .module_instances
                                .get_mut(&instance_id)
                                .ok_or_else(|| format!("Missing Module instance {instance_id}"))?
                                .parameter_overrides
                                .insert(parameter_id, value);
                            Ok(())
                        },
                    )
                    .map(|(_, changes)| changes)
                    .map_err(LibraryError::Validation)
            }
            TransitionAutomationOwner::Instance {
                transition_id,
                instance_path,
            } => self.set_transition_module_instance_parameter(
                instance_path,
                *transition_id,
                parameter_id,
                value,
            ),
        }
    }

    fn edit_transition_parameter_track<T>(
        &self,
        owner: &TransitionAutomationOwner,
        parameter_id: PublishedParameterId,
        edit: impl FnOnce(&mut AutomationTrack) -> Result<T, String>,
    ) -> Result<(T, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        let invalidations =
            transition_parameter_invalidations(session.project(), owner, parameter_id)?;
        session
            .transact(invalidations, |project| {
                edit_transition_parameter_track_in_project(project, owner, parameter_id, edit)
            })
            .map_err(LibraryError::Validation)
    }
}

pub(super) fn transition_parameter_invalidations(
    project: &AuthoringProject,
    owner: &TransitionAutomationOwner,
    parameter_id: PublishedParameterId,
) -> Result<Vec<ProjectInvalidation>, LibraryError> {
    let transition_id = match owner {
        TransitionAutomationOwner::Definition(transition_id)
        | TransitionAutomationOwner::Instance { transition_id, .. } => *transition_id,
    };
    let (timeline_id, interval, contract) = transition_module_context(project, transition_id)?;
    require_transition_parameter_automation(project, transition_id, parameter_id)?;
    require_editable_parameter(transition_id, &contract, parameter_id)?;

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
    let transition_id = match owner {
        TransitionAutomationOwner::Definition(transition_id)
        | TransitionAutomationOwner::Instance { transition_id, .. } => *transition_id,
    };
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
