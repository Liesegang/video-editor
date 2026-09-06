use std::collections::HashMap;

use ordered_float::OrderedFloat;

use super::*;
use crate::animation::EasingFunction;
use crate::model::authoring::{
    ModuleConnection, ModuleDefinitionSharing, ModulePortAddress, ModuleTemplateOrigin,
};
use crate::model::node::ValueContent;
use crate::model::project::{
    FMOD_X_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NUMBER_RESULT_OUTPUT_PORT,
    NUMERIC_B_INPUT_PORT, PortDataType,
};

struct PublicationFixture {
    service: TimelineEditorService,
    reusable_definition_id: ModuleDefinitionId,
    item_id: TimelineItemId,
    instance_id: ModuleInstanceId,
    sibling_item_id: TimelineItemId,
    sibling_instance_id: ModuleInstanceId,
    target: ModulePortAddress,
    state_before_sibling: Arc<AuthoringProject>,
}

fn time(seconds: i64) -> MediaTime {
    MediaTime::new(seconds, 1).expect("fixture time")
}

fn placement(
    track_id: TimelineTrackId,
    output_id: ModuleOutputId,
    start: MediaTime,
    layer: i64,
) -> ModuleItemPlacement {
    ModuleItemPlacement {
        track_id,
        name: "Numeric Node Clip".to_string(),
        output_id,
        interval: TimelineInterval::new(start, time(4)).expect("fixture interval"),
        layer,
        parameter_overrides: HashMap::new(),
        input_bindings: HashMap::new(),
    }
}

fn fixture(
    mutate_definition: impl FnOnce(&mut ModuleDefinition, uuid::Uuid),
) -> PublicationFixture {
    let service = TimelineEditorService::create_default("Node parameter publication")
        .expect("authoring service");
    let snapshot = service.snapshot().expect("default project");
    let track_id = snapshot.timelines[&snapshot.root_timeline_id].track_order[0];
    drop(snapshot);

    let control = Node::new_value("Control", ValueContent::Add);
    let control_id = control.id;
    let merge = Node::new_merge("Image");
    let merge_id = merge.id;
    let (mut definition, output_id) = ModuleDefinition::new_image(
        "Reusable numeric module",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
    );
    let output_target = definition
        .output(output_id)
        .expect("Image Output")
        .target(PortDataType::Image)
        .expect("Image target");
    definition
        .graph
        .nodes
        .extend([(control_id, control), (merge_id, merge)]);
    definition.graph.connections.push(ModuleConnection {
        id: ModuleConnectionId::new(),
        from: ModulePortAddress {
            node_id: merge_id,
            port: IMAGE_OUTPUT_PORT.to_string(),
        },
        to: output_target,
        order: 0,
        blend_mode: BlendMode::Normal,
    });
    mutate_definition(&mut definition, control_id);
    definition.topology_revision += 1;
    let reusable_definition_id = definition.id;
    service
        .add_module_definition(definition)
        .expect("reusable definition");
    let (item_id, instance_id, _) = service
        .place_module_item(
            reusable_definition_id,
            placement(track_id, output_id, time(0), 0),
        )
        .expect("first placement");
    let state_before_sibling = service.snapshot().expect("state before sibling");
    let (sibling_item_id, sibling_instance_id, _) = service
        .place_module_item(
            reusable_definition_id,
            placement(track_id, output_id, time(5), 1),
        )
        .expect("sibling placement");
    PublicationFixture {
        service,
        reusable_definition_id,
        item_id,
        instance_id,
        sibling_item_id,
        sibling_instance_id,
        target: ModulePortAddress {
            node_id: control_id,
            port: NUMERIC_B_INPUT_PORT.to_string(),
        },
        state_before_sibling,
    }
}

fn clean_fixture() -> PublicationFixture {
    fixture(|_, _| {})
}

fn invocation(project: &AuthoringProject, item_id: TimelineItemId) -> &ModuleInvocation {
    match &project.items[&item_id].source {
        SourceRef::Module(invocation) => invocation,
        _ => panic!("fixture item is not a Node Clip"),
    }
}

