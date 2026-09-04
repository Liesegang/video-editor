use std::collections::HashSet;
use std::sync::Arc;

use ordered_float::OrderedFloat;
use uuid::Uuid;

use super::FrameEvaluator;
use crate::model::project::connection::{
    LIST_INDEX_INPUT_PORT, LIST_INPUT_PORT, LIST_ITEM_OUTPUT_PORT, LIST_ITEMS_INPUT_PORT,
    LIST_LENGTH_OUTPUT_PORT, LIST_OUTPUT_PORT,
};
use crate::model::project::{
    DURATION_PORT, EvalOutput, FRAME_PORT, NUMBER_RESULT_OUTPUT_PORT, NUMERIC_A_INPUT_PORT,
    NUMERIC_B_INPUT_PORT, NodeContainer, PortAddress, PortDataType, PortOwner, Project, TIME_PORT,
};
use crate::model::property::{Property, PropertyValue};
use crate::model::{Clip, Composition, ListContent, Node};
use crate::plugin::PluginManager;

struct ListFixture {
    project: Project,
    clip_id: Uuid,
}

impl ListFixture {
    fn new() -> Self {
        let mut project = Project::new("List graph");
        let (composition, track) = Composition::new("Main", 64, 64, 30.0, 4.0);
        let track_id = track.id;
        project.add_track(track).unwrap();
        project.add_composition(composition).unwrap();
        let clip = Clip::new("List scope", 0.0, 2.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();
        Self { project, clip_id }
    }

    fn add(&mut self, node: Node) -> Uuid {
        let id = node.id;
        self.project.add_node(node);
        self.project
            .attach_node_to_container(NodeContainer::Clip(self.clip_id), id)
            .unwrap();
        id
    }

    fn clip_output(&self, port: &str) -> PortAddress {
        PortAddress::new(PortOwner::Clip(self.clip_id), port)
    }

    fn node_input(node_id: Uuid, port: &str) -> PortAddress {
        PortAddress::new(PortOwner::Node(node_id), port)
    }

    fn evaluate(&self, node_id: Uuid, port: &str, time: f64) -> EvalOutput<PropertyValue> {
        self.evaluate_port(PortAddress::new(PortOwner::Node(node_id), port), time)
    }

    fn evaluate_port(&self, source: PortAddress, time: f64) -> EvalOutput<PropertyValue> {
        let plugins = Arc::new(PluginManager::default());
        FrameEvaluator::new(
            &self.project,
            &self.project.compositions[0],
            plugins.get_property_evaluators(),
            plugins.as_ref(),
        )
        .resolve_metadata_value(&source, time, &mut HashSet::new())
        .unwrap()
    }
}

#[test]
fn make_list_preserves_duplicate_sources_order_and_project_roundtrip() {
    let mut fixture = ListFixture::new();
    let make_id = fixture.add(Node::new_list("Make List", ListContent::Make));
    let target = ListFixture::node_input(make_id, LIST_ITEMS_INPUT_PORT);
    let source = fixture.clip_output(TIME_PORT);
    let first = fixture
        .project
        .connect_ports(source.clone(), target.clone())
        .unwrap();
    let second = fixture
        .project
        .connect_ports(source, target.clone())
        .unwrap();
    assert_ne!(first, second, "[x, x] requires two connection identities");
    assert!(fixture.project.validate_connections().is_empty());
    assert_eq!(
        fixture.evaluate(make_id, LIST_OUTPUT_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::Array(vec![
            PropertyValue::Number(OrderedFloat(0.5)),
            PropertyValue::Number(OrderedFloat(0.5)),
        ]))
    );

    let encoded = serde_json::to_string(&fixture.project).unwrap();
    let restored: Project = serde_json::from_str(&encoded).unwrap();
    let mut restored_inputs = restored
        .connections
        .iter()
        .filter(|connection| connection.to == target)
        .map(|connection| (connection.order, connection.id, connection.from.clone()))
        .collect::<Vec<_>>();
    restored_inputs.sort_by_key(|(order, id, _)| (*order, *id));
    assert_eq!(restored_inputs.len(), 2);
    assert_eq!(restored_inputs[0].0, 0);
    assert_eq!(restored_inputs[1].0, 1);
    assert_eq!(restored_inputs[0].2, restored_inputs[1].2);
    assert_eq!(
        [restored_inputs[0].1, restored_inputs[1].1],
        [first, second]
    );
    assert!(restored.validate_connections().is_empty());
}

#[test]
fn list_order_reorder_length_get_and_dynamic_any_feed_one_value_graph() {
    let mut fixture = ListFixture::new();
    let make_id = fixture.add(Node::new_list("Make List", ListContent::Make));
    let length_id = fixture.add(Node::new_list("List Length", ListContent::Length));
    let mut get = Node::new_list("Get List Item", ListContent::GetItem);
    get.set_property(
        LIST_INDEX_INPUT_PORT.to_string(),
        Property::constant(PropertyValue::Integer(0)),
    )
    .unwrap();
    let get_id = fixture.add(get);
    let add_id = fixture.add(Node::new_add("Add"));
    let make_target = ListFixture::node_input(make_id, LIST_ITEMS_INPUT_PORT);
    let time_connection = fixture
        .project
        .connect_ports(fixture.clip_output(TIME_PORT), make_target.clone())
        .unwrap();
    let duration_connection = fixture
        .project
        .connect_ports(fixture.clip_output(DURATION_PORT), make_target)
        .unwrap();
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Node(make_id), LIST_OUTPUT_PORT),
            ListFixture::node_input(length_id, LIST_INPUT_PORT),
        )
        .unwrap();
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Node(make_id), LIST_OUTPUT_PORT),
            ListFixture::node_input(get_id, LIST_INPUT_PORT),
        )
        .unwrap();
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Node(get_id), LIST_ITEM_OUTPUT_PORT),
            ListFixture::node_input(add_id, crate::model::project::NUMERIC_A_INPUT_PORT),
        )
        .unwrap();
    assert!(fixture.project.validate_connections().is_empty());
    assert_eq!(
        fixture.evaluate(make_id, LIST_OUTPUT_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::Array(vec![
            PropertyValue::Number(OrderedFloat(0.5)),
            PropertyValue::Number(OrderedFloat(2.0)),
        ]))
    );
    assert_eq!(
        fixture.evaluate(length_id, LIST_LENGTH_OUTPUT_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::Integer(2))
    );
    assert_eq!(
        fixture.evaluate(add_id, NUMBER_RESULT_OUTPUT_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::Number(OrderedFloat(0.5)))
    );
    fixture
        .project
        .get_node_mut(get_id)
        .unwrap()
        .set_property(
            LIST_INDEX_INPUT_PORT.to_string(),
            Property::constant(PropertyValue::Integer(9)),
        )
        .unwrap();
    assert_eq!(
        fixture.evaluate(get_id, LIST_ITEM_OUTPUT_PORT, 0.5),
        EvalOutput::NoOutput,
        "an explicit positive out-of-range index must be NoOutput"
    );
    fixture
        .project
        .get_node_mut(get_id)
        .unwrap()
        .set_property(
            LIST_INDEX_INPUT_PORT.to_string(),
            Property::constant(PropertyValue::Integer(0)),
        )
        .unwrap();

    fixture
        .project
        .reorder_connection(duration_connection, 0)
        .unwrap();
    assert_eq!(
        fixture.evaluate(make_id, LIST_OUTPUT_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::Array(vec![
            PropertyValue::Number(OrderedFloat(2.0)),
            PropertyValue::Number(OrderedFloat(0.5)),
        ]))
    );
    assert_eq!(
        fixture.evaluate(get_id, LIST_ITEM_OUTPUT_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::Number(OrderedFloat(2.0)))
    );
    let ordered = fixture
        .project
        .connections
        .iter()
        .find(|connection| connection.id == time_connection)
        .unwrap();
    assert_eq!(ordered.order, 1);
}

