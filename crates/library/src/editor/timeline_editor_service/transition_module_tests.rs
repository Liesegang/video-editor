use super::*;

use ordered_float::OrderedFloat;

use crate::animation::EasingFunction;
use crate::core::render_plan::{ModuleHost, RenderPlanCompiler};
use crate::model::authoring::{
    AutomationKeyframe, CompositionInstance, DurationPolicy, InstanceLocator, ItemOutputStage,
    MediaOutputKind, ModuleDefinitionSharing, ModuleHostContract, ModulePortAddress,
    ModuleTemplateOrigin, OperationRef, ProcessorParameterContract, PublishedMediaInput,
    PublishedParameter, SourceRef, TRANSITION_APPLY_OPERATION, TRANSITION_CATEGORY,
    TransitionAlignment, TransitionContractSnapshot, TransitionMediaType, TransitionProcessor,
};
use crate::model::frame::color::Color;
use crate::model::node::{Node, ValueContent};
use crate::model::project::connection::{
    MERGE_IMAGES_PORT, MERGE_SOUNDS_PORT, NUMERIC_A_INPUT_PORT, PortDataType,
};

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).expect("whole seconds")
}

fn add_transition_pair(
    service: &TimelineEditorService,
    track_id: TimelineTrackId,
    name: &str,
) -> (
    TimelineItemId,
    TimelineItemId,
    crate::model::authoring::TransitionId,
) {
    let source = |red| SourceRef::Solid {
        color: Color {
            r: red,
            g: 0,
            b: 0,
            a: 255,
        },
    };
    let (from, _) = service
        .add_item(
            track_id,
            format!("{name} A"),
            source(32),
            TimelineInterval::new(seconds(0), seconds(7)).unwrap(),
            0,
        )
        .unwrap();
    let (to, _) = service
        .add_item(
            track_id,
            format!("{name} B"),
            source(224),
            TimelineInterval::new(seconds(3), seconds(7)).unwrap(),
            1,
        )
        .unwrap();
    let (transition_id, _) = service
        .add_transition(TransitionPlacement {
            from_item_id: from,
            to_item_id: to,
            edit_point: seconds(5),
            duration: seconds(4),
            alignment: TransitionAlignment::CenteredOnEdit,
            processor: TransitionProcessor::cross_dissolve(),
            parameters: HashMap::new(),
        })
        .unwrap();
    (from, to, transition_id)
}

fn service_with_two_transitions() -> (
    TimelineEditorService,
    crate::model::authoring::TransitionId,
    crate::model::authoring::TransitionId,
) {
    let service = TimelineEditorService::create_default("Transition Modules").unwrap();
    let project = service.snapshot().unwrap();
    let timeline_id = project.root_timeline_id;
    let first_track = project.timelines[&timeline_id].track_order[0];
    drop(project);
    let second_track = service
        .add_track(
            timeline_id,
            "Video 2".to_string(),
            TimelineTrackKind::Visual,
        )
        .unwrap()
        .0;
    let first = add_transition_pair(&service, first_track, "First").2;
    let second = add_transition_pair(&service, second_track, "Second").2;
    (service, first, second)
}

fn transition_definition_with_public_controls(
    required_input: bool,
) -> (
    ModuleDefinition,
    crate::model::authoring::PublishedMediaInputId,
    PublishedParameterId,
) {
    let (mut definition, _) = ModuleDefinition::new_transition(
        "Controlled Transition",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
        TransitionMediaType::Image,
    )
    .unwrap();
    let media = Node::new_merge("Optional Matte");
    let media_id = crate::model::authoring::PublishedMediaInputId::new();
    definition.interface.media_inputs.push(PublishedMediaInput {
        id: media_id,
        name: "Matte".to_string(),
        data_type: PortDataType::Image,
        target: ModulePortAddress {
            node_id: media.id,
            port: MERGE_IMAGES_PORT.to_string(),
        },
        required: required_input,
        primary: false,
    });
    let value = Node::new_value("Amount", ValueContent::Add);
    let parameter_id = PublishedParameterId::new();
    definition.interface.parameters.push(PublishedParameter {
        id: parameter_id,
        name: "Amount".to_string(),
        data_type: PortDataType::Number,
        default_value: PropertyValue::Number(OrderedFloat(0.0)),
        target: ModulePortAddress {
            node_id: value.id,
            port: NUMERIC_A_INPUT_PORT.to_string(),
        },
    });
    definition
        .graph
        .nodes
        .extend([(media.id, media), (value.id, value)]);
    definition.topology_revision += 1;
    definition.interface_version += 1;
    definition.validate().unwrap();
    (definition, media_id, parameter_id)
}

