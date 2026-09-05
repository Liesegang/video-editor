//! Timeline-owned controls for one Transition Module invocation.

use super::*;
use crate::model::authoring::TransitionModuleInterface;

impl TimelineEditorService {
    pub fn bind_transition_module_input(
        &self,
        transition_id: TransitionId,
        input_id: PublishedMediaInputId,
        binding: MediaInputBinding,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let (timeline_id, interval, contract) =
            transition_module_context(session.project(), transition_id)?;
        require_editable_input(transition_id, &contract, input_id)?;
        session
            .transact(
                transition_range_invalidation(timeline_id, interval),
                |project| {
                    transition_module_mut(project, transition_id)?
                        .input_bindings
                        .insert(input_id, binding);
                    Ok(())
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn unbind_transition_module_input(
        &self,
        transition_id: TransitionId,
        input_id: PublishedMediaInputId,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let (timeline_id, interval, contract) =
            transition_module_context(session.project(), transition_id)?;
        require_editable_input(transition_id, &contract, input_id)?;
        session
            .transact(
                transition_range_invalidation(timeline_id, interval),
                |project| {
                    transition_module_mut(project, transition_id)?
                        .input_bindings
                        .remove(&input_id)
                        .map(|_| ())
                        .ok_or_else(|| {
                            format!(
                                "Transition {transition_id} has no binding for Published input {input_id}"
                            )
                        })
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn set_transition_module_parameter_automation(
        &self,
        transition_id: TransitionId,
        parameter_id: PublishedParameterId,
        automation: AutomationTrack,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let (timeline_id, interval, contract) =
            transition_module_context(session.project(), transition_id)?;
        require_editable_parameter(transition_id, &contract, parameter_id)?;
        require_transition_parameter_automation(session.project(), transition_id, parameter_id)?;
        session
            .transact(
                transition_range_invalidation(timeline_id, interval),
                |project| {
                    transition_module_mut(project, transition_id)?
                        .automation_tracks
                        .insert(parameter_id, automation);
                    Ok(())
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn clear_transition_module_parameter_automation(
        &self,
        transition_id: TransitionId,
        parameter_id: PublishedParameterId,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let (timeline_id, interval, contract) =
            transition_module_context(session.project(), transition_id)?;
        require_editable_parameter(transition_id, &contract, parameter_id)?;
        session
            .transact(
                transition_range_invalidation(timeline_id, interval),
                |project| {
                    transition_module_mut(project, transition_id)?
                        .automation_tracks
                        .remove(&parameter_id)
                        .map(|_| ())
                        .ok_or_else(|| {
                            format!(
                                "Transition {transition_id} has no automation for Published parameter {parameter_id}"
                            )
                        })
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }
}

pub(super) fn transition_module_context(
    project: &AuthoringProject,
    transition_id: TransitionId,
) -> Result<(TimelineId, TimelineInterval, TransitionModuleInterface), LibraryError> {
    let transition = project
        .transitions
        .get(&transition_id)
        .ok_or_else(|| LibraryError::Validation(format!("Missing Transition {transition_id}")))?;
    let module = transition.processor.module_processor().ok_or_else(|| {
        LibraryError::Validation(format!("Transition {transition_id} does not use a Module"))
    })?;
    let instance = project
        .module_instances
        .get(&module.instance_id)
        .ok_or_else(|| {
            LibraryError::Validation(format!("Missing Module instance {}", module.instance_id))
        })?;
    let definition = project
        .module_definitions
        .get(&instance.definition_id)
        .ok_or_else(|| {
            LibraryError::Validation(format!(
                "Missing Module definition {}",
                instance.definition_id
            ))
        })?;
    let contract = definition
        .host_contract
        .transition()
        .cloned()
        .ok_or_else(|| {
            LibraryError::Validation(format!(
                "Transition {transition_id} does not use a Transition Module"
            ))
        })?;
    Ok((
        transition.timeline_id,
        transition.interval().map_err(LibraryError::Validation)?,
        contract,
    ))
}

pub(super) fn transition_module_mut(
    project: &mut AuthoringProject,
    transition_id: TransitionId,
) -> Result<&mut crate::model::authoring::TransitionModuleProcessor, String> {
    project
        .transitions
        .get_mut(&transition_id)
        .ok_or_else(|| format!("Missing Transition {transition_id}"))?
        .processor
        .module_processor_mut()
        .ok_or_else(|| format!("Transition {transition_id} does not use a Module"))
}

pub(super) fn require_editable_input(
    transition_id: TransitionId,
    contract: &TransitionModuleInterface,
    input_id: PublishedMediaInputId,
) -> Result<(), LibraryError> {
    if input_id == contract.from_input_id || input_id == contract.to_input_id {
        Err(LibraryError::Validation(format!(
            "Transition {transition_id} A/B are host-owned and cannot be explicitly bound"
        )))
    } else {
        contract
            .validate_additional_media_input(contract.media_type.port_data_type())
            .map_err(LibraryError::Validation)
    }
}

pub(super) fn require_editable_parameter(
    transition_id: TransitionId,
    contract: &TransitionModuleInterface,
    parameter_id: PublishedParameterId,
) -> Result<(), LibraryError> {
    if parameter_id == contract.progress_parameter_id {
        Err(LibraryError::Validation(format!(
            "Transition {transition_id} Progress is host-owned and cannot be automated"
        )))
    } else {
        Ok(())
    }
}

pub(super) fn require_transition_parameter_automation(
    project: &AuthoringProject,
    transition_id: TransitionId,
    parameter_id: PublishedParameterId,
) -> Result<(), LibraryError> {
    let transition = project
        .transitions
        .get(&transition_id)
        .ok_or_else(|| LibraryError::Validation(format!("Missing Transition {transition_id}")))?;
    let module = transition.processor.module_processor().ok_or_else(|| {
        LibraryError::Validation(format!("Transition {transition_id} does not use a Module"))
    })?;
    let instance = project
        .module_instances
        .get(&module.instance_id)
        .ok_or_else(|| {
            LibraryError::Validation(format!("Missing Module instance {}", module.instance_id))
        })?;
    project
        .module_definitions
        .get(&instance.definition_id)
        .ok_or_else(|| {
            LibraryError::Validation(format!(
                "Missing Module definition {}",
                instance.definition_id
            ))
        })?
        .require_parameter_automation(parameter_id)
        .map_err(LibraryError::Validation)
}

fn transition_range_invalidation(
    timeline_id: TimelineId,
    interval: TimelineInterval,
) -> Vec<ProjectInvalidation> {
    vec![ProjectInvalidation::TimelineRange {
        timeline_id,
        start: interval.start,
        duration: interval.duration,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::authoring::{ModuleDefinition, ModuleDefinitionSharing, TransitionMediaType};

    #[test]
    fn audio_transition_binding_guard_rejects_every_additional_media_input() {
        let (_, contract) = ModuleDefinition::new_transition(
            "Audio Transition",
            ModuleDefinitionSharing::Private,
            TransitionMediaType::Audio,
        )
        .unwrap();

        let error =
            require_editable_input(TransitionId::new(), &contract, PublishedMediaInputId::new())
                .expect_err("Audio Transition bindings are not executable yet");
        assert!(
            error
                .to_string()
                .contains("supplies only the host-owned A/B"),
            "{error}"
        );
    }

    #[test]
    fn image_transition_binding_guard_keeps_additional_inputs_available() {
        let (_, contract) = ModuleDefinition::new_transition(
            "Image Transition",
            ModuleDefinitionSharing::Private,
            TransitionMediaType::Image,
        )
        .unwrap();

        require_editable_input(TransitionId::new(), &contract, PublishedMediaInputId::new())
            .unwrap();
    }
}
