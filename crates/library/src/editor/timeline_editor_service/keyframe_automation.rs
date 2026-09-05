//! Shared, ID-preserving keyframe updates for committed and transient edits.

use super::attachment::{attachment_owner, builtin_effect_parameter_mut, owner_invalidations};
use super::authoring::{authored_properties_mut, property_owner_invalidations};
use super::module::{item_module_invocation_mut, require_item_parameter_automation};
use super::transition_parameter_automation::{
    edit_transition_parameter_track_in_project, transition_parameter_invalidations,
};
use super::*;

/// Identifies one authoritative automation Track without embedding a second
/// copy of its owning model in the editor.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AuthoringKeyframeTarget {
    AuthoredProperty {
        owner: AuthoringPropertyOwner,
        key: String,
    },
    ModuleParameter {
        item_id: TimelineItemId,
        parameter_id: PublishedParameterId,
    },
    BuiltinEffectParameter {
        attachment_id: AttachmentId,
        key: String,
    },
    TransitionParameter {
        owner: TransitionAutomationOwner,
        parameter_id: PublishedParameterId,
    },
}

impl TimelineEditorService {
    /// Updates an existing Keyframe in one undoable transaction. This never
    /// creates a replacement Keyframe when `keyframe_id` is missing.
    pub fn update_keyframe(
        &self,
        target: &AuthoringKeyframeTarget,
        keyframe_id: KeyframeId,
        update: AuthoringKeyframeUpdate,
    ) -> Result<ChangeSet, LibraryError> {
        validate_keyframe_update(&update)?;
        let mut session = self.write_session()?;
        let invalidations = keyframe_target_invalidations(session.project(), target)?;
        session
            .transact(invalidations, |project| {
                apply_keyframe_update(project, target, keyframe_id, update)
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    /// Projects the same checked Keyframe mutation onto an immutable Project
    /// snapshot without changing revision, history, or the authoritative
    /// session. Used by held Curve/Dope Sheet gestures for live Preview.
    pub fn project_keyframe_update(
        project: &AuthoringProject,
        target: &AuthoringKeyframeTarget,
        keyframe_id: KeyframeId,
        update: AuthoringKeyframeUpdate,
    ) -> Result<AuthoringProject, LibraryError> {
        validate_keyframe_update(&update)?;
        keyframe_target_invalidations(project, target)?;
        let mut projected = project.clone();
        apply_keyframe_update(&mut projected, target, keyframe_id, update)
            .map_err(LibraryError::Validation)?;
        projected.validate().map_err(LibraryError::Validation)?;
        Ok(projected)
    }
}

fn validate_keyframe_update(update: &AuthoringKeyframeUpdate) -> Result<(), LibraryError> {
    if update.time.is_some_and(MediaTime::is_negative) {
        return Err(LibraryError::Validation(
            "Keyframe time must be non-negative".to_string(),
        ));
    }
    Ok(())
}

fn keyframe_target_invalidations(
    project: &AuthoringProject,
    target: &AuthoringKeyframeTarget,
) -> Result<Vec<ProjectInvalidation>, LibraryError> {
    match target {
        AuthoringKeyframeTarget::AuthoredProperty { owner, .. } => {
            property_owner_invalidations(project, *owner)
        }
        AuthoringKeyframeTarget::ModuleParameter {
            item_id,
            parameter_id,
        } => {
            require_item_parameter_automation(project, *item_id, *parameter_id)?;
            Ok(vec![ProjectInvalidation::Item {
                timeline_id: timeline_for_item(project, *item_id)?,
                item_id: *item_id,
            }])
        }
        AuthoringKeyframeTarget::BuiltinEffectParameter { attachment_id, .. } => {
            let owner = attachment_owner(project, *attachment_id)?;
            owner_invalidations(project, &owner)
        }
        AuthoringKeyframeTarget::TransitionParameter {
            owner,
            parameter_id,
        } => transition_parameter_invalidations(project, owner, *parameter_id),
    }
}

fn apply_keyframe_update(
    project: &mut AuthoringProject,
    target: &AuthoringKeyframeTarget,
    keyframe_id: KeyframeId,
    update: AuthoringKeyframeUpdate,
) -> Result<(), String> {
    match target {
        AuthoringKeyframeTarget::AuthoredProperty { owner, key } => {
            let property = authored_properties_mut(project, *owner)?
                .get_mut(key)
                .ok_or_else(|| format!("Missing authored Property '{key}'"))?;
            property
                .update_keyframe_by_id(
                    keyframe_id,
                    crate::model::property::KeyframeUpdate {
                        time: update.time.map(MediaTime::to_seconds_f64),
                        value: update.value,
                        easing: update.easing,
                    },
                )
                .then_some(())
                .ok_or_else(|| format!("Missing Keyframe {keyframe_id}"))
        }
        AuthoringKeyframeTarget::ModuleParameter {
            item_id,
            parameter_id,
        } => item_module_invocation_mut(project, *item_id)?
            .automation_tracks
            .get_mut(parameter_id)
            .ok_or_else(|| format!("Missing automation for Published parameter {parameter_id}"))?
            .update_keyframe(keyframe_id, update.time, update.value, update.easing),
        AuthoringKeyframeTarget::BuiltinEffectParameter { attachment_id, key } => {
            builtin_effect_parameter_mut(project, *attachment_id, key)?
                .automation
                .as_mut()
                .ok_or_else(|| format!("Effect parameter '{key}' has no Automation"))?
                .update_keyframe(keyframe_id, update.time, update.value, update.easing)
        }
        AuthoringKeyframeTarget::TransitionParameter {
            owner,
            parameter_id,
        } => edit_transition_parameter_track_in_project(project, owner, *parameter_id, |track| {
            track.update_keyframe(keyframe_id, update.time, update.value, update.easing)
        }),
    }
}
