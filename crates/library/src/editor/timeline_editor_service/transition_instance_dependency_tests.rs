use std::collections::HashMap;

use ordered_float::OrderedFloat;

use super::*;
use crate::model::authoring::{
    CompositionInstance, DurationPolicy, InstancePath, ModuleDefinitionSharing, ModulePortAddress,
    ModuleTemplateOrigin, PublishedParameter, SourceRef, TransitionAlignment, TransitionMediaType,
    TransitionProcessor,
};
use crate::model::frame::color::Color;
use crate::model::node::{Node, ValueContent};
use crate::model::project::{NUMERIC_A_INPUT_PORT, PortDataType};

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).expect("whole seconds")
}

fn add_transition_pair(service: &TimelineEditorService, track_id: TimelineTrackId) -> TransitionId {
    let source = |red| SourceRef::Solid {
        color: Color {
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
            source(32),
            TimelineInterval::new(seconds(0), seconds(7)).unwrap(),
            0,
        )
        .unwrap();
    let (to_item_id, _) = service
        .add_item(
            track_id,
            "To".to_string(),
            source(224),
            TimelineInterval::new(seconds(3), seconds(7)).unwrap(),
            1,
        )
        .unwrap();
    service
        .add_transition(TransitionPlacement {
            from_item_id,
            to_item_id,
            edit_point: seconds(5),
            duration: seconds(4),
            alignment: TransitionAlignment::CenteredOnEdit,
            processor: TransitionProcessor::cross_dissolve(),
            parameters: HashMap::new(),
        })
        .unwrap()
        .0
}

struct DeepPathFixture {
    service: TimelineEditorService,
    transition_id: TransitionId,
    owner_item_id: TimelineItemId,
    nested_item_id: TimelineItemId,
}

fn deep_path_fixture() -> DeepPathFixture {
    let service = TimelineEditorService::create_default("Deep Transition path").unwrap();
    let root = service.snapshot().unwrap();
    let root_timeline_id = root.root_timeline_id;
    let root_track_id = root.timelines[&root_timeline_id].track_order[0];
    drop(root);

    let (leaf_timeline_id, leaf_track_id, _) = service
        .add_timeline(
            "Leaf".to_string(),
            320,
            180,
            RationalRate::new(30, 1).unwrap(),
            seconds(10),
        )
        .unwrap();
    let transition_id = add_transition_pair(&service, leaf_track_id);
    let (mut definition, _) = ModuleDefinition::new_transition(
        "Parameterized Transition",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
        TransitionMediaType::Image,
    )
    .unwrap();
    let value_node = Node::new_value("Amount", ValueContent::Add);
    let parameter_id = PublishedParameterId::new();
    definition.interface.parameters.push(PublishedParameter {
        id: parameter_id,
        name: "Amount".to_string(),
        data_type: PortDataType::Number,
        default_value: PropertyValue::Number(OrderedFloat(0.0)),
        target: ModulePortAddress {
            node_id: value_node.id,
            port: NUMERIC_A_INPUT_PORT.to_string(),
        },
    });
    definition.graph.nodes.insert(value_node.id, value_node);
    definition.topology_revision += 1;
    definition.interface_version += 1;
    let definition_id = definition.id;
    service.add_module_definition(definition).unwrap();
    service
        .assign_transition_module(transition_id, definition_id)
        .unwrap();

    let (middle_timeline_id, middle_track_id, _) = service
        .add_timeline(
            "Middle".to_string(),
            320,
            180,
            RationalRate::new(30, 1).unwrap(),
            seconds(10),
        )
        .unwrap();
    let (nested_item_id, _) = service
        .add_item(
            middle_track_id,
            "Leaf placement".to_string(),
            SourceRef::Composition(CompositionInstance {
                timeline_id: leaf_timeline_id,
                duration_policy: DurationPolicy::Fixed,
                parameter_overrides: HashMap::new(),
                transition_module_overrides: Vec::new(),
            }),
            TimelineInterval::new(seconds(0), seconds(10)).unwrap(),
            0,
        )
        .unwrap();
    let (owner_item_id, _) = service
        .add_item(
            root_track_id,
            "Middle placement".to_string(),
            SourceRef::Composition(CompositionInstance {
                timeline_id: middle_timeline_id,
                duration_policy: DurationPolicy::Fixed,
                parameter_overrides: HashMap::new(),
                transition_module_overrides: Vec::new(),
            }),
            TimelineInterval::new(seconds(0), seconds(10)).unwrap(),
            0,
        )
        .unwrap();
    let instance_path = InstancePath::root(root_timeline_id)
        .nested(owner_item_id)
        .nested(nested_item_id);
    service
        .set_transition_module_instance_parameter(
            &instance_path,
            transition_id,
            parameter_id,
            PropertyValue::Number(OrderedFloat(0.5)),
        )
        .unwrap();
    DeepPathFixture {
        service,
        transition_id,
        owner_item_id,
        nested_item_id,
    }
}

#[test]
fn deleting_an_item_in_a_sparse_target_path_requires_explicit_cascade() {
    let fixture = deep_path_fixture();
    let before = fixture.service.snapshot().unwrap();
    let dependencies = fixture
        .service
        .item_input_dependencies(fixture.nested_item_id)
        .unwrap();
    assert!(
        dependencies.contains(&TimelineItemDependency::TransitionInstancePath {
            owner_item_id: fixture.owner_item_id,
            transition_id: fixture.transition_id,
        })
    );

    let error = fixture
        .service
        .delete_item(fixture.nested_item_id)
        .expect_err("ordinary deletion must report the sparse target path");
    assert!(error.to_string().contains("instance path"), "{error}");
    assert_eq!(
        fixture.service.snapshot().unwrap().as_ref(),
        before.as_ref()
    );
}

#[test]
fn cascade_deletion_prunes_the_sparse_target_subtree_atomically() {
    let fixture = deep_path_fixture();
    let before = fixture.service.snapshot().unwrap();

    fixture
        .service
        .delete_item_cascade(fixture.nested_item_id)
        .unwrap();
    let project = fixture.service.snapshot().unwrap();
    assert!(!project.items.contains_key(&fixture.nested_item_id));
    assert!(project.transitions.contains_key(&fixture.transition_id));
    let SourceRef::Composition(instance) = &project.items[&fixture.owner_item_id].source else {
        panic!("root owner must remain a Composition placement");
    };
    assert!(instance.transition_module_overrides.is_empty());
    project.validate().unwrap();
    drop(project);

    fixture.service.undo().unwrap().expect("one cascade undo");
    assert_eq!(
        fixture.service.snapshot().unwrap().as_ref(),
        before.as_ref()
    );
}
