use super::*;
use library::model::authoring::{ModuleTemplateOrigin, RationalRate};

#[test]
fn effect_stack_keeps_every_attachment_in_authored_order() {
    let plugins = PluginManager::default();
    let service = TimelineEditorService::create_default("Effect stack").expect("service");
    let timeline_id = service.snapshot().expect("project").root_timeline_id;
    let owner = AttachmentOwner::Timeline { timeline_id };
    for effect_id in ["blur", "tile"] {
        service
            .add_builtin_effect_by_id(
                &plugins,
                owner.clone(),
                AttachmentStage::TimelinePostComposite,
                effect_id,
            )
            .expect("effect");
    }

    let project = service.snapshot().expect("project");
    let attachments =
        ordered_stage_attachments(&project, &owner, AttachmentStage::TimelinePostComposite);
    assert_eq!(attachments.len(), 2);
    assert!(attachments
        .windows(2)
        .all(|pair| pair[0].order < pair[1].order));
}

#[test]
fn new_node_effect_has_an_implicit_primary_input_and_image_output() {
    let (definition, output_id) = image_effect_module_definition("Effect");

    assert!(matches!(
        definition.sharing,
        ModuleDefinitionSharing::Private
    ));
    assert!(definition.interface.media_inputs.iter().any(|input| {
        input.primary && input.required && input.data_type == PortDataType::Image
    }));
    assert!(definition
        .output(output_id)
        .is_some_and(|output| output.supports(PortDataType::Image)));
    let primary = definition
        .interface
        .media_inputs
        .iter()
        .find(|input| input.primary)
        .expect("primary input");
    let output = definition.output(output_id).expect("Output terminal");
    assert_ne!(primary.target.node_id, output.node_id);
    assert!(definition.graph.connections.iter().any(|connection| {
        connection.from.node_id == primary.target.node_id
            && connection.to == output.target(PortDataType::Image).unwrap()
    }));
    definition.validate().expect("valid effect Module");
}

#[test]
fn node_effect_graph_can_process_the_implicit_host_before_the_output_terminal() {
    use library::core::render_plan::{evaluate_render_plan_frame, RenderPlanCompiler};
    use library::editor::ModuleNodeRequest;
    use library::model::frame::color::Color;
    use library::model::frame::entity::FrameItem;
    use library::model::project::connection::IMAGE_INPUT_PORT;
    use library::model::property::{Property, PropertyValue};
    use library::plugin::{EFFECT_APPLY_OPERATION, EFFECT_CATEGORY};
    use ordered_float::OrderedFloat;

    fn contains_nonzero_blur(items: &[FrameItem]) -> bool {
        items.iter().any(|item| match item {
            FrameItem::Group(group) => {
                group.effects.iter().any(|effect| {
                    effect.effect_type == "blur"
                        && matches!(
                            effect.properties.get("sigma_x"),
                            Some(PropertyValue::Number(value)) if value.into_inner() > 0.0
                        )
                }) || contains_nonzero_blur(&group.items)
            }
            FrameItem::Object(_) => false,
        })
    }

    let plugins = PluginManager::default();
    let service = TimelineEditorService::create_default("Effect runtime").expect("service");
    let initial = service.snapshot().expect("project");
    let timeline_id = initial.root_timeline_id;
    let track_id = *initial.tracks.keys().next().expect("default track");
    drop(initial);
    service
        .add_item(
            track_id,
            "Host".to_string(),
            SourceRef::Solid {
                color: Color::white(),
            },
            library::model::authoring::TimelineInterval::new(
                library::model::authoring::MediaTime::zero(),
                library::model::authoring::MediaTime::new(5, 1).expect("duration"),
            )
            .expect("interval"),
            0,
        )
        .expect("host item");

    let (mut definition, output_id) = image_effect_module_definition("Blur graph");
    let definition_id = definition.id;
    let input_node_id = definition.interface.media_inputs[0].target.node_id;
    let output = definition.output(output_id).expect("Output terminal");
    let mut blur = service
        .create_module_node(
            &plugins,
            ModuleNodeRequest::PluginOperation {
                category: EFFECT_CATEGORY.to_string(),
                component_id: "blur".to_string(),
                operation: EFFECT_APPLY_OPERATION.to_string(),
            },
            1920,
            1080,
        )
        .expect("Blur Node");
    blur.set_property(
        "sigma_x".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(4.0))),
    )
    .expect("non-zero Blur radius");
    blur.ui_position = [300.0, 120.0];
    let blur_node_id = blur.id;
    definition.graph.nodes.insert(blur_node_id, blur);
    definition.graph.connections = vec![
        ModuleConnection {
            id: ModuleConnectionId::new(),
            from: ModulePortAddress {
                node_id: input_node_id,
                port: IMAGE_OUTPUT_PORT.to_string(),
            },
            to: ModulePortAddress {
                node_id: blur_node_id,
                port: IMAGE_INPUT_PORT.to_string(),
            },
            order: 0,
            blend_mode: library::model::BlendMode::Normal,
        },
        ModuleConnection {
            id: ModuleConnectionId::new(),
            from: ModulePortAddress {
                node_id: blur_node_id,
                port: IMAGE_OUTPUT_PORT.to_string(),
            },
            to: output.target(PortDataType::Image).unwrap(),
            order: 0,
            blend_mode: library::model::BlendMode::Normal,
        },
    ];
    definition.topology_revision += 1;
    definition.validate().expect("processed graph");
    service
        .create_private_module_attachment(
            definition,
            ModuleAttachmentPlacement {
                owner: AttachmentOwner::Timeline { timeline_id },
                stage: AttachmentStage::TimelinePostComposite,
                definition_id,
                output_id,
                parameter_overrides: HashMap::new(),
                input_bindings: HashMap::new(),
            },
        )
        .expect("Node Effect");

    let project = service.snapshot().expect("project");
    let plan = RenderPlanCompiler::compile(&project).expect("RenderPlan");
    let frame = evaluate_render_plan_frame(&project, &plan, &plugins, 0, 1.0, None)
        .expect("evaluated frame");
    assert!(contains_nonzero_blur(&frame.items));
}