fn assert_rejected_without_state_or_history(
    fixture: PublicationFixture,
    publish: impl FnOnce(
        &PublicationFixture,
        ProjectRevision,
    ) -> Result<NodeParameterKeyframePublication, LibraryError>,
    expected_message: &str,
) {
    let before = fixture.service.snapshot().expect("before rejection");
    let revision = fixture.service.revision().expect("before revision");
    let error = publish(&fixture, revision).expect_err("publication must be rejected");
    assert!(
        error.to_string().contains(expected_message),
        "unexpected rejection: {error}"
    );
    assert_eq!(
        fixture.service.revision().expect("after revision"),
        revision
    );
    assert_eq!(fixture.service.snapshot().expect("after rejection"), before);
    fixture
        .service
        .undo()
        .expect("undo after rejection")
        .expect("sibling placement remains the last history entry");
    assert_eq!(
        fixture.service.snapshot().expect("undo state"),
        fixture.state_before_sibling,
        "a rejected publication must not add a hidden history entry"
    );
}

#[test]
fn publish_and_first_key_are_one_cow_edit_with_stable_ids_and_sibling_isolation() {
    let fixture = clean_fixture();
    let before = fixture.service.snapshot().expect("before publication");
    let revision = fixture.service.revision().expect("source revision");
    let publication = fixture
        .service
        .publish_node_clip_parameter_keyframe(
            fixture.item_id,
            fixture.instance_id,
            fixture.target.clone(),
            time(2),
            revision,
        )
        .expect("atomic publication");
    assert_eq!(publication.changes.revision.get(), revision.get() + 1);
    assert!(
        publication
            .changes
            .invalidations
            .contains(&ProjectInvalidation::ModuleInstance {
                instance_id: fixture.instance_id,
            })
    );
    assert!(
        publication
            .changes
            .invalidations
            .iter()
            .any(|invalidation| {
                matches!(
                    invalidation,
                    ProjectInvalidation::Item { item_id, .. } if *item_id == fixture.item_id
                )
            })
    );

    let after = fixture.service.snapshot().expect("after publication");
    assert_ne!(publication.definition_id, fixture.reusable_definition_id);
    assert_eq!(
        after.module_instances[&fixture.instance_id].definition_id,
        publication.definition_id
    );
    assert_eq!(
        after.module_instances[&fixture.sibling_instance_id].definition_id,
        fixture.reusable_definition_id
    );
    assert!(
        after.module_definitions[&fixture.reusable_definition_id]
            .interface
            .parameters
            .is_empty()
    );
    let parameter = &after.module_definitions[&publication.definition_id]
        .interface
        .parameters[0];
    assert_eq!(parameter.id, publication.parameter_id);
    assert_eq!(parameter.name, "B");
    assert_eq!(parameter.target, fixture.target);
    let track = &invocation(&after, fixture.item_id).automation_tracks[&publication.parameter_id];
    assert_eq!(track.keyframes.len(), 1);
    assert_eq!(track.keyframes[0].id, publication.keyframe_id);
    assert_eq!(track.keyframes[0].time, time(2));
    assert_eq!(track.keyframes[0].value, parameter.default_value);
    assert_eq!(track.keyframes[0].easing, EasingFunction::Linear);
    assert!(
        invocation(&after, fixture.sibling_item_id)
            .automation_tracks
            .is_empty()
    );
    assert!(
        after.module_instances[&fixture.sibling_instance_id]
            .parameter_overrides
            .is_empty()
    );

    fixture
        .service
        .undo()
        .expect("undo publication")
        .expect("one atomic edit");
    assert_eq!(fixture.service.snapshot().expect("undo state"), before);
    fixture
        .service
        .redo()
        .expect("redo publication")
        .expect("one atomic edit");
    assert_eq!(fixture.service.snapshot().expect("redo state"), after);
}

