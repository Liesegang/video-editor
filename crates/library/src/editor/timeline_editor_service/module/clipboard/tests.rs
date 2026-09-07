use super::*;
use crate::animation::EasingFunction;
use crate::model::authoring::{
    ModuleConnection, ModuleDefinition, ModuleDefinitionSharing, ModulePortAddress,
    ModuleTemplateOrigin, PublishedAction, PublishedMediaInput, PublishedParameter,
    PublishedSignal, TimelineInterval, TransitionAlignment, TransitionMediaType,
    TransitionProcessor,
};
use crate::model::node::{MediaContent, MediaOutputSelection, Node, ValueContent};
use crate::model::project::{
    IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NUMBER_RESULT_OUTPUT_PORT, NUMERIC_A_INPUT_PORT,
    NUMERIC_B_INPUT_PORT, PortDataType,
};
use ordered_float::OrderedFloat;

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).expect("whole seconds")
}

struct Fixture {
    service: TimelineEditorService,
    item_id: TimelineItemId,
    instance_id: ModuleInstanceId,
    sibling_instance_id: ModuleInstanceId,
    first_node_id: uuid::Uuid,
    second_node_id: uuid::Uuid,
    output_node_id: uuid::Uuid,
    parameter_id: PublishedParameterId,
    connection_id: ModuleConnectionId,
}

