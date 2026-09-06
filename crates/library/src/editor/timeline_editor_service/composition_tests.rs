use super::*;

use crate::model::authoring::{
    ItemOutputStage, MediaOutputKind, ModuleDefinitionSharing, ModulePortAddress,
    PublishedMediaInput, TransitionAlignment, TransitionProcessor,
};
use crate::model::node::Node;
use crate::model::project::{MERGE_IMAGES_PORT, PortDataType};

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).expect("whole seconds")
}

fn text_source(text: &str) -> SourceRef {
    SourceRef::Text {
        text: text.to_string(),
        appearance_operations: Vec::new(),
        ensemble_operations: Vec::new(),
    }
}

fn image_effect() -> BuiltinEffectInstance {
    BuiltinEffectInstance {
        operation: crate::model::authoring::OperationRef {
            category: "effect".to_string(),
            component_id: "qa-effect".to_string(),
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

fn private_image_effect(name: &str) -> (ModuleDefinition, ModuleOutputId) {
    let (mut definition, output_id) =
        ModuleDefinition::new_image(name, ModuleDefinitionSharing::Private);
    let target = definition
        .output(output_id)
        .expect("Output terminal")
        .target(PortDataType::Image)
        .expect("Image input");
    definition.interface.media_inputs.push(PublishedMediaInput {
        id: PublishedMediaInputId::new(),
        name: "Input".to_string(),
        data_type: PortDataType::Image,
        target,
        required: true,
        primary: true,
    });
    (definition, output_id)
}

fn text_fixture() -> (TimelineEditorService, TimelineItemId, AttachmentId) {
    let service = TimelineEditorService::create_default("Composition reuse").expect("service");
    let project = service.snapshot().expect("snapshot");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let (item_id, _) = service
        .add_item(
            track_id,
            "Title".to_string(),
            text_source("Shared words"),
            TimelineInterval::new(seconds(3), seconds(5)).expect("interval"),
            0,
        )
        .expect("Text item");
    service
        .set_item_blend_mode(item_id, BlendMode::Multiply)
        .expect("blend");
    let (attachment_id, _) = service
        .add_builtin_attachment(
            AttachmentOwner::Item { item_id },
            AttachmentStage::ItemPostTransform,
            image_effect(),
        )
        .expect("attachment");
    (service, item_id, attachment_id)
}

#[test]
fn extraction_is_one_transparent_nested_timeline_and_preserves_outer_placement() {
    let (service, item_id, attachment_id) = text_fixture();
    let before = service.snapshot().expect("before");
    let outer_before = before.items[&item_id].clone();
    let host_timeline = before.timelines[&before.root_timeline_id].clone();
    let revision = service.revision().expect("revision");

    let (timeline_id, changes) = service
        .extract_item_to_composition(item_id, "Reusable title".to_string())
        .expect("extract");
    assert_eq!(changes.revision.get(), revision.get() + 1);
    let extracted = service.snapshot().expect("extracted");
    let outer = &extracted.items[&item_id];
    assert_eq!(outer.id, outer_before.id);
    assert_eq!(outer.track_id, outer_before.track_id);
    assert_eq!(outer.interval, outer_before.interval);
    assert_eq!(outer.layer, outer_before.layer);
    assert_eq!(outer.name, outer_before.name);
    assert_eq!(outer.blend_mode, BlendMode::Multiply);
    assert_eq!(outer.time_map, TimeMap::default());
    assert!(outer.authored_properties.iter().next().is_none());
    let SourceRef::Composition(instance) = &outer.source else {
        panic!("Composition source")
    };
    assert_eq!(instance.timeline_id, timeline_id);
    assert_eq!(instance.duration_policy, DurationPolicy::Fixed);
    assert!(instance.parameter_overrides.is_empty());
    assert!(instance.transition_module_overrides.is_empty());

    let nested = &extracted.timelines[&timeline_id];
    assert_eq!(
        (nested.width, nested.height, nested.fps),
        (host_timeline.width, host_timeline.height, host_timeline.fps)
    );
    assert_eq!(nested.duration, outer_before.interval.duration);
    assert_eq!(nested.background_color.a, 0);
    assert_eq!(nested.published_parameters.len(), 1);
    assert_eq!(nested.published_parameters[0].name, "Text");
    assert_eq!(
        nested.published_parameters[0].default_value,
        PropertyValue::String("Shared words".to_string())
    );
    let inner_item_id = nested.published_parameters[0].target.item_id();
    let inner = &extracted.items[&inner_item_id];
    assert_eq!(
        inner.interval,
        TimelineInterval::new(MediaTime::zero(), seconds(5)).expect("inner interval")
    );
    assert_eq!(inner.time_map, outer_before.time_map);
    assert_eq!(inner.blend_mode, BlendMode::Normal);
    assert_eq!(inner.source, outer_before.source);
    assert_eq!(
        extracted.attachments[&attachment_id].owner,
        AttachmentOwner::Item {
            item_id: inner_item_id
        }
    );
    assert_eq!(extracted.composition_reference_count(timeline_id), 1);

    service.undo().expect("undo").expect("history");
    assert_eq!(
        service.snapshot().expect("undo snapshot").as_ref(),
        before.as_ref()
    );
    service.redo().expect("redo").expect("history");
    assert_eq!(
        service.snapshot().expect("redo snapshot").as_ref(),
        extracted.as_ref()
    );
}

#[test]
fn unique_copy_remaps_public_text_control_and_isolates_linked_placement() {
    let (service, item_id, _) = text_fixture();
    let (shared_timeline_id, _) = service
        .extract_item_to_composition(item_id, "Reusable title".to_string())
        .expect("extract");
    let shared = service.snapshot().expect("shared");
    let shared_parameter_id = shared.timelines[&shared_timeline_id].published_parameters[0].id;
    let outer = &shared.items[&item_id];
    let (sibling_id, _) = service
        .duplicate_item(item_id, seconds(10), outer.layer + 1)
        .expect("linked duplicate");
    service
        .set_composition_parameter_override(
            item_id,
            shared_parameter_id,
            PropertyValue::String("Only this placement".to_string()),
        )
        .expect("override");
    let before_unique = service.snapshot().expect("before unique");
    assert_eq!(
        before_unique.composition_reference_count(shared_timeline_id),
        2
    );

    let revision = service.revision().expect("revision");
    let (unique_timeline_id, changes) = service
        .make_composition_unique(item_id)
        .expect("make unique");
    assert_eq!(changes.revision.get(), revision.get() + 1);
    let unique = service.snapshot().expect("unique");
    assert_eq!(unique.composition_reference_count(shared_timeline_id), 1);
    assert_eq!(unique.composition_reference_count(unique_timeline_id), 1);
    let SourceRef::Composition(unique_instance) = &unique.items[&item_id].source else {
        panic!("unique Composition")
    };
    let SourceRef::Composition(sibling_instance) = &unique.items[&sibling_id].source else {
        panic!("linked Composition")
    };
    assert_eq!(sibling_instance.timeline_id, shared_timeline_id);
    assert_eq!(unique_instance.timeline_id, unique_timeline_id);
    let unique_parameter = &unique.timelines[&unique_timeline_id].published_parameters[0];
    assert_ne!(unique_parameter.id, shared_parameter_id);
    assert_ne!(
        unique_parameter.target,
        unique.timelines[&shared_timeline_id].published_parameters[0].target
    );
    assert_eq!(
        unique_instance.parameter_overrides[&unique_parameter.id],
        PropertyValue::String("Only this placement".to_string())
    );
    assert!(sibling_instance.parameter_overrides.is_empty());

    service.undo().expect("undo").expect("history");
    assert_eq!(
        service.snapshot().expect("undo snapshot").as_ref(),
        before_unique.as_ref()
    );
    service.redo().expect("redo").expect("history");
    assert_eq!(
        service.snapshot().expect("redo snapshot").as_ref(),
        unique.as_ref()
    );
}

#[test]
fn extraction_and_unique_reject_unsupported_scope_without_history() {
    let (service, item_id, _) = text_fixture();
    let initial = service.snapshot().expect("initial");
    let track_id = initial.items[&item_id].track_id;
    drop(initial);
    let (parent_id, _) = service
        .add_item(
            track_id,
            "Parent".to_string(),
            SourceRef::Solid {
                color: Color::black(),
            },
            TimelineInterval::new(MediaTime::zero(), seconds(5)).expect("interval"),
            1,
        )
        .expect("parent");
    service
        .set_item_parent(item_id, Some(parent_id))
        .expect("valid parent");
    let before = service.snapshot().expect("before");
    let before_revision = service.revision().expect("revision");
    let error = service
        .extract_item_to_composition(item_id, "Unsupported".to_string())
        .expect_err("parented extraction rejected");
    assert!(error.to_string().contains("parented items"));
    assert_eq!(service.revision().expect("revision"), before_revision);
    assert_eq!(
        service.snapshot().expect("unchanged").as_ref(),
        before.as_ref()
    );

    let error = service
        .make_composition_unique(parent_id)
        .expect_err("ordinary item is not a Composition");
    assert!(error.to_string().contains("is not a Composition"));
    assert_eq!(service.revision().expect("revision"), before_revision);
    assert_eq!(
        service.snapshot().expect("unchanged").as_ref(),
        before.as_ref()
    );
}

#[test]
fn unique_copy_remaps_transition_binding_and_private_module_owners() {
    let service = TimelineEditorService::create_default("Composition topology").expect("service");
    let (timeline_id, track_id, _) = service
        .add_timeline(
            "Reusable scene".to_string(),
            1920,
            1080,
            RationalRate::new(30, 1).expect("fps"),
            seconds(10),
        )
        .expect("nested Timeline");
    let (from_item_id, _) = service
        .add_item(
            track_id,
            "From".to_string(),
            SourceRef::Solid {
                color: Color::black(),
            },
            TimelineInterval::new(MediaTime::zero(), seconds(7)).expect("from interval"),
            0,
        )
        .expect("from");
    let (to_item_id, _) = service
        .add_item(
            track_id,
            "To".to_string(),
            SourceRef::Solid {
                color: Color::white(),
            },
            TimelineInterval::new(seconds(3), seconds(7)).expect("to interval"),
            1,
        )
        .expect("to");
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
        .expect("transition");

    let (mut node_definition, node_output_id) =
        ModuleDefinition::new_image("Private Node Clip", ModuleDefinitionSharing::Private);
    let input_node = Node::new_merge("Reference");
    let input_node_id = input_node.id;
    node_definition
        .graph
        .nodes
        .insert(input_node_id, input_node);
    let media_input_id = PublishedMediaInputId::new();
    node_definition
        .interface
        .media_inputs
        .push(PublishedMediaInput {
            id: media_input_id,
            name: "Reference".to_string(),
            data_type: PortDataType::Image,
            target: ModulePortAddress {
                node_id: input_node_id,
                port: MERGE_IMAGES_PORT.to_string(),
            },
            required: false,
            primary: false,
        });
    let node_definition_id = node_definition.id;
    let (node_item_id, node_instance_id, _) = service
        .create_private_module_item(
            node_definition,
            ModuleItemPlacement {
                track_id,
                name: "Node Clip".to_string(),
                output_id: node_output_id,
                interval: TimelineInterval::new(MediaTime::zero(), seconds(10))
                    .expect("Node interval"),
                layer: 2,
                parameter_overrides: HashMap::new(),
                input_bindings: HashMap::from([(
                    media_input_id,
                    MediaInputBinding::TimelineItemOutput {
                        locator: InstanceLocator::SameTimeline,
                        item_id: from_item_id,
                        output: MediaOutputKind::Image,
                        stage: ItemOutputStage::PostTransform,
                    },
                )]),
            },
        )
        .expect("Node Clip");

    let (attachment_definition, attachment_output_id) = private_image_effect("Private Effect");
    let attachment_definition_id = attachment_definition.id;
    let (attachment_id, attachment_instance_id, _) = service
        .create_private_module_attachment(
            attachment_definition,
            ModuleAttachmentPlacement {
                owner: AttachmentOwner::Item {
                    item_id: to_item_id,
                },
                stage: AttachmentStage::ItemPostTransform,
                definition_id: attachment_definition_id,
                output_id: attachment_output_id,
                parameter_overrides: HashMap::new(),
                input_bindings: HashMap::new(),
            },
        )
        .expect("Module Attachment");

    let root = service.snapshot().expect("root");
    let root_track_id = root.timelines[&root.root_timeline_id].track_order[0];
    drop(root);
    let (outer_item_id, _) = service
        .add_item(
            root_track_id,
            "Reusable scene".to_string(),
            SourceRef::Composition(CompositionInstance {
                timeline_id,
                duration_policy: DurationPolicy::Fixed,
                parameter_overrides: HashMap::new(),
                transition_module_overrides: Vec::new(),
            }),
            TimelineInterval::new(MediaTime::zero(), seconds(10)).expect("outer interval"),
            0,
        )
        .expect("outer Composition");
    let before = service.snapshot().expect("before unique");

    let (unique_timeline_id, _) = service
        .make_composition_unique(outer_item_id)
        .expect("make unique");
    let unique = service.snapshot().expect("unique");
    assert_eq!(
        unique.timelines[&timeline_id],
        before.timelines[&timeline_id]
    );
    assert_eq!(unique.items[&node_item_id], before.items[&node_item_id]);
    assert_eq!(
        unique.attachments[&attachment_id],
        before.attachments[&attachment_id]
    );
    assert_eq!(
        unique.module_instances[&node_instance_id],
        before.module_instances[&node_instance_id]
    );
    assert_eq!(
        unique.module_instances[&attachment_instance_id],
        before.module_instances[&attachment_instance_id]
    );
    assert_eq!(
        unique.module_definitions[&node_definition_id],
        before.module_definitions[&node_definition_id]
    );
    assert_eq!(
        unique.module_definitions[&attachment_definition_id],
        before.module_definitions[&attachment_definition_id]
    );

    let unique_track_ids = &unique.timelines[&unique_timeline_id].track_order;
    let unique_items = unique
        .items
        .values()
        .filter(|item| unique_track_ids.contains(&item.track_id))
        .collect::<Vec<_>>();
    let copied_from = unique_items
        .iter()
        .find(|item| item.name == "From")
        .expect("copied From");
    let copied_node = unique_items
        .iter()
        .find(|item| item.name == "Node Clip")
        .expect("copied Node Clip");
    let SourceRef::Module(copied_invocation) = &copied_node.source else {
        panic!("copied Module")
    };
    assert_ne!(copied_invocation.instance_id, node_instance_id);
    let copied_node_definition_id =
        unique.module_instances[&copied_invocation.instance_id].definition_id;
    assert_ne!(copied_node_definition_id, node_definition_id);
    assert!(matches!(
        unique.module_definitions[&copied_node_definition_id].sharing,
        ModuleDefinitionSharing::Private
    ));
    let MediaInputBinding::TimelineItemOutput {
        locator, item_id, ..
    } = &copied_invocation.input_bindings[&media_input_id];
    assert_eq!(locator, &InstanceLocator::SameTimeline);
    assert_eq!(*item_id, copied_from.id);

    let copied_transition = unique
        .transitions
        .values()
        .find(|transition| transition.timeline_id == unique_timeline_id)
        .expect("copied transition");
    assert_ne!(copied_transition.id, transition_id);
    assert!(
        unique_items
            .iter()
            .any(|item| item.id == copied_transition.from_item_id)
    );
    assert!(
        unique_items
            .iter()
            .any(|item| item.id == copied_transition.to_item_id)
    );
    let copied_attachment = unique
        .attachments
        .values()
        .find(|attachment| {
            matches!(
                attachment.owner,
                AttachmentOwner::Item { item_id } if unique_items.iter().any(|item| item.id == item_id)
            ) && matches!(attachment.processor, AttachmentProcessor::Module(_))
        })
        .expect("copied Module Attachment");
    assert_ne!(copied_attachment.id, attachment_id);
    let AttachmentProcessor::Module(copied_attachment_invocation) = &copied_attachment.processor
    else {
        panic!("Module Attachment")
    };
    assert_ne!(
        copied_attachment_invocation.instance_id,
        attachment_instance_id
    );
    assert_ne!(
        unique.module_instances[&copied_attachment_invocation.instance_id].definition_id,
        attachment_definition_id
    );

    service.undo().expect("undo").expect("one transaction");
    assert_eq!(
        service.snapshot().expect("undo snapshot").as_ref(),
        before.as_ref()
    );
}

#[test]
fn unique_copy_preserves_authoritative_order_when_source_layers_and_starts_tie() {
    let service = TimelineEditorService::create_default("Composition tied order").expect("service");
    let (timeline_id, track_id, _) = service
        .add_timeline(
            "Tied scene".to_string(),
            640,
            360,
            RationalRate::new(30, 1).expect("fps"),
            seconds(5),
        )
        .expect("Timeline");
    let (first_id, _) = service
        .add_item(
            track_id,
            "First".to_string(),
            text_source("First"),
            TimelineInterval::new(MediaTime::zero(), seconds(5)).expect("interval"),
            0,
        )
        .expect("first");
    let (second_id, _) = service
        .add_item(
            track_id,
            "Second".to_string(),
            text_source("Second"),
            TimelineInterval::new(MediaTime::zero(), seconds(5)).expect("interval"),
            1,
        )
        .expect("second");
    let mut tied = service.snapshot().expect("snapshot").as_ref().clone();
    tied.items.get_mut(&first_id).expect("first").layer = 0;
    tied.items.get_mut(&second_id).expect("second").layer = 0;
    tied.validate()
        .expect("tied layers are valid authoring state");
    let service = TimelineEditorService::new(tied).expect("tied service");
    let source = service.snapshot().expect("source");
    let source_order = ordered_track_item_ids(&source, track_id, None)
        .into_iter()
        .map(|item_id| source.items[&item_id].name.clone())
        .collect::<Vec<_>>();
    drop(source);
    let root = service.snapshot().expect("root");
    let root_track_id = root.timelines[&root.root_timeline_id].track_order[0];
    drop(root);
    let (outer_item_id, _) = service
        .add_item(
            root_track_id,
            "Tied scene".to_string(),
            SourceRef::Composition(CompositionInstance {
                timeline_id,
                duration_policy: DurationPolicy::Fixed,
                parameter_overrides: HashMap::new(),
                transition_module_overrides: Vec::new(),
            }),
            TimelineInterval::new(MediaTime::zero(), seconds(5)).expect("outer interval"),
            0,
        )
        .expect("outer");
    let source_before = service.snapshot().expect("before");

    let (unique_timeline_id, _) = service
        .make_composition_unique(outer_item_id)
        .expect("make unique");
    let unique = service.snapshot().expect("unique");
    let unique_track_id = unique.timelines[&unique_timeline_id].track_order[0];
    let unique_order = ordered_track_item_ids(&unique, unique_track_id, None)
        .into_iter()
        .map(|item_id| unique.items[&item_id].name.clone())
        .collect::<Vec<_>>();
    assert_eq!(unique_order, source_order);
    assert_eq!(
        ordered_track_item_ids(&unique, unique_track_id, None)
            .into_iter()
            .map(|item_id| unique.items[&item_id].layer)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(unique.items[&first_id], source_before.items[&first_id]);
    assert_eq!(unique.items[&second_id], source_before.items[&second_id]);
}
