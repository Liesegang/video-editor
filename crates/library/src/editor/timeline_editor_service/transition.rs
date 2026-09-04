use super::*;

use crate::model::authoring::{
    AutomatableParameter, ModuleDefinition, ModuleDefinitionId, ModuleDefinitionSharing,
    ModuleInstance, ModuleInstanceId, ModuleTemplateOrigin, Transition, TransitionAlignment,
    TransitionId, TransitionMediaType, TransitionProcessor, TransitionProcessorImplementation,
};

/// Complete authored payload for one first-class Timeline transition.
#[derive(Clone, PartialEq, Debug)]
pub struct TransitionPlacement {
    pub from_item_id: TimelineItemId,
    pub to_item_id: TimelineItemId,
    pub edit_point: MediaTime,
    pub duration: MediaTime,
    pub alignment: TransitionAlignment,
    pub processor: TransitionProcessor,
    pub parameters: HashMap<String, AutomatableParameter>,
}

impl TimelineEditorService {
    pub fn add_transition(
        &self,
        placement: TransitionPlacement,
    ) -> Result<(TransitionId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        let timeline_id = timeline_for_item(session.project(), placement.from_item_id)?;
        let transition_id = TransitionId::new();
        let transition = Transition {
            id: transition_id,
            timeline_id,
            from_item_id: placement.from_item_id,
            to_item_id: placement.to_item_id,
            edit_point: placement.edit_point,
            duration: placement.duration,
            alignment: placement.alignment,
            processor: placement.processor,
            parameters: placement.parameters,
        };
        let interval = transition.interval().map_err(LibraryError::Validation)?;
        session
            .transact(
                vec![ProjectInvalidation::TimelineRange {
                    timeline_id,
                    start: interval.start,
                    duration: interval.duration,
                }],
                |project| {
                    project.transitions.insert(transition_id, transition);
                    Ok(transition_id)
                },
            )
            .map_err(LibraryError::Validation)
    }

