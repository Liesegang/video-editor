//! Contract tests for transient Curve/Dope Sheet keyframe projection.
//!
//! Projection and release use the same target and update command. These tests
//! keep that boundary pure, ID-preserving, isolated, and exactly undoable.

use std::sync::Arc;

use ordered_float::OrderedFloat;

use super::node_clip_conversion_tests::{color, rendered_pixels, small_service, time};
use super::transition_parameter_automation_tests::{
    transition_project, wrap_with_two_composition_instances,
};
use super::*;
use crate::animation::EasingFunction;
use crate::editor::{AppearanceOperationFactory, TextEnsembleOperationKind};
use crate::model::authoring::{AttachmentProcessor, ModuleDefinitionSharing};
use crate::model::project::property::{ColorValue, Keyframe, Property};
use crate::plugin::PROPERTY_PORT_PREFIX;

struct TextFixture {
    service: TimelineEditorService,
    plugins: Arc<PluginManager>,
    item_id: TimelineItemId,
    fill_id: uuid::Uuid,
    tracking_id: uuid::Uuid,
    size_key: KeyframeId,
    fill_key: KeyframeId,
    tracking_key: KeyframeId,
}

fn text_fixture() -> TextFixture {
    let plugins = Arc::new(PluginManager::default());
    let (service, track_id) = small_service("Keyframe projection");
    let fill = AppearanceOperationFactory::create(plugins.as_ref(), "fill").expect("Fill");
    let fill_id = fill.id;
    let (item_id, _) = service
        .add_item(
            track_id,
            "Projected title".to_string(),
            SourceRef::Text {
                text: "ABCD".to_string(),
                appearance_operations: vec![fill],
                ensemble_operations: Vec::new(),
            },
            TimelineInterval::new(time(1), time(3)).expect("Text interval"),
            0,
        )
        .expect("Text item");
    let (tracking_id, _) = service
        .add_text_ensemble_operation_by_id(
            plugins.as_ref(),
            item_id,
            TextEnsembleOperationKind::Effector,
            "tracking",
        )
        .expect("Tracking");
    let (size_key, _) = service
        .set_authored_property_keyframe_mode(
            AuthoringPropertyOwner::Item(item_id),
            "size".to_string(),
            MediaTime::zero(),
            PropertyValue::from(48.0),
        )
        .expect("Size Keyframe");
    let (fill_key, _) = service
        .set_authored_property_keyframe_mode(
            AuthoringPropertyOwner::Appearance {
                item_id,
                operation_id: fill_id,
            },
            "color".to_string(),
            MediaTime::zero(),
            PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&color(220, 40, 80, 255))),
        )
        .expect("Fill Keyframe");
    let (tracking_key, _) = service
        .set_authored_property_keyframe_mode(
            AuthoringPropertyOwner::TextEnsemble {
                item_id,
                operation_id: tracking_id,
            },
            "amount".to_string(),
            MediaTime::zero(),
            PropertyValue::from(0.0),
        )
        .expect("Tracking Keyframe");
    TextFixture {
        service,
        plugins,
        item_id,
        fill_id,
        tracking_id,
        size_key,
        fill_key,
        tracking_key,
    }
}

fn authored_property<'a>(
    project: &'a AuthoringProject,
    owner: &AuthoringPropertyOwner,
    key: &str,
) -> &'a Property {
    let properties = match owner {
        AuthoringPropertyOwner::Timeline(timeline_id) => {
            &project.timelines[timeline_id].authored_properties
        }
        AuthoringPropertyOwner::Track(track_id) => &project.tracks[track_id].authored_properties,
        AuthoringPropertyOwner::Item(item_id) => &project.items[item_id].authored_properties,
        AuthoringPropertyOwner::TextEnsemble {
            item_id,
            operation_id,
        } => {
            let SourceRef::Text {
                ensemble_operations,
                ..
            } = &project.items[item_id].source
            else {
                panic!("Text Ensemble owner must reference Text")
            };
            &ensemble_operations
                .iter()
                .find(|operation| operation.id == *operation_id)
                .expect("Ensemble operation")
                .properties
        }
        AuthoringPropertyOwner::Appearance {
            item_id,
            operation_id,
        } => {
            let SourceRef::Text {
                appearance_operations,
                ..
            } = &project.items[item_id].source
            else {
                panic!("Appearance owner must reference Text in this fixture")
            };
            &appearance_operations
                .iter()
                .find(|operation| operation.id == *operation_id)
                .expect("Appearance operation")
                .properties
        }
    };
    properties.get(key).expect("authored Property")
}