struct NestedTransitionFixture {
    service: TimelineEditorService,
    transition_id: TransitionId,
    composition_item_id: TimelineItemId,
    source_item_id: TimelineItemId,
    input_id: PublishedMediaInputId,
    parameter_id: PublishedParameterId,
    instance_path: InstancePath,
}

fn nested_transition_fixture() -> NestedTransitionFixture {
    let service = TimelineEditorService::create_default("Nested Transition controls").unwrap();
    let root = service.snapshot().unwrap();
    let root_timeline_id = root.root_timeline_id;
    let root_track_id = root.timelines[&root_timeline_id].track_order[0];
    drop(root);
    let (child_timeline_id, child_track_id, _) = service
        .add_timeline(
            "Nested".to_string(),
            320,
            180,
            RationalRate::new(30, 1).unwrap(),
            seconds(10),
        )
        .unwrap();
    let transition_id = add_transition_pair(&service, child_track_id, "Nested").2;
    let (definition, input_id, parameter_id) = transition_definition_with_public_controls(false);
    let definition_id = definition.id;
    service.add_module_definition(definition).unwrap();
    service
        .assign_transition_module(transition_id, definition_id)
        .unwrap();
    let (source_item_id, _) = service
        .add_item(
            root_track_id,
            "External source".to_string(),
            SourceRef::Solid {
                color: Color::white(),
            },
            TimelineInterval::new(seconds(0), seconds(10)).unwrap(),
            0,
        )
        .unwrap();
    let (composition_item_id, _) = service
        .add_item(
            root_track_id,
            "Nested placement".to_string(),
            SourceRef::Composition(CompositionInstance {
                timeline_id: child_timeline_id,
                duration_policy: DurationPolicy::Fixed,
                parameter_overrides: HashMap::new(),
                transition_module_overrides: Vec::new(),
            }),
            TimelineInterval::new(seconds(0), seconds(10)).unwrap(),
            1,
        )
        .unwrap();
    let instance_path = InstancePath::root(root_timeline_id).nested(composition_item_id);
    service
        .set_transition_module_instance_parameter(
            &instance_path,
            transition_id,
            parameter_id,
            PropertyValue::Number(OrderedFloat(0.75)),
        )
        .unwrap();
    service
        .bind_transition_module_input_at_instance(
            &instance_path,
            transition_id,
            input_id,
            MediaInputBinding::TimelineItemOutput {
                locator: InstanceLocator::Exact(InstancePath::root(root_timeline_id)),
                item_id: source_item_id,
                output: MediaOutputKind::Image,
                stage: ItemOutputStage::PostTransform,
            },
        )
        .unwrap();
    NestedTransitionFixture {
        service,
        transition_id,
        composition_item_id,
        source_item_id,
        input_id,
        parameter_id,
        instance_path,
    }
}

#[test]
fn promote_changes_only_the_processor_and_is_one_undoable_edit() {
    let (service, first, _) = service_with_two_transitions();
    let before = service.snapshot().unwrap();
    let original = before.transitions[&first].clone();
    let original_item_count = before.items.len();

    let (definition_id, instance_id, change) = service
        .promote_transition_to_module(first, "Custom Dissolve")
        .expect("promote Transition");

    assert_eq!(change.revision.get(), service.revision().unwrap().get());
    let changed = service.snapshot().unwrap();
    let transition = &changed.transitions[&first];
    assert_eq!(transition.timeline_id, original.timeline_id);
    assert_eq!(transition.from_item_id, original.from_item_id);
    assert_eq!(transition.to_item_id, original.to_item_id);
    assert_eq!(transition.edit_point, original.edit_point);
    assert_eq!(transition.duration, original.duration);
    assert_eq!(changed.items.len(), original_item_count);
    assert!(
        changed
            .items
            .values()
            .all(|item| !matches!(item.source, SourceRef::Module(_)))
    );
    assert_eq!(
        transition
            .processor
            .module_processor()
            .expect("Module processor")
            .instance_id,
        instance_id
    );
    assert_eq!(
        changed.module_instances[&instance_id].definition_id,
        definition_id
    );
    assert!(matches!(
        changed.module_definitions[&definition_id].host_contract,
        ModuleHostContract::Transition(_)
    ));
    drop(changed);

    service.undo().unwrap().expect("undo promotion");
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}

