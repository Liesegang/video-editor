//! Authoritative owners for Timeline-authored published Module parameters.

use super::attachment::{attachment_module_invocation_mut, attachment_owner, owner_invalidations};
use super::module::item_module_invocation_mut;
use super::transition_module_controls::transition_module_context;
use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ModuleAutomationOwner {
    Item(TimelineItemId),
    Attachment(AttachmentId),
}

/// Scope for a Timeline-owned Transition Module parameter.
/// Definition scope edits every placement; Instance scope persists a sparse
/// difference on one concrete nested Composition placement.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TransitionAutomationOwner {
    Definition(TransitionId),
    Instance {
        transition_id: TransitionId,
        instance_path: InstancePath,
    },
}

/// One published Module parameter owner across every Timeline host.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ModuleParameterOwner {
    Invocation(ModuleAutomationOwner),
    Transition(TransitionAutomationOwner),
}

impl From<ModuleAutomationOwner> for ModuleParameterOwner {
    fn from(owner: ModuleAutomationOwner) -> Self {
        Self::Invocation(owner)
    }
}

impl From<TransitionAutomationOwner> for ModuleParameterOwner {
    fn from(owner: TransitionAutomationOwner) -> Self {
        Self::Transition(owner)
    }
}

impl ModuleAutomationOwner {
    pub fn invocation(self, project: &AuthoringProject) -> Result<&ModuleInvocation, String> {
        match self {
            Self::Item(item_id) => project
                .items
                .get(&item_id)
                .ok_or_else(|| format!("Missing Timeline item {item_id}"))
                .and_then(|item| match &item.source {
                    SourceRef::Module(invocation) => Ok(invocation),
                    _ => Err(format!("Timeline item {item_id} is not a Node Clip")),
                }),
            Self::Attachment(attachment_id) => project
                .attachments
                .get(&attachment_id)
                .ok_or_else(|| format!("Missing Attachment {attachment_id}"))
                .and_then(|attachment| match &attachment.processor {
                    AttachmentProcessor::Module(invocation) => Ok(invocation),
                    AttachmentProcessor::BuiltinEffect(_) => {
                        Err(format!("Attachment {attachment_id} is not a Module Effect"))
                    }
                }),
        }
    }

    pub fn timeline_id(self, project: &AuthoringProject) -> Result<TimelineId, String> {
        match self {
            Self::Item(item_id) => {
                timeline_for_item(project, item_id).map_err(|error| error.to_string())
            }
            Self::Attachment(attachment_id) => {
                let attachment_owner =
                    attachment_owner(project, attachment_id).map_err(|error| error.to_string())?;
                match attachment_owner {
                    AttachmentOwner::Timeline { timeline_id } => Ok(timeline_id),
                    AttachmentOwner::Track { track_id } => {
                        timeline_for_track(project, track_id).map_err(|error| error.to_string())
                    }
                    AttachmentOwner::Item { item_id } => {
                        timeline_for_item(project, item_id).map_err(|error| error.to_string())
                    }
                }
            }
        }
    }

    pub(super) fn invocation_mut(
        self,
        project: &mut AuthoringProject,
    ) -> Result<&mut ModuleInvocation, String> {
        match self {
            Self::Item(item_id) => item_module_invocation_mut(project, item_id),
            Self::Attachment(attachment_id) => {
                attachment_module_invocation_mut(project, attachment_id)
            }
        }
    }

    pub(super) fn invalidations(
        self,
        project: &AuthoringProject,
    ) -> Result<Vec<ProjectInvalidation>, LibraryError> {
        match self {
            Self::Item(item_id) => Ok(vec![ProjectInvalidation::Item {
                timeline_id: self
                    .timeline_id(project)
                    .map_err(LibraryError::Validation)?,
                item_id,
            }]),
            Self::Attachment(attachment_id) => {
                let owner = attachment_owner(project, attachment_id)?;
                owner_invalidations(project, &owner)
            }
        }
    }
}

impl ModuleParameterOwner {
    pub fn instance_id(&self, project: &AuthoringProject) -> Result<ModuleInstanceId, String> {
        match self {
            Self::Invocation(owner) => Ok(owner.invocation(project)?.instance_id),
            Self::Transition(owner) => {
                let transition_id = transition_id(owner);
                let transition = project
                    .transitions
                    .get(&transition_id)
                    .ok_or_else(|| format!("Missing Transition {transition_id}"))?;
                let processor = transition
                    .processor
                    .module_processor()
                    .ok_or_else(|| format!("Transition {transition_id} does not use a Module"))?;
                if let TransitionAutomationOwner::Instance { instance_path, .. } = owner {
                    let target = project
                        .resolve_transition_module_instance_target(instance_path, transition_id)?;
                    if target.module_instance_id != processor.instance_id {
                        return Err("Transition Module instance target is stale".to_string());
                    }
                }
                Ok(processor.instance_id)
            }
        }
    }

    pub fn timeline_id(&self, project: &AuthoringProject) -> Result<TimelineId, String> {
        match self {
            Self::Invocation(owner) => owner.timeline_id(project),
            Self::Transition(owner) => {
                self.instance_id(project)?;
                transition_module_context(project, transition_id(owner))
                    .map(|(timeline_id, _, _)| timeline_id)
                    .map_err(|error| error.to_string())
            }
        }
    }

    pub(super) fn invalidations(
        &self,
        project: &AuthoringProject,
    ) -> Result<Vec<ProjectInvalidation>, LibraryError> {
        match self {
            Self::Invocation(owner) => owner.invalidations(project),
            Self::Transition(owner) => {
                super::transition_parameter_automation::transition_owner_invalidations(
                    project, owner,
                )
            }
        }
    }
}

pub(super) fn transition_id(owner: &TransitionAutomationOwner) -> TransitionId {
    match owner {
        TransitionAutomationOwner::Definition(transition_id)
        | TransitionAutomationOwner::Instance { transition_id, .. } => *transition_id,
    }
}