fn authored_keyframe(
    project: &AuthoringProject,
    owner: &AuthoringPropertyOwner,
    key: &str,
    keyframe_id: KeyframeId,
) -> Keyframe {
    authored_property(project, owner, key)
        .keyframe_by_id(keyframe_id)
        .expect("authored Keyframe")
}

fn assert_authored_projection_cycle(
    service: &TimelineEditorService,
    owner: AuthoringPropertyOwner,
    key: &str,
    keyframe_id: KeyframeId,
    update: AuthoringKeyframeUpdate,
) {
    let target = AuthoringKeyframeTarget::AuthoredProperty {
        owner,
        key: key.to_string(),
    };
    let before = service.snapshot().expect("baseline");
    let source_revision = service.revision().expect("source revision");
    let editing = TimelineEditorService::new(before.as_ref().clone()).expect("clean edit session");
    let revision = editing.revision().expect("revision");
    assert!(!editing.can_undo().expect("clean history"));
    let projected = TimelineEditorService::project_keyframe_update(
        &before,
        &target,
        keyframe_id,
        update.clone(),
    )
    .expect("project Keyframe");
    assert_eq!(editing.revision().expect("pure revision"), revision);
    assert!(!editing.can_undo().expect("projection history"));
    assert_eq!(
        service.revision().expect("source revision"),
        source_revision
    );
    assert_eq!(service.snapshot().expect("pure snapshot"), before);

    let projected_key = authored_keyframe(&projected, &owner, key, keyframe_id);
    if let Some(time) = update.time {
        assert_eq!(projected_key.time, OrderedFloat(time.to_seconds_f64()));
    }
    if let Some(value) = &update.value {
        assert_eq!(&projected_key.value, value);
    }
    if let Some(easing) = &update.easing {
        assert_eq!(&projected_key.easing, easing);
    }

    let changes = editing
        .update_keyframe(&target, keyframe_id, update)
        .expect("commit Keyframe");
    assert_eq!(changes.revision.get(), revision.get() + 1);
    assert_eq!(
        editing.snapshot().expect("committed snapshot").as_ref(),
        &projected
    );
    assert!(editing.can_undo().expect("one committed command"));
    editing.undo().expect("Undo").expect("one command");
    assert_eq!(editing.snapshot().expect("Undo snapshot"), before);
    assert!(!editing.can_undo().expect("single command undone"));
}

#[test]
fn direct_text_ensemble_and_appearance_projection_match_one_undoable_commit() {
    let fixture = text_fixture();
    assert_authored_projection_cycle(
        &fixture.service,
        AuthoringPropertyOwner::Item(fixture.item_id),
        "size",
        fixture.size_key,
        AuthoringKeyframeUpdate {
            time: Some(MediaTime::new(1, 2).expect("half second")),
            value: Some(PropertyValue::from(64.0)),
            easing: Some(EasingFunction::EaseInOutQuad),
        },
    );
    assert_authored_projection_cycle(
        &fixture.service,
        AuthoringPropertyOwner::TextEnsemble {
            item_id: fixture.item_id,
            operation_id: fixture.tracking_id,
        },
        "amount",
        fixture.tracking_key,
        AuthoringKeyframeUpdate {
            time: Some(MediaTime::new(1, 3).expect("third second")),
            value: Some(PropertyValue::from(24.0)),
            easing: Some(EasingFunction::EaseOutQuad),
        },
    );
    assert_authored_projection_cycle(
        &fixture.service,
        AuthoringPropertyOwner::Appearance {
            item_id: fixture.item_id,
            operation_id: fixture.fill_id,
        },
        "color",
        fixture.fill_key,
        AuthoringKeyframeUpdate {
            time: None,
            value: Some(PropertyValue::ColorValue(ColorValue::from_straight_srgba8(
                &color(30, 210, 120, 255),
            ))),
            easing: Some(EasingFunction::Constant),
        },
    );
}

