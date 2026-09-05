//! Concrete nested-placement controls for Transition Module invocations.

use super::transition_module_controls::{
    require_editable_input, require_editable_parameter, require_transition_parameter_automation,
    transition_module_context, transition_module_mut,
};
use super::*;
use crate::model::authoring::{InstancePath, TransitionModuleInstanceTarget};

impl TimelineEditorService {
    pub fn set_transition_module_instance_parameter(
        &self,
        instance_path: &InstancePath,
        transition_id: TransitionId,
        parameter_id: PublishedParameterId,
        value: PropertyValue,
    ) -> Result<ChangeSet, LibraryError> {
        let (_, _, contract) = {
            let session = self.read_session()?;
            transition_module_context(session.project(), transition_id)?
        };
        require_editable_parameter(transition_id, &contract, parameter_id)?;
        self.edit_transition_instance(instance_path, transition_id, |project, target, is_root| {
            if is_root {
                project
                    .module_instances
                    .get_mut(&target.module_instance_id)
                    .ok_or_else(|| {
                        format!("Missing Module instance {}", target.module_instance_id)
                    })?
                    .parameter_overrides
                    .insert(parameter_id, value);
                transition_module_mut(project, transition_id)?
                    .automation_tracks
                    .remove(&parameter_id);
            } else {
                project.edit_transition_module_instance_overrides(target, |controls| {
                    controls.parameter_overrides.insert(parameter_id, value);
                    // A concrete constant must not be shadowed by inherited automation.
                    controls.automation_tracks.insert(parameter_id, None);
                    Ok(())
                })?;
            }
            Ok(())
        })
    }

    pub fn clear_transition_module_instance_parameter(
        &self,
        instance_path: &InstancePath,
        transition_id: TransitionId,
        parameter_id: PublishedParameterId,
    ) -> Result<ChangeSet, LibraryError> {
        let (_, _, contract) = {
            let session = self.read_session()?;
            transition_module_context(session.project(), transition_id)?
        };
        require_editable_parameter(transition_id, &contract, parameter_id)?;
        self.edit_transition_instance(
            instance_path,
            transition_id,
            |project, target, is_root| {
                if is_root {
                    project
                        .module_instances
                        .get_mut(&target.module_instance_id)
                        .and_then(|instance| {
                            instance.parameter_overrides.remove(&parameter_id)
                        })
                        .map(|_| ())
                        .ok_or_else(|| {
                            format!(
                                "Module instance {} has no override for Published parameter {parameter_id}",
                                target.module_instance_id
                            )
                        })
                } else {
                    project.edit_transition_module_instance_overrides(target, |controls| {
                        controls
                            .parameter_overrides
                            .remove(&parameter_id)
                            .map(|_| ())
                            .ok_or_else(|| {
                                format!(
                                    "Transition {} concrete instance has no override for Published parameter {parameter_id}",
                                    transition_id
                                )
                            })?;
                        // Restore inherited automation if this mask came from a constant edit.
                        controls.automation_tracks.remove(&parameter_id);
                        Ok(())
                    })
                }
            },
        )
    }

    pub fn bind_transition_module_input_at_instance(
        &self,
        instance_path: &InstancePath,
        transition_id: TransitionId,
        input_id: PublishedMediaInputId,
        binding: MediaInputBinding,
    ) -> Result<ChangeSet, LibraryError> {
        let (_, _, contract) = {
            let session = self.read_session()?;
            transition_module_context(session.project(), transition_id)?
        };
        require_editable_input(transition_id, &contract, input_id)?;
        self.edit_transition_instance(instance_path, transition_id, |project, target, is_root| {
            if is_root {
                transition_module_mut(project, transition_id)?
                    .input_bindings
                    .insert(input_id, binding);
            } else {
                project.edit_transition_module_instance_overrides(target, |controls| {
                    controls.input_bindings.insert(input_id, Some(binding));
                    Ok(())
                })?;
            }
            Ok(())
        })
    }

    pub fn unbind_transition_module_input_at_instance(
        &self,
        instance_path: &InstancePath,
        transition_id: TransitionId,
        input_id: PublishedMediaInputId,
    ) -> Result<ChangeSet, LibraryError> {
        let (_, _, contract) = {
            let session = self.read_session()?;
            transition_module_context(session.project(), transition_id)?
        };
        require_editable_input(transition_id, &contract, input_id)?;
        self.edit_transition_instance(
            instance_path,
            transition_id,
            |project, target, is_root| {
                if is_root {
                    transition_module_mut(project, transition_id)?
                        .input_bindings
                        .remove(&input_id)
                        .map(|_| ())
                        .ok_or_else(|| {
                            format!(
                                "Transition {transition_id} has no binding for Published input {input_id}"
                            )
                        })
                } else {
                    let effective = project.effective_transition_module_controls(target)?;
                    if !effective.input_bindings.contains_key(&input_id) {
                        return Err(format!(
                            "Transition {transition_id} concrete instance has no binding for Published input {input_id}"
                        ));
                    }
                    project.edit_transition_module_instance_overrides(target, |controls| {
                        controls.input_bindings.insert(input_id, None);
                        Ok(())
                    })
                }
            },
        )
    }

