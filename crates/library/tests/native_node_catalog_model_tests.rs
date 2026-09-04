#![allow(
    clippy::panic,
    reason = "integration-test fixture helpers report the exact failed native catalog contract"
)]

use library::model::project::{NodeContainer, PortAddress, PortOwner};
use library::model::{
    Composition, NativeNodeFactory, NativeNodeRuntimeStatus, Node, NodeContent, Project,
    native_node_catalog, native_node_descriptor, native_node_descriptor_for_node,
};
use uuid::Uuid;

fn detached(catalog_id: &str) -> Node {
    native_node_descriptor(catalog_id)
        .unwrap_or_else(|| panic!("missing test catalog descriptor {catalog_id}"))
        .create_detached_node()
        .unwrap_or_else(|error| panic!("cannot create {catalog_id}: {error}"))
}

fn add(project: &mut Project, composition_id: Uuid, catalog_id: &str) -> Uuid {
    let node = detached(catalog_id);
    let id = node.id;
    project.add_node(node);
    project
        .attach_node_to_container(NodeContainer::Composition(composition_id), id)
        .unwrap_or_else(|error| panic!("cannot attach {catalog_id}: {error}"));
    id
}

fn connect(project: &mut Project, from: Uuid, output: &str, to: Uuid, input: &str) {
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(from), output),
            PortAddress::new(PortOwner::Node(to), input),
        )
        .unwrap_or_else(|error| panic!("cannot connect {from}.{output} -> {to}.{input}: {error}"));
}

#[test]
fn detached_catalog_factories_round_trip_to_their_stable_descriptor() {
    for descriptor in native_node_catalog() {
        if matches!(descriptor.factory(), NativeNodeFactory::Generator(_)) {
            assert!(descriptor.create_detached_node().is_err());
            continue;
        }
        let node = Node::new_catalog_node(descriptor.catalog_id())
            .unwrap_or_else(|error| panic!("{}: {error}", descriptor.catalog_id()));
        assert_eq!(node.name, descriptor.label());
        assert_eq!(
            native_node_descriptor_for_node(&node).map(|found| found.catalog_id()),
            Some(descriptor.catalog_id())
        );
        let encoded = serde_json::to_string(&node).expect("native Node serializes");
        let decoded: Node = serde_json::from_str(&encoded).expect("native Node deserializes");
        assert_eq!(decoded, node);
        assert_eq!(
            native_node_descriptor_for_node(&decoded).map(|found| found.catalog_id()),
            Some(descriptor.catalog_id())
        );
        if descriptor.runtime_status() == NativeNodeRuntimeStatus::DesignNeeded {
            assert!(matches!(decoded.content(), NodeContent::NativeOperation(_)));
        }
    }
}

#[test]
fn particle_and_mograph_typed_graphs_survive_project_save_load() {
    let mut project = Project::new("typed native placeholder graph");
    let (composition, track) = Composition::new("main", 1920, 1080, 30.0, 10.0);
    let composition_id = composition.id;
    project
        .add_track(track)
        .expect("Composition Track and structural Sound/Image Merge Nodes are valid");
    project
        .add_composition(composition)
        .expect("Composition and structural Sound/Image Merge Nodes are valid");

    let emitter = add(&mut project, composition_id, "native.particle.emitter");
    let gravity = add(
        &mut project,
        composition_id,
        "native.particle.gravity-force",
    );
    let sprite = add(
        &mut project,
        composition_id,
        "native.particle.sprite-renderer",
    );
    connect(&mut project, emitter, "particles", gravity, "particles");
    connect(&mut project, gravity, "particles", sprite, "particles");

    let camera = add(&mut project, composition_id, "native.3d.camera");
    let object = add(&mut project, composition_id, "native.3d.mesh-instance");
    let points = add(&mut project, composition_id, "native.3d.point-source");
    let field = add(&mut project, composition_id, "native.3d.field");
    let field_stack = add(&mut project, composition_id, "native.3d.field-stack");
    let effector = add(&mut project, composition_id, "native.3d.transform-effector");
    let effector_stack = add(&mut project, composition_id, "native.3d.effector-stack");
    let motion = add(&mut project, composition_id, "native.motion.behavior");
    let cloner = add(&mut project, composition_id, "native.3d.cloner");
    let render = add(&mut project, composition_id, "native.3d.render");

    connect(&mut project, field, "field", field_stack, "fields");
    connect(&mut project, field, "field", effector, "field");
    connect(
        &mut project,
        motion,
        "motion_behavior",
        effector,
        "motion_behavior",
    );
    connect(
        &mut project,
        effector,
        "effector",
        effector_stack,
        "effectors",
    );
    connect(&mut project, object, "object", cloner, "object");
    connect(&mut project, points, "points", cloner, "points");
    connect(
        &mut project,
        effector_stack,
        "effectors",
        cloner,
        "effectors",
    );
    connect(&mut project, field_stack, "fields", cloner, "fields");
    connect(
        &mut project,
        motion,
        "motion_behavior",
        cloner,
        "motion_behavior",
    );
    connect(&mut project, cloner, "instances", render, "instances");
    connect(&mut project, camera, "camera", render, "camera");

    let issues = project.validation_issues();
    assert!(issues.is_empty(), "unexpected graph issues: {issues:#?}");
    let json = project
        .save()
        .expect("typed placeholder Project serializes");
    let loaded = Project::load(&json).expect("typed placeholder Project deserializes");
    assert_eq!(loaded, project);
    let loaded_issues = loaded.validation_issues();
    assert!(
        loaded_issues.is_empty(),
        "unexpected loaded graph issues: {loaded_issues:#?}"
    );
    assert!(loaded.connections.len() >= 13);
}