fn fixture() -> Fixture {
    let service = TimelineEditorService::create_default("Module clipboard").expect("service");
    let project = service.snapshot().expect("project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let mut first = Node::new_value("First", ValueContent::Add);
    first.ui_position = [100.0, 120.0];
    let first_node_id = first.id;
    let mut second = Node::new_value("Second", ValueContent::Multiply);
    second.ui_position = [300.0, 180.0];
    let second_node_id = second.id;
    let merge = Node::new_merge("Image");
    let merge_id = merge.id;
    let (mut definition, output_id) = ModuleDefinition::new_image(
        "Reusable clipboard",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
    );
    let output = definition.output(output_id).expect("Output");
    let output_node_id = output.node_id;
    let output_target = output
        .target(PortDataType::Image)
        .expect("Image Output target");
    definition.graph.nodes.extend([
        (first_node_id, first),
        (second_node_id, second),
        (merge_id, merge),
    ]);
    let connection_id = ModuleConnectionId::new();
    definition.graph.connections.extend([
        ModuleConnection {
            id: connection_id,
            from: ModulePortAddress {
                node_id: first_node_id,
                port: NUMBER_RESULT_OUTPUT_PORT.to_string(),
            },
            to: ModulePortAddress {
                node_id: second_node_id,
                port: NUMERIC_A_INPUT_PORT.to_string(),
            },
            order: 0,
            blend_mode: BlendMode::Normal,
        },
        ModuleConnection {
            id: ModuleConnectionId::new(),
            from: ModulePortAddress {
                node_id: merge_id,
                port: IMAGE_OUTPUT_PORT.to_string(),
            },
            to: output_target,
            order: 0,
            blend_mode: BlendMode::Normal,
        },
    ]);
    let parameter_id = PublishedParameterId::new();
    definition.interface.parameters.push(PublishedParameter {
        id: parameter_id,
        name: "Amount".to_string(),
        data_type: PortDataType::Number,
        default_value: PropertyValue::Number(OrderedFloat(1.0)),
        target: ModulePortAddress {
            node_id: first_node_id,
            port: NUMERIC_B_INPUT_PORT.to_string(),
        },
    });
    definition.interface.signals.push(PublishedSignal {
        id: PublishedSignalId::new(),
        name: "Result".to_string(),
        data_type: PortDataType::Number,
        source: ModulePortAddress {
            node_id: second_node_id,
            port: NUMBER_RESULT_OUTPUT_PORT.to_string(),
        },
    });
    definition.interface.actions.push(PublishedAction {
        id: PublishedActionId::new(),
        name: "Set amount".to_string(),
        target: ModulePortAddress {
            node_id: second_node_id,
            port: NUMERIC_B_INPUT_PORT.to_string(),
        },
    });
    definition.topology_revision += 1;
    definition.interface_version += 1;
    let definition_id = definition.id;
    service
        .add_module_definition(definition)
        .expect("definition");
    let placement = |start, layer| ModuleItemPlacement {
        track_id,
        name: "Node Clip".to_string(),
        output_id,
        interval: TimelineInterval::new(start, seconds(4)).expect("interval"),
        layer,
        parameter_overrides: HashMap::new(),
        input_bindings: HashMap::new(),
    };
    let (item_id, instance_id, _) = service
        .place_module_item(definition_id, placement(MediaTime::zero(), 0))
        .expect("first item");
    let (_, sibling_instance_id, _) = service
        .place_module_item(definition_id, placement(seconds(5), 1))
        .expect("sibling item");
    service
        .set_module_parameter(
            instance_id,
            parameter_id,
            PropertyValue::Number(OrderedFloat(7.0)),
        )
        .expect("current override");
    service
        .upsert_module_parameter_keyframe(
            &ModuleParameterOwner::Invocation(ModuleAutomationOwner::Item(item_id)),
            parameter_id,
            MediaTime::zero(),
            PropertyValue::Number(OrderedFloat(2.0)),
            Some(EasingFunction::Linear),
        )
        .expect("first key");
    service
        .upsert_module_parameter_keyframe(
            &ModuleParameterOwner::Invocation(ModuleAutomationOwner::Item(item_id)),
            parameter_id,
            seconds(1),
            PropertyValue::Number(OrderedFloat(9.0)),
            Some(EasingFunction::EaseInOutQuad),
        )
        .expect("second key");
    Fixture {
        service,
        item_id,
        instance_id,
        sibling_instance_id,
        first_node_id,
        second_node_id,
        output_node_id,
        parameter_id,
        connection_id,
    }
}

fn item_invocation(project: &AuthoringProject, item_id: TimelineItemId) -> &ModuleInvocation {
    let SourceRef::Module(invocation) = &project.items[&item_id].source else {
        panic!("expected Node Clip")
    };
    invocation
}

fn simple_image_definition(
    sharing: ModuleDefinitionSharing,
    primary_input: bool,
) -> (ModuleDefinition, ModuleOutputId) {
    let merge = Node::new_merge("Image");
    let merge_id = merge.id;
    let (mut definition, output_id) = ModuleDefinition::new_image("Paste target", sharing);
    let output_target = definition
        .output(output_id)
        .expect("Output")
        .target(PortDataType::Image)
        .expect("Image target");
    definition.graph.nodes.insert(merge_id, merge);
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
    if primary_input {
        definition.interface.media_inputs.push(PublishedMediaInput {
            id: PublishedMediaInputId::new(),
            name: "Image".to_string(),
            data_type: PortDataType::Image,
            target: ModulePortAddress {
                node_id: merge_id,
                port: MERGE_IMAGES_PORT.to_string(),
            },
            required: true,
            primary: true,
        });
        definition.interface_version += 1;
    }
    definition.topology_revision += 1;
    (definition, output_id)
}

fn paste_target_item(service: &TimelineEditorService) -> (TimelineItemId, ModuleInstanceId) {
    let project = service.snapshot().expect("project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let (definition, output_id) = simple_image_definition(ModuleDefinitionSharing::Private, false);
    service
        .create_private_module_item(
            definition,
            ModuleItemPlacement {
                track_id,
                name: "Paste target".to_string(),
                output_id,
                interval: TimelineInterval::new(MediaTime::zero(), seconds(8)).expect("interval"),
                layer: 0,
                parameter_overrides: HashMap::new(),
                input_bindings: HashMap::new(),
            },
        )
        .map(|(item_id, instance_id, _)| (item_id, instance_id))
        .expect("target Node Clip")
}

fn pasted_parameter<'a>(
    project: &'a AuthoringProject,
    definition_id: ModuleDefinitionId,
    pasted_node_ids: &[uuid::Uuid],
) -> &'a PublishedParameter {
    project.module_definitions[&definition_id]
        .interface
        .parameters
        .iter()
        .find(|parameter| {
            parameter.name == "Amount" && pasted_node_ids.contains(&parameter.target.node_id)
        })
        .expect("pasted parameter")
}

