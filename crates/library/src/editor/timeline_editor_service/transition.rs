use super::*;

use crate::model::authoring::{
    AutomatableParameter, Transition, TransitionAlignment, TransitionId, TransitionProcessor,
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
                |project| {
                    project
                        .transitions
                        .remove(&transition_id)
                        .ok_or_else(|| format!("Missing Transition {transition_id}"))?;
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