fn invocation(project: &AuthoringProject, item_id: TimelineItemId) -> &ModuleInvocation {
    let SourceRef::Module(invocation) = &project.items[&item_id].source else {
        panic!("expected promoted Node Clip")
    };
    invocation
}

fn tracking_parameter(
    project: &AuthoringProject,
    item_id: TimelineItemId,
    operation_id: uuid::Uuid,
) -> PublishedParameterId {
    let invocation = invocation(project, item_id);
    let instance = &project.module_instances[&invocation.instance_id];
    project.module_definitions[&instance.definition_id]
        .interface
        .parameters
        .iter()
        .find(|parameter| {
            parameter.target.node_id == operation_id
                && parameter.target.port == format!("{PROPERTY_PORT_PREFIX}amount")
        })
        .expect("published Tracking amount")
        .id
}

#[test]
fn promoted_tracking_projection_changes_pixels_and_isolates_the_sibling_instance() {
    let fixture = text_fixture();
    fixture
        .service
        .convert_source_to_node_clip(fixture.plugins.as_ref(), fixture.item_id)
        .expect("promote Text");
    let (sibling_id, _) = fixture
        .service
        .duplicate_item(fixture.item_id, time(3), 1)
        .expect("duplicate Node Clip");
    let before = fixture.service.snapshot().expect("promoted baseline");
    let revision = fixture.service.revision().expect("revision");
    let parameter_id = tracking_parameter(&before, fixture.item_id, fixture.tracking_id);
    let target = AuthoringKeyframeTarget::ModuleParameter {
        item_id: fixture.item_id,
        parameter_id,
    };
    let definition_id =
        before.module_instances[&invocation(&before, fixture.item_id).instance_id].definition_id;
    assert_eq!(
        before.module_definitions[&definition_id].sharing,
        ModuleDefinitionSharing::SharedLocal
    );
    let sibling_before = invocation(&before, sibling_id).clone();
    let definition_before = before.module_definitions[&definition_id].clone();
    let update = AuthoringKeyframeUpdate {
        time: None,
        value: Some(PropertyValue::from(28.0)),
        easing: Some(EasingFunction::EaseInOutQuad),
    };
    let projected = TimelineEditorService::project_keyframe_update(
        &before,
        &target,
        fixture.tracking_key,
        update.clone(),
    )
    .expect("project promoted Tracking");
    assert_eq!(fixture.service.snapshot().expect("pure snapshot"), before);
    assert_eq!(fixture.service.revision().expect("pure revision"), revision);
    assert_eq!(invocation(&projected, sibling_id), &sibling_before);
    assert_eq!(
        projected.module_definitions[&definition_id],
        definition_before
    );
    assert_ne!(
        rendered_pixels(&before, Arc::clone(&fixture.plugins), 30),
        rendered_pixels(&projected, Arc::clone(&fixture.plugins), 30),
        "projected Tracking amount must reach production pixels"
    );

    fixture
        .service
        .update_keyframe(&target, fixture.tracking_key, update)
        .expect("commit promoted Tracking");
    let committed = fixture.service.snapshot().expect("committed");
    assert_eq!(committed.as_ref(), &projected);
    assert_eq!(
        rendered_pixels(&committed, Arc::clone(&fixture.plugins), 30),
        rendered_pixels(&projected, Arc::clone(&fixture.plugins), 30)
    );
    let encoded = ProjectDocument::new(committed.as_ref().clone())
        .to_json()
        .expect("persist projected Tracking");
    assert_eq!(
        ProjectDocument::from_json(&encoded)
            .expect("load projected Tracking")
            .project,
        committed.as_ref().clone()
    );
    fixture.service.undo().expect("Undo").expect("one command");
    assert_eq!(fixture.service.snapshot().expect("Undo state"), before);
}

