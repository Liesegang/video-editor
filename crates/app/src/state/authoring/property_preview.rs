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
                let AuthoringPropertyValueTarget::Keyframe { local_time } = self.value_target
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
                item_id,
                instance_id,
                parameter_id,
            } => match self.value_target {
                AuthoringPropertyValueTarget::Constant => service
                    .set_module_parameter(*instance_id, *parameter_id, self.value.clone())
                    .map(|_| ()),
                AuthoringPropertyValueTarget::Keyframe { local_time } => service
                    .upsert_module_parameter_keyframe(
                        *item_id,
                        *parameter_id,
                        local_time,
                        self.value.clone(),
                        None,
                    )
                    .map(|_| ()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::animation::EasingFunction;
    use library::model::authoring::{MediaTime, SourceRef, TimelineInterval};
    use library::model::frame::color::Color;
    use library::plugin::PluginManager;

    fn solid_item_fixture(title: &str) -> (TimelineEditorService, TimelineItemId) {
        let service = TimelineEditorService::create_default(title).unwrap();
        let project = service.snapshot().unwrap();
        let track_id = project.timelines[&project.root_timeline_id].track_order[0];
        let (item_id, _) = service
            .add_item(
                track_id,
                "Solid".to_string(),
                SourceRef::Solid {
                    color: Color::black(),
                },
                TimelineInterval::new(MediaTime::zero(), MediaTime::new(2, 1).unwrap()).unwrap(),
                0,
            )
            .unwrap();
        (service, item_id)
    }

    fn module_parameter_fixture() -> (
        TimelineEditorService,
        TimelineItemId,
        ModuleInstanceId,
        PublishedParameterId,
    ) {
        let plugins = PluginManager::default();
        let (service, item_id) = solid_item_fixture("Property edit commit");
        let conversion = service
            .convert_source_to_node_clip(&plugins, item_id)
            .unwrap();
        let project = service.snapshot().unwrap();
        let SourceRef::Module(invocation) = &project.items[&item_id].source else {
            panic!("converted Solid must be a Node Clip")
        };
        let parameter_id = project.module_definitions[&conversion.definition_id]
            .interface
            .parameters
            .iter()
            .find(|parameter| parameter.target.port == "color")
            .unwrap()
            .id;
        (service, item_id, invocation.instance_id, parameter_id)
    }

    fn assert_projection_matches_commit(
        service: &TimelineEditorService,
        edit: &TransientPropertyEdit,
    ) {
        let source = service.snapshot().unwrap();
        let projected = edit.project(&source).unwrap();

        edit.commit(service).unwrap();

        assert_eq!(service.snapshot().unwrap().as_ref(), &projected);
    }

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

    #[test]
    fn commit_rejects_a_stale_revision_without_overwriting_the_project() {
        let service = TimelineEditorService::create_default("Stale property edit").unwrap();
        let project = service.snapshot().unwrap();
        let item_id = TimelineItemId::new();
        let source_revision = service.revision().unwrap();
        let edit = TransientPropertyEdit::authored(
            source_revision,
            AuthoringPropertyOwner::Item(item_id),
            AuthoringPropertyValueUpdate {
                key: "opacity".to_string(),
                value: PropertyValue::from(0.25),
                target: AuthoringPropertyValueTarget::Constant,
            },
        );
        service
            .add_track(
                project.root_timeline_id,
                "Changed".to_string(),
                library::model::authoring::TimelineTrackKind::Visual,
            )
            .unwrap();
        let changed = service.snapshot().unwrap();

        let error = edit.commit(&service).expect_err("stale edit must fail");
        assert!(error.to_string().contains("started at revision"));
        assert_eq!(service.snapshot().unwrap(), changed);
    }

    #[test]
    fn module_parameter_projection_and_commit_use_the_same_typed_edit() {
        let (service, item_id, instance_id, parameter_id) = module_parameter_fixture();
        let source = service.snapshot().unwrap();
        let source_revision = service.revision().unwrap();
        let replacement = PropertyValue::ColorValue(
            library::model::property::ColorValue::from_straight_srgba8(&Color {
                r: 20,
                g: 80,
                b: 140,
                a: 255,
            }),
        );
        let edit = TransientPropertyEdit::module_parameter(
            source_revision,
            item_id,
            instance_id,
            parameter_id,
            replacement,
            AuthoringPropertyValueTarget::Constant,
        );
        let projected = edit.project(&source).unwrap();

        edit.commit(&service).unwrap();

        assert_eq!(service.snapshot().unwrap().as_ref(), &projected);
        assert_eq!(service.revision().unwrap().get(), source_revision.get() + 1);
    }

    #[test]
    fn module_keyframe_projection_and_commit_update_the_existing_time() {
        let (service, item_id, instance_id, parameter_id) = module_parameter_fixture();
        let initial = PropertyValue::ColorValue(
            library::model::property::ColorValue::from_straight_srgba8(&Color {
                r: 20,
                g: 40,
                b: 60,
                a: 255,
            }),
        );
        service
            .upsert_module_parameter_keyframe(
                item_id,
                parameter_id,
                MediaTime::zero(),
                initial,
                Some(EasingFunction::EaseInOutQuad),
            )
            .unwrap();
        let replacement = PropertyValue::ColorValue(
            library::model::property::ColorValue::from_straight_srgba8(&Color {
                r: 80,
                g: 100,
                b: 120,
                a: 255,
            }),
        );
        let edit = TransientPropertyEdit::module_parameter(
            service.revision().unwrap(),
            item_id,
            instance_id,
            parameter_id,
            replacement,
            AuthoringPropertyValueTarget::Keyframe {
                local_time: MediaTime::zero(),
            },
        );

        assert_projection_matches_commit(&service, &edit);
    }

    #[test]
    fn authored_constant_projection_and_commit_use_the_same_checked_owner() {
        let (service, item_id) = solid_item_fixture("Authored constant commit");
        let owner = AuthoringPropertyOwner::Item(item_id);
        service
            .set_authored_property_constant(owner, "opacity".into(), PropertyValue::from(1.0))
            .unwrap();
        let edit = TransientPropertyEdit::authored(
            service.revision().unwrap(),
            owner,
            AuthoringPropertyValueUpdate {
                key: "opacity".into(),
                value: PropertyValue::from(0.25),
                target: AuthoringPropertyValueTarget::Constant,
            },
        );

        assert_projection_matches_commit(&service, &edit);
    }

    #[test]
    fn authored_constant_edit_does_not_replace_keyframe_ownership() {
        let (service, item_id) = solid_item_fixture("Authored evaluator ownership");
        let owner = AuthoringPropertyOwner::Item(item_id);
        service
            .set_authored_property_keyframe_mode(
                owner,
                "opacity".into(),
                MediaTime::zero(),
                PropertyValue::from(1.0),
            )
            .unwrap();
        let source = service.snapshot().unwrap();
        let source_revision = service.revision().unwrap();
        let edit = TransientPropertyEdit::authored(
            source_revision,
            owner,
            AuthoringPropertyValueUpdate {
                key: "opacity".into(),
                value: PropertyValue::from(0.25),
                target: AuthoringPropertyValueTarget::Constant,
            },
        );

        let projection_error = edit.project(&source).unwrap_err();
        let commit_error = edit.commit(&service).unwrap_err();

        assert!(projection_error
            .to_string()
            .contains("changed from Constant"));
        assert!(commit_error.to_string().contains("changed from Constant"));
        assert_eq!(service.revision().unwrap(), source_revision);
        assert_eq!(service.snapshot().unwrap(), source);
    }

    #[test]
    fn authored_keyframe_projection_and_commit_update_the_existing_time() {
        let (service, item_id) = solid_item_fixture("Authored keyframe value commit");
        let owner = AuthoringPropertyOwner::Item(item_id);
        service
            .set_authored_property_keyframe_mode(
                owner,
                "opacity".into(),
                MediaTime::zero(),
                PropertyValue::from(1.0),
            )
            .unwrap();
        let edit = TransientPropertyEdit::authored(
            service.revision().unwrap(),
            owner,
            AuthoringPropertyValueUpdate {
                key: "opacity".into(),
                value: PropertyValue::from(0.4),
                target: AuthoringPropertyValueTarget::Keyframe {
                    local_time: MediaTime::zero(),
                },
            },
        );

        assert_projection_matches_commit(&service, &edit);
    }

    #[test]
    fn between_time_keyframe_projection_preserves_neighbors_and_effective_value() {
        let (service, item_id) = solid_item_fixture("Between-time keyframe value commit");
        let owner = AuthoringPropertyOwner::Item(item_id);
        let (first_id, _) = service
            .set_authored_property_keyframe_mode(
                owner,
                "opacity".into(),
                MediaTime::zero(),
                PropertyValue::from(0.0),
            )
            .unwrap();
        let (last_id, _) = service
            .upsert_authored_property_keyframe(
                owner,
                "opacity".into(),
                MediaTime::new(1, 1).unwrap(),
                PropertyValue::from(1.0),
                Some(EasingFunction::Constant),
            )
            .unwrap();
        let source = service.snapshot().unwrap();
        let edit = TransientPropertyEdit::authored(
            service.revision().unwrap(),
            owner,
            AuthoringPropertyValueUpdate {
                key: "opacity".into(),
                value: PropertyValue::from(0.4),
                target: AuthoringPropertyValueTarget::Keyframe {
                    local_time: MediaTime::new(1, 2).unwrap(),
                },
            },
        );

        let projected = edit.project(&source).unwrap();
        edit.commit(&service).unwrap();
        let committed = service.snapshot().unwrap();

        assert_eq!(
            source.items[&item_id]
                .authored_properties
                .get("opacity")
                .unwrap()
                .keyframes()
                .len(),
            2
        );
        for project in [&projected, committed.as_ref()] {
            let property = project.items[&item_id]
                .authored_properties
                .get("opacity")
                .unwrap();
            let keys = property.keyframes();
            assert_eq!(keys.len(), 3);
            assert_eq!(property.evaluate_at(0.5).unwrap(), PropertyValue::from(0.4));
            let first = keys.iter().find(|key| key.id == first_id).unwrap();
            assert_eq!(first.time.0, 0.0);
            assert_eq!(first.value, PropertyValue::from(0.0));
            assert_eq!(first.easing, EasingFunction::Linear);
            let last = keys.iter().find(|key| key.id == last_id).unwrap();
            assert_eq!(last.time.0, 1.0);
            assert_eq!(last.value, PropertyValue::from(1.0));
            assert_eq!(last.easing, EasingFunction::Constant);
        }
    }

    #[test]
    fn existing_keyframe_projection_and_commit_preserve_identity_and_easing() {
        let (service, item_id) = solid_item_fixture("Existing keyframe commit");
        let owner = AuthoringPropertyOwner::Item(item_id);
        let (keyframe_id, _) = service
            .set_authored_property_keyframe_mode(
                owner,
                "opacity".into(),
                MediaTime::zero(),
                PropertyValue::from(1.0),
            )
            .unwrap();
        service
            .update_keyframe(
                &AuthoringKeyframeTarget::AuthoredProperty {
                    owner,
                    key: "opacity".into(),
                },
                keyframe_id,
                AuthoringKeyframeUpdate {
                    time: None,
                    value: None,
                    easing: Some(EasingFunction::EaseInOutQuad),
                },
            )
            .unwrap();
        let edit = TransientPropertyEdit::keyframe(
            service.revision().unwrap(),
            AuthoringKeyframeTarget::AuthoredProperty {
                owner,
                key: "opacity".into(),
            },
            keyframe_id,
            MediaTime::new(1, 2).unwrap(),
            PropertyValue::from(0.6),
        );

        assert_projection_matches_commit(&service, &edit);
        let project = service.snapshot().unwrap();
        let keyframe = project.items[&item_id]
            .authored_properties
            .get("opacity")
            .unwrap()
            .keyframes()
            .into_iter()
            .find(|keyframe| keyframe.id == keyframe_id)
            .unwrap();
        assert_eq!(keyframe.time.0, 0.5);
        assert_eq!(keyframe.value, PropertyValue::from(0.6));
        assert_eq!(keyframe.easing, EasingFunction::EaseInOutQuad);
    }

    #[test]
    fn deleted_keyframe_cannot_be_recreated_by_a_stale_commit() {
        let (service, item_id) = solid_item_fixture("Deleted keyframe commit");
        let owner = AuthoringPropertyOwner::Item(item_id);
        let (keyframe_id, _) = service
            .set_authored_property_keyframe_mode(
                owner,
                "opacity".into(),
                MediaTime::zero(),
                PropertyValue::from(1.0),
            )
            .unwrap();
        let edit = TransientPropertyEdit::keyframe(
            service.revision().unwrap(),
            AuthoringKeyframeTarget::AuthoredProperty {
                owner,
                key: "opacity".into(),
            },
            keyframe_id,
            MediaTime::new(1, 2).unwrap(),
            PropertyValue::from(0.5),
        );
        service
            .remove_authored_property_keyframe(owner, "opacity", keyframe_id)
            .unwrap();
        let after_delete = service.snapshot().unwrap();

        let error = edit.commit(&service).unwrap_err();

        assert!(error.to_string().contains("started at revision"));
        assert_eq!(service.snapshot().unwrap(), after_delete);
    }
}
