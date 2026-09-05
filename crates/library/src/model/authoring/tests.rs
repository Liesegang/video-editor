use std::collections::HashMap;

use ordered_float::OrderedFloat;

use super::*;
use crate::editor::{
    AuthoringPropertyOwner, ModuleInterfaceCommand, ModuleInterfaceEditResult, ModuleItemPlacement,
    ModuleNodeRequest, TimelineEditorService, TimelineSettingsUpdate,
};
use crate::model::AssetKind;
use crate::model::node::{GeneratorContent, Node, NodeContent, ValueContent};
use crate::model::project::{
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NUMBER_RESULT_OUTPUT_PORT,
    NUMERIC_A_INPUT_PORT, NUMERIC_B_INPUT_PORT, PortDataType, PortDirection,
};
use crate::model::property::{PropertyValue, Vec2};
use crate::plugin::PluginManager;

fn time(value: i64, timescale: u32) -> MediaTime {
    MediaTime::new(value, timescale).expect("valid fixture time")
}

fn interval(start: i64, duration: i64) -> TimelineInterval {
    TimelineInterval::new(time(start, 1), time(duration, 1)).expect("valid fixture interval")
}

fn root_track(service: &TimelineEditorService) -> TimelineTrackId {
    let project = service.snapshot().expect("snapshot");
    project.timelines[&project.root_timeline_id].track_order[0]
}

fn reusable_image_module(
    with_media_input: bool,
) -> (
    ModuleDefinition,
    PublishedParameterId,
    ModuleOutputId,
    Option<PublishedMediaInputId>,
    uuid::Uuid,
) {
    let numeric = Node::new_value("Control", ValueContent::Add);
    let numeric_id = numeric.id;
    let merge = Node::new_merge("Merge");
    let merge_id = merge.id;
    let parameter_id = PublishedParameterId::new();
    let input_id = with_media_input.then(PublishedMediaInputId::new);
    let (mut definition, output_id) = ModuleDefinition::new_image(
        "Reusable Lower Third",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
    );
    let output_target = definition
        .output(output_id)
        .unwrap()
        .target(PortDataType::Image)
        .unwrap();
    definition
        .graph
        .nodes
        .extend([(numeric_id, numeric), (merge_id, merge)]);
    definition.graph.connections.push(ModuleConnection {
        id: ModuleConnectionId::new(),
        from: ModulePortAddress {
            node_id: merge_id,
            port: IMAGE_OUTPUT_PORT.to_string(),
        },
        to: output_target,
        order: 0,
        blend_mode: crate::model::BlendMode::Normal,
    });
    definition.interface = ModuleInterface {
        parameters: vec![PublishedParameter {
            id: parameter_id,
            name: "Amount".to_string(),
            data_type: PortDataType::Number,
            default_value: PropertyValue::Number(OrderedFloat(1.0)),
            target: ModulePortAddress {
                node_id: numeric_id,
                port: NUMERIC_A_INPUT_PORT.to_string(),
            },
        }],
        media_inputs: input_id
            .map(|id| {
                vec![PublishedMediaInput {
                    id,
                    name: "Image".to_string(),
                    data_type: PortDataType::Image,
                    target: ModulePortAddress {
                        node_id: merge_id,
                        port: MERGE_IMAGES_PORT.to_string(),
                    },
                    required: true,
                    primary: true,
                }]
            })
            .unwrap_or_default(),
        signals: Vec::new(),
        actions: Vec::new(),
    };
    definition.topology_revision = 2;
    (definition, parameter_id, output_id, input_id, numeric_id)
}

fn placement(track_id: TimelineTrackId, output_id: ModuleOutputId) -> ModuleItemPlacement {
    ModuleItemPlacement {
        track_id,
        name: "Node Clip".to_string(),
        output_id,
        interval: interval(0, 2),
        layer: 0,
        parameter_overrides: HashMap::new(),
        input_bindings: HashMap::new(),
    }
}

#[test]
fn exact_time_normalizes_checks_overflow_and_converts_frame_boundaries() {
    let rate = RationalRate::new(30_000, 1_001).expect("NTSC rate");
    let frame_time = MediaTime::from_frame_index(100, rate).expect("frame time");

    assert_eq!(frame_time.checked_frame_index(rate).expect("frame"), 100);
    assert_eq!(time(2, 4), time(1, 2));
    assert!(
        MediaTime::new(i64::MAX, 1)
            .expect("maximum")
            .checked_add(time(1, 1))
            .is_err()
    );
}