#[test]
fn builtin_effect_projection_preserves_key_identity_and_matches_commit() {
    let plugins = PluginManager::default();
    let service = TimelineEditorService::create_default("Effect Keyframe projection")
        .expect("default project");
    let timeline_id = service.snapshot().expect("project").root_timeline_id;
    let (attachment_id, _) = service
        .add_builtin_effect_by_id(
            &plugins,
            AttachmentOwner::Timeline { timeline_id },
            AttachmentStage::TimelinePostComposite,
            "blur",
        )
        .expect("Blur");
    let (keyframe_id, _) = service
        .upsert_builtin_effect_parameter_keyframe(
            attachment_id,
            "sigma_x",
            MediaTime::zero(),
            PropertyValue::from(2.0),
            Some(EasingFunction::Linear),
        )
        .expect("Blur Keyframe");
    let target = AuthoringKeyframeTarget::BuiltinEffectParameter {
        attachment_id,
        key: "sigma_x".to_string(),
    };
    let before = service.snapshot().expect("baseline");
    let revision = service.revision().expect("revision");
    let update = AuthoringKeyframeUpdate {
        time: Some(time(1)),
        value: Some(PropertyValue::from(7.5)),
        easing: Some(EasingFunction::Constant),
    };
    let projected = TimelineEditorService::project_keyframe_update(
        &before,
        &target,
        keyframe_id,
        update.clone(),
    )
    .expect("project Blur Keyframe");
    let AttachmentProcessor::BuiltinEffect(effect) =
        &projected.attachments[&attachment_id].processor
    else {
        panic!("built-in Blur")
    };
    let keyframe = effect.parameters["sigma_x"]
        .automation
        .as_ref()
        .expect("Blur automation")
        .keyframes
        .iter()
        .find(|keyframe| keyframe.id == keyframe_id)
        .expect("same Blur Keyframe");
    assert_eq!(keyframe.time, time(1));
    assert_eq!(keyframe.value, PropertyValue::from(7.5));
    assert_eq!(keyframe.easing, EasingFunction::Constant);
    assert_eq!(service.snapshot().expect("pure snapshot"), before);

    let changes = service
        .update_keyframe(&target, keyframe_id, update)
        .expect("commit Blur Keyframe");
    assert_eq!(changes.revision.get(), revision.get() + 1);
    assert_eq!(service.snapshot().expect("committed").as_ref(), &projected);
    service.undo().expect("Undo").expect("one command");
    assert_eq!(service.snapshot().expect("Undo state"), before);
}

#[test]
fn transition_instance_projection_is_path_local_and_matches_commit() {
    let (project, transition_id, parameter_id) = transition_project();
    let definition_service = TimelineEditorService::new(project).expect("definition service");
    definition_service
        .upsert_transition_parameter_keyframe(
            &TransitionAutomationOwner::Definition(transition_id),
            parameter_id,
            MediaTime::zero(),
            PropertyValue::from(1.0),
            Some(EasingFunction::Linear),
        )
        .expect("definition Keyframe");
    let mut nested = definition_service
        .snapshot()
        .expect("definition project")
        .as_ref()
        .clone();
    let nested_timeline_id = nested.root_timeline_id;
    let (root_timeline_id, first_item_id, second_item_id) =
        wrap_with_two_composition_instances(&mut nested, nested_timeline_id);
    nested.validate().expect("nested project");
    let service = TimelineEditorService::new(nested).expect("nested service");
    let first_path = InstancePath::root(root_timeline_id).nested(first_item_id);
    let second_path = InstancePath::root(root_timeline_id).nested(second_item_id);
    let first_owner = TransitionAutomationOwner::Instance {
        transition_id,
        instance_path: first_path.clone(),
    };
    let (keyframe_id, _) = service
        .upsert_transition_parameter_keyframe(
            &first_owner,
            parameter_id,
            time(1),
            PropertyValue::from(2.0),
            Some(EasingFunction::EaseOutQuad),
        )
        .expect("placement Keyframe");
    let before = service.snapshot().expect("instance baseline");
    let first_target = before
        .resolve_transition_module_instance_target(&first_path, transition_id)
        .expect("first target");
    let second_target = before
        .resolve_transition_module_instance_target(&second_path, transition_id)
        .expect("second target");
    let sibling_before = before
        .effective_transition_module_controls(&second_target)
        .expect("sibling controls");
    let target = AuthoringKeyframeTarget::TransitionParameter {
        owner: first_owner,
        parameter_id,
    };
    let update = AuthoringKeyframeUpdate {
        time: Some(time(2)),
        value: Some(PropertyValue::from(3.0)),
        easing: Some(EasingFunction::Constant),
    };
    let projected = TimelineEditorService::project_keyframe_update(
        &before,
        &target,
        keyframe_id,
        update.clone(),
    )
    .expect("project instance Keyframe");
    let first = projected
        .effective_transition_module_controls(&first_target)
        .expect("projected first controls");
    let keyframe = first.automation_tracks[&parameter_id]
        .keyframes
        .iter()
        .find(|keyframe| keyframe.id == keyframe_id)
        .expect("same placement Keyframe");
    assert_eq!(keyframe.time, time(2));
    assert_eq!(keyframe.value, PropertyValue::from(3.0));
    assert_eq!(keyframe.easing, EasingFunction::Constant);
    assert_eq!(
        projected
            .effective_transition_module_controls(&second_target)
            .expect("unchanged sibling"),
        sibling_before
    );
    assert_eq!(service.snapshot().expect("pure snapshot"), before);

    service
        .update_keyframe(&target, keyframe_id, update)
        .expect("commit instance Keyframe");
    assert_eq!(service.snapshot().expect("committed").as_ref(), &projected);
    service.undo().expect("Undo").expect("one command");
    assert_eq!(service.snapshot().expect("Undo state"), before);
}