#[test]
fn paste_is_one_cow_edit_with_fresh_topology_interface_and_automation_ids() {
    let fixture = fixture();
    let before = fixture.service.snapshot().expect("before paste");
    let source_definition_id = before.module_instances[&fixture.instance_id].definition_id;
    let source_track =
        item_invocation(&before, fixture.item_id).automation_tracks[&fixture.parameter_id].clone();
    let clipboard = ModuleSelectionClipboard::capture(
        &before,
        fixture.instance_id,
        None,
        &[
            fixture.output_node_id,
            fixture.first_node_id,
            fixture.second_node_id,
        ],
    )
    .expect("capture filters Output");
    assert_eq!(clipboard.nodes.len(), 2);
    assert_eq!(clipboard.connections.len(), 1);

    let revision = fixture.service.revision().expect("revision");
    let receipt = fixture
        .service
        .paste_instance_module_selection(fixture.instance_id, None, &clipboard, [500.0, 600.0])
        .expect("paste");
    assert_eq!(receipt.changes.revision.get(), revision.get() + 1);
    assert_eq!(receipt.node_ids.len(), 2);
    assert!(
        receipt
            .changes
            .invalidations
            .contains(&ProjectInvalidation::ModuleInstance {
                instance_id: fixture.instance_id
            })
    );
    let after = fixture.service.snapshot().expect("after paste");
    assert_ne!(receipt.definition_id, source_definition_id);
    assert_eq!(
        after.module_instances[&fixture.sibling_instance_id].definition_id,
        source_definition_id
    );
    let pasted_definition = &after.module_definitions[&receipt.definition_id];
    assert!(
        receipt
            .node_ids
            .iter()
            .all(|node_id| pasted_definition.graph.nodes.contains_key(node_id))
    );
    let pasted_connection = pasted_definition
        .graph
        .connections
        .iter()
        .find(|connection| {
            receipt.node_ids.contains(&connection.from.node_id)
                && receipt.node_ids.contains(&connection.to.node_id)
        })
        .expect("pasted internal connection");
    assert_ne!(pasted_connection.id, fixture.connection_id);
    let pasted_parameter = pasted_definition
        .interface
        .parameters
        .iter()
        .find(|parameter| {
            receipt.node_ids.contains(&parameter.target.node_id) && parameter.name == "Amount"
        })
        .expect("pasted parameter");
    assert_ne!(pasted_parameter.id, fixture.parameter_id);
    assert_eq!(
        after.module_instances[&fixture.instance_id]
            .parameter_overrides
            .get(&pasted_parameter.id),
        Some(&PropertyValue::Number(OrderedFloat(7.0)))
    );
    let pasted_track =
        &item_invocation(&after, fixture.item_id).automation_tracks[&pasted_parameter.id];
    assert_eq!(pasted_track.keyframes.len(), source_track.keyframes.len());
    for (source, pasted) in source_track.keyframes.iter().zip(&pasted_track.keyframes) {
        assert_ne!(source.id, pasted.id);
        assert_eq!(source.time, pasted.time);
        assert_eq!(source.value, pasted.value);
        assert_eq!(source.easing, pasted.easing);
    }
    assert_eq!(
        pasted_definition
            .interface
            .signals
            .iter()
            .filter(|signal| receipt.node_ids.contains(&signal.source.node_id))
            .count(),
        1
    );
    assert_eq!(
        pasted_definition
            .interface
            .actions
            .iter()
            .filter(|action| receipt.node_ids.contains(&action.target.node_id))
            .count(),
        1
    );
    assert_eq!(
        receipt
            .node_ids
            .iter()
            .map(|node_id| pasted_definition.graph.nodes[node_id].ui_position[0])
            .fold(f32::INFINITY, f32::min),
        500.0
    );
    assert_eq!(
        receipt
            .node_ids
            .iter()
            .map(|node_id| pasted_definition.graph.nodes[node_id].ui_position[1])
            .fold(f32::INFINITY, f32::min),
        600.0
    );
    fixture.service.undo().expect("undo").expect("paste");
    assert_eq!(fixture.service.snapshot().expect("restored"), before);
}