#[test]
fn template_choices_require_a_compatible_primary_input() {
    let mut project = AuthoringProject::new(
        "Project",
        1920,
        1080,
        RationalRate::new(30, 1).expect("rate"),
        library::model::authoring::MediaTime::new(10, 1).expect("duration"),
    )
    .expect("project");
    let (mut compatible, output_id) = image_effect_module_definition("Compatible");
    compatible.sharing = ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project);
    let compatible_id = compatible.id;
    project.module_definitions.insert(compatible.id, compatible);
    let (mut generator, _) = image_effect_module_definition("Generator only");
    generator.sharing = ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project);
    generator.interface.media_inputs.clear();
    project.module_definitions.insert(generator.id, generator);

    assert_eq!(
        compatible_module_outputs(&project, PortDataType::Image),
        vec![(compatible_id, output_id, "Compatible / Output".to_string())]
    );
}

#[test]
fn node_effect_attachment_and_state_changes_are_atomic_and_undoable() {
    let service = TimelineEditorService::create_default("Project").expect("service");
    let timeline_id = service.snapshot().expect("project").root_timeline_id;
    let (definition, output_id) = image_effect_module_definition("Effect");
    let definition_id = definition.id;
    let (attachment_id, instance_id, _) = service
        .create_private_module_attachment(
            definition,
            ModuleAttachmentPlacement {
                owner: AttachmentOwner::Timeline { timeline_id },
                stage: AttachmentStage::TimelinePostComposite,
                definition_id,
                output_id,
                parameter_overrides: HashMap::new(),
                input_bindings: HashMap::new(),
            },
        )
        .expect("Node Effect");
    let project = service.snapshot().expect("project");
    assert!(project.attachments.contains_key(&attachment_id));
    assert_eq!(
        project.module_instances[&instance_id].definition_id,
        definition_id
    );
    project.validate().expect("valid project");
    drop(project);

    service
        .set_attachment_state(attachment_id, false, true)
        .expect("state");
    let disabled = service.snapshot().expect("project");
    assert!(!disabled.attachments[&attachment_id].enabled);
    assert!(disabled.attachments[&attachment_id].bypassed);
    drop(disabled);
    service.undo().expect("undo state").expect("state change");
    let restored = service.snapshot().expect("project");
    assert!(restored.attachments[&attachment_id].enabled);
    assert!(!restored.attachments[&attachment_id].bypassed);
    drop(restored);

    service.undo().expect("undo create").expect("creation");
    let empty = service.snapshot().expect("project");
    assert!(!empty.attachments.contains_key(&attachment_id));
    assert!(!empty.module_instances.contains_key(&instance_id));
    assert!(!empty.module_definitions.contains_key(&definition_id));
}
