use super::*;

fn keyed_edit(owner: AuthoringPropertyOwner, revision: ProjectRevision) -> TransientPropertyEdit {
    TransientPropertyEdit::authored(
        revision,
        owner,
        AuthoringPropertyValueUpdate {
            key: "opacity".into(),
            value: PropertyValue::from(0.5),
            target: AuthoringPropertyValueTarget::Keyframe {
                local_time: MediaTime::new(1, 2).unwrap(),
                insertion_id: KeyframeId::new(),
            },
        },
    )
}

#[test]
fn held_values_keep_one_id_through_projection_commit_undo_and_redo() {
    let (service, item_id) = solid_item_fixture("Reserved authored key");
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
    let revision = service.revision().unwrap();
    let mut held = keyed_edit(owner, revision);
    let reserved = held.insertion_id().unwrap();
    let mut previous_digest = held.digest();
    for value in [0.2, 0.4, 0.8] {
        let mut proposal = keyed_edit(owner, revision);
        assert_ne!(proposal.insertion_id(), Some(reserved));
        proposal.value = PropertyValue::from(value);
        held.update(proposal);
        assert_eq!(held.insertion_id(), Some(reserved));
        assert_ne!(held.digest(), previous_digest);
        previous_digest = held.digest();
        let projected = held.project(&source).unwrap();
        let keys = projected.items[&item_id]
            .authored_properties
            .get("opacity")
            .unwrap()
            .keyframes();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[1].id, reserved);
        assert_eq!(keys[1].value, PropertyValue::from(value));
        assert_eq!(service.snapshot().unwrap(), source);
        assert_eq!(service.revision().unwrap(), revision);
    }
    let projected = held.project(&source).unwrap();
    held.commit(&service).unwrap();
    assert_eq!(service.snapshot().unwrap().as_ref(), &projected);
    assert_eq!(service.revision().unwrap().get(), revision.get() + 1);
    service.undo().unwrap();
    assert_eq!(service.snapshot().unwrap(), source);
    service.redo().unwrap();
    assert_eq!(service.snapshot().unwrap().as_ref(), &projected);
}

#[test]
fn changing_owner_revision_time_or_mode_replaces_the_reserved_slot() {
    let (service, item_id) = solid_item_fixture("Property slot identity");
    let owner = AuthoringPropertyOwner::Item(item_id);
    let revision = service.revision().unwrap();
    let original = keyed_edit(owner, revision);
    service
        .set_authored_property_constant(owner, "opacity".into(), PropertyValue::from(0.8))
        .unwrap();
    let mut changed_time = keyed_edit(owner, revision);
    changed_time.value_target = AuthoringPropertyValueTarget::Keyframe {
        local_time: MediaTime::new(3, 4).unwrap(),
        insertion_id: KeyframeId::new(),
    };
    let mut constant = keyed_edit(owner, revision);
    constant.value_target = AuthoringPropertyValueTarget::Constant;
    for replacement in [
        keyed_edit(
            AuthoringPropertyOwner::Item(TimelineItemId::new()),
            revision,
        ),
        keyed_edit(owner, service.revision().unwrap()),
        changed_time,
        constant,
    ] {
        let mut held = original.clone();
        held.update(replacement.clone());
        assert_eq!(held, replacement);
        assert_ne!(held.insertion_id(), original.insertion_id());
    }
    let another_session = keyed_edit(owner, revision);
    assert_ne!(original.digest(), another_session.digest());
    assert_ne!(original.insertion_id(), another_session.insertion_id());
}

#[test]
fn module_key_insertion_projects_and_commits_the_reserved_id_without_changing_definition() {
    let (service, item_id, instance_id, parameter_id) = module_parameter_fixture();
    let color = |r| {
        PropertyValue::ColorValue(library::model::property::ColorValue::from_straight_srgba8(
            &Color {
                r,
                g: 80,
                b: 100,
                a: 255,
            },
        ))
    };
    service
        .upsert_module_parameter_keyframe(
            item_id,
            parameter_id,
            MediaTime::zero(),
            color(10),
            Some(EasingFunction::Constant),
        )
        .unwrap();
    let source = service.snapshot().unwrap();
    let revision = service.revision().unwrap();
    let edit_at = |r| {
        TransientPropertyEdit::module_parameter(
            revision,
            item_id,
            instance_id,
            parameter_id,
            color(r),
            AuthoringPropertyValueTarget::Keyframe {
                local_time: MediaTime::new(1, 2).unwrap(),
                insertion_id: KeyframeId::new(),
            },
        )
    };
    let mut held = edit_at(40);
    let reserved = held.insertion_id().unwrap();
    for r in [50, 90, 120] {
        held.update(edit_at(r));
        assert_eq!(held.insertion_id(), Some(reserved));
        let projected = held.project(&source).unwrap();
        let SourceRef::Module(invocation) = &projected.items[&item_id].source else {
            panic!()
        };
        let keys = &invocation.automation_tracks[&parameter_id].keyframes;
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[1].id, reserved);
        assert_eq!(keys[1].value, color(r));
        assert_eq!(keys[0].easing, EasingFunction::Constant);
        assert_eq!(projected.module_definitions, source.module_definitions);
        assert_eq!(projected.module_instances, source.module_instances);
        assert_eq!(service.snapshot().unwrap(), source);
    }
    let projected = held.project(&source).unwrap();
    held.commit(&service).unwrap();
    assert_eq!(service.snapshot().unwrap().as_ref(), &projected);
    assert_eq!(service.revision().unwrap().get(), revision.get() + 1);
    service.undo().unwrap();
    assert_eq!(service.snapshot().unwrap(), source);
    service.redo().unwrap();
    assert_eq!(service.snapshot().unwrap().as_ref(), &projected);
}

#[test]
fn module_constant_commit_cannot_silently_override_existing_automation() {
    let (service, item_id, instance_id, parameter_id) = module_parameter_fixture();
    let color = PropertyValue::ColorValue(
        library::model::property::ColorValue::from_straight_srgba8(&Color::white()),
    );
    service
        .upsert_module_parameter_keyframe(
            item_id,
            parameter_id,
            MediaTime::zero(),
            color.clone(),
            None,
        )
        .unwrap();
    let source = service.snapshot().unwrap();
    let revision = service.revision().unwrap();
    let edit = TransientPropertyEdit::module_parameter(
        revision,
        item_id,
        instance_id,
        parameter_id,
        color,
        AuthoringPropertyValueTarget::Constant,
    );
    assert!(edit.project(&source).is_err());
    assert!(edit.commit(&service).is_err());
    assert_eq!(service.snapshot().unwrap(), source);
    assert_eq!(service.revision().unwrap(), revision);
}