    pub fn remove_transition(
        &self,
        transition_id: TransitionId,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let transition = session
            .project()
            .transitions
            .get(&transition_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!("Missing Transition {transition_id}"))
            })?;
        let timeline_id = transition.timeline_id;
        let interval = transition.interval().map_err(LibraryError::Validation)?;
        session
            .transact(
                vec![ProjectInvalidation::TimelineRange {
                    timeline_id,
                    start: interval.start,
                    duration: interval.duration,
                }],
                |project| remove_transition_and_owned_module(project, transition_id),
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    /// Changes only the Timeline-owned duration of a Transition. Both the old
    /// and new derived intervals are covered by one exact invalidation range
    /// so shrinking a Transition also clears frames which it no longer owns.
    pub fn set_transition_duration(
        &self,
        transition_id: TransitionId,
        duration: MediaTime,
    ) -> Result<ChangeSet, LibraryError> {
        self.update_transition_timing(transition_id, Some(duration), None)
    }

    /// Changes only the Timeline-owned alignment of a Transition. Processor
    /// topology and instance values remain untouched.
    pub fn set_transition_alignment(
        &self,
        transition_id: TransitionId,
        alignment: TransitionAlignment,
    ) -> Result<ChangeSet, LibraryError> {
        self.update_transition_timing(transition_id, None, Some(alignment))
    }

    fn update_transition_timing(
        &self,
        transition_id: TransitionId,
        duration: Option<MediaTime>,
        alignment: Option<TransitionAlignment>,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let transition = session
            .project()
            .transitions
            .get(&transition_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!("Missing Transition {transition_id}"))
            })?;
        let old_interval = transition.interval().map_err(LibraryError::Validation)?;
        let mut updated = transition.clone();
        if let Some(duration) = duration {
            updated.duration = duration;
        }
        if let Some(alignment) = alignment {
            updated.alignment = alignment;
        }
        let new_interval = updated.interval().map_err(LibraryError::Validation)?;
        let invalidation =
            transition_timing_invalidation(transition.timeline_id, old_interval, new_interval)?;

        session
            .transact(vec![invalidation], |project| {
                let transition = project
                    .transitions
                    .get_mut(&transition_id)
                    .ok_or_else(|| format!("Missing Transition {transition_id}"))?;
                transition.duration = updated.duration;
                transition.alignment = updated.alignment;
                Ok(())
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    /// Explicitly promotes one built-in Transition to an editable private
    /// Node Module. Timeline placement and participants are not converted to
    /// Nodes; only the processor implementation changes.
    pub fn promote_transition_to_module(
        &self,
        transition_id: TransitionId,
        name: impl Into<String>,
    ) -> Result<(ModuleDefinitionId, ModuleInstanceId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        let transition = session
            .project()
            .transitions
            .get(&transition_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!("Missing Transition {transition_id}"))
            })?;
        if transition.processor.module_processor().is_some() {
            return Err(LibraryError::Validation(format!(
                "Transition {transition_id} already uses a Module"
            )));
        }
        if !transition.processor.is_builtin_cross_dissolve()
            && !transition.processor.is_builtin_audio_crossfade()
        {
            return Err(LibraryError::Validation(format!(
                "Transition {transition_id} uses a custom operation that cannot be losslessly promoted"
            )));
        }
        let media_type = transition.processor.contract.media_type;
        let timeline_id = transition.timeline_id;
        let interval = transition.interval().map_err(LibraryError::Validation)?;
        let (definition, _) =
            ModuleDefinition::new_transition(name, ModuleDefinitionSharing::Private, media_type)
                .map_err(LibraryError::Validation)?;
        let definition_id = definition.id;
        let instance_id = ModuleInstanceId::new();
        session
            .transact(
                vec![
                    ProjectInvalidation::TimelineRange {
                        timeline_id,
                        start: interval.start,
                        duration: interval.duration,
                    },
                    ProjectInvalidation::ModuleDefinition { definition_id },
                ],
                |project| {
                    project.module_definitions.insert(definition_id, definition);
                    project.module_instances.insert(
                        instance_id,
                        ModuleInstance {
                            id: instance_id,
                            definition_id,
                            parameter_overrides: HashMap::new(),
                        },
                    );
                    let transition = project
                        .transitions
                        .get_mut(&transition_id)
                        .ok_or_else(|| format!("Missing Transition {transition_id}"))?;
                    transition.processor = TransitionProcessor::module(instance_id, media_type);
                    transition.parameters.clear();
                    Ok((definition_id, instance_id))
                },
            )
            .map(|((definition_id, instance_id), changes)| (definition_id, instance_id, changes))
            .map_err(LibraryError::Validation)
    }

    /// Assigns an existing reusable Transition Module by creating one
    /// instance owned solely by this Timeline Transition.
    pub fn assign_transition_module(
        &self,
        transition_id: TransitionId,
        definition_id: ModuleDefinitionId,
    ) -> Result<(ModuleInstanceId, ChangeSet), LibraryError> {
        self.assign_transition_module_with_controls(
            transition_id,
            definition_id,
            HashMap::new(),
            HashMap::new(),
        )
    }

    /// Atomically assigns a reusable Transition Module together with all
    /// required public-ID inputs and Timeline-local automation. This is the
    /// creation path for definitions whose extra inputs are required.
    pub fn assign_transition_module_with_controls(
        &self,
        transition_id: TransitionId,
        definition_id: ModuleDefinitionId,
        input_bindings: HashMap<PublishedMediaInputId, MediaInputBinding>,
        automation_tracks: HashMap<PublishedParameterId, AutomationTrack>,
    ) -> Result<(ModuleInstanceId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        let transition = session
            .project()
            .transitions
            .get(&transition_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!("Missing Transition {transition_id}"))
            })?;
        let media_type = transition.processor.contract.media_type;
        let timeline_id = transition.timeline_id;
        let interval = transition.interval().map_err(LibraryError::Validation)?;
        let definition = session
            .project()
            .module_definitions
            .get(&definition_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!("Missing Module definition {definition_id}"))
            })?;
        if !matches!(
            definition.sharing,
            ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project)
                | ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::External { .. })
        ) {
            return Err(LibraryError::Validation(
                "Only a reusable Transition Module can be assigned to another Transition"
                    .to_string(),
            ));
        }
        let Some(contract) = definition.host_contract.transition() else {
            return Err(LibraryError::Validation(
                "Selected Module is not a Transition Module".to_string(),
            ));
        };
        if contract.media_type != media_type {
            return Err(LibraryError::Validation(format!(
                "Transition Module {:?} cannot replace a {:?} processor",
                contract.media_type, media_type
            )));
        }
        if input_bindings.is_empty() {
            contract
                .validate_atomic_assignment(definition)
                .map_err(LibraryError::Validation)?;
        }
        let previous_instance_id = transition
            .processor
            .module_processor()
            .map(|module| module.instance_id);
        let instance_id = ModuleInstanceId::new();
        session
            .transact(
                vec![
                    ProjectInvalidation::TimelineRange {
                        timeline_id,
                        start: interval.start,
                        duration: interval.duration,
                    },
                    ProjectInvalidation::ModuleInstance { instance_id },
                ],
                |project| {
                    if let Some(previous_instance_id) = previous_instance_id {
                        remove_instance_and_private_definition(project, previous_instance_id);
                    }
                    project.remove_transition_module_instance_overrides(transition_id);
                    project.module_instances.insert(
                        instance_id,
                        ModuleInstance {
                            id: instance_id,
                            definition_id,
                            parameter_overrides: HashMap::new(),
                        },
                    );
                    let transition = project
                        .transitions
                        .get_mut(&transition_id)
                        .ok_or_else(|| format!("Missing Transition {transition_id}"))?;
                    transition.processor = TransitionProcessor::module_with_controls(
                        instance_id,
                        media_type,
                        input_bindings,
                        automation_tracks,
                    );
                    transition.parameters.clear();
                    Ok(instance_id)
                },
            )
            .map_err(LibraryError::Validation)
    }

    /// Replaces a Module processor with an operation-backed preset and
    /// removes the Transition-owned Module instance in the same undo step.
    pub fn assign_transition_operation(
        &self,
        transition_id: TransitionId,
        processor: TransitionProcessor,
    ) -> Result<ChangeSet, LibraryError> {
        if !matches!(
            processor.implementation,
            TransitionProcessorImplementation::Operation(_)
        ) {
            return Err(LibraryError::Validation(
                "assign_transition_operation requires an operation-backed processor".to_string(),
            ));
        }
        let mut session = self.write_session()?;
        let transition = session
            .project()
            .transitions
            .get(&transition_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!("Missing Transition {transition_id}"))
            })?;
        let old_media_type = transition.processor.contract.media_type;
        require_same_transition_media_type(transition_id, old_media_type, &processor)?;
        let timeline_id = transition.timeline_id;
        let interval = transition.interval().map_err(LibraryError::Validation)?;
        let previous_instance_id = transition
            .processor
            .module_processor()
            .map(|module| module.instance_id);
        session
            .transact(
                vec![ProjectInvalidation::TimelineRange {
                    timeline_id,
                    start: interval.start,
                    duration: interval.duration,
                }],
                |project| {
                    if let Some(previous_instance_id) = previous_instance_id {
                        remove_instance_and_private_definition(project, previous_instance_id);
                    }
                    project.remove_transition_module_instance_overrides(transition_id);
                    let transition = project
                        .transitions
                        .get_mut(&transition_id)
                        .ok_or_else(|| format!("Missing Transition {transition_id}"))?;
                    transition.processor = processor;
                    transition.parameters = transition
                        .processor
                        .contract
                        .parameters
                        .iter()
                        .map(|contract| {
                            (
                                contract.key.clone(),
                                AutomatableParameter {
                                    value: contract.default_value.clone(),
                                    automation: None,
                                },
                            )
                        })
                        .collect();
                    Ok(())
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn set_transition_parameter_value(
        &self,
        transition_id: TransitionId,
        key: &str,
        value: PropertyValue,
    ) -> Result<ChangeSet, LibraryError> {
        self.edit_transition_parameter(transition_id, key, move |parameter| {
            parameter.value = value;
            Ok(())
        })
    }

    pub fn set_transition_parameter_automation(
        &self,
        transition_id: TransitionId,
        key: &str,
        automation: Option<AutomationTrack>,
    ) -> Result<ChangeSet, LibraryError> {
        self.edit_transition_parameter(transition_id, key, move |parameter| {
            parameter.automation = automation;
            Ok(())
        })
    }

    fn edit_transition_parameter(
        &self,
        transition_id: TransitionId,
        key: &str,
        edit: impl FnOnce(&mut AutomatableParameter) -> Result<(), String>,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let transition = session
            .project()
            .transitions
            .get(&transition_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!("Missing Transition {transition_id}"))
            })?;
        let timeline_id = transition.timeline_id;
        let interval = transition.interval().map_err(LibraryError::Validation)?;
        let key = key.to_string();
        session
            .transact(
                vec![ProjectInvalidation::TimelineRange {
                    timeline_id,
                    start: interval.start,
                    duration: interval.duration,
                }],
                |project| {
                    let parameter = project
                        .transitions
                        .get_mut(&transition_id)
                        .and_then(|transition| transition.parameters.get_mut(&key))
                        .ok_or_else(|| {
                            format!("Transition {transition_id} has no parameter '{key}'")
                        })?;
                    edit(parameter)
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }
}

pub(super) fn remove_transition_and_owned_module(
    project: &mut AuthoringProject,
    transition_id: TransitionId,
) -> Result<(), String> {
    project.remove_transition_module_instance_overrides(transition_id);
    let transition = project
        .transitions
        .remove(&transition_id)
        .ok_or_else(|| format!("Missing Transition {transition_id}"))?;
    if let Some(module) = transition.processor.module_processor() {
        remove_instance_and_private_definition(project, module.instance_id);
    }
    Ok(())
}

fn transition_timing_invalidation(
    timeline_id: TimelineId,
    old_interval: TimelineInterval,
    new_interval: TimelineInterval,
) -> Result<ProjectInvalidation, LibraryError> {
    let start = old_interval.start.min(new_interval.start);
    let end = old_interval
        .end()
        .map_err(LibraryError::Validation)?
        .max(new_interval.end().map_err(LibraryError::Validation)?);
    Ok(ProjectInvalidation::TimelineRange {
        timeline_id,
        start,
        duration: end.checked_sub(start).map_err(LibraryError::Validation)?,
    })
}

fn require_same_transition_media_type(
    transition_id: TransitionId,
    expected: TransitionMediaType,
    processor: &TransitionProcessor,
) -> Result<(), LibraryError> {
    if processor.contract.media_type == expected {
        Ok(())
    } else {
        Err(LibraryError::Validation(format!(
            "Transition {transition_id} cannot change media type from {expected:?} to {:?}",
            processor.contract.media_type
        )))
    }
}