#[test]
fn protected_transition_boundaries_reject_ordinary_interface_and_node_edits() {
    let (service, first, _) = service_with_two_transitions();
    let (_, instance_id, _) = service
        .promote_transition_to_module(first, "Protected")
        .unwrap();
    let project = service.snapshot().unwrap();
    let definition =
        &project.module_definitions[&project.module_instances[&instance_id].definition_id];
    let contract = definition
        .host_contract
        .transition()
        .expect("Transition contract")
        .clone();
    let from_node = definition
        .interface
        .media_inputs
        .iter()
        .find(|input| input.id == contract.from_input_id)
        .unwrap()
        .target
        .node_id;
    let output_node = definition
        .output(contract.output_id)
        .expect("protected Output")
        .node_id;
    let progress_node = definition
        .interface
        .parameters
        .iter()
        .find(|parameter| parameter.id == contract.progress_parameter_id)
        .unwrap()
        .target
        .node_id;
    let from_target = definition
        .interface
        .media_inputs
        .iter()
        .find(|input| input.id == contract.from_input_id)
        .unwrap()
        .target
        .clone();
    drop(project);
    let before = service.snapshot().unwrap();

    let unpublish = service
        .edit_instance_module_interface(
            instance_id,
            ModuleInterfaceCommand::UnpublishMediaInput {
                input_id: contract.from_input_id,
            },
        )
        .unwrap_err()
        .to_string();
    assert!(unpublish.contains("protected host input"));
    let retarget = service
        .edit_instance_module_interface(
            instance_id,
            ModuleInterfaceCommand::RetargetPrimaryMediaInput {
                input_id: contract.from_input_id,
                target: from_target,
            },
        )
        .unwrap_err()
        .to_string();
    assert!(retarget.contains("protected host input"));
    let progress = service
        .edit_instance_module_interface(
            instance_id,
            ModuleInterfaceCommand::UnpublishParameter {
                parameter_id: contract.progress_parameter_id,
            },
        )
        .unwrap_err()
        .to_string();
    assert!(progress.contains("protected host input"));
    let remove = service
        .remove_instance_module_node(instance_id, from_node)
        .unwrap_err()
        .to_string();
    assert!(remove.contains("protected A/B/Progress/Output boundary"));
    let remove_output = service
        .remove_instance_module_node(instance_id, output_node)
        .unwrap_err()
        .to_string();
    assert!(remove_output.contains("protected A/B/Progress/Output boundary"));
    let edit_progress = service
        .set_instance_module_node_property(
            instance_id,
            progress_node,
            crate::model::project::TRANSITION_PROGRESS_PROPERTY.to_string(),
            Property::constant(PropertyValue::Number(OrderedFloat(0.75))),
        )
        .unwrap_err()
        .to_string();
    assert!(edit_progress.contains("value is supplied by the Timeline"));
    let override_progress = service
        .set_module_parameter(
            instance_id,
            contract.progress_parameter_id,
            PropertyValue::Number(OrderedFloat(0.5)),
        )
        .unwrap_err()
        .to_string();
    assert!(override_progress.contains("host-owned parameter"));
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}

#[test]
fn custom_operation_is_not_silently_replaced_by_builtin_starter_logic() {
    let (service, first, _) = service_with_two_transitions();
    let custom = TransitionProcessor::from_operation(
        OperationRef {
            category: TRANSITION_CATEGORY.to_string(),
            component_id: "custom_wipe".to_string(),
            operation: TRANSITION_APPLY_OPERATION.to_string(),
            version: "7".to_string(),
        },
        TransitionContractSnapshot {
            media_type: TransitionMediaType::Image,
            parameters: vec![ProcessorParameterContract {
                key: "softness".to_string(),
                data_type: PortDataType::Number,
                default_value: PropertyValue::Number(OrderedFloat(0.25)),
            }],
        },
    );
    service
        .assign_transition_operation(first, custom)
        .expect("assign custom operation");
    let before = service.snapshot().unwrap();

    let error = service
        .promote_transition_to_module(first, "Must Stay Custom")
        .unwrap_err()
        .to_string();

    assert!(error.contains("cannot be losslessly promoted"));
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}