#[test]
fn malformed_clipboard_is_rejected_without_cow_or_history() {
    let fixture = fixture();
    let before = fixture.service.snapshot().expect("before");
    let revision = fixture.service.revision().expect("revision");
    let mut clipboard = ModuleSelectionClipboard::capture(
        &before,
        fixture.instance_id,
        None,
        &[fixture.first_node_id, fixture.second_node_id],
    )
    .expect("capture");
    clipboard.connections[0].to.node_id = uuid::Uuid::new_v4();
    let error = fixture
        .service
        .paste_instance_module_selection(fixture.instance_id, None, &clipboard, [40.0, 60.0])
        .expect_err("foreign endpoint");
    assert!(error.to_string().contains("escapes"));
    assert_eq!(fixture.service.revision().expect("revision"), revision);
    assert_eq!(fixture.service.snapshot().expect("unchanged"), before);
}

#[test]
fn media_clipboard_missing_from_destination_assets_rolls_back_atomically() {
    let service =
        TimelineEditorService::create_default("Media clipboard destination").expect("service");
    let (_, instance_id) = paste_target_item(&service);
    let missing_asset_id = uuid::Uuid::new_v4();
    let media = Node::from_media_converter(
        "Missing media",
        MediaContent::new(missing_asset_id, MediaOutputSelection::Image, None, None)
            .expect("Media content"),
        &[],
        "missing.png".to_string(),
    )
    .expect("Media Node");
    let clipboard = ModuleSelectionClipboard {
        version: CLIPBOARD_VERSION,
        nodes: vec![media],
        connections: Vec::new(),
        parameters: Vec::new(),
        signals: Vec::new(),
        actions: Vec::new(),
    };
    let before = service.snapshot().expect("before");
    let revision = service.revision().expect("revision");
    let error = service
        .paste_instance_module_selection(instance_id, None, &clipboard, [40.0, 60.0])
        .expect_err("missing Asset");
    assert!(error.to_string().contains(&missing_asset_id.to_string()));
    assert_eq!(service.revision().expect("revision"), revision);
    assert_eq!(service.snapshot().expect("unchanged"), before);
}

#[test]
fn image_collection_clipboard_validates_asset_kind_before_cow() {
    use crate::model::node::DataContent;
    use crate::model::project::asset::{Asset, AssetKind};
    use crate::model::project::connection::DATA_VALUE_PROPERTY;
    use crate::model::property::{ImageCollectionValue, Property};

    let service = TimelineEditorService::create_default("Collection destination").unwrap();
    let (_, instance_id) = paste_target_item(&service);
    let audio = Asset::new("not image", "sound.wav", AssetKind::Audio);
    let audio_id = audio.id;
    service.add_asset(audio).unwrap();
    let mut collection = Node::new_data("Sprites", DataContent::ImageCollection);
    collection
        .set_property(
            DATA_VALUE_PROPERTY.into(),
            Property::constant(PropertyValue::ImageCollection(
                ImageCollectionValue::new(vec![audio_id]).unwrap(),
            )),
        )
        .unwrap();
    let clipboard = ModuleSelectionClipboard {
        version: CLIPBOARD_VERSION,
        nodes: vec![collection],
        connections: Vec::new(),
        parameters: Vec::new(),
        signals: Vec::new(),
        actions: Vec::new(),
    };
    let before = service.snapshot().unwrap();
    let revision = service.revision().unwrap();
    let error = service
        .paste_instance_module_selection(instance_id, None, &clipboard, [40.0, 60.0])
        .expect_err("Audio cannot enter an Image Collection");
    assert!(error.to_string().contains("non-Image Asset"), "{error}");
    assert_eq!(service.revision().unwrap(), revision);
    assert_eq!(service.snapshot().unwrap(), before);
}

