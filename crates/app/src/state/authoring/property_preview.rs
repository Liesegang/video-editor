//! Immutable property gestures shared by Inspector and Curve Editor. Value
//! edits and keyframe moves use the same library mutation owners as release; drafts
//! never become a second Project or Undo history.

use std::hash::{Hash, Hasher};

use library::editor::{
    AuthoringKeyframeTarget, AuthoringKeyframeUpdate, AuthoringPropertyOwner,
    AuthoringPropertyValueTarget, AuthoringPropertyValueUpdate, ModuleParameterOwner,
    TimelineEditorService,
};
use library::model::authoring::{
    AuthoringProject, ModuleInstanceId, ProjectRevision, PublishedParameterId,
};
use library::model::property::{KeyframeId, PropertyValue};
use library::LibraryError;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PropertyTarget {
    Keyframe {
        target: AuthoringKeyframeTarget,
        keyframe_id: KeyframeId,
    },
    Authored {
        owner: AuthoringPropertyOwner,
        key: String,
    },
    ModuleParameter {
        owner: ModuleParameterOwner,
        instance_id: ModuleInstanceId,
        parameter_id: PublishedParameterId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TransientPropertyEdit {
    pub(crate) source_revision: ProjectRevision,
    target: PropertyTarget,
    value: PropertyValue,
    value_target: AuthoringPropertyValueTarget,
}

impl TransientPropertyEdit {
    pub(crate) fn keyframe(
        source_revision: ProjectRevision,
        target: AuthoringKeyframeTarget,
        keyframe_id: KeyframeId,
        local_time: library::model::authoring::MediaTime,
        value: PropertyValue,
    ) -> Self {
        Self {
            source_revision,
            target: PropertyTarget::Keyframe {
                target,
                keyframe_id,
            },
            value,
            value_target: AuthoringPropertyValueTarget::Keyframe {
                local_time,
                insertion_id: keyframe_id,
            },
        }
    }

    pub(crate) fn authored(
        source_revision: ProjectRevision,
        owner: AuthoringPropertyOwner,
        update: AuthoringPropertyValueUpdate,
    ) -> Self {
        Self {
            source_revision,
            target: PropertyTarget::Authored {
                owner,
                key: update.key,
            },
            value: update.value,
            value_target: update.target,
        }
    }

    pub(crate) fn module_parameter(
        source_revision: ProjectRevision,
        owner: ModuleParameterOwner,
        instance_id: ModuleInstanceId,
        parameter_id: PublishedParameterId,
        value: PropertyValue,
        value_target: AuthoringPropertyValueTarget,
    ) -> Self {
        Self {
            source_revision,
            target: PropertyTarget::ModuleParameter {
                owner,
                instance_id,
                parameter_id,
            },
            value,
            value_target,
        }
    }

    pub(crate) fn digest(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.source_revision.get().hash(&mut hasher);
        self.target.hash(&mut hasher);
        self.value.hash(&mut hasher);
        self.value_target.hash(&mut hasher);
        hasher.finish()
    }

    /// A property row may emit many values during one scrub. Keep its reserved
    /// key identity until the edit changes owner, time, mode, or source revision.
    pub(crate) fn update(&mut self, next: Self) {
        let same_slot = match (self.value_target, next.value_target) {
            (AuthoringPropertyValueTarget::Constant, AuthoringPropertyValueTarget::Constant) => {
                true
            }
            (
                AuthoringPropertyValueTarget::Keyframe {
                    local_time: current,
                    ..
                },
                AuthoringPropertyValueTarget::Keyframe {
                    local_time: next, ..
                },
            ) => current == next,
            _ => false,
        };
        if self.source_revision == next.source_revision && self.target == next.target && same_slot {
            self.value = next.value;
        } else {
            *self = next;
        }
    }

    /// Read-only diagnostics for the identity reserved by a keyed value edit.
    /// At an already-keyed time the model retains that existing key instead.
    pub(crate) fn pending_keyframe(
        &self,
    ) -> Option<(KeyframeId, library::model::authoring::MediaTime)> {
        match self.value_target {
            AuthoringPropertyValueTarget::Keyframe {
                insertion_id,
                local_time,
            } if !matches!(self.target, PropertyTarget::Keyframe { .. }) => {
                Some((insertion_id, local_time))
            }
            _ => None,
        }
    }

    #[cfg(test)]
    fn insertion_id(&self) -> Option<KeyframeId> {
        self.pending_keyframe().map(|(id, _)| id)
    }

    pub(crate) fn matches(&self, owner: AuthoringPropertyOwner, key: &str) -> bool {
        matches!(&self.target, PropertyTarget::Authored { owner: current, key: current_key }
            if *current == owner && current_key == key)
    }

    pub(crate) fn matches_module_parameter(
        &self,
        owner: &ModuleParameterOwner,
        parameter_id: PublishedParameterId,
    ) -> bool {
        matches!(&self.target, PropertyTarget::ModuleParameter {
            owner: current_owner, parameter_id: current_parameter, ..
        } if current_owner == owner && *current_parameter == parameter_id)
    }

    pub(crate) fn project(
        &self,
        project: &AuthoringProject,
    ) -> Result<AuthoringProject, LibraryError> {
        match &self.target {
            PropertyTarget::Keyframe {
                target,
                keyframe_id,
            } => {
                let AuthoringPropertyValueTarget::Keyframe { local_time, .. } = self.value_target
                else {
                    return Err(LibraryError::Validation(
                        "A keyframe move must retain its time".into(),
                    ));
                };
                TimelineEditorService::project_keyframe_update(
                    project,
                    target,
                    *keyframe_id,
                    AuthoringKeyframeUpdate {
                        time: Some(local_time),
                        value: Some(self.value.clone()),
                        easing: None,
                    },
                )
            }
            PropertyTarget::Authored { owner, key } => {
                TimelineEditorService::project_authored_property_values(
                    project,
                    *owner,
                    vec![AuthoringPropertyValueUpdate {
                        key: key.clone(),
                        value: self.value.clone(),
                        target: self.value_target,
                    }],
                )
            }
            PropertyTarget::ModuleParameter {
                owner,
                instance_id,
                parameter_id,
            } => TimelineEditorService::project_module_parameter_value(
                project,
                owner,
                *instance_id,
                *parameter_id,
                self.value.clone(),
                self.value_target,
            ),
        }
    }

    /// Commits the exact typed target used by [`Self::project`]. The revision
    /// guard prevents a held UI draft from overwriting a newer Project edit.
    pub(crate) fn commit(&self, service: &TimelineEditorService) -> Result<(), LibraryError> {
        let current_revision = service.revision()?;
        if current_revision != self.source_revision {
            return Err(LibraryError::Validation(format!(
                "Property edit started at revision {} but the Project is now at revision {}",
                self.source_revision.get(),
                current_revision.get()
            )));
        }
        match &self.target {
            PropertyTarget::Keyframe {
                target,
                keyframe_id,
            } => {
                let AuthoringPropertyValueTarget::Keyframe { local_time, .. } = self.value_target
                else {
                    return Err(LibraryError::Validation(
                        "A keyframe move must retain its time".into(),
                    ));
                };
                service
                    .update_keyframe(
                        target,
                        *keyframe_id,
                        AuthoringKeyframeUpdate {
                            time: Some(local_time),
                            value: Some(self.value.clone()),
                            easing: None,
                        },
                    )
                    .map(|_| ())
            }
            PropertyTarget::Authored { owner, key } => service
                .apply_authored_property_values(
                    *owner,
                    vec![AuthoringPropertyValueUpdate {
                        key: key.clone(),
                        value: self.value.clone(),
                        target: self.value_target,
                    }],
                )
                .map(|_| ()),
            PropertyTarget::ModuleParameter {
                owner,
                instance_id,
                parameter_id,
            } => service
                .apply_module_parameter_value(
                    owner,
                    *instance_id,
                    *parameter_id,
                    self.value.clone(),
                    self.value_target,
                )
                .map(|_| ()),
        }
    }
}

#[cfg(test)]
mod tests;