#[test]
fn one_reusable_definition_can_back_multiple_transition_instances() {
    let (service, first, second) = service_with_two_transitions();
    let (definition, contract) = ModuleDefinition::new_transition(
        "Shared Dissolve",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
        TransitionMediaType::Image,
    )
    .unwrap();
    let definition_id = definition.id;
    service.add_module_definition(definition).unwrap();

    let first_instance = service
        .assign_transition_module(first, definition_id)
        .unwrap()
        .0;
    let second_instance = service
        .assign_transition_module(second, definition_id)
        .unwrap()
        .0;
    let project = service.snapshot().unwrap();

    assert_ne!(first_instance, second_instance);
    assert_eq!(project.module_definitions.len(), 1);
    assert_eq!(project.module_instances.len(), 2);
    assert_eq!(
        project.module_definitions[&definition_id]
            .host_contract
            .transition(),
        Some(&contract)
    );
    assert_eq!(
        project.module_instances[&first_instance].definition_id,
        definition_id
    );
    assert_eq!(
        project.module_instances[&second_instance].definition_id,
        definition_id
    );
}

#[test]
fn public_id_controls_compile_and_invalidate_only_the_transition_range() {
    let (service, first, _) = service_with_two_transitions();
    let (definition, input_id, parameter_id) = transition_definition_with_public_controls(false);
    let definition_id = definition.id;
    let contract = definition.host_contract.transition().unwrap().clone();
    service.add_module_definition(definition).unwrap();
    service
        .assign_transition_module(first, definition_id)
        .unwrap();
    let project = service.snapshot().unwrap();
    let timeline_id = project.transitions[&first].timeline_id;
    let interval = project.transitions[&first].interval().unwrap();
    drop(project);
    let source_track = service
        .add_track(timeline_id, "Matte".to_string(), TimelineTrackKind::Visual)
        .unwrap()
        .0;
    let (source_id, _) = service
        .add_item(
            source_track,
            "Matte".to_string(),
            SourceRef::Solid {
                color: Color::white(),
            },
            TimelineInterval::new(seconds(0), seconds(10)).unwrap(),
            0,
        )
        .unwrap();
    let binding = MediaInputBinding::TimelineItemOutput {
        locator: InstanceLocator::SameTimeline,
        item_id: source_id,
        output: MediaOutputKind::Image,
        stage: ItemOutputStage::PostEffects,
    };
    let before_protected = service.snapshot().unwrap();
    assert!(
        service
            .bind_transition_module_input(first, contract.from_input_id, binding.clone())
            .unwrap_err()
            .to_string()
            .contains("host-owned")
    );
    assert!(
        service
            .set_transition_module_parameter_automation(
                first,
                contract.progress_parameter_id,
                AutomationTrack::new(AutomationKeyframe::new(
                    seconds(0),
                    PropertyValue::Number(OrderedFloat(0.5)),
                    EasingFunction::Linear,
                ))
                .unwrap(),
            )
            .unwrap_err()
            .to_string()
            .contains("host-owned")
    );
    assert_eq!(
        service.snapshot().unwrap().as_ref(),
        before_protected.as_ref()
    );
    let before_invalid_automation = service.snapshot().unwrap();
    assert!(
        service
            .set_transition_module_parameter_automation(
                first,
                parameter_id,
                AutomationTrack::new(AutomationKeyframe::new(
                    seconds(5),
                    PropertyValue::Number(OrderedFloat(0.5)),
                    EasingFunction::Linear,
                ))
                .unwrap(),
            )
            .unwrap_err()
            .to_string()
            .contains("invalid Keyframes")
    );
    assert_eq!(
        service.snapshot().unwrap().as_ref(),
        before_invalid_automation.as_ref()
    );

    let binding_change = service
        .bind_transition_module_input(first, input_id, binding.clone())
        .unwrap();
    let expected_invalidation = vec![ProjectInvalidation::TimelineRange {
        timeline_id,
        start: interval.start,
        duration: interval.duration,
    }];
    assert_eq!(binding_change.invalidations, expected_invalidation);
    let automation = AutomationTrack::new(AutomationKeyframe::new(
        seconds(0),
        PropertyValue::Number(OrderedFloat(0.75)),
        EasingFunction::Linear,
    ))
    .unwrap();
    let automation_change = service
        .set_transition_module_parameter_automation(first, parameter_id, automation.clone())
        .unwrap();
    assert_eq!(automation_change.invalidations, expected_invalidation);
    let project = service.snapshot().unwrap();
    let module = project.transitions[&first]
        .processor
        .module_processor()
        .unwrap();
    assert_eq!(module.input_bindings.get(&input_id), Some(&binding));
    assert_eq!(
        module.automation_tracks.get(&parameter_id),
        Some(&automation)
    );
    let plan = RenderPlanCompiler::compile(project.as_ref()).unwrap();
    let host = ModuleHost::Transition {
        timeline_id,
        transition_id: first,
    };
    let invocation = plan.invocation(host).unwrap();
    assert_eq!(invocation.input_bindings.get(&input_id), Some(&binding));
    assert_eq!(
        invocation.automation_tracks.get(&parameter_id),
        Some(&automation)
    );
    let source_invalidation = plan.dependencies.affected_by_item(source_id);
    assert!(source_invalidation.invocations.contains(&host));
    assert!(source_invalidation.ranges.contains(
        &crate::core::render_plan::TimelineRangeDependency {
            timeline_id,
            start: interval.start,
            duration: interval.duration,
        }
    ));
    drop(project);
    assert!(
        service
            .delete_item(source_id)
            .unwrap_err()
            .to_string()
            .contains("Transition")
    );

    let clear_change = service
        .clear_transition_module_parameter_automation(first, parameter_id)
        .unwrap();
    assert_eq!(clear_change.invalidations, expected_invalidation);
    let unbind_change = service
        .unbind_transition_module_input(first, input_id)
        .unwrap();
    assert_eq!(unbind_change.invalidations, expected_invalidation);
    let project = service.snapshot().unwrap();
    let module = project.transitions[&first]
        .processor
        .module_processor()
        .unwrap();
    assert!(module.input_bindings.is_empty());
    assert!(module.automation_tracks.is_empty());
}

