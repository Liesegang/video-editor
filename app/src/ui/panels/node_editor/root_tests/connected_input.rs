use super::*;
use library::model::project::{
    FMOD_DIVISOR_INPUT_PORT, FMOD_X_INPUT_PORT, NUMBER_RESULT_OUTPUT_PORT, TIME_PORT,
};

fn connected_value_fixture() -> (Project, Uuid, Uuid, PortAddress, PortAddress) {
    let mut project = Project::new("connected input value UI");
    let (composition, track) = Composition::new("Main", 640, 360, 30.0, 5.0);
    let composition_id = composition.id;
    project.add_track(track).unwrap();
    project.add_composition(composition).unwrap();
    let container = NodeContainer::Composition(composition_id);

    let mut source = Node::new_fmod("Source");
    source.ui_position = [220.0, 200.0];
    let source_id = source.id;
    project.add_node(source);
    project
        .attach_node_to_container(container, source_id)
        .unwrap();
    project
        .connect_ports(
            PortAddress::new(PortOwner::Composition(composition_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(source_id), FMOD_X_INPUT_PORT),
        )
        .unwrap();

    let mut target = Node::new_fmod("Target");
    target.ui_position = [580.0, 240.0];
    let target_id = target.id;
    project.add_node(target);
    project
        .attach_node_to_container(container, target_id)
        .unwrap();
    let from = PortAddress::new(PortOwner::Node(source_id), NUMBER_RESULT_OUTPUT_PORT);
    let to = PortAddress::new(PortOwner::Node(target_id), FMOD_DIVISOR_INPUT_PORT);
    project.connect_ports(from.clone(), to.clone()).unwrap();
    assert!(project.validate_connections().is_empty());
    (project, composition_id, target_id, from, to)
}

#[test]
fn connected_property_qa_reports_resolved_value_and_disconnect_restores_authored_editor() {
    let (mut project, composition_id, target_id, from, to) = connected_value_fixture();
    let plugins = PluginManager::default();
    let component_id = format!("node_editor.property.node:{target_id}:{FMOD_DIVISOR_INPUT_PORT}");

    render_test_graph_at_time_with_plugins(&project, composition_id, 2.25, Some(&plugins));
    let connected = test_metadata(&component_id).expect("connected property QA metadata");
    assert_eq!(connected["connected"], true);
    assert_eq!(connected["control_kind"], "connected_value");
    assert_eq!(connected["input_status"], "value");
    assert_eq!(connected["evaluation"], "resolved");
    assert_eq!(connected["timeline_time"], 2.25);
    assert_eq!(connected["resolved_value"], 0.25);
    assert_eq!(connected["value"], 0.25);
    assert_eq!(connected["read_only"], true);
    assert_eq!(connected["sources"][0]["port"], NUMBER_RESULT_OUTPUT_PORT);

    assert!(project.disconnect_ports(&from, &to));
    render_test_graph_at_time_with_plugins(&project, composition_id, 2.25, Some(&plugins));
    let disconnected = test_metadata(&component_id).expect("disconnected property QA metadata");
    assert_eq!(disconnected["connected"], false);
    assert_eq!(disconnected["control_kind"], "float");
    assert_eq!(disconnected["value"], 1.0);
    assert!(disconnected.get("input_status").is_none());
    assert!(test_rect(&component_id).is_some_and(|rect| rect.is_positive()));
}