#[test]
fn timeline_interval_uses_exact_exclusive_end() {
    let interval = TimelineInterval::new(time(1, 3), time(2, 3)).expect("interval");

    assert_eq!(interval.end().expect("end"), time(1, 1));
    assert!(interval.contains(time(999, 1_000)).expect("contains"));
    assert!(!interval.contains(time(1, 1)).expect("contains"));
}

#[test]
fn project_document_has_one_strict_format_and_rejects_duplicate_asset_ids() {
    let project = AuthoringProject::new(
        "Strict",
        1920,
        1080,
        RationalRate::new(30, 1).expect("rate"),
        time(60, 1),
    )
    .expect("project");
    let document = ProjectDocument::new(project.clone());
    let json = document.to_json().expect("json");
    assert_eq!(
        ProjectDocument::from_json(&json).expect("round trip"),
        document
    );
    assert!(ProjectDocument::from_json(r#"{"name":"legacy"}"#).is_err());

    let mut wrong_version = serde_json::to_value(&document).expect("document value");
    wrong_version["format_version"] = serde_json::json!(0);
    assert!(
        ProjectDocument::from_json(
            &serde_json::to_string(&wrong_version).expect("wrong-version JSON")
        )
        .is_err(),
        "pre-v1 documents must not enter a compatibility or migration path"
    );

    let mut persisted_plan = serde_json::to_value(&document).expect("document value");
    persisted_plan["render_plan"] = serde_json::json!({"compiled": true});
    assert!(
        ProjectDocument::from_json(
            &serde_json::to_string(&persisted_plan).expect("RenderPlan JSON")
        )
        .is_err(),
        "RenderPlan is derived data and must not be accepted as persisted state"
    );

    let mut duplicate = project;
    let asset = crate::model::asset::Asset::new("image", "image.png", AssetKind::Image);
    duplicate.assets.push(asset.clone());
    duplicate.assets.push(asset);
    assert!(duplicate.validate().is_err());
}

#[test]
fn ordinary_and_nested_timeline_items_never_create_module_topology() {
    let service = TimelineEditorService::create_default("Timeline only").expect("service");
    let project = service.snapshot().expect("snapshot");
    let root_timeline_id = project.root_timeline_id;
    let root_track_id = project.timelines[&root_timeline_id].track_order[0];
    drop(project);
    let (nested_timeline_id, _, _) = service
        .add_timeline(
            "Nested".to_string(),
            1920,
            1080,
            RationalRate::new(30, 1).expect("rate"),
            time(5, 1),
        )
        .expect("nested Timeline");
    service
        .add_item(
            root_track_id,
            "Title".to_string(),
            SourceRef::Text {
                text: "Timeline-owned".to_string(),
                appearance_operations: Vec::new(),
                ensemble_operations: Vec::new(),
            },
            interval(0, 2),
            0,
        )
        .expect("Text item");
    service
        .add_item(
            root_track_id,
            "Nested".to_string(),
            SourceRef::Composition(CompositionInstance {
                timeline_id: nested_timeline_id,
                duration_policy: DurationPolicy::Fixed,
                parameter_overrides: HashMap::new(),
                transition_module_overrides: Vec::new(),
            }),
            interval(2, 3),
            1,
        )
        .expect("nested Timeline item");

    let project = service.snapshot().expect("snapshot");
    assert!(project.module_definitions.is_empty());
    assert!(project.module_instances.is_empty());
    assert!(
        project
            .items
            .values()
            .all(|item| !matches!(item.source, SourceRef::Module(_)))
    );
    project.validate().expect("Timeline-only Project");
}

#[test]
fn hostile_missing_track_document_returns_error_without_panicking() {
    let service = TimelineEditorService::create_default("Hostile").expect("service");
    let track_id = root_track(&service);
    service
        .add_item(
            track_id,
            "Text".to_string(),
            SourceRef::Text {
                text: "hello".to_string(),
                appearance_operations: Vec::new(),
                ensemble_operations: Vec::new(),
            },
            interval(0, 1),
            0,
        )
        .expect("item");
    let mut value = serde_json::to_value(service.document().expect("document")).expect("value");
    let items = value["project"]["items"]
        .as_object_mut()
        .expect("items object");
    let item = items.values_mut().next().expect("item value");
    item["track_id"] = serde_json::to_value(TimelineTrackId::new()).expect("track id");
    let source = serde_json::to_string(&value).expect("hostile json");

    let result = std::panic::catch_unwind(|| ProjectDocument::from_json(&source));
    assert!(result.is_ok(), "validation must be total");
    assert!(result.expect("no panic").is_err());
}

#[test]
fn module_graph_validation_is_fail_closed_and_typed() {
    let source = Node::new_merge("Image");
    let source_id = source.id;
    let target = Node::new_value("Number", ValueContent::Add);
    let target_id = target.id;
    let graph = ModuleGraph {
        nodes: HashMap::from([(source_id, source), (target_id, target)]),
        connections: vec![ModuleConnection {
            id: ModuleConnectionId::new(),
            from: ModulePortAddress {
                node_id: source_id,
                port: IMAGE_OUTPUT_PORT.to_string(),
            },
            to: ModulePortAddress {
                node_id: target_id,
                port: NUMERIC_A_INPUT_PORT.to_string(),
            },
            order: 0,
            blend_mode: crate::model::BlendMode::Normal,
        }],
    };
    assert!(graph.validate().is_err(), "Image must not feed Numeric");

    let mut wrong_direction = graph;
    wrong_direction.connections[0].from = ModulePortAddress {
        node_id: target_id,
        port: NUMERIC_A_INPUT_PORT.to_string(),
    };
    assert!(wrong_direction.validate().is_err(), "source must be Output");
}

#[test]
fn module_connection_blend_is_per_image_merge_edge() {
    let source = Node::new_merge("Source");
    let target = Node::new_merge("Target");
    let transform = PluginManager::default()
        .create_image_transform_operation_node()
        .expect("image transform");
    let source_id = source.id;
    let target_id = target.id;
    let transform_id = transform.id;
    let mut graph = ModuleGraph {
        nodes: HashMap::from([
            (source_id, source),
            (target_id, target),
            (transform_id, transform),
        ]),
        connections: vec![ModuleConnection {
            id: ModuleConnectionId::new(),
            from: ModulePortAddress {
                node_id: source_id,
                port: IMAGE_OUTPUT_PORT.to_string(),
            },
            to: ModulePortAddress {
                node_id: target_id,
                port: MERGE_IMAGES_PORT.to_string(),
            },
            order: 0,
            blend_mode: crate::model::BlendMode::Screen,
        }],
    };
    graph.validate().expect("Image Merge edge accepts Blend");

    graph.connections[0].to = ModulePortAddress {
        node_id: transform_id,
        port: IMAGE_INPUT_PORT.to_string(),
    };
    assert!(
        graph.validate().is_err(),
        "an ordinary typed Image input cannot smuggle per-edge compositing"
    );
}

#[test]
fn a_dedicated_output_node_cannot_be_deleted_and_the_rejected_edit_is_atomic() {
    let service = TimelineEditorService::create_default("Required Output").unwrap();
    let track_id = root_track(&service);
    let (definition, _, output_id, _, _) = reusable_image_module(false);
    let definition_id = definition.id;
    let output_node_id = definition.output(output_id).unwrap().node_id;
    service.add_module_definition(definition).unwrap();
    let (_, instance_id, _) = service
        .place_module_item(definition_id, placement(track_id, output_id))
        .unwrap();
    let before = service.snapshot().unwrap();

    let error = service
        .remove_instance_module_node(instance_id, output_node_id)
        .expect_err("the visible Output boundary is mandatory");

    assert!(error.to_string().contains("required render terminal"));
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}

#[test]
fn output_node_state_and_unknown_invocation_output_fail_atomically() {
    let service = TimelineEditorService::create_default("Output guards").unwrap();
    let track_id = root_track(&service);
    let (definition, _, output_id, _, _) = reusable_image_module(false);
    let definition_id = definition.id;
    let output_node_id = definition.output(output_id).unwrap().node_id;
    service.add_module_definition(definition).unwrap();
    let (_, instance_id, _) = service
        .place_module_item(definition_id, placement(track_id, output_id))
        .unwrap();
    let before = service.snapshot().unwrap();
    let revision = service.revision().unwrap();

    assert!(
        service
            .set_instance_module_node_state(
                instance_id,
                output_node_id,
                "Output".to_string(),
                false,
                false,
            )
            .is_err()
    );
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
    assert_eq!(service.revision().unwrap(), revision);

    let mut invalid = placement(track_id, ModuleOutputId::new());
    invalid.layer = 1;
    assert!(service.place_module_item(definition_id, invalid).is_err());
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
    assert_eq!(service.revision().unwrap(), revision);
}

#[test]
fn output_terminal_is_the_only_persisted_render_boundary() {
    let (definition, _, output_id, _, _) = reusable_image_module(false);
    let json = serde_json::to_string(&definition).expect("serialize Module");

    assert!(json.contains("ModuleOutput"));
    assert!(json.contains(&output_id.to_string()));
    assert!(!json.contains("media_outputs"));
    assert!(!json.contains("PublishedMediaOutput"));
}

#[test]
fn node_clip_parameters_automation_split_and_graph_edits_are_instance_local() {
    let service = TimelineEditorService::create_default("Node Clip").expect("service");
    let track_id = root_track(&service);
    let (definition, parameter_id, output_id, _, numeric_node_id) = reusable_image_module(false);
    let reusable_id = definition.id;
    service
        .add_module_definition(definition)
        .expect("definition");
    let (first_item, first_instance, _) = service
        .place_module_item(reusable_id, placement(track_id, output_id))
        .expect("first placement");
    let (_second_item, second_instance, _) = service
        .place_module_item(reusable_id, placement(track_id, output_id))
        .expect("second placement");

    service
        .set_module_parameter(
            first_instance,
            parameter_id,
            PropertyValue::Number(OrderedFloat(2.0)),
        )
        .expect("instance value");
    let (keyframe_id, _) = service
        .upsert_module_parameter_keyframe(
            first_item,
            parameter_id,
            time(1, 2),
            PropertyValue::Number(OrderedFloat(3.0)),
            None,
        )
        .expect("automation");
    service
        .update_module_parameter_keyframe(
            first_item,
            parameter_id,
            keyframe_id,
            crate::editor::AuthoringKeyframeUpdate {
                time: Some(time(3, 4)),
                value: Some(PropertyValue::Number(OrderedFloat(4.0))),
                easing: None,
            },
        )
        .expect("stable-id automation update");
    let (removable_id, _) = service
        .upsert_module_parameter_keyframe(
            first_item,
            parameter_id,
            time(1, 1),
            PropertyValue::Number(OrderedFloat(5.0)),
            None,
        )
        .expect("second automation key");
    service
        .remove_module_parameter_keyframe(first_item, parameter_id, removable_id)
        .expect("stable-id automation remove");
    let before_logic = service.snapshot().expect("snapshot");
    assert!(
        before_logic.module_instances[&second_instance]
            .parameter_overrides
            .is_empty()
    );
    assert_eq!(
        before_logic.module_instances[&first_instance].definition_id,
        reusable_id
    );
    drop(before_logic);

    let (private_id, _) = service
        .set_instance_module_node_state(
            first_instance,
            numeric_node_id,
            "Private Control".to_string(),
            true,
            false,
        )
        .expect("copy-on-write edit");
    assert_ne!(private_id, reusable_id);
    let after_logic = service.snapshot().expect("snapshot");
    assert!(matches!(
        after_logic.module_definitions[&private_id].sharing,
        ModuleDefinitionSharing::Private
    ));
    assert_eq!(
        after_logic.module_instances[&second_instance].definition_id,
        reusable_id
    );
    drop(after_logic);

    let (right_item, _) = service
        .split_item(first_item, time(1, 1))
        .expect("exact split");
    let split = service.snapshot().expect("split snapshot");
    let SourceRef::Module(left_invocation) = &split.items[&first_item].source else {
        panic!("left Node Clip");
    };
    let SourceRef::Module(right_invocation) = &split.items[&right_item].source else {
        panic!("right Node Clip");
    };
    assert_ne!(left_invocation.instance_id, right_invocation.instance_id);
    assert_eq!(
        split.module_instances[&left_invocation.instance_id].definition_id,
        split.module_instances[&right_invocation.instance_id].definition_id
    );
    assert!(matches!(
        split.module_definitions
            [&split.module_instances[&left_invocation.instance_id].definition_id]
            .sharing,
        ModuleDefinitionSharing::SharedLocal
    ));
    assert_eq!(split.items[&right_item].time_map.source_start, time(1, 1));
}

#[test]
fn deleting_a_referenced_item_requires_explicit_cascade() {
    let service = TimelineEditorService::create_default("Dependencies").expect("service");
    let track_id = root_track(&service);
    let (source_item, _) = service
        .add_item(
            track_id,
            "Source".to_string(),
            SourceRef::Solid {
                color: crate::model::frame::color::Color::white(),
            },
            interval(0, 2),
            0,
        )
        .expect("source");
    let (definition, _, output_id, input_id, _) = reusable_image_module(true);
    let definition_id = definition.id;
    let input_id = input_id.expect("published input");
    service
        .add_module_definition(definition)
        .expect("definition");
    let mut node_placement = placement(track_id, output_id);
    node_placement.input_bindings.insert(
        input_id,
        MediaInputBinding::TimelineItemOutput {
            locator: InstanceLocator::SameTimeline,
            item_id: source_item,
            output: MediaOutputKind::Image,
            stage: ItemOutputStage::PostTransform,
        },
    );
    let (dependent_item, _, _) = service
        .place_module_item(definition_id, node_placement)
        .expect("dependent Node Clip");

    let error = service
        .delete_item(source_item)
        .expect_err("ordinary delete must report dependency");
    assert!(error.to_string().contains("delete_item_cascade"));
    assert!(
        service
            .snapshot()
            .expect("unchanged")
            .items
            .contains_key(&source_item)
    );

    service
        .delete_item_cascade(source_item)
        .expect("explicit cascade");
    let deleted = service.snapshot().expect("deleted snapshot");
    assert!(!deleted.items.contains_key(&source_item));
    assert!(!deleted.items.contains_key(&dependent_item));
    deleted.validate().expect("valid cascade result");
}

#[test]
fn builtin_effect_is_a_lightweight_attachment_and_undo_redo_are_project_atomic() {
    let service = TimelineEditorService::create_default("Effects").expect("service");
    let track_id = root_track(&service);
    let (item_id, _) = service
        .add_item(
            track_id,
            "Text".to_string(),
            SourceRef::Text {
                text: "Title".to_string(),
                appearance_operations: Vec::new(),
                ensemble_operations: Vec::new(),
            },
            interval(0, 2),
            0,
        )
        .expect("text");
    let effect = BuiltinEffectInstance {
        operation: OperationRef {
            category: "effect".to_string(),
            component_id: "builtin".to_string(),
            operation: "blur".to_string(),
            version: "1".to_string(),
        },
        contract: EffectContractSnapshot {
            input_type: PortDataType::Image,
            output_type: PortDataType::Image,
            parameters: vec![ProcessorParameterContract {
                key: "radius".to_string(),
                data_type: PortDataType::Number,
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
            }],
        },
        parameters: HashMap::from([(
            "radius".to_string(),
            AutomatableParameter {
                value: PropertyValue::Number(OrderedFloat(4.0)),
                automation: None,
            },
        )]),
        blend_mode: crate::model::BlendMode::Normal,
    };
    service
        .add_builtin_attachment(
            AttachmentOwner::Item { item_id },
            AttachmentStage::ItemPostTransform,
            effect,
        )
        .expect("effect");
    let with_effect = service.snapshot().expect("effect snapshot");
    assert_eq!(with_effect.attachments.len(), 1);
    assert!(with_effect.module_definitions.is_empty());
    drop(with_effect);

    service.undo().expect("undo").expect("change");
    assert!(
        service
            .snapshot()
            .expect("undo snapshot")
            .attachments
            .is_empty()
    );
    service.redo().expect("redo").expect("change");
    assert_eq!(
        service.snapshot().expect("redo snapshot").attachments.len(),
        1
    );
}

#[test]
fn project_file_store_round_trips_only_the_authoring_document() {
    let service = TimelineEditorService::create_default("Persistence").expect("service");
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("project.ruvie");
    service.save_as(&path).expect("save");
    let reopened = TimelineEditorService::open(&path).expect("open");

    assert_eq!(
        reopened.document().expect("reopened document"),
        service.document().expect("saved document")
    );
    let json = std::fs::read_to_string(path).expect("saved json");
    assert!(!json.contains("structural_merge_node_id"));
    assert!(!json.contains("render_plan"));
}

#[test]
fn published_interface_rejects_output_used_as_parameter_target() {
    let output = Node::new_value("Value", ValueContent::Add);
    let node_id = output.id;
    let (mut definition, _) = ModuleDefinition::new_image(
        "Invalid Interface",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
    );
    definition.graph.nodes.insert(node_id, output);
    definition.interface.parameters.push(PublishedParameter {
        id: PublishedParameterId::new(),
        name: "Wrong".to_string(),
        data_type: PortDataType::Number,
        default_value: PropertyValue::Number(OrderedFloat(0.0)),
        target: ModulePortAddress {
            node_id,
            port: NUMBER_RESULT_OUTPUT_PORT.to_string(),
        },
    });
    assert!(definition.validate().is_err());
}

#[test]
fn published_interface_edit_is_cow_and_cleans_instance_state_atomically() {
    let service = TimelineEditorService::create_default("Interface").expect("service");
    let track_id = root_track(&service);
    let (definition, _, output_id, _, numeric_node_id) = reusable_image_module(false);
    let reusable_id = definition.id;
    service
        .add_module_definition(definition)
        .expect("definition");
    let (item_id, instance_id, _) = service
        .place_module_item(reusable_id, placement(track_id, output_id))
        .expect("placement");

    let (result, private_id, _) = service
        .edit_instance_module_interface(
            instance_id,
            ModuleInterfaceCommand::PublishParameter {
                name: "Offset".to_string(),
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                target: ModulePortAddress {
                    node_id: numeric_node_id,
                    port: NUMERIC_B_INPUT_PORT.to_string(),
                },
            },
        )
        .expect("publish parameter");
    let ModuleInterfaceEditResult::PublishedParameter(parameter_id) = result else {
        panic!("published parameter result");
    };
    assert_ne!(
        private_id, reusable_id,
        "template edit must be copy-on-write"
    );
    service
        .set_module_parameter(
            instance_id,
            parameter_id,
            PropertyValue::Number(OrderedFloat(2.0)),
        )
        .expect("override");
    service
        .upsert_module_parameter_keyframe(
            item_id,
            parameter_id,
            time(1, 2),
            PropertyValue::Number(OrderedFloat(3.0)),
            None,
        )
        .expect("automation");
    let (removed, same_private_id, _) = service
        .edit_instance_module_interface(
            instance_id,
            ModuleInterfaceCommand::UnpublishParameter { parameter_id },
        )
        .expect("unpublish parameter");
    assert_eq!(same_private_id, private_id);
    let ModuleInterfaceEditResult::Unpublished(impact) = removed else {
        panic!("unpublish impact");
    };
    assert_eq!(impact.removed_parameter_overrides, 1);
    assert_eq!(impact.removed_automation_tracks, 1);
    let snapshot = service.snapshot().expect("snapshot");
    assert!(
        !snapshot.module_definitions[&reusable_id]
            .interface
            .parameters
            .is_empty(),
        "source template remains untouched"
    );
    assert!(
        snapshot.module_definitions[&private_id]
            .interface
            .parameters
            .iter()
            .all(|parameter| parameter.id != parameter_id)
    );
    snapshot.validate().expect("valid interface edit");
}

#[test]
fn module_requires_a_dedicated_output_and_terminal_has_no_outgoing_port() {
    let (definition, _, output_id, _, _) = reusable_image_module(false);
    let output = definition.output(output_id).expect("Output terminal");
    for data_type in [PortDataType::Image, PortDataType::Audio] {
        let target = output.target(data_type).expect("media target");
        assert!(
            definition
                .graph
                .port_definition(&target, PortDirection::Input)
                .is_ok()
        );
    }
    assert!(
        definition
            .graph
            .port_definition(
                &ModulePortAddress {
                    node_id: output.node_id,
                    port: IMAGE_OUTPUT_PORT.to_string(),
                },
                PortDirection::Output,
            )
            .is_err(),
        "a terminal cannot be used as a processing source"
    );

    let mut missing = definition;
    missing
        .graph
        .nodes
        .retain(|_, node| !matches!(node.content(), NodeContent::ModuleOutput(_)));
    assert!(
        missing.validate().is_err(),
        "every current Module renders media"
    );
}

#[test]
fn detached_node_and_builtin_effect_factories_need_no_legacy_project() {
    let plugins = PluginManager::default();
    let service = TimelineEditorService::create_default("Factories").expect("service");
    let node = service
        .create_module_node(
            &plugins,
            ModuleNodeRequest::Text {
                text: "Hello".to_string(),
                font: "Arial".to_string(),
            },
            1920,
            1080,
        )
        .expect("Text Node");
    assert!(matches!(
        node.content(),
        NodeContent::Generator(GeneratorContent::Text)
    ));
    assert_eq!(
        node.properties()
            .get_constant_value("text")
            .and_then(|value| value.get_as::<String>()),
        Some("Hello".to_string())
    );

    let effect = service
        .create_builtin_effect(&plugins, "blur")
        .expect("descriptor-backed blur");
    assert_eq!(effect.operation.component_id, "blur");
    assert_eq!(effect.contract.input_type, PortDataType::Image);
    assert_eq!(effect.contract.output_type, PortDataType::Image);
    assert_eq!(effect.parameters.len(), effect.contract.parameters.len());
    assert!(!effect.parameters.is_empty());
}

#[test]
fn timeline_authoring_commands_and_effect_reorder_are_single_undo_steps() {
    let plugins = PluginManager::default();
    let service = TimelineEditorService::create_default("Authoring").expect("service");
    let root = service.snapshot().expect("snapshot").root_timeline_id;
    let track_id = root_track(&service);
    let (item_id, _) = service
        .add_item(
            track_id,
            "Title".to_string(),
            SourceRef::Text {
                text: "Before".to_string(),
                appearance_operations: Vec::new(),
                ensemble_operations: Vec::new(),
            },
            interval(0, 2),
            0,
        )
        .expect("item");
    service
        .update_timeline_settings(
            root,
            TimelineSettingsUpdate {
                name: Some("Main".to_string()),
                ..TimelineSettingsUpdate::default()
            },
        )
        .expect("timeline settings");
    service
        .set_text(item_id, "After".to_string())
        .expect("text edit");
    service
        .set_authored_property_constant(
            AuthoringPropertyOwner::Item(item_id),
            "position".to_string(),
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(10.0),
                y: OrderedFloat(20.0),
            }),
        )
        .expect("position");
    let (first, _) = service
        .add_builtin_effect_by_id(
            &plugins,
            AttachmentOwner::Item { item_id },
            AttachmentStage::ItemPostTransform,
            "blur",
        )
        .expect("first effect");
    let (effect_keyframe_id, _) = service
        .upsert_builtin_effect_parameter_keyframe(
            first,
            "sigma_x",
            time(1, 2),
            PropertyValue::Number(OrderedFloat(8.0)),
            None,
        )
        .expect("effect automation");
    service
        .update_builtin_effect_parameter_keyframe(
            first,
            "sigma_x",
            effect_keyframe_id,
            crate::editor::AuthoringKeyframeUpdate {
                time: Some(time(3, 4)),
                value: None,
                easing: None,
            },
        )
        .expect("effect automation update");
    service
        .remove_builtin_effect_parameter_keyframe(first, "sigma_x", effect_keyframe_id)
        .expect("effect automation remove");
    let (second, _) = service
        .add_builtin_effect_by_id(
            &plugins,
            AttachmentOwner::Item { item_id },
            AttachmentStage::ItemPostTransform,
            "tile",
        )
        .expect("second effect");
    service
        .reorder_attachment(second, 0)
        .expect("reorder effect");
    let reordered = service.snapshot().expect("reordered");
    assert_eq!(reordered.attachments[&second].order, 0);
    assert_eq!(reordered.attachments[&first].order, 1);
    drop(reordered);
    service.undo().expect("undo").expect("reorder change");
    let original = service.snapshot().expect("original order");
    assert_eq!(original.attachments[&first].order, 0);
    assert_eq!(original.attachments[&second].order, 1);
}