#[test]
fn serde_clipboard_pastes_automation_into_item_attachment_and_transition_hosts() {
    let source = fixture();
    let source_project = source.service.snapshot().expect("source");
    let clipboard = ModuleSelectionClipboard::capture(
        &source_project,
        source.instance_id,
        None,
        &[source.first_node_id, source.second_node_id],
    )
    .expect("capture");
    let encoded = serde_json::to_string(&clipboard).expect("serialize clipboard");
    let decoded: ModuleSelectionClipboard =
        serde_json::from_str(&encoded).expect("deserialize clipboard");
    assert_eq!(decoded, clipboard);
    let clipboard = decoded;

    let service = TimelineEditorService::create_default("Cross-document paste").expect("service");
    let (item_id, item_instance_id) = paste_target_item(&service);
    let item_receipt = service
        .paste_instance_module_selection(item_instance_id, None, &clipboard, [40.0, 60.0])
        .expect("paste into unrelated Node Clip");
    let project = service.snapshot().expect("item paste");
    let item_parameter =
        pasted_parameter(&project, item_receipt.definition_id, &item_receipt.node_ids);
    assert_eq!(
        item_invocation(&project, item_id).automation_tracks[&item_parameter.id]
            .keyframes
            .len(),
        2
    );
    drop(project);

    let (attachment_definition, attachment_output_id) =
        simple_image_definition(ModuleDefinitionSharing::Private, true);
    let attachment_definition_id = attachment_definition.id;
    let (attachment_id, attachment_instance_id, _) = service
        .create_private_module_attachment(
            attachment_definition,
            ModuleAttachmentPlacement {
                owner: AttachmentOwner::Item { item_id },
                stage: AttachmentStage::ItemPostTransform,
                definition_id: attachment_definition_id,
                output_id: attachment_output_id,
                parameter_overrides: HashMap::new(),
                input_bindings: HashMap::new(),
            },
        )
        .expect("target Module Effect");
    let attachment_receipt = service
        .paste_instance_module_selection(attachment_instance_id, None, &clipboard, [80.0, 100.0])
        .expect("paste into Module Effect");
    let project = service.snapshot().expect("attachment paste");
    let attachment_parameter = pasted_parameter(
        &project,
        attachment_receipt.definition_id,
        &attachment_receipt.node_ids,
    );
    let AttachmentProcessor::Module(attachment_invocation) =
        &project.attachments[&attachment_id].processor
    else {
        panic!("expected Module Effect")
    };
    assert_eq!(
        attachment_invocation.automation_tracks[&attachment_parameter.id]
            .keyframes
            .len(),
        2
    );
    drop(project);

    let project = service.snapshot().expect("project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let source_ref = |red| SourceRef::Solid {
        color: crate::model::frame::color::Color {
            r: red,
            g: 0,
            b: 0,
            a: 255,
        },
    };
    let (from_item_id, _) = service
        .add_item(
            track_id,
            "From".to_string(),
            source_ref(32),
            TimelineInterval::new(MediaTime::zero(), seconds(7)).expect("interval"),
            1,
        )
        .expect("from item");
    let (to_item_id, _) = service
        .add_item(
            track_id,
            "To".to_string(),
            source_ref(224),
            TimelineInterval::new(seconds(3), seconds(7)).expect("interval"),
            2,
        )
        .expect("to item");
    let (transition_id, _) = service
        .add_transition(TransitionPlacement {
            from_item_id,
            to_item_id,
            edit_point: seconds(5),
            duration: seconds(4),
            alignment: TransitionAlignment::CenteredOnEdit,
            processor: TransitionProcessor::cross_dissolve(),
            parameters: HashMap::new(),
        })
        .expect("Transition");
    let (transition_definition, _) = ModuleDefinition::new_transition(
        "Paste Transition",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
        TransitionMediaType::Image,
    )
    .expect("Transition definition");
    let transition_definition_id = transition_definition.id;
    service
        .add_module_definition(transition_definition)
        .expect("Transition definition");
    let transition_instance_id = service
        .assign_transition_module(transition_id, transition_definition_id)
        .expect("Transition Module")
        .0;
    let transition_receipt = service
        .paste_instance_module_selection(transition_instance_id, None, &clipboard, [120.0, 140.0])
        .expect("paste into Transition Module");
    let project = service.snapshot().expect("transition paste");
    let transition_parameter = pasted_parameter(
        &project,
        transition_receipt.definition_id,
        &transition_receipt.node_ids,
    );
    let transition = project.transitions[&transition_id]
        .processor
        .module_processor()
        .expect("Module Transition");
    assert_eq!(
        transition.automation_tracks[&transition_parameter.id]
            .keyframes
            .len(),
        2
    );
}