fn assert_rejected_atomically(
    service: &TimelineEditorService,
    target: &AuthoringKeyframeTarget,
    keyframe_id: KeyframeId,
    update: AuthoringKeyframeUpdate,
) {
    let before = service.snapshot().expect("invalid baseline");
    let revision = service.revision().expect("invalid revision");
    TimelineEditorService::project_keyframe_update(&before, target, keyframe_id, update.clone())
        .expect_err("projection must reject invalid update");
    service
        .update_keyframe(target, keyframe_id, update)
        .expect_err("commit must reject invalid update");
    assert_eq!(service.revision().expect("unchanged revision"), revision);
    assert_eq!(service.snapshot().expect("unchanged project"), before);
}

#[test]
fn missing_negative_duplicate_and_invalid_typed_updates_are_atomic_errors() {
    let fixture = text_fixture();
    let owner = AuthoringPropertyOwner::Item(fixture.item_id);
    let target = AuthoringKeyframeTarget::AuthoredProperty {
        owner,
        key: "size".to_string(),
    };
    assert_rejected_atomically(
        &fixture.service,
        &target,
        KeyframeId::new(),
        AuthoringKeyframeUpdate {
            value: Some(PropertyValue::from(60.0)),
            ..AuthoringKeyframeUpdate::default()
        },
    );
    assert_rejected_atomically(
        &fixture.service,
        &target,
        fixture.size_key,
        AuthoringKeyframeUpdate {
            time: Some(MediaTime::new(-1, 1).expect("negative time")),
            ..AuthoringKeyframeUpdate::default()
        },
    );
    fixture
        .service
        .upsert_authored_property_keyframe(
            owner,
            "size".to_string(),
            time(1),
            PropertyValue::from(80.0),
            Some(EasingFunction::Linear),
        )
        .expect("second Size Keyframe");
    assert_rejected_atomically(
        &fixture.service,
        &target,
        fixture.size_key,
        AuthoringKeyframeUpdate {
            time: Some(time(1)),
            ..AuthoringKeyframeUpdate::default()
        },
    );

    fixture
        .service
        .convert_source_to_node_clip(fixture.plugins.as_ref(), fixture.item_id)
        .expect("promote Text");
    let promoted = fixture.service.snapshot().expect("promoted");
    let parameter_id = tracking_parameter(&promoted, fixture.item_id, fixture.tracking_id);
    assert_rejected_atomically(
        &fixture.service,
        &AuthoringKeyframeTarget::ModuleParameter {
            item_id: fixture.item_id,
            parameter_id,
        },
        fixture.tracking_key,
        AuthoringKeyframeUpdate {
            value: Some(PropertyValue::String("not a number".to_string())),
            ..AuthoringKeyframeUpdate::default()
        },
    );
}