#[test]
fn published_key_uses_the_shared_curve_target_without_mutating_topology_or_sibling() {
    let fixture = clean_fixture();
    let publication = fixture
        .service
        .publish_node_clip_parameter_keyframe(
            fixture.item_id,
            fixture.instance_id,
            fixture.target.clone(),
            time(1),
            fixture.service.revision().expect("source revision"),
        )
        .expect("publication");
    let before_update = fixture.service.snapshot().expect("before Curve update");
    let definition_before = before_update.module_definitions[&publication.definition_id].clone();
    let sibling_before = invocation(&before_update, fixture.sibling_item_id).clone();
    fixture
        .service
        .update_keyframe(
            &AuthoringKeyframeTarget::ModuleParameter {
                item_id: fixture.item_id,
                parameter_id: publication.parameter_id,
            },
            publication.keyframe_id,
            AuthoringKeyframeUpdate {
                time: Some(time(2)),
                value: Some(PropertyValue::Number(OrderedFloat(9.0))),
                easing: Some(EasingFunction::EaseInOutQuad),
            },
        )
        .expect("Curve edit");
    let updated = fixture.service.snapshot().expect("updated key");
    let key = &invocation(&updated, fixture.item_id).automation_tracks[&publication.parameter_id]
        .keyframes[0];
    assert_eq!(key.id, publication.keyframe_id);
    assert_eq!(key.time, time(2));
    assert_eq!(key.value, PropertyValue::Number(OrderedFloat(9.0)));
    assert_eq!(key.easing, EasingFunction::EaseInOutQuad);
    assert_eq!(
        updated.module_definitions[&publication.definition_id],
        definition_before
    );
    assert_eq!(
        invocation(&updated, fixture.sibling_item_id),
        &sibling_before
    );
    fixture
        .service
        .undo()
        .expect("undo Curve edit")
        .expect("Curve change");
    assert_eq!(
        fixture.service.snapshot().expect("undo Curve"),
        before_update
    );
}

#[test]
fn stale_revision_and_wrong_owner_reject_without_state_or_history() {
    assert_rejected_without_state_or_history(
        clean_fixture(),
        |fixture, _revision| {
            fixture.service.publish_node_clip_parameter_keyframe(
                fixture.item_id,
                fixture.instance_id,
                fixture.target.clone(),
                time(1),
                ProjectRevision::initial(),
            )
        },
        "stale",
    );
    assert_rejected_without_state_or_history(
        clean_fixture(),
        |fixture, revision| {
            fixture.service.publish_node_clip_parameter_keyframe(
                fixture.item_id,
                fixture.sibling_instance_id,
                fixture.target.clone(),
                time(1),
                revision,
            )
        },
        "changed Module instance",
    );
}

#[test]
fn invalid_target_modes_and_times_roll_back_cow_publication() {
    assert_rejected_without_state_or_history(
        clean_fixture(),
        |fixture, revision| {
            fixture.service.publish_node_clip_parameter_keyframe(
                fixture.item_id,
                fixture.instance_id,
                ModulePortAddress {
                    node_id: fixture
                        .service
                        .snapshot()
                        .expect("project")
                        .module_definitions[&fixture.reusable_definition_id]
                        .graph
                        .nodes
                        .values()
                        .find(|node| node.name == "Image")
                        .expect("Merge")
                        .id,
                    port: MERGE_IMAGES_PORT.to_string(),
                },
                time(1),
                revision,
            )
        },
        "not a Property value",
    );
    assert_rejected_without_state_or_history(
        fixture(|definition, control_id| {
            definition
                .graph
                .nodes
                .get_mut(&control_id)
                .expect("Control")
                .set_property(
                    NUMERIC_B_INPUT_PORT.to_string(),
                    Property::expression(
                        "time".to_string(),
                        PropertyValue::Number(OrderedFloat(0.0)),
                    ),
                )
                .expect("expression fixture");
        }),
        |fixture, revision| {
            fixture.service.publish_node_clip_parameter_keyframe(
                fixture.item_id,
                fixture.instance_id,
                fixture.target.clone(),
                time(1),
                revision,
            )
        },
        "constant authored Property",
    );
    let missing_property = fixture(|definition, _| {
        let fmod = Node::new_fmod("Fmod without x fallback");
        definition.graph.nodes.insert(fmod.id, fmod);
    });
    let fmod_id = missing_property
        .service
        .snapshot()
        .expect("Fmod project")
        .module_definitions[&missing_property.reusable_definition_id]
        .graph
        .nodes
        .values()
        .find(|node| node.name == "Fmod without x fallback")
        .expect("Fmod")
        .id;
    assert_rejected_without_state_or_history(
        missing_property,
        |fixture, revision| {
            fixture.service.publish_node_clip_parameter_keyframe(
                fixture.item_id,
                fixture.instance_id,
                ModulePortAddress {
                    node_id: fmod_id,
                    port: FMOD_X_INPUT_PORT.to_string(),
                },
                time(1),
                revision,
            )
        },
        "has no authored Property",
    );
    assert_rejected_without_state_or_history(
        clean_fixture(),
        |fixture, revision| {
            fixture.service.publish_node_clip_parameter_keyframe(
                fixture.item_id,
                fixture.instance_id,
                fixture.target.clone(),
                MediaTime::new(-1, 1).expect("negative time"),
                revision,
            )
        },
        "non-negative",
    );
}