#[test]
fn empty_list_is_a_value_but_invalid_index_disabled_and_bypass_are_no_output() {
    let mut fixture = ListFixture::new();
    let make_id = fixture.add(Node::new_list("Empty", ListContent::Make));
    let length_id = fixture.add(Node::new_list("Length", ListContent::Length));
    let mut get = Node::new_list("Get", ListContent::GetItem);
    get.set_property(
        LIST_INDEX_INPUT_PORT.to_string(),
        Property::constant(PropertyValue::Integer(0)),
    )
    .unwrap();
    let get_id = fixture.add(get);
    for target in [length_id, get_id] {
        fixture
            .project
            .connect_ports(
                PortAddress::new(PortOwner::Node(make_id), LIST_OUTPUT_PORT),
                ListFixture::node_input(target, LIST_INPUT_PORT),
            )
            .unwrap();
    }
    assert_eq!(
        fixture.evaluate(make_id, LIST_OUTPUT_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::Array(Vec::new()))
    );
    assert_eq!(
        fixture.evaluate(length_id, LIST_LENGTH_OUTPUT_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::Integer(0))
    );
    assert_eq!(
        fixture.evaluate(get_id, LIST_ITEM_OUTPUT_PORT, 0.5),
        EvalOutput::NoOutput
    );

    fixture.project.get_node_mut(make_id).unwrap().enabled = false;
    assert_eq!(
        fixture.evaluate(length_id, LIST_LENGTH_OUTPUT_PORT, 0.5),
        EvalOutput::NoOutput
    );
    fixture.project.get_node_mut(make_id).unwrap().enabled = true;
    fixture.project.get_node_mut(make_id).unwrap().bypassed = true;
    assert!(!fixture.project.get_node(make_id).unwrap().supports_bypass());
    assert_eq!(
        fixture.evaluate(make_id, LIST_OUTPUT_PORT, 0.5),
        EvalOutput::NoOutput
    );
}