#[test]
fn nested_transition_clipboard_uses_only_the_concrete_sparse_controls() {
    use crate::editor::timeline_editor_service::transition_parameter_automation_tests::{
        transition_project, wrap_with_two_composition_instances,
    };

    let (project, transition_id, source_parameter_id) = transition_project();
    let definition_service = TimelineEditorService::new(project).expect("definition service");
    definition_service
        .upsert_module_parameter_keyframe(
            &ModuleParameterOwner::Transition(TransitionAutomationOwner::Definition(transition_id)),
            source_parameter_id,
            MediaTime::zero(),
            PropertyValue::Number(OrderedFloat(1.0)),
            Some(EasingFunction::Linear),
        )
        .expect("inherited key");
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
    service
        .set_module_parameter_constant(
            &ModuleParameterOwner::Transition(first_owner.clone()),
            source_parameter_id,
            PropertyValue::Number(OrderedFloat(7.0)),
        )
        .expect("concrete value");
    let (first_key_id, _) = service
        .upsert_module_parameter_keyframe(
            &ModuleParameterOwner::Transition(first_owner.clone()),
            source_parameter_id,
            MediaTime::zero(),
            PropertyValue::Number(OrderedFloat(2.0)),
            Some(EasingFunction::Linear),
        )
        .expect("first concrete key");
    let (second_key_id, _) = service
        .upsert_module_parameter_keyframe(
            &ModuleParameterOwner::Transition(first_owner),
            source_parameter_id,
            seconds(1),
            PropertyValue::Number(OrderedFloat(9.0)),
            Some(EasingFunction::EaseInOutQuad),
        )
        .expect("second concrete key");

    let before = service.snapshot().expect("before paste");
    let instance_id = before.transitions[&transition_id]
        .processor
        .module_processor()
        .expect("Module Transition")
        .instance_id;
    let definition_id = before.module_instances[&instance_id].definition_id;
    let source_node_id = before.module_definitions[&definition_id]
        .interface
        .parameters
        .iter()
        .find(|parameter| parameter.id == source_parameter_id)
        .expect("source parameter")
        .target
        .node_id;
    let clipboard = ModuleSelectionClipboard::capture(
        &before,
        instance_id,
        Some(&first_path),
        &[source_node_id],
    )
    .expect("capture concrete controls");
    let sibling_target = before
        .resolve_transition_module_instance_target(&second_path, transition_id)
        .expect("sibling target");
    let source_target = before
        .resolve_transition_module_instance_target(&first_path, transition_id)
        .expect("source target");
    let source_effective = before
        .effective_transition_module_controls(&source_target)
        .expect("source controls");
    let source_track = &source_effective.automation_tracks[&source_parameter_id];
    let sibling_before = before
        .effective_transition_module_controls(&sibling_target)
        .expect("sibling controls");

    let receipt = service
        .paste_instance_module_selection(instance_id, Some(&first_path), &clipboard, [700.0, 500.0])
        .expect("paste concrete controls");
    let after = service.snapshot().expect("after paste");
    let pasted_parameter = pasted_parameter(&after, receipt.definition_id, &receipt.node_ids);
    let first_target = after
        .resolve_transition_module_instance_target(&first_path, transition_id)
        .expect("first target");
    let first_effective = after
        .effective_transition_module_controls(&first_target)
        .expect("first controls");
    assert_eq!(
        first_effective.parameter_overrides[&pasted_parameter.id],
        PropertyValue::Number(OrderedFloat(7.0))
    );
    let pasted_track = &first_effective.automation_tracks[&pasted_parameter.id];
    assert_eq!(pasted_track.keyframes.len(), source_track.keyframes.len());
    for (source, pasted) in source_track.keyframes.iter().zip(&pasted_track.keyframes) {
        assert_ne!(source.id, pasted.id);
        assert_eq!(source.time, pasted.time);
        assert_eq!(source.value, pasted.value);
        assert_eq!(source.easing, pasted.easing);
    }
    assert!(
        pasted_track
            .keyframes
            .iter()
            .all(|keyframe| keyframe.id != first_key_id && keyframe.id != second_key_id)
    );
    let sibling_after = after
        .effective_transition_module_controls(&sibling_target)
        .expect("sibling controls");
    assert_eq!(sibling_after, sibling_before);
    assert!(
        !sibling_after
            .parameter_overrides
            .contains_key(&pasted_parameter.id)
    );
    assert!(
        !sibling_after
            .automation_tracks
            .contains_key(&pasted_parameter.id)
    );
    let persisted = after
        .transition_module_instance_overrides(&first_target)
        .expect("override lookup")
        .expect("first sparse controls");
    assert!(
        persisted
            .parameter_overrides
            .contains_key(&pasted_parameter.id)
    );
    assert!(matches!(
        persisted.automation_tracks.get(&pasted_parameter.id),
        Some(Some(track)) if track.keyframes.len() == 2
    ));
    assert!(
        after
            .transition_module_instance_overrides(&sibling_target)
            .expect("sibling lookup")
            .is_none()
    );

    service.undo().expect("Undo").expect("paste transaction");
    assert_eq!(service.snapshot().expect("Undo state"), before);
    service.redo().expect("Redo").expect("paste transaction");
    assert_eq!(service.snapshot().expect("Redo state"), after);
}

