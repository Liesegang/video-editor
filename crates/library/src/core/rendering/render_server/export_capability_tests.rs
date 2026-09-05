use super::*;

#[test]
fn export_gpu_requirement_ignores_particle_bound_to_dead_module_input() {
    let mut project = particle_export_project().as_ref().clone();
    let particle_item_id = project
        .items
        .values()
        .find(|item| matches!(&item.source, SourceRef::Module(_)))
        .expect("Particle Node Clip")
        .id;
    project.items.get_mut(&particle_item_id).unwrap().interval =
        TimelineInterval::new(MediaTime::new(6, 1).unwrap(), MediaTime::new(1, 1).unwrap())
            .unwrap();

    let (mut definition, output_id) =
        ModuleDefinition::new_image("CPU-only output", ModuleDefinitionSharing::Private);
    let dead_effect = PluginManager::default()
        .create_effect_operation_node("blur")
        .expect("Blur operation");
    let dead_effect_id = dead_effect.id;
    definition.graph.nodes.insert(dead_effect_id, dead_effect);
    let input_id = PublishedMediaInputId::new();
    definition.interface.media_inputs.push(PublishedMediaInput {
        id: input_id,
        name: "Unused image".to_string(),
        data_type: PortDataType::Image,
        target: ModulePortAddress {
            node_id: dead_effect_id,
            port: IMAGE_INPUT_PORT.to_string(),
        },
        required: false,
        primary: false,
    });
    definition.topology_revision += 1;
    definition.interface_version += 1;
    let definition_id = definition.id;
    project.module_definitions.insert(definition_id, definition);
    let instance_id = ModuleInstanceId::new();
    project.module_instances.insert(
        instance_id,
        ModuleInstance {
            id: instance_id,
            definition_id,
            parameter_overrides: HashMap::new(),
        },
    );
    let timeline_id = project.root_timeline_id;
    let track_id = project.timelines[&timeline_id].track_order[0];
    let item_id = TimelineItemId::new();
    project.items.insert(
        item_id,
        TimelineItem {
            id: item_id,
            track_id,
            name: "CPU-only Node Clip".to_string(),
            source: SourceRef::Module(ModuleInvocation {
                instance_id,
                output_id,
                input_bindings: HashMap::from([(
                    input_id,
                    MediaInputBinding::TimelineItemOutput {
                        locator: InstanceLocator::SameTimeline,
                        item_id: particle_item_id,
                        output: MediaOutputKind::Image,
                        stage: ItemOutputStage::Content,
                    },
                )]),
                automation_tracks: HashMap::new(),
            }),
            interval: TimelineInterval::new(MediaTime::zero(), MediaTime::new(5, 1).unwrap())
                .unwrap(),
            time_map: TimeMap::default(),
            layer: 0,
            parent: None,
            blend_mode: crate::model::BlendMode::Normal,
            authored_properties: PropertyMap::new(),
        },
    );
    project.validate().unwrap();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let compiled_output = &plan.module_definitions[&definition_id].outputs[&output_id];
    assert!(!compiled_output.requires(RenderCapability::Gpu));
    assert!(compiled_output.reachable_media_inputs.is_empty());

    assert!(
        !plan
            .timeline_may_require_capability(&project, timeline_id, None, RenderCapability::Gpu)
            .unwrap(),
        "an unused published input cannot force an otherwise CPU-only export onto the GPU"
    );
}