#[test]
fn configured_assignment_satisfies_required_public_inputs_atomically() {
    let (service, first, second) = service_with_two_transitions();
    let project = service.snapshot().unwrap();
    let timeline_id = project.root_timeline_id;
    drop(project);
    let source_track = service
        .add_track(
            timeline_id,
            "Required Input".to_string(),
            TimelineTrackKind::Visual,
        )
        .unwrap()
        .0;
    let (source_id, _) = service
        .add_item(
            source_track,
            "Source".to_string(),
            SourceRef::Solid {
                color: Color::white(),
            },
            TimelineInterval::new(seconds(0), seconds(10)).unwrap(),
            0,
        )
        .unwrap();
    let binding = MediaInputBinding::TimelineItemOutput {
        locator: InstanceLocator::SameTimeline,
        item_id: source_id,
        output: MediaOutputKind::Image,
        stage: ItemOutputStage::PostEffects,
    };
    let (definition, input_id, _) = transition_definition_with_public_controls(true);
    let definition_id = definition.id;
    service.add_module_definition(definition).unwrap();

    service
        .assign_transition_module_with_controls(
            first,
            definition_id,
            HashMap::from([(input_id, binding.clone())]),
            HashMap::new(),
        )
        .unwrap();
    assert_eq!(
        service.snapshot().unwrap().transitions[&first]
            .processor
            .module_processor()
            .unwrap()
            .input_bindings
            .get(&input_id),
        Some(&binding)
    );
    let before_failed_assignment = service.snapshot().unwrap();
    assert!(
        service
            .assign_transition_module(second, definition_id)
            .unwrap_err()
            .to_string()
            .contains("required media input")
    );
    assert_eq!(
        service.snapshot().unwrap().as_ref(),
        before_failed_assignment.as_ref()
    );
}