    /// Removes the placement-level input decision so the Timeline-definition
    /// binding is visible again. This differs from `unbind`, which stores an
    /// explicit unbound value for this concrete placement.
    pub fn inherit_transition_module_input_at_instance(
        &self,
        instance_path: &InstancePath,
        transition_id: TransitionId,
        input_id: PublishedMediaInputId,
    ) -> Result<ChangeSet, LibraryError> {
        let (_, _, contract) = {
            let session = self.read_session()?;
            transition_module_context(session.project(), transition_id)?
        };
        require_editable_input(transition_id, &contract, input_id)?;
        self.edit_transition_instance(instance_path, transition_id, |project, target, is_root| {
            if is_root {
                return Err("Root Timeline controls have no inherited placement scope".into());
            }
            project.edit_transition_module_instance_overrides(target, |controls| {
                controls
                    .input_bindings
                    .remove(&input_id)
                    .map(|_| ())
                    .ok_or_else(|| {
                        format!(
                            "Transition {transition_id} concrete input already inherits its binding"
                        )
                    })
            })
        })
    }

    pub fn set_transition_module_instance_parameter_automation(
        &self,
        instance_path: &InstancePath,
        transition_id: TransitionId,
        parameter_id: PublishedParameterId,
        automation: AutomationTrack,
    ) -> Result<ChangeSet, LibraryError> {
        let (_, _, contract) = {
            let session = self.read_session()?;
            let context = transition_module_context(session.project(), transition_id)?;
            require_transition_parameter_automation(
                session.project(),
                transition_id,
                parameter_id,
            )?;
            context
        };
        require_editable_parameter(transition_id, &contract, parameter_id)?;
        self.edit_transition_instance(instance_path, transition_id, |project, target, is_root| {
            if is_root {
                transition_module_mut(project, transition_id)?
                    .automation_tracks
                    .insert(parameter_id, automation);
            } else {
                project.edit_transition_module_instance_overrides(target, |controls| {
                    controls
                        .automation_tracks
                        .insert(parameter_id, Some(automation));
                    Ok(())
                })?;
            }
            Ok(())
        })
    }

    pub fn clear_transition_module_instance_parameter_automation(
        &self,
        instance_path: &InstancePath,
        transition_id: TransitionId,
        parameter_id: PublishedParameterId,
    ) -> Result<ChangeSet, LibraryError> {
        let (_, _, contract) = {
            let session = self.read_session()?;
            transition_module_context(session.project(), transition_id)?
        };
        require_editable_parameter(transition_id, &contract, parameter_id)?;
        self.edit_transition_instance(
            instance_path,
            transition_id,
            |project, target, is_root| {
                if is_root {
                    transition_module_mut(project, transition_id)?
                        .automation_tracks
                        .remove(&parameter_id)
                        .map(|_| ())
                        .ok_or_else(|| {
                            format!(
                                "Transition {transition_id} has no automation for Published parameter {parameter_id}"
                            )
                        })
                } else {
                    let effective = project.effective_transition_module_controls(target)?;
                    if !effective.automation_tracks.contains_key(&parameter_id) {
                        return Err(format!(
                            "Transition {transition_id} concrete instance has no automation for Published parameter {parameter_id}"
                        ));
                    }
                    project.edit_transition_module_instance_overrides(target, |controls| {
                        controls.automation_tracks.insert(parameter_id, None);
                        Ok(())
                    })
                }
            },
        )
    }

    /// Removes the placement decision so definition-scope automation is
    /// inherited again. `clear` instead explicitly suppresses it.
    pub fn inherit_transition_module_instance_parameter_automation(
        &self,
        instance_path: &InstancePath,
        transition_id: TransitionId,
        parameter_id: PublishedParameterId,
    ) -> Result<ChangeSet, LibraryError> {
        let (_, _, contract) = {
            let session = self.read_session()?;
            transition_module_context(session.project(), transition_id)?
        };
        require_editable_parameter(transition_id, &contract, parameter_id)?;
        self.edit_transition_instance(
            instance_path,
            transition_id,
            |project, target, is_root| {
                if is_root {
                    return Err("Root Timeline controls have no inherited placement scope".into());
                }
                project.edit_transition_module_instance_overrides(target, |controls| {
                    controls
                        .automation_tracks
                        .remove(&parameter_id)
                        .map(|_| ())
                        .ok_or_else(|| {
                            format!(
                                "Transition {transition_id} concrete parameter already inherits automation"
                            )
                        })
                })
            },
        )
    }

    pub(super) fn edit_transition_instance(
        &self,
        instance_path: &InstancePath,
        transition_id: TransitionId,
        edit: impl FnOnce(
            &mut AuthoringProject,
            &TransitionModuleInstanceTarget,
            bool,
        ) -> Result<(), String>,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let (timeline_id, interval, _) =
            transition_module_context(session.project(), transition_id)?;
        let target = session
            .project()
            .resolve_transition_module_instance_target(instance_path, transition_id)
            .map_err(LibraryError::Validation)?;
        let is_root = instance_path.composition_items.is_empty();
        let invalidations = if is_root {
            vec![ProjectInvalidation::TimelineRange {
                timeline_id,
                start: interval.start,
                duration: interval.duration,
            }]
        } else {
            vec![ProjectInvalidation::TimelineInstanceRange {
                instance_path: instance_path.clone(),
                timeline_id,
                transition_id,
                start: interval.start,
                duration: interval.duration,
            }]
        };
        session
            .transact(invalidations, |project| edit(project, &target, is_root))
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }
}
