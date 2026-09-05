//! Immutable property gestures shared by Inspector and Curve Editor. Value
//! edits and keyframe moves use the same library mutation owners as release; drafts
//! never become a second Project or Undo history.

use std::hash::{Hash, Hasher};

use library::editor::{
    AuthoringKeyframeTarget, AuthoringKeyframeUpdate, AuthoringPropertyOwner,
    AuthoringPropertyValueTarget, AuthoringPropertyValueUpdate, TimelineEditorService,
};
use library::model::authoring::{
    AuthoringProject, ModuleInstanceId, ProjectRevision, PublishedParameterId, TimelineItemId,
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
        item_id: TimelineItemId,
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
            value_target: AuthoringPropertyValueTarget::Keyframe { local_time },
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
        item_id: TimelineItemId,
        instance_id: ModuleInstanceId,
        parameter_id: PublishedParameterId,
        value: PropertyValue,
        value_target: AuthoringPropertyValueTarget,
    ) -> Self {
        Self {
            source_revision,
            target: PropertyTarget::ModuleParameter {
                item_id,
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
        match self.value_target {
            AuthoringPropertyValueTarget::Constant => 0_u8.hash(&mut hasher),
            AuthoringPropertyValueTarget::Keyframe { local_time } => {
                1_u8.hash(&mut hasher);
                local_time.hash(&mut hasher);
            }
        }
        hasher.finish()
    }

    pub(crate) fn matches(&self, owner: AuthoringPropertyOwner, key: &str) -> bool {
        matches!(&self.target, PropertyTarget::Authored { owner: current, key: current_key }
            if *current == owner && current_key == key)
    }

    pub(crate) fn matches_module_parameter(
        &self,
        item_id: TimelineItemId,
        parameter_id: PublishedParameterId,
    ) -> bool {
        matches!(self.target, PropertyTarget::ModuleParameter {
            item_id: current_item, parameter_id: current_parameter, ..
        } if current_item == item_id && current_parameter == parameter_id)
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
                let AuthoringPropertyValueTarget::Keyframe { local_time } = self.value_target
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
                item_id,
                instance_id,
                parameter_id,
            } => TimelineEditorService::project_module_parameter_value(
                project,
                *item_id,
                *instance_id,
                *parameter_id,
                self.value.clone(),
                self.value_target,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::authoring::MediaTime;

    #[test]
    fn digest_tracks_value_owner_and_keyframe_time() {
        let item_id = TimelineItemId::new();
        let owner = AuthoringPropertyOwner::TextEnsemble {
            item_id,
            operation_id: uuid::Uuid::new_v4(),
        };
        let edit = TransientPropertyEdit::authored(
            ProjectRevision::initial(),
            owner,
            AuthoringPropertyValueUpdate {
                key: "tx".to_string(),
                value: PropertyValue::from(12.0),
                target: AuthoringPropertyValueTarget::Constant,
            },
        );
        assert_eq!(edit.digest(), edit.digest());
        assert!(edit.matches(owner, "tx"));
        assert!(!edit.matches(owner, "ty"));
        let mut changed = edit.clone();
        changed.value = PropertyValue::from(-7.0);
        assert_ne!(edit.digest(), changed.digest());
        changed = edit.clone();
        changed.value_target = AuthoringPropertyValueTarget::Keyframe {
            local_time: MediaTime::new(1, 2).unwrap(),
        };
        assert_ne!(edit.digest(), changed.digest());
        changed = edit.clone();
        changed.target = PropertyTarget::Authored {
            owner: AuthoringPropertyOwner::Item(item_id),
            key: "tx".to_string(),
        };
        assert_ne!(edit.digest(), changed.digest());
    }

    #[test]
    fn module_digest_includes_instance_parameter_and_time() {
        let item_id = TimelineItemId::new();
        let parameter_id = PublishedParameterId::new();
        let instance_id = ModuleInstanceId::new();
        let edit = TransientPropertyEdit::module_parameter(
            ProjectRevision::initial(),
            item_id,
            instance_id,
            parameter_id,
            PropertyValue::from(12.0),
            AuthoringPropertyValueTarget::Constant,
        );
        assert!(edit.matches_module_parameter(item_id, parameter_id));
        assert!(!edit.matches_module_parameter(TimelineItemId::new(), parameter_id));
        for target in [
            PropertyTarget::ModuleParameter {
                item_id,
                instance_id: ModuleInstanceId::new(),
                parameter_id,
            },
            PropertyTarget::ModuleParameter {
                item_id,
                instance_id,
                parameter_id: PublishedParameterId::new(),
            },
        ] {
            let mut changed = edit.clone();
            changed.target = target;
            assert_ne!(edit.digest(), changed.digest());
        }
        let mut changed = edit.clone();
        changed.value_target = AuthoringPropertyValueTarget::Keyframe {
            local_time: MediaTime::new(2, 1).unwrap(),
        };
        assert_ne!(edit.digest(), changed.digest());
    }

    #[test]
    fn moved_key_digest_includes_identity_time_and_owner() {
        let item_id = TimelineItemId::new();
        let target = AuthoringKeyframeTarget::AuthoredProperty {
            owner: AuthoringPropertyOwner::Item(item_id),
            key: "opacity".into(),
        };
        let key_id = KeyframeId::new();
        let edit = TransientPropertyEdit::keyframe(
            ProjectRevision::initial(),
            target.clone(),
            key_id,
            MediaTime::new(1, 1).unwrap(),
            PropertyValue::from(0.5),
        );
        let other_key = TransientPropertyEdit::keyframe(
            ProjectRevision::initial(),
            target,
            KeyframeId::new(),
            MediaTime::new(1, 1).unwrap(),
            PropertyValue::from(0.5),
        );
        assert_ne!(edit.digest(), other_key.digest());
        let mut retimed = edit.clone();
        retimed.value_target = AuthoringPropertyValueTarget::Keyframe {
            local_time: MediaTime::new(2, 1).unwrap(),
        };
        assert_ne!(edit.digest(), retimed.digest());
        let mut other_owner = edit.clone();
        other_owner.target = PropertyTarget::Keyframe {
            target: AuthoringKeyframeTarget::AuthoredProperty {
                owner: AuthoringPropertyOwner::Item(TimelineItemId::new()),
                key: "opacity".into(),
            },
            keyframe_id: key_id,
        };
        assert_ne!(edit.digest(), other_owner.digest());
    }
}