#[test]
fn transition_clipboard_rejects_wrong_host_and_stale_path_atomically() {
    let fixture = fixture();
    let item_project = fixture.service.snapshot().expect("item project");
    let unrelated_path = InstancePath::root(item_project.root_timeline_id);
    let error = ModuleSelectionClipboard::capture(
        &item_project,
        fixture.instance_id,
        Some(&unrelated_path),
        &[fixture.first_node_id],
    )
    .expect_err("Node Clip cannot accept a Transition path");
    assert!(error.contains("Transition instance path"), "{error}");

    use crate::editor::timeline_editor_service::transition_parameter_automation_tests::{
        transition_project, wrap_with_two_composition_instances,
    };
    let (mut project, transition_id, parameter_id) = transition_project();
    let root_instance_id = project.transitions[&transition_id]
        .processor
        .module_processor()
        .expect("root Module Transition")
        .instance_id;
    let root_definition_id = project.module_instances[&root_instance_id].definition_id;
    let root_node_id = project.module_definitions[&root_definition_id]
        .interface
        .parameters
        .iter()
        .find(|parameter| parameter.id == parameter_id)
        .expect("root parameter")
        .target
        .node_id;
    let root_path = InstancePath::root(project.root_timeline_id);
    let definition_clipboard =
        ModuleSelectionClipboard::capture(&project, root_instance_id, None, &[root_node_id])
            .expect("definition capture");
    let root_path_clipboard = ModuleSelectionClipboard::capture(
        &project,
        root_instance_id,
        Some(&root_path),
        &[root_node_id],
    )
    .expect("validated root-path capture");
    assert_eq!(root_path_clipboard, definition_clipboard);

    let nested_timeline_id = project.root_timeline_id;
    let (root_timeline_id, first_item_id, _) =
        wrap_with_two_composition_instances(&mut project, nested_timeline_id);
    let instance_id = project.transitions[&transition_id]
        .processor
        .module_processor()
        .expect("Module Transition")
        .instance_id;
    let definition_id = project.module_instances[&instance_id].definition_id;
    let node_id = project.module_definitions[&definition_id]
        .interface
        .parameters
        .iter()
        .find(|parameter| parameter.id == parameter_id)
        .expect("parameter")
        .target
        .node_id;
    let valid_path = InstancePath::root(root_timeline_id).nested(first_item_id);
    let clipboard =
        ModuleSelectionClipboard::capture(&project, instance_id, Some(&valid_path), &[node_id])
            .expect("valid concrete capture");
    let service = TimelineEditorService::new(project).expect("service");
    let stale_path = InstancePath::root(root_timeline_id).nested(TimelineItemId::new());
    let before = service.snapshot().expect("before rejected paste");
    let revision = service.revision().expect("revision");
    let wrong_root_path = InstancePath::root(root_timeline_id);
    let error = service
        .paste_instance_module_selection(
            instance_id,
            Some(&wrong_root_path),
            &clipboard,
            [300.0, 400.0],
        )
        .expect_err("root-only path cannot address a nested Transition");
    assert!(error.to_string().contains("does not belong"), "{error}");
    assert_eq!(service.revision().expect("unchanged revision"), revision);
    assert_eq!(service.snapshot().expect("unchanged project"), before);

    let error = service
        .paste_instance_module_selection(instance_id, Some(&stale_path), &clipboard, [300.0, 400.0])
        .expect_err("stale path");
    assert!(error.to_string().contains("missing item"), "{error}");
    assert_eq!(service.revision().expect("unchanged revision"), revision);
    assert_eq!(service.snapshot().expect("unchanged project"), before);
}