#[test]
fn removing_or_restoring_transition_cleans_up_its_private_instance() {
    let (service, first, _) = service_with_two_transitions();
    let (definition_id, instance_id, _) = service
        .promote_transition_to_module(first, "Temporary")
        .unwrap();
    let before_restore = service.snapshot().unwrap();

    service
        .assign_transition_operation(first, TransitionProcessor::cross_dissolve())
        .unwrap();
    let restored = service.snapshot().unwrap();
    assert!(!restored.module_instances.contains_key(&instance_id));
    assert!(!restored.module_definitions.contains_key(&definition_id));
    assert!(
        restored.transitions[&first]
            .processor
            .is_builtin_cross_dissolve()
    );
    drop(restored);
    service.undo().unwrap().expect("undo restore");
    assert_eq!(
        service.snapshot().unwrap().as_ref(),
        before_restore.as_ref()
    );

    service.remove_transition(first).unwrap();
    let removed = service.snapshot().unwrap();
    assert!(!removed.transitions.contains_key(&first));
    assert!(!removed.module_instances.contains_key(&instance_id));
    assert!(!removed.module_definitions.contains_key(&definition_id));
}

#[test]
fn audio_transition_service_rejects_unsupported_nodes_and_additional_media_inputs() {
    let service = TimelineEditorService::create_default("Audio capability").unwrap();
    let (mut definition, _) = ModuleDefinition::new_transition(
        "Reusable Audio Transition",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
        TransitionMediaType::Audio,
    )
    .unwrap();
    let input_target = Node::new_sound_merge("Additional Audio Target");
    let input_target_id = input_target.id;
    definition.graph.nodes.insert(input_target_id, input_target);
    definition.topology_revision += 1;
    let definition_id = definition.id;
    service.add_module_definition(definition).unwrap();
    let before = service.snapshot().unwrap();

    let node_error = service
        .add_shared_module_node(definition_id, Node::new_merge("Unsupported Image Merge"))
        .expect_err("unsupported Node must be rejected before it reaches the graph");
    assert!(
        node_error
            .to_string()
            .contains("has no authoring audio runtime"),
        "{node_error}"
    );
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());

    let input_error = service
        .edit_shared_module_interface(
            definition_id,
            ModuleInterfaceCommand::PublishMediaInput {
                name: "Sidechain".to_string(),
                target: ModulePortAddress {
                    node_id: input_target_id,
                    port: MERGE_SOUNDS_PORT.to_string(),
                },
                required: false,
                primary: false,
            },
        )
        .expect_err("additional Audio input must be rejected by the service");
    assert!(
        input_error
            .to_string()
            .contains("supplies only the host-owned A/B"),
        "{input_error}"
    );
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}

#[test]
fn image_transition_service_rejects_an_additional_audio_input() {
    let service = TimelineEditorService::create_default("Image input capability").unwrap();
    let (definition, contract) = ModuleDefinition::new_transition(
        "Reusable Image Transition",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
        TransitionMediaType::Image,
    )
    .unwrap();
    let output_node_id = definition
        .output(contract.output_id)
        .expect("protected Output")
        .node_id;
    let definition_id = definition.id;
    service.add_module_definition(definition).unwrap();
    let before = service.snapshot().unwrap();

    let error = service
        .edit_shared_module_interface(
            definition_id,
            ModuleInterfaceCommand::PublishMediaInput {
                name: "Audio".to_string(),
                target: ModulePortAddress {
                    node_id: output_node_id,
                    port: crate::model::project::SOUND_INPUT_PORT.to_string(),
                },
                required: false,
                primary: false,
            },
        )
        .expect_err("Image Transition must reject a Published Audio input");
    assert!(
        error
            .to_string()
            .contains("accepts only additional Image inputs"),
        "{error}"
    );
    assert_eq!(service.snapshot().unwrap().as_ref(), before.as_ref());
}

#[test]
fn transition_base_binding_cannot_reenter_its_containing_composition() {
    let fixture = nested_transition_fixture();
    let mut project = fixture.service.snapshot().unwrap().as_ref().clone();
    let transition = project
        .transitions
        .get_mut(&fixture.transition_id)
        .expect("nested Transition");
    transition
        .processor
        .module_processor_mut()
        .expect("Transition Module")
        .input_bindings
        .insert(
            fixture.input_id,
            MediaInputBinding::TimelineItemOutput {
                locator: InstanceLocator::Exact(InstancePath::root(project.root_timeline_id)),
                item_id: fixture.composition_item_id,
                output: MediaOutputKind::Image,
                stage: ItemOutputStage::PostTransform,
            },
        );

    let error = project
        .validate()
        .expect_err("a base binding must not evaluate its containing Composition");
    assert!(
        error.contains("Media evaluation dependency cycle"),
        "{error}"
    );
}