#[test]
fn negative_authored_and_connected_integer_indices_are_no_output() {
    let mut fixture = ListFixture::new();
    let composition_id = fixture.project.compositions[0].id;
    let track_id = fixture.project.compositions[0].track_ids[0];

    let make = Node::new_list("One item", ListContent::Make);
    let make_id = make.id;
    fixture.project.add_node(make);
    fixture
        .project
        .attach_node_to_container(NodeContainer::Track(track_id), make_id)
        .unwrap();
    let mut get = Node::new_list("Get", ListContent::GetItem);
    get.set_property(
        LIST_INDEX_INPUT_PORT.to_string(),
        Property::constant(PropertyValue::Integer(-1)),
    )
    .unwrap();
    let get_id = get.id;
    fixture.project.add_node(get);
    fixture
        .project
        .attach_node_to_container(NodeContainer::Track(track_id), get_id)
        .unwrap();
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Track(track_id), TIME_PORT),
            ListFixture::node_input(make_id, LIST_ITEMS_INPUT_PORT),
        )
        .unwrap();
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Node(make_id), LIST_OUTPUT_PORT),
            ListFixture::node_input(get_id, LIST_INPUT_PORT),
        )
        .unwrap();
    assert_eq!(
        fixture.evaluate(get_id, LIST_ITEM_OUTPUT_PORT, 0.5),
        EvalOutput::NoOutput,
        "usize::try_from must reject an authored negative index"
    );

    // Remap the Track's Time from 0.5 to -3.5. Track scope has no authored
    // activity range, so its derived Frame output is therefore a
    // genuinely connected negative Integer rather than a floating-point
    // approximation or an authored fallback.
    let subtract = Node::new_subtract("Negative track time");
    let subtract_id = subtract.id;
    fixture.project.add_node(subtract);
    fixture
        .project
        .attach_node_to_container(NodeContainer::Composition(composition_id), subtract_id)
        .unwrap();
    for (source_port, target_port) in [
        (TIME_PORT, NUMERIC_A_INPUT_PORT),
        (DURATION_PORT, NUMERIC_B_INPUT_PORT),
    ] {
        fixture
            .project
            .connect_ports(
                PortAddress::new(PortOwner::Composition(composition_id), source_port),
                ListFixture::node_input(subtract_id, target_port),
            )
            .unwrap();
    }
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Node(subtract_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(PortOwner::Track(track_id), TIME_PORT),
        )
        .unwrap();
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Track(track_id), FRAME_PORT),
            ListFixture::node_input(get_id, LIST_INDEX_INPUT_PORT),
        )
        .unwrap();
    assert!(fixture.project.validate_connections().is_empty());
    assert_eq!(
        fixture.evaluate_port(
            PortAddress::new(PortOwner::Track(track_id), FRAME_PORT),
            0.5,
        ),
        EvalOutput::Produced(PropertyValue::Integer(-105))
    );
    assert_eq!(
        fixture.evaluate(get_id, LIST_ITEM_OUTPUT_PORT, 0.5),
        EvalOutput::NoOutput,
        "usize::try_from must reject a connected negative Integer"
    );
}

