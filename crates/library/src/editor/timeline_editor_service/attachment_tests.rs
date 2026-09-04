use super::*;

use crate::model::project::connection::PortDataType;

fn image_effect(component_id: &str) -> BuiltinEffectInstance {
    BuiltinEffectInstance {
        operation: crate::model::authoring::OperationRef {
            category: "effect".to_string(),
            component_id: component_id.to_string(),
            operation: "apply".to_string(),
            version: "1".to_string(),
        },
        contract: crate::model::authoring::EffectContractSnapshot {
            input_type: PortDataType::Image,
            output_type: PortDataType::Image,
            parameters: Vec::new(),
        },
        parameters: HashMap::new(),
        blend_mode: BlendMode::Normal,
    }
}

fn ordered_attachment_ids(
    project: &AuthoringProject,
    owner: &AttachmentOwner,
    stage: AttachmentStage,
) -> Vec<AttachmentId> {
    let mut entries = project
        .attachments
        .values()
        .filter(|attachment| attachment.owner == *owner && attachment.stage == stage)
        .map(|attachment| (attachment.order, attachment.id))
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| *entry);
    entries.into_iter().map(|(_, id)| id).collect()
}

#[test]
fn moving_attachment_between_stages_is_atomic_and_preserves_siblings() {
    let service = TimelineEditorService::create_default("attachment move").unwrap();
    let project = service.snapshot().unwrap();
    let timeline_id = project.root_timeline_id;
    let track_id = project.timelines[&timeline_id].track_order[0];
    drop(project);
    let (item_id, _) = service
        .add_item(
            track_id,
            "Host".to_string(),
            SourceRef::Solid {
                color: Color::black(),
            },
            TimelineInterval::new(MediaTime::zero(), MediaTime::new(5, 1).unwrap()).unwrap(),
            0,
        )
        .unwrap();
    let (sibling_item_id, _) = service
        .add_item(
            track_id,
            "Sibling host".to_string(),
            SourceRef::Solid {
                color: Color::black(),
            },
            TimelineInterval::new(MediaTime::zero(), MediaTime::new(5, 1).unwrap()).unwrap(),
            1,
        )
        .unwrap();
    let owner = AttachmentOwner::Item { item_id };
    let sibling_owner = AttachmentOwner::Item {
        item_id: sibling_item_id,
    };
    let (first, _) = service
        .add_builtin_attachment(
            owner.clone(),
            AttachmentStage::ItemPreTransform,
            image_effect("first"),
        )
        .unwrap();
    let (moved, _) = service
        .add_builtin_attachment(
            owner.clone(),
            AttachmentStage::ItemPreTransform,
            image_effect("moved"),
        )
        .unwrap();
    let (last, _) = service
        .add_builtin_attachment(
            owner.clone(),
            AttachmentStage::ItemPostTransform,
            image_effect("last"),
        )
        .unwrap();
    let (untouched, _) = service
        .add_builtin_attachment(
            sibling_owner,
            AttachmentStage::ItemPreTransform,
            image_effect("untouched"),
        )
        .unwrap();
    let before = service.snapshot().unwrap();
    let revision = service.revision().unwrap();

    let change = service
        .move_attachment(moved, AttachmentStage::ItemPostTransform, 0)
        .unwrap();
    assert_eq!(change.revision.get(), revision.get() + 1);
    let changed = service.snapshot().unwrap();
    assert_eq!(
        ordered_attachment_ids(&changed, &owner, AttachmentStage::ItemPreTransform),
        vec![first]
    );
    assert_eq!(
        ordered_attachment_ids(&changed, &owner, AttachmentStage::ItemPostTransform),
        vec![moved, last]
    );
    assert_eq!(
        changed.attachments[&untouched], before.attachments[&untouched],
        "moving one Effect must not mutate an Effect owned by a sibling item"
    );

    service.undo().unwrap().expect("one attachment move undo");
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}

#[test]
fn moving_attachment_reorders_one_stack_and_rejects_incompatible_stage() {
    let service = TimelineEditorService::create_default("attachment reorder").unwrap();
    let project = service.snapshot().unwrap();
    let timeline_id = project.root_timeline_id;
    let track_id = project.timelines[&timeline_id].track_order[0];
    drop(project);
    let (item_id, _) = service
        .add_item(
            track_id,
            "Host".to_string(),
            SourceRef::Solid {
                color: Color::black(),
            },
            TimelineInterval::new(MediaTime::zero(), MediaTime::new(5, 1).unwrap()).unwrap(),
            0,
        )
        .unwrap();
    let owner = AttachmentOwner::Item { item_id };
    let mut ids = Vec::new();
    for component in ["first", "middle", "last"] {
        ids.push(
            service
                .add_builtin_attachment(
                    owner.clone(),
                    AttachmentStage::ItemPreTransform,
                    image_effect(component),
                )
                .unwrap()
                .0,
        );
    }

    service
        .move_attachment(ids[0], AttachmentStage::ItemPreTransform, 2)
        .unwrap();
    let reordered = service.snapshot().unwrap();
    assert_eq!(
        ordered_attachment_ids(&reordered, &owner, AttachmentStage::ItemPreTransform),
        vec![ids[1], ids[2], ids[0]]
    );

    let before_invalid = service.snapshot().unwrap();
    let revision = service.revision().unwrap();
    let error = service
        .move_attachment(ids[0], AttachmentStage::TrackPostComposite, 0)
        .unwrap_err();
    assert!(error.to_string().contains("invalid"));
    assert_eq!(service.revision().unwrap(), revision);
    assert_eq!(
        service.snapshot().unwrap().as_ref(),
        before_invalid.as_ref()
    );

    let error = service
        .move_attachment(ids[0], AttachmentStage::AudioPreFader, 0)
        .unwrap_err();
    assert!(error.to_string().contains("incompatible"));
    assert_eq!(service.revision().unwrap(), revision);
    assert_eq!(
        service.snapshot().unwrap().as_ref(),
        before_invalid.as_ref()
    );
}

#[test]
fn switching_effect_automation_to_constant_is_one_undoable_edit() {
    let plugins = PluginManager::default();
    let service = TimelineEditorService::create_default("Effect parameter mode").unwrap();
    let project = service.snapshot().unwrap();
    let timeline_id = project.root_timeline_id;
    drop(project);
    let (attachment_id, _) = service
        .add_builtin_effect_by_id(
            &plugins,
            AttachmentOwner::Timeline { timeline_id },
            AttachmentStage::TimelinePostComposite,
            "blur",
        )
        .unwrap();
    service
        .upsert_builtin_effect_parameter_keyframe(
            attachment_id,
            "sigma_x",
            MediaTime::new(1, 1).unwrap(),
            PropertyValue::from(12.0),
            None,
        )
        .unwrap();
    let before = service.snapshot().unwrap();
    let revision = service.revision().unwrap();

    let change = service
        .set_builtin_effect_parameter_constant(attachment_id, "sigma_x", PropertyValue::from(3.0))
        .unwrap();
    assert_eq!(change.revision.get(), revision.get() + 1);
    let changed = service.snapshot().unwrap();
    let AttachmentProcessor::BuiltinEffect(effect) = &changed.attachments[&attachment_id].processor
    else {
        panic!("expected built-in Effect");
    };
    let parameter = &effect.parameters["sigma_x"];
    assert_eq!(parameter.value, PropertyValue::from(3.0));
    assert!(parameter.automation.is_none());
    drop(changed);

    service.undo().unwrap().expect("change");
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}