#[test]
fn exact_sparse_transition_binding_cannot_reenter_its_instance_path() {
    let fixture = nested_transition_fixture();
    let before = fixture.service.snapshot().unwrap();

    let error = fixture
        .service
        .bind_transition_module_input_at_instance(
            &fixture.instance_path,
            fixture.transition_id,
            fixture.input_id,
            MediaInputBinding::TimelineItemOutput {
                locator: InstanceLocator::Exact(InstancePath::root(before.root_timeline_id)),
                item_id: fixture.composition_item_id,
                output: MediaOutputKind::Image,
                stage: ItemOutputStage::PostTransform,
            },
        )
        .expect_err("a sparse binding must not evaluate its containing Composition");
    assert!(
        error
            .to_string()
            .contains("Media evaluation dependency cycle"),
        "{error}"
    );
    assert_eq!(
        fixture.service.snapshot().unwrap().as_ref(),
        before.as_ref()
    );
}

#[test]
fn unpublishing_cleans_base_and_sparse_transition_controls_atomically() {
    let fixture = nested_transition_fixture();
    let project = fixture.service.snapshot().unwrap();
    let instance_id = project.transitions[&fixture.transition_id]
        .processor
        .module_processor()
        .expect("Transition Module")
        .instance_id;
    drop(project);

    let (result, _, _) = fixture
        .service
        .edit_instance_module_interface(
            instance_id,
            ModuleInterfaceCommand::UnpublishParameter {
                parameter_id: fixture.parameter_id,
            },
        )
        .unwrap();
    let ModuleInterfaceEditResult::Unpublished(impact) = result else {
        panic!("parameter unpublish must report cleanup impact");
    };
    assert_eq!(impact.removed_parameter_overrides, 1);
    let project = fixture.service.snapshot().unwrap();
    let SourceRef::Composition(instance) = &project.items[&fixture.composition_item_id].source
    else {
        panic!("fixture placement must remain a Composition");
    };
    assert_eq!(instance.transition_module_overrides.len(), 1);
    assert!(
        instance.transition_module_overrides[0]
            .parameter_overrides
            .is_empty()
    );
    drop(project);

    let (result, _, _) = fixture
        .service
        .edit_instance_module_interface(
            instance_id,
            ModuleInterfaceCommand::UnpublishMediaInput {
                input_id: fixture.input_id,
            },
        )
        .unwrap();
    let ModuleInterfaceEditResult::Unpublished(impact) = result else {
        panic!("media-input unpublish must report cleanup impact");
    };
    assert_eq!(impact.removed_media_input_bindings, 1);
    let project = fixture.service.snapshot().unwrap();
    let SourceRef::Composition(instance) = &project.items[&fixture.composition_item_id].source
    else {
        panic!("fixture placement must remain a Composition");
    };
    assert!(instance.transition_module_overrides.is_empty());
    project.validate().unwrap();
}

#[test]
fn sparse_transition_binding_participates_in_delete_and_cascade_dependency_cleanup() {
    let fixture = nested_transition_fixture();
    let dependencies = fixture
        .service
        .item_input_dependencies(fixture.source_item_id)
        .unwrap();
    assert!(dependencies.iter().any(|dependency| {
        *dependency
            == TimelineItemDependency::ModuleInput {
                host: ModuleInputHost::Transition(fixture.transition_id),
                input_id: fixture.input_id,
            }
    }));
    assert!(
        fixture
            .service
            .delete_item(fixture.source_item_id)
            .unwrap_err()
            .to_string()
            .contains("Transition")
    );

    fixture
        .service
        .delete_item_cascade(fixture.source_item_id)
        .unwrap();
    let project = fixture.service.snapshot().unwrap();
    assert!(!project.items.contains_key(&fixture.source_item_id));
    assert!(!project.transitions.contains_key(&fixture.transition_id));
    let SourceRef::Composition(instance) = &project.items[&fixture.composition_item_id].source
    else {
        panic!("fixture placement must remain a Composition");
    };
    assert!(instance.transition_module_overrides.is_empty());
    project.validate().unwrap();
}