#[test]
fn any_accepts_only_property_value_backed_types_and_list_is_distinct_from_variadic() {
    for accepted in [
        PortDataType::Any,
        PortDataType::List,
        PortDataType::Numeric,
        PortDataType::Number,
        PortDataType::Integer,
        PortDataType::Boolean,
        PortDataType::String,
        PortDataType::Color,
        PortDataType::Path,
        PortDataType::Vec2,
        PortDataType::Vec3,
        PortDataType::Vec4,
    ] {
        assert!(PortDataType::Any.accepts(accepted), "{accepted:?}");
    }
    for rejected in [
        PortDataType::Image,
        PortDataType::Shape,
        PortDataType::Audio,
        PortDataType::Spectrum,
        PortDataType::Object3D,
    ] {
        assert!(!PortDataType::Any.accepts(rejected), "{rejected:?}");
    }
    assert!(!PortDataType::List.accepts(PortDataType::Number));
    assert!(PortDataType::List.accepts(PortDataType::List));
}

#[test]
fn dynamically_typed_get_item_connection_fails_safely_when_runtime_value_mismatches() {
    let mut fixture = ListFixture::new();
    let inner_id = fixture.add(Node::new_list("Inner", ListContent::Make));
    let outer_id = fixture.add(Node::new_list("Outer", ListContent::Make));
    let get_id = fixture.add(Node::new_list("Get nested", ListContent::GetItem));
    let add_id = fixture.add(Node::new_add("Needs numeric"));
    fixture
        .project
        .connect_ports(
            fixture.clip_output(TIME_PORT),
            ListFixture::node_input(inner_id, LIST_ITEMS_INPUT_PORT),
        )
        .unwrap();
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Node(inner_id), LIST_OUTPUT_PORT),
            ListFixture::node_input(outer_id, LIST_ITEMS_INPUT_PORT),
        )
        .unwrap();
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Node(outer_id), LIST_OUTPUT_PORT),
            ListFixture::node_input(get_id, LIST_INPUT_PORT),
        )
        .unwrap();
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Node(get_id), LIST_ITEM_OUTPUT_PORT),
            ListFixture::node_input(add_id, crate::model::project::NUMERIC_A_INPUT_PORT),
        )
        .unwrap();
    assert!(fixture.project.validate_connections().is_empty());
    assert_eq!(
        fixture.evaluate(get_id, LIST_ITEM_OUTPUT_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::Array(vec![PropertyValue::Number(
            OrderedFloat(0.5)
        )]))
    );
    assert_eq!(
        fixture.evaluate(add_id, NUMBER_RESULT_OUTPUT_PORT, 0.5),
        EvalOutput::NoOutput,
        "Any permits a static connection but the concrete consumer must reject a runtime type mismatch"
    );
}

#[test]
fn dynamically_typed_get_item_can_feed_a_list_consumer_when_the_item_is_a_list() {
    let mut fixture = ListFixture::new();
    let inner_id = fixture.add(Node::new_list("Inner", ListContent::Make));
    let outer_id = fixture.add(Node::new_list("Outer", ListContent::Make));
    let get_id = fixture.add(Node::new_list("Get nested", ListContent::GetItem));
    let length_id = fixture.add(Node::new_list("Nested length", ListContent::Length));

    fixture
        .project
        .connect_ports(
            fixture.clip_output(TIME_PORT),
            ListFixture::node_input(inner_id, LIST_ITEMS_INPUT_PORT),
        )
        .unwrap();
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Node(inner_id), LIST_OUTPUT_PORT),
            ListFixture::node_input(outer_id, LIST_ITEMS_INPUT_PORT),
        )
        .unwrap();
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Node(outer_id), LIST_OUTPUT_PORT),
            ListFixture::node_input(get_id, LIST_INPUT_PORT),
        )
        .unwrap();
    fixture
        .project
        .connect_ports(
            PortAddress::new(PortOwner::Node(get_id), LIST_ITEM_OUTPUT_PORT),
            ListFixture::node_input(length_id, LIST_INPUT_PORT),
        )
        .unwrap();

    assert!(fixture.project.validate_connections().is_empty());
    assert_eq!(
        fixture.evaluate(length_id, LIST_LENGTH_OUTPUT_PORT, 0.5),
        EvalOutput::Produced(PropertyValue::Integer(1)),
        "Any-to-List is a checked dynamic cast: an actual nested list must remain consumable"
    );
}