#[test]
fn already_published_input_rejects_without_an_extra_edit() {
    let fixture = clean_fixture();
    let before_manual_publish = fixture
        .service
        .snapshot()
        .expect("before manual publication");
    fixture
        .service
        .edit_instance_module_interface(
            fixture.instance_id,
            ModuleInterfaceCommand::PublishParameter {
                name: "B".to_string(),
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                target: fixture.target.clone(),
            },
        )
        .expect("manual publication");
    let published = fixture.service.snapshot().expect("published baseline");
    let revision = fixture.service.revision().expect("published revision");
    let error = fixture
        .service
        .publish_node_clip_parameter_keyframe(
            fixture.item_id,
            fixture.instance_id,
            fixture.target,
            time(1),
            revision,
        )
        .expect_err("already published input");
    assert!(error.to_string().contains("already published"));
    assert_eq!(fixture.service.snapshot().expect("unchanged"), published);
    assert_eq!(
        fixture.service.revision().expect("unchanged revision"),
        revision
    );
    fixture
        .service
        .undo()
        .expect("undo after rejection")
        .expect("manual publication remains the last edit");
    assert_eq!(
        fixture
            .service
            .snapshot()
            .expect("manual publication undone"),
        before_manual_publish
    );
}

#[test]
fn connected_and_constant_only_inputs_reject_atomically() {
    assert_rejected_without_state_or_history(
        fixture(|definition, control_id| {
            let source = Node::new_value("Source", ValueContent::Add);
            let source_id = source.id;
            definition.graph.nodes.insert(source_id, source);
            definition.graph.connections.push(ModuleConnection {
                id: ModuleConnectionId::new(),
                from: ModulePortAddress {
                    node_id: source_id,
                    port: NUMBER_RESULT_OUTPUT_PORT.to_string(),
                },
                to: ModulePortAddress {
                    node_id: control_id,
                    port: NUMERIC_B_INPUT_PORT.to_string(),
                },
                order: 0,
                blend_mode: BlendMode::Normal,
            });
        }),
        |fixture, revision| {
            fixture.service.publish_node_clip_parameter_keyframe(
                fixture.item_id,
                fixture.instance_id,
                fixture.target.clone(),
                time(1),
                revision,
            )
        },
        "connected",
    );

    let particle = fixture(|definition, _| {
        let emitter = Node::new_catalog_node("native.particle.emitter").expect("Particle Emitter");
        definition.graph.nodes.insert(emitter.id, emitter);
    });
    let emitter_id = particle
        .service
        .snapshot()
        .expect("particle project")
        .module_definitions[&particle.reusable_definition_id]
        .graph
        .nodes
        .values()
        .find(|node| node.name == "Particle Emitter")
        .expect("Particle Emitter")
        .id;
    assert_rejected_without_state_or_history(
        particle,
        |fixture, revision| {
            fixture.service.publish_node_clip_parameter_keyframe(
                fixture.item_id,
                fixture.instance_id,
                ModulePortAddress {
                    node_id: emitter_id,
                    port: "rate".to_string(),
                },
                time(1),
                revision,
            )
        },
        "constant-only",
    );
}
