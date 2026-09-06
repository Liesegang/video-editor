//! One owner for Timeline-authored Module parameter values and keyframes.

use super::attachment::{attachment_module_invocation_mut, attachment_owner, owner_invalidations};
use super::module::item_module_invocation_mut;
use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ModuleAutomationOwner {
    Item(TimelineItemId),
    Attachment(AttachmentId),
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
