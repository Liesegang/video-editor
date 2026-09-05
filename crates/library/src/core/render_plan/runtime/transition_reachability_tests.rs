use std::collections::HashMap;

use super::transition_tests::{
    adjacent_solid_project, promote_cross_dissolve_to_module, render_instance_pixel,
    wrap_root_with_two_instances,
};
use crate::editor::TimelineEditorService;
use crate::model::BlendMode;
use crate::model::authoring::{
    InstanceLocator, InstancePath, ItemOutputStage, MediaInputBinding, MediaOutputKind,
    ModuleDefinition, ModuleDefinitionSharing, ModuleInstance, ModuleInstanceId, ModuleInvocation,
    ModulePortAddress, PublishedMediaInput, PublishedMediaInputId, SourceRef, TimeMap,
    TimelineInterval, TimelineItem, TimelineItemId,
};
use crate::model::node::Node;
use crate::model::project::property::PropertyMap;
use crate::model::project::{MERGE_IMAGES_PORT, PortDataType};

#[test]
fn dead_required_transition_input_is_not_evaluated_for_an_instance_override() {
    let (mut project, transition_id, _, _) = adjacent_solid_project();
    let nested_timeline_id = project.root_timeline_id;
    let definition_id = promote_cross_dissolve_to_module(&mut project, transition_id);
    let (empty_definition, empty_output_id) =
        ModuleDefinition::new_image("Empty media source", ModuleDefinitionSharing::Private);
    let empty_definition_id = empty_definition.id;
    let empty_instance_id = ModuleInstanceId::new();
    let empty_item_id = TimelineItemId::new();
    let track_id = project.timelines[&nested_timeline_id].track_order[0];
    project
        .module_definitions
        .insert(empty_definition_id, empty_definition);
    project.module_instances.insert(
        empty_instance_id,
        ModuleInstance {
            id: empty_instance_id,
            definition_id: empty_definition_id,
            parameter_overrides: HashMap::new(),
        },
    );
    project.items.insert(
        empty_item_id,
        TimelineItem {
            id: empty_item_id,
            track_id,
            name: "Empty image Node Clip".to_string(),
            source: SourceRef::Module(ModuleInvocation {
                instance_id: empty_instance_id,
                output_id: empty_output_id,
                input_bindings: HashMap::new(),
                automation_tracks: HashMap::new(),
            }),
            interval: TimelineInterval::new(
                crate::model::authoring::MediaTime::zero(),
                crate::model::authoring::MediaTime::new(10, 1).expect("duration"),
            )
            .expect("interval"),
            time_map: TimeMap::default(),
            layer: 2,
            parent: None,
            blend_mode: BlendMode::Normal,
            authored_properties: PropertyMap::new(),
        },
    );
    let dead_input_id = PublishedMediaInputId::new();
    {
        let definition = project
            .module_definitions
            .get_mut(&definition_id)
            .expect("Transition definition");
        let dead_merge = Node::new_merge("Disconnected input");
        let dead_merge_id = dead_merge.id;
        definition.graph.nodes.insert(dead_merge_id, dead_merge);
        definition.interface.media_inputs.push(PublishedMediaInput {
            id: dead_input_id,
            name: "Disconnected required input".to_string(),
            data_type: PortDataType::Image,
            target: ModulePortAddress {
                node_id: dead_merge_id,
                port: MERGE_IMAGES_PORT.to_string(),
            },
            required: true,
            primary: false,
        });
        definition.topology_revision += 1;
        definition.interface_version += 1;
        definition.validate().expect("valid disconnected interface");
    }
    let binding = |item_id| MediaInputBinding::TimelineItemOutput {
        locator: InstanceLocator::SameTimeline,
        item_id,
        output: MediaOutputKind::Image,
        stage: ItemOutputStage::PostTransform,
    };
    project
        .transitions
        .get_mut(&transition_id)
        .expect("Transition")
        .processor
        .module_processor_mut()
        .expect("Module processor")
        .input_bindings
        .insert(dead_input_id, binding(empty_item_id));
    let (root_timeline_id, first_item_id, _) =
        wrap_root_with_two_instances(&mut project, nested_timeline_id);
    project.validate().expect("valid nested Project");
    let service = TimelineEditorService::new(project).expect("editor service");
    let first_path = InstancePath::root(root_timeline_id).nested(first_item_id);

    // The concrete override points at a valid, full-interval Image Node Clip
    // whose terminal intentionally produces no frame. Evaluating it would
    // fail the old required-input check even though it cannot reach Output.
    service
        .bind_transition_module_input_at_instance(
            &first_path,
            transition_id,
            dead_input_id,
            binding(empty_item_id),
        )
        .expect("instance override");
    let project = service.snapshot().expect("snapshot");
    let midpoint = render_instance_pixel(&project, nested_timeline_id, &first_path, 150);
    assert!((187..=189).contains(&midpoint[0]));
    assert!(midpoint[1] <= 1);
    assert!((187..=189).contains(&midpoint[2]));
    assert_eq!(midpoint[3], 255);
}
