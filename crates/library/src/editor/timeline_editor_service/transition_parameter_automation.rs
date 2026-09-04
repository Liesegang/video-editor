//! Atomic keyframe commands for Timeline-owned Transition Module parameters.

use super::transition_module_controls::{
    require_editable_parameter, transition_module_context, transition_module_mut,
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

    pub fn update_transition_parameter_keyframe(
        &self,
        owner: &TransitionAutomationOwner,
        parameter_id: PublishedParameterId,
        keyframe_id: KeyframeId,
        update: AuthoringKeyframeUpdate,
    ) -> Result<ChangeSet, LibraryError> {
        self.edit_transition_parameter_track(owner, parameter_id, move |track| {
            track.update_keyframe(keyframe_id, update.time, update.value, update.easing)
        })
        .map(|(_, changes)| changes)
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
        let transition_id = match owner {
            TransitionAutomationOwner::Definition(transition_id)
            | TransitionAutomationOwner::Instance { transition_id, .. } => *transition_id,
        };
        let (_, _, contract) = {
            let session = self.read_session()?;
            transition_module_context(session.project(), transition_id)?
        };
        require_editable_parameter(transition_id, &contract, parameter_id)?;
        match owner {
            TransitionAutomationOwner::Definition(_) => {
                let mut session = self.write_session()?;
                let (timeline_id, interval, _) =
                    transition_module_context(session.project(), transition_id)?;
                session
                    .transact(
                        vec![ProjectInvalidation::TimelineRange {
                            timeline_id,
                            start: interval.start,
                            duration: interval.duration,
                        }],
                        |project| {
                            let module = transition_module_mut(project, transition_id)?;
                            let mut track = module
                                .automation_tracks
                                .get(&parameter_id)
                                .cloned()
                                .unwrap_or(AutomationTrack {
                                    keyframes: Vec::new(),
                                });
                            let value = edit(&mut track)?;
                            if track.keyframes.is_empty() {
                                module.automation_tracks.remove(&parameter_id);
                            } else {
                                module.automation_tracks.insert(parameter_id, track);
                            }
                            Ok(value)
                        },
                    )
                    .map_err(LibraryError::Validation)
            }
            TransitionAutomationOwner::Instance { instance_path, .. } => {
                let mut result = None;
                let changes = self.edit_transition_instance(
                    instance_path,
                    transition_id,
                    |project, target, is_root| {
                        let mut track = if is_root {
                            transition_module_mut(project, transition_id)?
                                .automation_tracks
                                .get(&parameter_id)
                                .cloned()
                        } else {
                            project
                                .effective_transition_module_controls(target)?
                                .automation_tracks
                                .get(&parameter_id)
                                .cloned()
                        }
                        .unwrap_or(AutomationTrack {
                            keyframes: Vec::new(),
                        });
                        result = Some(edit(&mut track)?);
                        if is_root {
                            if track.keyframes.is_empty() {
                                transition_module_mut(project, transition_id)?
                                    .automation_tracks
                                    .remove(&parameter_id);
                            } else {
                                transition_module_mut(project, transition_id)?
                                    .automation_tracks
                                    .insert(parameter_id, track);
                            }
                        } else {
                            project.edit_transition_module_instance_overrides(
                                target,
                                |controls| {
                                    controls.automation_tracks.insert(
                                        parameter_id,
                                        (!track.keyframes.is_empty()).then_some(track),
                                    );
                                    Ok(())
                                },
                            )?;
                        }
                        Ok(())
                    },
                )?;
                let value = result.ok_or_else(|| {
                    LibraryError::Validation("Automation edit produced no result".to_string())
                })?;
                Ok((value, changes))
            }
        }
    }
}
