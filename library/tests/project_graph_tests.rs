use std::sync::Arc;

use library::animation::EasingFunction;
use library::cache::CacheManager;
use library::framing::get_frame_from_project;
use library::model::frame::Image;
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameContent, FrameGroup, FrameGroupKind, FrameItem};
use library::model::project::{
    Composition, CompositionSettingsError, DURATION_PORT, FPS_PORT, FRAME_PORT, IMAGE_INPUT_PORT,
    IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer, NodeGraphBundle, PortAddress,
    PortDataType, PortDefinition, PortDirection, PortExposure, PortMultiplicity, PortOwner,
    PortSide, Project, ProjectConnection, ProjectGraphError, RESOLUTION_PORT, SHAPE_INPUT_PORT,
    SHAPE_OUTPUT_PORT, TIME_PORT,
};
use library::model::property::{Keyframe, Property, PropertyValue};
use library::model::{
    Asset, AssetKind, BlendMode, Clip, GeneratorContent, MediaContent, Node, NodeContent,
    PluginOperationContent, ReferenceContent, Track,
};
use library::plugin::PluginManager;
use library::rendering::renderer::RenderOutput;
use library::{RenderService, SkiaRenderer};
use ordered_float::OrderedFloat;
use uuid::Uuid;

fn project_with_composition() -> (Project, Uuid, Uuid) {
    let mut project = Project::new("direct graph");
    let (composition, track) = Composition::new("main", 320, 180, 30.0, 10.0);
    let composition_id = composition.id;
    let track_id = track.id;
    project.add_track(track);
    project.add_composition(composition);
    (project, composition_id, track_id)
}

fn add_clip(project: &mut Project, track_id: Uuid, name: &str) -> Uuid {
    let clip = Clip::new(name, 0.0, 10.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();
    clip_id
}

fn solid_node(name: &str) -> Node {
    let mut node = Node::new(name, NodeContent::Generator(GeneratorContent::Solid));
    node.properties.set(
        "color".to_string(),
        Property::constant(PropertyValue::Color(Color::black())),
    );
    node
}

fn colored_solid_node(name: &str, color: Color) -> Node {
    let mut node = solid_node(name);
    node.properties.set(
        "color".to_string(),
        Property::constant(PropertyValue::Color(color)),
    );
    node
}

fn graph_output(key: &str, label: &str, data_type: PortDataType) -> PortDefinition {
    PortDefinition::output(key, label, data_type, PortSide::Right, PortExposure::Graph)
}

fn plugin_operation_node(
    name: &str,
    category: &str,
    component_id: &str,
    operation: &str,
    declared_ports: Vec<PortDefinition>,
) -> Node {
    Node::new(
        name,
        NodeContent::PluginOperation(PluginOperationContent {
            category: category.to_string(),
            component_id: component_id.to_string(),
            operation: operation.to_string(),
            declared_ports,
        }),
    )
}

fn add_node(project: &mut Project, container: NodeContainer, node: Node) -> Uuid {
    let node_id = node.id;
    project.add_node(node);
    project
        .attach_node_to_container(container, node_id)
        .unwrap();
    node_id
}

fn address(owner: PortOwner, port: &str) -> PortAddress {
    PortAddress::new(owner, port)
}

fn frame(project: &Project, frame_number: u64) -> library::model::frame::frame::FrameInfo {
    frame_for_composition(project, 0, frame_number)
}

fn frame_for_composition(
    project: &Project,
    composition_index: usize,
    frame_number: u64,
) -> library::model::frame::frame::FrameInfo {
    let plugins = Arc::new(PluginManager::default());
    get_frame_from_project(
        project,
        composition_index,
        frame_number,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        &plugins,
    )
    .unwrap()
}

fn preview(project: &Project) -> Image {
    let plugins = Arc::new(PluginManager::default());
    let frame = get_frame_from_project(
        project,
        0,
        0,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        &plugins,
    )
    .unwrap();
    let renderer = SkiaRenderer::new(
        frame.width as u32,
        frame.height as u32,
        frame.background_color.clone(),
        false,
        None,
        None,
    )
    .unwrap();
    let mut service = RenderService::new(
        renderer,
        Arc::clone(&plugins),
        Arc::new(CacheManager::new()),
    );
    match service.render_from_frame_info(&frame).unwrap() {
        RenderOutput::Image(image) => image,
        RenderOutput::Texture(_) => panic!("CPU renderer unexpectedly returned a texture"),
    }
}

fn center_pixel(image: &Image) -> [u8; 4] {
    let index = ((image.height / 2 * image.width + image.width / 2) * 4) as usize;
    image.data[index..index + 4].try_into().unwrap()
}

fn container_owner(container: NodeContainer) -> PortOwner {
    match container {
        NodeContainer::Composition(id) => PortOwner::Composition(id),
        NodeContainer::Track(id) => PortOwner::Track(id),
        NodeContainer::Clip(id) => PortOwner::Clip(id),
    }
}

fn container_output(project: &Project, container: NodeContainer) -> Option<Uuid> {
    match container {
        NodeContainer::Composition(id) => project.get_composition(id).unwrap().output_node_id,
        NodeContainer::Track(id) => project.get_track(id).unwrap().output_node_id,
        NodeContainer::Clip(id) => project.get_clip(id).unwrap().output_node_id,
    }
}

fn find_group(items: &[FrameItem], source_id: Uuid) -> Option<&FrameGroup> {
    items.iter().find_map(|item| match item {
        FrameItem::Group(group) if group.source_id == source_id => Some(group),
        FrameItem::Group(group) => find_group(&group.items, source_id),
        FrameItem::Object(_) => None,
    })
}

fn first_content(items: &[FrameItem]) -> Option<&FrameContent> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(object) => Some(&object.content),
        FrameItem::Group(group) => first_content(&group.items),
    })
}

fn object_source_ids(items: &[FrameItem]) -> Vec<Uuid> {
    fn collect(items: &[FrameItem], ids: &mut Vec<Uuid>) {
        for item in items {
            match item {
                FrameItem::Object(object) => ids.push(object.source_node_id),
                FrameItem::Group(group) => collect(&group.items, ids),
            }
        }
    }
    let mut ids = Vec::new();
    collect(items, &mut ids);
    ids
}

#[test]
fn direct_pre_v1_schema_roundtrips_without_version_or_legacy_node_timing() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "timeline placement");
    let node_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("leaf"),
    );
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(node_id))
        .unwrap();
    let json = project.save().unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let root = value.as_object().unwrap();
    assert!(!root.contains_key("schema_version"));
    assert!(!root.contains_key("migration"));
    assert!(root.contains_key("compositions"));
    assert!(root.contains_key("tracks"));
    assert!(root.contains_key("clips"));
    assert!(root.contains_key("nodes"));

    let serialized_node = value["nodes"].get(node_id.to_string()).unwrap();
    assert!(serialized_node.get("start_time").is_none());
    assert!(serialized_node.get("duration").is_none());
    assert!(serialized_node.get("trim_in").is_none());
    assert!(serialized_node.get("time_stretch").is_none());
    assert!(
        value["clips"]
            .get(clip_id.to_string())
            .unwrap()
            .get("start_time")
            .is_some()
    );

    let loaded = Project::load(&json).unwrap();
    assert_eq!(loaded, project);
    assert!(loaded.validate_containment().is_empty());
}

#[test]
fn wire_blend_is_required_and_distinct_fanout_values_roundtrip_losslessly() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "fanout");
    let container = NodeContainer::Clip(clip_id);
    let source_id = add_node(&mut project, container, solid_node("shared source"));
    let first_merge_id = add_node(
        &mut project,
        container,
        Node::new("first merge", NodeContent::Merge),
    );
    let second_merge_id = add_node(
        &mut project,
        container,
        Node::new("second merge", NodeContent::Merge),
    );
    let source = address(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT);
    let first_wire = project
        .connect_ports(
            source.clone(),
            address(PortOwner::Node(first_merge_id), MERGE_IMAGES_PORT),
        )
        .unwrap();
    let second_wire = project
        .connect_ports(
            source,
            address(PortOwner::Node(second_merge_id), MERGE_IMAGES_PORT),
        )
        .unwrap();
    project
        .set_connection_blend_mode(first_wire, BlendMode::Add)
        .unwrap();
    project
        .set_connection_blend_mode(second_wire, BlendMode::Multiply)
        .unwrap();

    let json = project.save().unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let serialized_connections = value["connections"].as_array().unwrap();
    let serialized_blend = |id: Uuid| {
        serialized_connections
            .iter()
            .find(|connection| connection["id"] == id.to_string())
            .unwrap()["blend_mode"]
            .as_str()
            .unwrap()
    };
    assert_eq!(serialized_blend(first_wire), "Add");
    assert_eq!(serialized_blend(second_wire), "Multiply");
    assert_eq!(Project::load(&json).unwrap(), project);

    let mut missing_field = value;
    missing_field["connections"]
        .as_array_mut()
        .unwrap()
        .first_mut()
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("blend_mode");
    let error = Project::load(&serde_json::to_string(&missing_field).unwrap()).unwrap_err();
    assert!(
        error.to_string().contains("missing field `blend_mode`"),
        "pre-v1 Projects with an old connection shape must be rejected explicitly: {error}",
    );
}

#[test]
fn unknown_plugin_operation_roundtrips_identity_ports_properties_keyframes_and_wires() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "unknown operation graph");

    let mut shape = plugin_operation_node(
        "Future Shape",
        "future_shape_category",
        "dev.example.future-shape",
        "shape.produce.v47",
        vec![graph_output(
            SHAPE_OUTPUT_PORT,
            "Shape",
            PortDataType::Shape,
        )],
    );
    shape.properties.set(
        "strength".to_string(),
        Property::keyframe(vec![
            Keyframe::new(
                0.0,
                PropertyValue::Number(OrderedFloat(0.25)),
                EasingFunction::Linear,
            ),
            Keyframe::new(
                1.0,
                PropertyValue::Number(OrderedFloat(0.75)),
                EasingFunction::EaseInQuad,
            ),
        ]),
    );
    let shape_id = shape.id;

    let consumer = plugin_operation_node(
        "Future Consumer",
        "future_generator_category",
        "dev.example.future-consumer",
        "image.produce.v103",
        vec![
            PortDefinition::input(SHAPE_INPUT_PORT, "Shapes", PortDataType::Shape).variadic(),
            graph_output(IMAGE_OUTPUT_PORT, "Image", PortDataType::Image),
        ],
    );
    let consumer_id = consumer.id;
    let connection = ProjectConnection::new(
        address(PortOwner::Node(shape_id), SHAPE_OUTPUT_PORT),
        address(PortOwner::Node(consumer_id), SHAPE_INPUT_PORT),
        7,
    );
    let connection_id = connection.id;

    project
        .insert_node_graph(
            NodeContainer::Clip(clip_id),
            NodeGraphBundle::new(vec![shape, consumer], vec![connection], Some(consumer_id)),
        )
        .unwrap();
    assert!(project.validate_connections().is_empty());

    let json = project.save().unwrap();
    let serialized: serde_json::Value = serde_json::from_str(&json).unwrap();
    let operation_data = &serialized["nodes"][shape_id.to_string()]["content"]["data"];
    assert_eq!(
        operation_data["component_id"],
        serde_json::Value::String("dev.example.future-shape".to_string())
    );
    assert!(operation_data.get("plugin_id").is_none());
    let loaded = Project::load(&json).unwrap();
    assert_eq!(loaded, project);
    assert_eq!(loaded.connections[0].id, connection_id);
    assert_eq!(loaded.connections[0].order, 7);
    let loaded_shape = loaded.get_node(shape_id).unwrap();
    let NodeContent::PluginOperation(operation) = &loaded_shape.content else {
        panic!("unknown operation must remain a PluginOperation");
    };
    assert_eq!(operation.category, "future_shape_category");
    assert_eq!(operation.component_id, "dev.example.future-shape");
    assert_eq!(operation.operation, "shape.produce.v47");
    assert_eq!(
        operation.declared_ports,
        vec![graph_output(
            SHAPE_OUTPUT_PORT,
            "Shape",
            PortDataType::Shape,
        )]
    );
    assert_eq!(
        loaded_shape.properties,
        project.get_node(shape_id).unwrap().properties
    );
    assert!(loaded.validate_connections().is_empty());
    assert!(
        frame(&loaded, 0).items.is_empty(),
        "an unavailable operation must produce NoOutput instead of failing the frame"
    );
}

#[test]
fn plugin_operation_declared_ports_are_required_persisted_project_data() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "required ports");
    let operation = plugin_operation_node(
        "Unknown",
        "effect",
        "dev.example.unknown",
        "effect.apply",
        vec![graph_output(
            IMAGE_OUTPUT_PORT,
            "Image",
            PortDataType::Image,
        )],
    );
    let operation_id = operation.id;
    project
        .insert_node_graph(
            NodeContainer::Clip(clip_id),
            NodeGraphBundle::new(vec![operation], Vec::new(), Some(operation_id)),
        )
        .unwrap();

    let mut json: serde_json::Value = serde_json::from_str(&project.save().unwrap()).unwrap();
    json["nodes"][operation_id.to_string()]["content"]["data"]
        .as_object_mut()
        .unwrap()
        .remove("declared_ports");
    let malformed = serde_json::to_string(&json).unwrap();

    let error = Project::load(&malformed).unwrap_err();
    assert!(error.to_string().contains("declared_ports"));
}

#[test]
fn plugin_and_leaf_nodes_expose_only_declared_or_consumed_ports() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "non-image helper");
    let declared_ports = vec![
        PortDefinition::input("amount", "Amount", PortDataType::Number),
        graph_output(SHAPE_OUTPUT_PORT, "Shape", PortDataType::Shape),
    ];
    let mut operation = plugin_operation_node(
        "Shape helper",
        "utility",
        "dev.example.shape-helper",
        "shape.produce",
        declared_ports.clone(),
    );
    operation.properties.set(
        "authored_but_not_connectable".to_string(),
        Property::constant(PropertyValue::Boolean(true)),
    );
    let operation_id = operation.id;
    project
        .insert_node_graph(
            NodeContainer::Clip(clip_id),
            NodeGraphBundle::new(vec![operation], Vec::new(), None),
        )
        .unwrap();

    let ports = project.port_definitions(PortOwner::Node(operation_id));
    assert_eq!(ports, declared_ports);
    assert!(!ports.iter().any(|port| {
        matches!(
            port.key.as_str(),
            TIME_PORT | FRAME_PORT | FPS_PORT | DURATION_PORT | RESOLUTION_PORT
        )
    }));
    assert!(
        !ports
            .iter()
            .any(|port| port.key == "authored_but_not_connectable")
    );
    assert!(
        project
            .container_image_sources(PortOwner::Clip(clip_id))
            .is_empty(),
        "a non-image operation must not become an implicit container image source"
    );

    let leaf_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("leaf"),
    );
    let leaf_ports = project.port_definitions(PortOwner::Node(leaf_id));
    for (required, direction) in [
        (TIME_PORT, PortDirection::Input),
        ("color", PortDirection::Input),
        (IMAGE_OUTPUT_PORT, PortDirection::Output),
    ] {
        assert!(
            leaf_ports
                .iter()
                .any(|port| port.key == required && port.direction == direction)
        );
    }
    assert!(!leaf_ports.iter().any(|port| {
        matches!(
            port.key.as_str(),
            FRAME_PORT | FPS_PORT | DURATION_PORT | RESOLUTION_PORT
        )
    }));
    assert!(
        project
            .container_image_sources(PortOwner::Clip(clip_id))
            .is_empty()
    );
}

#[test]
fn typed_shape_plugin_connections_reject_cycles() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "typed cycle");
    let ports = || {
        vec![
            PortDefinition::input(SHAPE_INPUT_PORT, "Shapes", PortDataType::Shape).variadic(),
            graph_output(SHAPE_OUTPUT_PORT, "Shape", PortDataType::Shape),
        ]
    };
    let first = plugin_operation_node(
        "First Shape",
        "effector",
        "dev.example.first",
        "shape.transform",
        ports(),
    );
    let first_id = first.id;
    let second = plugin_operation_node(
        "Second Shape",
        "decorator",
        "dev.example.second",
        "shape.decorate",
        ports(),
    );
    let second_id = second.id;
    project
        .insert_node_graph(
            NodeContainer::Clip(clip_id),
            NodeGraphBundle::new(vec![first, second], Vec::new(), None),
        )
        .unwrap();

    project
        .connect_ports(
            address(PortOwner::Node(first_id), SHAPE_OUTPUT_PORT),
            address(PortOwner::Node(second_id), SHAPE_INPUT_PORT),
        )
        .unwrap();
    assert_eq!(
        project.connect_ports(
            address(PortOwner::Node(second_id), SHAPE_OUTPUT_PORT),
            address(PortOwner::Node(first_id), SHAPE_INPUT_PORT),
        ),
        Err(ProjectGraphError::ConnectionCycle {
            from: PortOwner::Node(second_id),
            to: PortOwner::Node(first_id),
        })
    );
    assert_eq!(project.connections.len(), 1);
    assert!(project.validate_connections().is_empty());
}

#[test]
fn descendant_value_cannot_override_ancestor_scope_but_internal_metadata_can_feed_child() {
    let (mut project, composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "scope cycle");
    let operation = plugin_operation_node(
        "Scope Value",
        "utility",
        "dev.example.scope-value",
        "number.produce",
        vec![
            PortDefinition::input(TIME_PORT, "Time", PortDataType::Number),
            graph_output("value", "Value", PortDataType::Number),
        ],
    );
    let operation_id = operation.id;
    project
        .insert_node_graph(
            NodeContainer::Clip(clip_id),
            NodeGraphBundle::new(vec![operation], Vec::new(), None),
        )
        .unwrap();

    let source = address(PortOwner::Node(operation_id), "value");
    for target in [
        address(PortOwner::Clip(clip_id), TIME_PORT),
        address(PortOwner::Track(track_id), DURATION_PORT),
        address(PortOwner::Composition(composition_id), DURATION_PORT),
    ] {
        assert_eq!(
            project.connect_ports(source.clone(), target.clone()),
            Err(ProjectGraphError::ConnectionCycle {
                from: PortOwner::Node(operation_id),
                to: target.owner,
            })
        );
    }
    assert!(project.connections.is_empty());

    for read_only in [
        address(PortOwner::Track(track_id), FPS_PORT),
        address(PortOwner::Composition(composition_id), FRAME_PORT),
    ] {
        assert_eq!(
            project.connect_ports(source.clone(), read_only.clone()),
            Err(ProjectGraphError::PortNotFound(read_only))
        );
    }

    project
        .connect_ports(
            address(PortOwner::Clip(clip_id), FPS_PORT),
            address(PortOwner::Node(operation_id), TIME_PORT),
        )
        .unwrap();
    assert_eq!(project.connections.len(), 1);
    assert!(project.validate_connections().is_empty());
}

#[test]
fn node_graph_bundle_commit_and_structural_failure_are_atomic() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "atomic graph");

    let invalid = plugin_operation_node(
        "Duplicate ports",
        "effector",
        "dev.example.invalid",
        "shape.transform",
        vec![
            graph_output(SHAPE_OUTPUT_PORT, "First", PortDataType::Shape),
            graph_output(SHAPE_OUTPUT_PORT, "Second", PortDataType::Shape),
        ],
    );
    let invalid_id = invalid.id;
    let before_invalid_insert = project.clone();
    assert_eq!(
        project.insert_node_graph(
            NodeContainer::Clip(clip_id),
            NodeGraphBundle::new(vec![invalid], Vec::new(), None),
        ),
        Err(ProjectGraphError::DuplicateNodePort {
            node_id: invalid_id,
            key: SHAPE_OUTPUT_PORT.to_string(),
            direction: PortDirection::Output,
        })
    );
    assert_eq!(project, before_invalid_insert);

    let detached = solid_node("detached");
    let detached_id = detached.id;
    let unrelated_connection = ProjectConnection::new(
        address(PortOwner::Composition(_composition_id), TIME_PORT),
        address(PortOwner::Track(track_id), TIME_PORT),
        0,
    );
    let unrelated_connection_id = unrelated_connection.id;
    let before_unrelated_wire = project.clone();
    assert_eq!(
        project.insert_node_graph(
            NodeContainer::Clip(clip_id),
            NodeGraphBundle::new(
                vec![detached],
                vec![unrelated_connection],
                Some(detached_id),
            ),
        ),
        Err(ProjectGraphError::NodeGraphConnectionOutsideBundle(
            unrelated_connection_id,
        ))
    );
    assert_eq!(project, before_unrelated_wire);

    let source = solid_node("source");
    let source_id = source.id;
    let merge = Node::new("merge", NodeContent::Merge);
    let merge_id = merge.id;
    let malformed_connection = ProjectConnection::new(
        address(PortOwner::Node(source_id), "missing_output"),
        address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        0,
    );
    let before_bad_wire = project.clone();
    assert_eq!(
        project.insert_node_graph(
            NodeContainer::Clip(clip_id),
            NodeGraphBundle::new(
                vec![source.clone(), merge.clone()],
                vec![malformed_connection],
                Some(merge_id),
            ),
        ),
        Err(ProjectGraphError::PortNotFound(address(
            PortOwner::Node(source_id),
            "missing_output",
        )))
    );
    assert_eq!(project, before_bad_wire);

    let connection = ProjectConnection::new(
        address(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
        address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        3,
    );
    let connection_id = connection.id;
    project
        .insert_node_graph_at(
            NodeContainer::Clip(clip_id),
            NodeGraphBundle::new(vec![source, merge], vec![connection], Some(merge_id)),
            Some(0),
        )
        .unwrap();

    assert_eq!(
        project.get_clip(clip_id).unwrap().node_ids,
        vec![source_id, merge_id]
    );
    assert_eq!(
        project.get_clip(clip_id).unwrap().output_node_id,
        Some(merge_id)
    );
    assert_eq!(project.connections.len(), 1);
    assert_eq!(project.connections[0].id, connection_id);
    assert_eq!(project.connections[0].order, 3);
    assert!(project.validate_connections().is_empty());
}

#[test]
fn containment_is_exact_and_reparenting_does_not_duplicate_ownership() {
    let (mut project, composition_id, first_track_id) = project_with_composition();
    let second_track = Track::new("second");
    let second_track_id = second_track.id;
    project.add_track(second_track);
    project
        .attach_track_to_composition(composition_id, second_track_id)
        .unwrap();

    let clip_id = add_clip(&mut project, first_track_id, "movable");
    let node_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("contained once"),
    );
    project
        .attach_clip_to_track(second_track_id, clip_id)
        .unwrap();
    project
        .attach_node_to_container(NodeContainer::Track(first_track_id), node_id)
        .unwrap();

    assert!(
        !project
            .get_track(first_track_id)
            .unwrap()
            .clip_ids
            .contains(&clip_id)
    );
    assert_eq!(
        project.get_track(second_track_id).unwrap().clip_ids,
        vec![clip_id]
    );
    assert!(project.get_clip(clip_id).unwrap().node_ids.is_empty());
    assert_eq!(
        project.get_track(first_track_id).unwrap().node_ids,
        vec![node_id]
    );
    assert_eq!(
        project.find_node_container(node_id),
        Some(NodeContainer::Track(first_track_id))
    );
    assert!(project.validate_containment().is_empty());

    let orphan = solid_node("orphan");
    let orphan_id = orphan.id;
    project.add_node(orphan);
    assert!(
        project
            .validate_containment()
            .contains(&ProjectGraphError::NodeHasNoContainer(orphan_id))
    );
}

#[test]
fn validation_reports_identity_and_composition_invariants() {
    let (project, composition_id, track_id) = project_with_composition();

    let mut duplicate_composition = project.clone();
    duplicate_composition
        .compositions
        .push(duplicate_composition.compositions[0].clone());
    assert!(
        duplicate_composition
            .validate_connections()
            .contains(&ProjectGraphError::DuplicateCompositionId(composition_id))
    );

    let mut bad_track_key = project.clone();
    let track = bad_track_key.tracks.get(&track_id).unwrap().clone();
    let wrong_track_key = Uuid::new_v4();
    bad_track_key.tracks.insert(wrong_track_key, track);
    assert!(
        bad_track_key
            .validate_connections()
            .contains(&ProjectGraphError::TrackKeyMismatch {
                key: wrong_track_key,
                entity_id: track_id,
            })
    );
    let issue = ProjectGraphError::TrackKeyMismatch {
        key: wrong_track_key,
        entity_id: track_id,
    };
    let serialized_issue = serde_json::to_value(&issue).unwrap();
    assert_eq!(serialized_issue["code"], "track_key_mismatch");
    assert_eq!(
        serialized_issue["context"]["key"],
        wrong_track_key.to_string()
    );
    assert_eq!(
        serialized_issue["context"]["entity_id"],
        track_id.to_string()
    );

    let mut invalid_settings = project.clone();
    invalid_settings
        .get_composition_mut(composition_id)
        .unwrap()
        .width = 0;
    assert!(invalid_settings.validate_connections().contains(
        &ProjectGraphError::InvalidCompositionSettings {
            composition_id,
            reason: CompositionSettingsError::WidthZero,
        }
    ));

    let mut unrepresentable_frame_count = project.clone();
    let composition = unrepresentable_frame_count
        .get_composition_mut(composition_id)
        .unwrap();
    composition.fps = f64::MAX;
    composition.duration = f64::MAX;
    assert!(unrepresentable_frame_count.validate_connections().contains(
        &ProjectGraphError::InvalidCompositionSettings {
            composition_id,
            reason: CompositionSettingsError::FrameCountOutOfRange,
        }
    ));

    let mut invalid_work_area = project;
    invalid_work_area
        .get_composition_mut(composition_id)
        .unwrap()
        .work_area_out = 301;
    assert!(invalid_work_area.validate_connections().contains(
        &ProjectGraphError::InvalidCompositionWorkArea {
            composition_id,
            work_area_in: 0,
            work_area_out: 301,
            frame_count: 300,
        }
    ));
}

#[test]
fn validation_reports_clip_node_asset_and_connection_identity_corruption() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "clip");
    let source_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("source"),
    );
    let merge_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new("merge", NodeContent::Merge),
    );
    let connection_id = project
        .connect_ports(
            address(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
            address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )
        .unwrap();
    let duplicate_connection = project.connections[0].clone();
    project.connections.push(duplicate_connection);

    let clip = project.get_clip(clip_id).unwrap().clone();
    let wrong_clip_key = Uuid::new_v4();
    project.clips.insert(wrong_clip_key, clip);
    let node = project.get_node(source_id).unwrap().clone();
    let wrong_node_key = Uuid::new_v4();
    project.nodes.insert(wrong_node_key, node);
    let asset = Asset::new("duplicate", "duplicate.png", AssetKind::Image);
    let asset_id = asset.id;
    project.assets.push(asset.clone());
    project.assets.push(asset);

    let errors = project.validate_connections();
    assert!(errors.contains(&ProjectGraphError::ClipKeyMismatch {
        key: wrong_clip_key,
        entity_id: clip_id,
    }));
    assert!(errors.contains(&ProjectGraphError::NodeKeyMismatch {
        key: wrong_node_key,
        entity_id: source_id,
    }));
    assert!(errors.contains(&ProjectGraphError::DuplicateAssetId(asset_id)));
    assert!(errors.contains(&ProjectGraphError::DuplicateConnectionId(connection_id)));
}

#[test]
fn attaching_a_node_to_any_missing_container_is_atomic() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "original container");
    let node_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("original output"),
    );
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(node_id))
        .unwrap();
    let original = project;

    let missing_composition = Uuid::new_v4();
    let missing_track = Uuid::new_v4();
    let missing_clip = Uuid::new_v4();
    let cases = [
        (
            NodeContainer::Composition(missing_composition),
            ProjectGraphError::CompositionNotFound(missing_composition),
        ),
        (
            NodeContainer::Track(missing_track),
            ProjectGraphError::TrackNotFound(missing_track),
        ),
        (
            NodeContainer::Clip(missing_clip),
            ProjectGraphError::ClipNotFound(missing_clip),
        ),
    ];

    for (container, expected_error) in cases {
        let mut attempted = original.clone();
        assert_eq!(
            attempted.attach_node_to_container_at(container, node_id, Some(0)),
            Err(expected_error)
        );
        assert_eq!(attempted, original);
    }
}

#[test]
fn track_and_clip_reparent_remap_only_direct_parent_metadata_and_still_render() {
    let (mut project, first_composition_id, first_track_id) = project_with_composition();
    let (second_composition, second_track) = Composition::new("second", 320, 180, 30.0, 10.0);
    let second_composition_id = second_composition.id;
    let second_track_id = second_track.id;
    project.add_track(second_track);
    project.add_composition(second_composition);

    let clip_id = add_clip(&mut project, first_track_id, "movable clip");
    let node_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("render after move"),
    );
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(node_id))
        .unwrap();

    let track_connection_id = project
        .connect_ports(
            address(PortOwner::Composition(first_composition_id), TIME_PORT),
            address(PortOwner::Track(first_track_id), TIME_PORT),
        )
        .unwrap();
    let clip_connection_id = project
        .connect_ports(
            address(PortOwner::Track(first_track_id), DURATION_PORT),
            address(PortOwner::Clip(clip_id), DURATION_PORT),
        )
        .unwrap();
    let unrelated_connection_id = project
        .connect_ports(
            address(
                PortOwner::Composition(second_composition_id),
                RESOLUTION_PORT,
            ),
            address(PortOwner::Track(second_track_id), RESOLUTION_PORT),
        )
        .unwrap();
    let original_track_connection = project
        .connections
        .iter()
        .find(|connection| connection.id == track_connection_id)
        .unwrap()
        .clone();
    let original_clip_connection = project
        .connections
        .iter()
        .find(|connection| connection.id == clip_connection_id)
        .unwrap()
        .clone();
    let original_unrelated_connection = project
        .connections
        .iter()
        .find(|connection| connection.id == unrelated_connection_id)
        .unwrap()
        .clone();

    project
        .attach_track_to_composition_at(second_composition_id, first_track_id, Some(0))
        .unwrap();
    let moved_track_connection = project
        .connections
        .iter()
        .find(|connection| connection.id == track_connection_id)
        .unwrap();
    assert_eq!(
        moved_track_connection.from,
        address(PortOwner::Composition(second_composition_id), TIME_PORT)
    );
    assert_eq!(moved_track_connection.to, original_track_connection.to);
    assert_eq!(
        moved_track_connection.order,
        original_track_connection.order
    );
    assert_eq!(moved_track_connection.id, original_track_connection.id);
    assert_eq!(
        project
            .connections
            .iter()
            .find(|connection| connection.id == unrelated_connection_id)
            .unwrap(),
        &original_unrelated_connection
    );

    project
        .attach_clip_to_track_at(second_track_id, clip_id, Some(0))
        .unwrap();
    let moved_clip_connection = project
        .connections
        .iter()
        .find(|connection| connection.id == clip_connection_id)
        .unwrap();
    assert_eq!(
        moved_clip_connection.from,
        address(PortOwner::Track(second_track_id), DURATION_PORT)
    );
    assert_eq!(moved_clip_connection.to, original_clip_connection.to);
    assert_eq!(moved_clip_connection.order, original_clip_connection.order);
    assert_eq!(moved_clip_connection.id, original_clip_connection.id);
    assert!(project.validate_connections().is_empty());
    assert!(frame_for_composition(&project, 1, 0).object_count() > 0);
}

#[test]
fn direct_node_reparent_remaps_metadata_for_every_container_pair() {
    for source_kind in 0..3 {
        for destination_kind in 0..3 {
            let mut project = Project::new("node parent matrix");
            let (first_composition, first_track) = Composition::new("first", 320, 180, 30.0, 10.0);
            let first_composition_id = first_composition.id;
            let first_track_id = first_track.id;
            project.add_track(first_track);
            project.add_composition(first_composition);
            let first_clip_id = add_clip(&mut project, first_track_id, "first clip");

            let (second_composition, second_track) =
                Composition::new("second", 320, 180, 30.0, 10.0);
            let second_composition_id = second_composition.id;
            let second_track_id = second_track.id;
            project.add_track(second_track);
            project.add_composition(second_composition);
            let second_clip_id = add_clip(&mut project, second_track_id, "second clip");

            let sources = [
                NodeContainer::Composition(first_composition_id),
                NodeContainer::Track(first_track_id),
                NodeContainer::Clip(first_clip_id),
            ];
            let destinations = [
                NodeContainer::Composition(second_composition_id),
                NodeContainer::Track(second_track_id),
                NodeContainer::Clip(second_clip_id),
            ];
            let source = sources[source_kind];
            let destination = destinations[destination_kind];
            let moved_node_id =
                add_node(&mut project, source, Node::new("moved", NodeContent::Merge));
            let destination_output_id =
                add_node(&mut project, destination, solid_node("destination output"));
            project
                .set_output_node(source, Some(moved_node_id))
                .unwrap();
            project
                .set_output_node(destination, Some(destination_output_id))
                .unwrap();
            let connection_id = project
                .connect_ports(
                    address(container_owner(source), TIME_PORT),
                    address(PortOwner::Node(moved_node_id), TIME_PORT),
                )
                .unwrap();
            let original = project
                .connections
                .iter()
                .find(|connection| connection.id == connection_id)
                .unwrap()
                .clone();

            project
                .attach_node_to_container_at(destination, moved_node_id, Some(0))
                .unwrap();

            let remapped = project
                .connections
                .iter()
                .find(|connection| connection.id == connection_id)
                .unwrap();
            assert_eq!(
                remapped.from,
                address(container_owner(destination), TIME_PORT),
                "source kind {source_kind}, destination kind {destination_kind}"
            );
            assert_eq!(remapped.id, original.id);
            assert_eq!(remapped.order, original.order);
            assert_eq!(remapped.to, original.to);
            assert_eq!(container_output(&project, source), None);
            assert_eq!(
                container_output(&project, destination),
                Some(destination_output_id)
            );
            assert_eq!(
                project.find_node_container(moved_node_id),
                Some(destination)
            );
            assert!(project.validate_connections().is_empty());
        }
    }
}

#[test]
fn same_parent_reorder_preserves_metadata_connections_and_output_binding() {
    let (mut project, composition_id, first_track_id) = project_with_composition();
    let second_track = Track::new("second");
    let second_track_id = second_track.id;
    project.add_track(second_track);
    project
        .attach_track_to_composition(composition_id, second_track_id)
        .unwrap();
    project
        .connect_ports(
            address(PortOwner::Composition(composition_id), TIME_PORT),
            address(PortOwner::Track(first_track_id), TIME_PORT),
        )
        .unwrap();

    let first_clip_id = add_clip(&mut project, second_track_id, "first");
    let second_clip_id = add_clip(&mut project, second_track_id, "second");
    project
        .connect_ports(
            address(PortOwner::Track(second_track_id), TIME_PORT),
            address(PortOwner::Clip(first_clip_id), TIME_PORT),
        )
        .unwrap();
    let first_node_id = add_node(
        &mut project,
        NodeContainer::Clip(second_clip_id),
        solid_node("first node"),
    );
    let second_node_id = add_node(
        &mut project,
        NodeContainer::Clip(second_clip_id),
        solid_node("second node"),
    );
    project
        .set_output_node(NodeContainer::Clip(second_clip_id), Some(first_node_id))
        .unwrap();
    project
        .connect_ports(
            address(PortOwner::Clip(second_clip_id), TIME_PORT),
            address(PortOwner::Node(first_node_id), TIME_PORT),
        )
        .unwrap();
    let original_connections = project.connections.clone();

    project
        .attach_track_to_composition_at(composition_id, first_track_id, Some(1))
        .unwrap();
    project
        .attach_clip_to_track_at(second_track_id, first_clip_id, Some(1))
        .unwrap();
    project
        .attach_node_to_container_at(NodeContainer::Clip(second_clip_id), first_node_id, Some(1))
        .unwrap();

    assert_eq!(
        project.get_composition(composition_id).unwrap().track_ids,
        vec![second_track_id, first_track_id]
    );
    assert_eq!(
        project.get_track(second_track_id).unwrap().clip_ids,
        vec![second_clip_id, first_clip_id]
    );
    assert_eq!(
        project.get_clip(second_clip_id).unwrap().node_ids,
        vec![second_node_id, first_node_id]
    );
    assert_eq!(project.connections, original_connections);
    assert_eq!(
        project.get_clip(second_clip_id).unwrap().output_node_id,
        Some(first_node_id)
    );
    assert!(project.validate_connections().is_empty());
}

#[test]
fn node_reparent_preserves_graph_image_wires_ids_orders_targets_and_rendering() {
    let (mut project, _composition_id, first_track_id) = project_with_composition();
    let second_track = Track::new("second");
    let second_track_id = second_track.id;
    project.add_track(second_track);
    let composition_id = project.compositions[0].id;
    project
        .attach_track_to_composition(composition_id, second_track_id)
        .unwrap();
    let first_source_id = add_node(
        &mut project,
        NodeContainer::Track(second_track_id),
        solid_node("first source"),
    );
    let second_source_id = add_node(
        &mut project,
        NodeContainer::Track(second_track_id),
        solid_node("second source"),
    );
    let merge_id = add_node(
        &mut project,
        NodeContainer::Track(first_track_id),
        Node::new("moved merge", NodeContent::Merge),
    );
    project
        .connect_ports(
            address(PortOwner::Node(first_source_id), IMAGE_OUTPUT_PORT),
            address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )
        .unwrap();
    let second_image_connection_id = project
        .connect_ports(
            address(PortOwner::Node(second_source_id), IMAGE_OUTPUT_PORT),
            address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )
        .unwrap();
    project
        .set_connection_blend_mode(second_image_connection_id, BlendMode::Overlay)
        .unwrap();
    project
        .reorder_connection(second_image_connection_id, 0)
        .unwrap();
    let metadata_connection_id = project
        .connect_ports(
            address(PortOwner::Track(first_track_id), TIME_PORT),
            address(PortOwner::Node(merge_id), TIME_PORT),
        )
        .unwrap();
    let original_connections = project.connections.clone();

    project
        .attach_node_to_container(NodeContainer::Track(second_track_id), merge_id)
        .unwrap();
    project
        .set_output_node(NodeContainer::Track(second_track_id), Some(merge_id))
        .unwrap();

    for original in &original_connections {
        let current = project
            .connections
            .iter()
            .find(|connection| connection.id == original.id)
            .unwrap();
        assert_eq!(current.id, original.id);
        assert_eq!(current.order, original.order);
        assert_eq!(current.blend_mode, original.blend_mode);
        assert_eq!(current.to, original.to);
        if original.id == metadata_connection_id {
            assert_eq!(
                current.from,
                address(PortOwner::Track(second_track_id), TIME_PORT)
            );
        } else {
            assert_eq!(current.from, original.from);
        }
    }
    assert!(project.validate_connections().is_empty());
    assert!(frame(&project, 0).object_count() > 0);
}

#[test]
fn cycle_created_by_reparent_rolls_back_containment_output_and_all_wires() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "cycle clip");
    let node_id = add_node(
        &mut project,
        NodeContainer::Track(track_id),
        plugin_operation_node(
            "cycle value",
            "utility",
            "dev.example.cycle-value",
            "value.produce",
            vec![
                PortDefinition::input(TIME_PORT, "Time", PortDataType::Number),
                graph_output("value", "Value", PortDataType::Number),
                graph_output(IMAGE_OUTPUT_PORT, "Image", PortDataType::Image),
            ],
        ),
    );
    project
        .set_output_node(NodeContainer::Track(track_id), Some(node_id))
        .unwrap();
    project
        .connect_ports(
            address(PortOwner::Track(track_id), TIME_PORT),
            address(PortOwner::Node(node_id), TIME_PORT),
        )
        .unwrap();
    project
        .connect_ports(
            address(PortOwner::Node(node_id), "value"),
            address(PortOwner::Clip(clip_id), DURATION_PORT),
        )
        .unwrap();
    assert!(project.validate_connections().is_empty());
    let original = project.clone();

    let result = project.attach_node_to_container(NodeContainer::Clip(clip_id), node_id);

    assert!(matches!(
        result,
        Err(ProjectGraphError::ConnectionCycle { .. })
    ));
    assert_eq!(project, original);
}

#[test]
fn unremappable_direct_parent_source_fails_without_mutating_project() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "destination");
    let node_id = add_node(
        &mut project,
        NodeContainer::Track(track_id),
        Node::new("moved", NodeContent::Merge),
    );
    let missing_source = address(PortOwner::Track(track_id), "missing_metadata");
    project.connections.push(ProjectConnection::new(
        missing_source.clone(),
        address(PortOwner::Node(node_id), TIME_PORT),
        0,
    ));
    let original = project.clone();

    assert_eq!(
        project.attach_node_to_container(NodeContainer::Clip(clip_id), node_id),
        Err(ProjectGraphError::PortNotFound(missing_source))
    );
    assert_eq!(project, original);
}

#[test]
fn container_ports_separate_authored_inputs_from_read_only_runtime_outputs() {
    let (mut project, composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "clip");

    for owner in [
        PortOwner::Composition(composition_id),
        PortOwner::Track(track_id),
        PortOwner::Clip(clip_id),
    ] {
        let ports = project.port_definitions(owner);
        assert_eq!(ports.len(), 9);
        for (key, data_type) in [
            (TIME_PORT, PortDataType::Number),
            (DURATION_PORT, PortDataType::Number),
            (RESOLUTION_PORT, PortDataType::Vec2),
        ] {
            let input = ports
                .iter()
                .find(|port| port.key == key && port.direction == PortDirection::Input)
                .unwrap();
            assert_eq!(input.side, PortSide::Left);
            assert_eq!(input.exposure, PortExposure::External);
            assert_eq!(input.data_type, data_type);
        }
        assert!(!ports.iter().any(|port| {
            port.direction == PortDirection::Input
                && matches!(port.key.as_str(), FRAME_PORT | FPS_PORT)
        }));
        for (key, data_type) in [
            (TIME_PORT, PortDataType::Number),
            (FRAME_PORT, PortDataType::Integer),
            (FPS_PORT, PortDataType::Number),
            (DURATION_PORT, PortDataType::Number),
            (RESOLUTION_PORT, PortDataType::Vec2),
        ] {
            let output = ports
                .iter()
                .find(|port| port.key == key && port.direction == PortDirection::Output)
                .unwrap();
            assert_eq!(output.side, PortSide::Left);
            assert_eq!(output.exposure, PortExposure::Internal);
            assert_eq!(output.data_type, data_type);
        }
        let image = ports
            .iter()
            .find(|port| port.key == IMAGE_OUTPUT_PORT && port.direction == PortDirection::Output)
            .unwrap();
        assert_eq!(image.side, PortSide::Right);
        assert_eq!(image.exposure, PortExposure::External);
        assert_eq!(image.data_type, PortDataType::Image);
    }
}

#[test]
fn cross_track_image_connection_preserves_containment_and_internal_metadata_cannot_escape() {
    let (mut project, composition_id, first_track_id) = project_with_composition();
    let second_track = Track::new("second");
    let second_track_id = second_track.id;
    project.add_track(second_track);
    project
        .attach_track_to_composition(composition_id, second_track_id)
        .unwrap();
    let first_clip = add_clip(&mut project, first_track_id, "source clip");
    let second_clip = add_clip(&mut project, second_track_id, "target clip");
    let source_id = add_node(
        &mut project,
        NodeContainer::Clip(first_clip),
        solid_node("source"),
    );
    let reference_id = add_node(
        &mut project,
        NodeContainer::Clip(second_clip),
        Node::new(
            "reference",
            NodeContent::Reference(ReferenceContent {
                target_id: composition_id,
                sync_global_time: false,
            }),
        ),
    );

    project
        .connect_ports(
            address(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
            address(PortOwner::Node(reference_id), IMAGE_INPUT_PORT),
        )
        .unwrap();
    assert_eq!(
        project.find_node_container(source_id),
        Some(NodeContainer::Clip(first_clip))
    );
    assert_eq!(
        project.find_node_container(reference_id),
        Some(NodeContainer::Clip(second_clip))
    );

    let escaped = project.connect_ports(
        address(PortOwner::Composition(composition_id), TIME_PORT),
        address(PortOwner::Node(reference_id), TIME_PORT),
    );
    assert_eq!(
        escaped,
        Err(ProjectGraphError::InternalPortEscapesContainer {
            source_owner: PortOwner::Composition(composition_id),
            target_owner: PortOwner::Node(reference_id),
        })
    );
}

#[test]
fn single_inputs_replace_while_variadic_inputs_reorder_disconnect_and_roundtrip() {
    let (mut project, composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "clip");
    let first_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("first"),
    );
    let second_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("second"),
    );
    let reference_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new(
            "single",
            NodeContent::Reference(ReferenceContent {
                target_id: composition_id,
                sync_global_time: false,
            }),
        ),
    );
    let merge_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new("merge", NodeContent::Merge),
    );

    let single_target = address(PortOwner::Node(reference_id), IMAGE_INPUT_PORT);
    let first_single = project
        .connect_ports(
            address(PortOwner::Node(first_id), IMAGE_OUTPUT_PORT),
            single_target.clone(),
        )
        .unwrap();
    let second_single = project
        .connect_ports(
            address(PortOwner::Node(second_id), IMAGE_OUTPUT_PORT),
            single_target.clone(),
        )
        .unwrap();
    assert_ne!(first_single, second_single);
    let singles = project
        .connections
        .iter()
        .filter(|connection| connection.to == single_target)
        .collect::<Vec<_>>();
    assert_eq!(singles.len(), 1);
    assert_eq!(singles[0].from.owner, PortOwner::Node(second_id));

    let merge_target = address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
    assert_eq!(
        project
            .port_definition(&merge_target, PortDirection::Input)
            .unwrap()
            .multiplicity,
        PortMultiplicity::Variadic
    );
    let first_connection = project
        .connect_ports(
            address(PortOwner::Node(first_id), IMAGE_OUTPUT_PORT),
            merge_target.clone(),
        )
        .unwrap();
    let second_connection = project
        .connect_ports(
            address(PortOwner::Node(second_id), IMAGE_OUTPUT_PORT),
            merge_target.clone(),
        )
        .unwrap();
    assert_eq!(
        project
            .connections
            .iter()
            .filter(|connection| connection.to == merge_target)
            .count(),
        2
    );
    project.reorder_connection(second_connection, 0).unwrap();
    assert_eq!(
        project
            .connections
            .iter()
            .find(|connection| connection.id == second_connection)
            .unwrap()
            .order,
        0
    );
    assert_eq!(
        project
            .connections
            .iter()
            .find(|connection| connection.id == first_connection)
            .unwrap()
            .order,
        1
    );

    let loaded = Project::load(&project.save().unwrap()).unwrap();
    assert_eq!(loaded.connections, project.connections);
    assert!(project.disconnect_connection(second_connection));
    assert_eq!(
        project
            .connections
            .iter()
            .find(|connection| connection.id == first_connection)
            .unwrap()
            .order,
        0
    );
}

#[test]
fn image_cycles_include_connections_containment_and_explicit_outputs() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "clip");
    let first_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new("first merge", NodeContent::Merge),
    );
    let second_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new("second merge", NodeContent::Merge),
    );
    project
        .connect_ports(
            address(PortOwner::Node(first_id), IMAGE_OUTPUT_PORT),
            address(PortOwner::Node(second_id), MERGE_IMAGES_PORT),
        )
        .unwrap();
    let reverse = project.connect_ports(
        address(PortOwner::Node(second_id), IMAGE_OUTPUT_PORT),
        address(PortOwner::Node(first_id), MERGE_IMAGES_PORT),
    );
    assert_eq!(
        reverse,
        Err(ProjectGraphError::ConnectionCycle {
            from: PortOwner::Node(second_id),
            to: PortOwner::Node(first_id),
        })
    );

    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(first_id))
        .unwrap();
    let container_cycle = project.connect_ports(
        address(PortOwner::Clip(clip_id), IMAGE_OUTPUT_PORT),
        address(PortOwner::Node(first_id), MERGE_IMAGES_PORT),
    );
    assert_eq!(
        container_cycle,
        Err(ProjectGraphError::ConnectionCycle {
            from: PortOwner::Clip(clip_id),
            to: PortOwner::Node(first_id),
        })
    );
}

#[test]
fn setting_an_output_rejects_a_preexisting_reverse_edge_atomically() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "clip");
    let feedback_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new("feedback merge", NodeContent::Merge),
    );
    let valid_output_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new("valid output", NodeContent::Merge),
    );

    project
        .connect_ports(
            address(PortOwner::Clip(clip_id), IMAGE_OUTPUT_PORT),
            address(PortOwner::Node(feedback_id), MERGE_IMAGES_PORT),
        )
        .unwrap();
    let before = project.clone();

    assert_eq!(
        project.set_output_node(NodeContainer::Clip(clip_id), Some(feedback_id)),
        Err(ProjectGraphError::ConnectionCycle {
            from: PortOwner::Clip(clip_id),
            to: PortOwner::Node(feedback_id),
        })
    );
    assert_eq!(project, before, "a rejected output binding must not mutate");

    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(valid_output_id))
        .unwrap();
    assert_eq!(
        project.get_clip(clip_id).unwrap().output_node_id,
        Some(valid_output_id)
    );
    project
        .set_output_node(NodeContainer::Clip(clip_id), None)
        .unwrap();
    assert_eq!(project.get_clip(clip_id).unwrap().output_node_id, None);
}

#[test]
fn clip_is_the_only_timing_owner_and_metadata_connection_overrides_authored_property() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let mut asset = Asset::new("video", "fixture.mp4", AssetKind::Video);
    asset.fps = Some(10.0);
    let asset_id = asset.id;
    project.assets.push(asset);

    let mut clip = Clip::new("timed", 2.0, 4.0);
    clip.trim_in = OrderedFloat(1.0);
    clip.time_stretch = OrderedFloat(2.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();
    let mut node = Node::new(
        "video",
        NodeContent::Media(MediaContent {
            asset_id,
            stream_index: None,
            audio_stream_index: None,
        }),
    );
    node.properties.set(
        "opacity".into(),
        Property::constant(PropertyValue::Number(OrderedFloat(100.0))),
    );
    let node_id = add_node(&mut project, NodeContainer::Clip(clip_id), node);
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(node_id))
        .unwrap();
    project
        .connect_ports(
            address(PortOwner::Clip(clip_id), TIME_PORT),
            address(PortOwner::Node(node_id), "opacity"),
        )
        .unwrap();

    assert_eq!(
        frame(&project, 30).object_count(),
        0,
        "t=1 is before the Clip"
    );
    let rendered = frame(&project, 90); // t=3, Clip-local time = (3-2)*2+1 = 3
    let FrameContent::Video {
        source_time,
        surface,
        ..
    } = first_content(&rendered.items).unwrap()
    else {
        panic!("expected video output");
    };
    assert!((*source_time - 3.0).abs() < 1e-9);
    assert!((surface.transform.opacity - 0.03).abs() < 1e-9);
}

#[test]
fn clip_does_not_adopt_direct_image_nodes_without_an_explicit_output() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "clip");
    let first_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("first"),
    );
    let second_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("second"),
    );

    assert_eq!(
        project.get_clip(clip_id).unwrap().node_ids,
        vec![first_id, second_id]
    );
    assert_eq!(
        project
            .container_image_sources(PortOwner::Clip(clip_id))
            .iter()
            .map(|source| source.source)
            .collect::<Vec<_>>(),
        Vec::<PortOwner>::new(),
        "ordered graph membership must not choose a Clip image output"
    );
    assert_eq!(frame(&project, 0).object_count(), 0);

    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(second_id))
        .unwrap();
    let rendered = frame(&project, 0);
    let clip_group = find_group(&rendered.items, clip_id).unwrap();
    assert_eq!(clip_group.items.len(), 1);
    assert_eq!(
        match &clip_group.items[0] {
            FrameItem::Group(group) => group.source_id,
            FrameItem::Object(_) => Uuid::nil(),
        },
        second_id
    );
}

#[test]
fn direct_track_and_composition_output_nodes_keep_their_interactive_source_identity() {
    let (mut track_project, _composition_id, track_id) = project_with_composition();
    let track_node_id = add_node(
        &mut track_project,
        NodeContainer::Track(track_id),
        solid_node("direct track output"),
    );
    track_project
        .set_output_node(NodeContainer::Track(track_id), Some(track_node_id))
        .unwrap();
    assert_eq!(
        object_source_ids(&frame(&track_project, 0).items),
        vec![track_node_id]
    );

    let (mut composition_project, composition_id, _track_id) = project_with_composition();
    let composition_node_id = add_node(
        &mut composition_project,
        NodeContainer::Composition(composition_id),
        solid_node("direct composition output"),
    );
    composition_project
        .set_output_node(
            NodeContainer::Composition(composition_id),
            Some(composition_node_id),
        )
        .unwrap();
    assert_eq!(
        object_source_ids(&frame(&composition_project, 0).items),
        vec![composition_node_id]
    );
}

#[test]
fn unbound_composition_and_track_outputs_use_only_ordered_child_containers() {
    let (mut project, composition_id, first_track_id) = project_with_composition();
    let first_clip_id = add_clip(&mut project, first_track_id, "first clip");
    let second_clip_id = add_clip(&mut project, first_track_id, "second clip");
    for (clip_id, name) in [
        (first_clip_id, "first clip image"),
        (second_clip_id, "second clip image"),
    ] {
        let node_id = add_node(&mut project, NodeContainer::Clip(clip_id), solid_node(name));
        project
            .set_output_node(NodeContainer::Clip(clip_id), Some(node_id))
            .unwrap();
    }
    let direct_track_node_id = add_node(
        &mut project,
        NodeContainer::Track(first_track_id),
        solid_node("track graph helper"),
    );

    let second_track = Track::new("second track");
    let second_track_id = second_track.id;
    project.add_track(second_track);
    project
        .attach_track_to_composition(composition_id, second_track_id)
        .unwrap();
    let third_clip_id = add_clip(&mut project, second_track_id, "third clip");
    let third_clip_node_id = add_node(
        &mut project,
        NodeContainer::Clip(third_clip_id),
        solid_node("third clip image"),
    );
    project
        .set_output_node(NodeContainer::Clip(third_clip_id), Some(third_clip_node_id))
        .unwrap();
    let direct_composition_node_id = add_node(
        &mut project,
        NodeContainer::Composition(composition_id),
        solid_node("composition graph helper"),
    );

    assert_eq!(
        project
            .container_image_sources(PortOwner::Track(first_track_id))
            .into_iter()
            .map(|source| source.source)
            .collect::<Vec<_>>(),
        vec![
            PortOwner::Clip(first_clip_id),
            PortOwner::Clip(second_clip_id)
        ]
    );
    assert_eq!(
        project
            .container_image_sources(PortOwner::Composition(composition_id))
            .into_iter()
            .map(|source| source.source)
            .collect::<Vec<_>>(),
        vec![
            PortOwner::Track(first_track_id),
            PortOwner::Track(second_track_id)
        ]
    );
    assert!(
        !project
            .container_image_sources(PortOwner::Track(first_track_id))
            .iter()
            .any(|source| source.source == PortOwner::Node(direct_track_node_id))
    );
    assert!(
        !project
            .container_image_sources(PortOwner::Composition(composition_id))
            .iter()
            .any(|source| source.source == PortOwner::Node(direct_composition_node_id))
    );
}

#[test]
fn text_and_shape_require_style_before_the_clip_can_output_an_image() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "shape graph");
    let text = Node::new("Text", NodeContent::Generator(GeneratorContent::Text));
    let text_id = text.id;
    let shape = Node::new("Shape", NodeContent::Generator(GeneratorContent::Shape));
    let shape_id = shape.id;
    let style = plugin_operation_node(
        "Style",
        "style",
        "builtin.style",
        "style.apply",
        vec![
            PortDefinition::input(SHAPE_INPUT_PORT, "Shape", PortDataType::Shape),
            graph_output(IMAGE_OUTPUT_PORT, "Image", PortDataType::Image),
        ],
    );
    let style_id = style.id;
    let text_to_style = ProjectConnection::new(
        address(PortOwner::Node(text_id), SHAPE_OUTPUT_PORT),
        address(PortOwner::Node(style_id), SHAPE_INPUT_PORT),
        0,
    );
    project
        .insert_node_graph(
            NodeContainer::Clip(clip_id),
            NodeGraphBundle::new(vec![text, shape, style], vec![text_to_style], None),
        )
        .unwrap();

    for source_id in [text_id, shape_id] {
        let ports = project.port_definitions(PortOwner::Node(source_id));
        assert!(ports.iter().any(|port| {
            port.key == SHAPE_OUTPUT_PORT
                && port.direction == PortDirection::Output
                && port.data_type == PortDataType::Shape
        }));
        assert!(!ports.iter().any(|port| {
            port.key == IMAGE_OUTPUT_PORT && port.direction == PortDirection::Output
        }));
    }
    assert!(
        project
            .port_definition(
                &address(PortOwner::Node(style_id), IMAGE_OUTPUT_PORT),
                PortDirection::Output,
            )
            .is_some_and(|port| port.data_type == PortDataType::Image)
    );
    assert!(
        project
            .container_image_sources(PortOwner::Clip(clip_id))
            .is_empty()
    );
    assert_eq!(
        project.set_output_node(NodeContainer::Clip(clip_id), Some(text_id)),
        Err(ProjectGraphError::OutputNodeHasNoImagePort {
            node_id: text_id,
            container: NodeContainer::Clip(clip_id),
        })
    );

    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(style_id))
        .unwrap();
    assert_eq!(
        project
            .container_image_sources(PortOwner::Clip(clip_id))
            .into_iter()
            .map(|source| source.source)
            .collect::<Vec<_>>(),
        vec![PortOwner::Node(style_id)],
        "only the explicitly bound post-Style Image is the Clip output"
    );
}

#[test]
fn child_container_images_feeding_a_direct_parent_sink_are_not_double_composed() {
    let (mut project, composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "child source");
    let clip_node_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("clip image"),
    );
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(clip_node_id))
        .unwrap();
    let track_merge_id = add_node(
        &mut project,
        NodeContainer::Track(track_id),
        Node::new("Track Merge", NodeContent::Merge),
    );
    project
        .connect_ports(
            address(PortOwner::Clip(clip_id), IMAGE_OUTPUT_PORT),
            address(PortOwner::Node(track_merge_id), MERGE_IMAGES_PORT),
        )
        .unwrap();
    project
        .set_output_node(NodeContainer::Track(track_id), Some(track_merge_id))
        .unwrap();
    let composition_merge_id = add_node(
        &mut project,
        NodeContainer::Composition(composition_id),
        Node::new("Composition Merge", NodeContent::Merge),
    );
    project
        .connect_ports(
            address(PortOwner::Track(track_id), IMAGE_OUTPUT_PORT),
            address(PortOwner::Node(composition_merge_id), MERGE_IMAGES_PORT),
        )
        .unwrap();
    project
        .set_output_node(
            NodeContainer::Composition(composition_id),
            Some(composition_merge_id),
        )
        .unwrap();

    assert_eq!(
        project
            .container_image_sources(PortOwner::Track(track_id))
            .into_iter()
            .map(|source| source.source)
            .collect::<Vec<_>>(),
        vec![PortOwner::Node(track_merge_id)]
    );
    assert_eq!(
        project
            .container_image_sources(PortOwner::Composition(composition_id))
            .into_iter()
            .map(|source| source.source)
            .collect::<Vec<_>>(),
        vec![PortOwner::Node(composition_merge_id)]
    );
    assert!(project.validate_connections().is_empty());
}

#[test]
fn cross_container_image_consumers_do_not_hide_the_source_containers_fallback() {
    let (mut project, source_composition_id, source_track_id) = project_with_composition();
    let (target_composition, target_track) = Composition::new("target", 320, 180, 30.0, 10.0);
    let target_composition_id = target_composition.id;
    project.add_track(target_track);
    project.add_composition(target_composition);
    let target_merge_id = add_node(
        &mut project,
        NodeContainer::Composition(target_composition_id),
        Node::new("Cross-container Merge", NodeContent::Merge),
    );
    project
        .connect_ports(
            address(PortOwner::Track(source_track_id), IMAGE_OUTPUT_PORT),
            address(PortOwner::Node(target_merge_id), MERGE_IMAGES_PORT),
        )
        .unwrap();

    assert_eq!(
        project
            .container_image_sources(PortOwner::Composition(source_composition_id))
            .into_iter()
            .map(|source| source.source)
            .collect::<Vec<_>>(),
        vec![PortOwner::Track(source_track_id)]
    );
}

#[test]
fn merge_order_and_wire_blend_change_real_pixels_without_reading_source_blend() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "clip");
    let mut first = colored_solid_node(
        "first",
        Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        },
    );
    first.blend_mode = BlendMode::Overlay;
    let first_id = add_node(&mut project, NodeContainer::Clip(clip_id), first);
    let mut second = colored_solid_node(
        "second",
        Color {
            r: 0,
            g: 255,
            b: 0,
            a: 255,
        },
    );
    second.blend_mode = BlendMode::Screen;
    let second_id = add_node(&mut project, NodeContainer::Clip(clip_id), second);
    let merge_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new("merge", NodeContent::Merge),
    );
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
        .unwrap();
    let target = address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
    let first_connection = project
        .connect_ports(
            address(PortOwner::Node(first_id), IMAGE_OUTPUT_PORT),
            target.clone(),
        )
        .unwrap();
    let second_connection = project
        .connect_ports(
            address(PortOwner::Node(second_id), IMAGE_OUTPUT_PORT),
            target,
        )
        .unwrap();
    project
        .set_connection_blend_mode(second_connection, BlendMode::Multiply)
        .unwrap();

    let rendered = frame(&project, 0);
    let merge = find_group(&rendered.items, merge_id).unwrap();
    assert_eq!(merge.kind, FrameGroupKind::Merge);
    assert_eq!(merge.items.len(), 2);
    let wrappers = merge
        .items
        .iter()
        .map(|item| match item {
            FrameItem::Group(group) => (group.source_id, group.blend_mode),
            FrameItem::Object(_) => panic!("Merge inputs must be isolated present images"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        wrappers,
        vec![
            (first_connection, BlendMode::Normal),
            (second_connection, BlendMode::Multiply),
        ]
    );
    assert_eq!(object_source_ids(&merge.items), vec![first_id, second_id]);
    assert_eq!(
        center_pixel(&preview(&project)),
        [0, 0, 0, 255],
        "red followed by a green Multiply wire must render black",
    );

    project.reorder_connection(second_connection, 0).unwrap();
    let rendered = frame(&project, 0);
    let merge = find_group(&rendered.items, merge_id).unwrap();
    let wrappers = merge
        .items
        .iter()
        .map(|item| match item {
            FrameItem::Group(group) => (group.source_id, group.blend_mode),
            FrameItem::Object(_) => unreachable!(),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        wrappers,
        vec![
            (second_connection, BlendMode::Normal),
            (first_connection, BlendMode::Normal),
        ]
    );
    assert_eq!(object_source_ids(&merge.items), vec![second_id, first_id]);
    assert_eq!(
        center_pixel(&preview(&project)),
        [255, 0, 0, 255],
        "the produced green base followed by a Normal red wire must render red",
    );
}

#[test]
fn reference_materializes_an_empty_nested_composition_as_its_opaque_background() {
    let (mut project, _parent_id, parent_track_id) = project_with_composition();
    let (mut nested, nested_track) = Composition::new("empty nested", 640, 360, 24.0, 2.0);
    let nested_background = Color {
        r: 17,
        g: 34,
        b: 51,
        a: 255,
    };
    nested.background_color = nested_background.clone();
    let nested_id = nested.id;
    project.add_track(nested_track);
    project.add_composition(nested);

    let clip_id = add_clip(&mut project, parent_track_id, "reference clip");
    let reference_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new(
            "empty composition reference",
            NodeContent::Reference(ReferenceContent {
                target_id: nested_id,
                sync_global_time: false,
            }),
        ),
    );
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(reference_id))
        .unwrap();

    let rendered = frame(&project, 0);
    let nested_group = find_group(&rendered.items, nested_id).unwrap();
    assert_eq!(nested_group.kind, FrameGroupKind::Composition);
    assert_eq!((nested_group.width, nested_group.height), (640, 360));
    assert_eq!(nested_group.background_color, nested_background);
    assert!(nested_group.items.is_empty());
}

#[test]
fn merge_keeps_an_empty_nested_composition_as_a_transparent_produced_input() {
    let (mut project, parent_id, _parent_track_id) = project_with_composition();
    let (mut nested, nested_track) = Composition::new("transparent nested", 800, 450, 30.0, 2.0);
    let nested_background = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    nested.background_color = nested_background.clone();
    let nested_id = nested.id;
    project.add_track(nested_track);
    project.add_composition(nested);

    let merge_id = add_node(
        &mut project,
        NodeContainer::Composition(parent_id),
        Node::new("composition merge", NodeContent::Merge),
    );
    project
        .set_output_node(NodeContainer::Composition(parent_id), Some(merge_id))
        .unwrap();
    let connection_id = project
        .connect_ports(
            address(PortOwner::Composition(nested_id), IMAGE_OUTPUT_PORT),
            address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )
        .unwrap();

    let rendered = frame(&project, 0);
    let merge_group = find_group(&rendered.items, merge_id).unwrap();
    assert_eq!(merge_group.kind, FrameGroupKind::Merge);
    assert_eq!(merge_group.items.len(), 1);
    let connected_group = find_group(&merge_group.items, connection_id).unwrap();
    assert_eq!(connected_group.kind, FrameGroupKind::ConnectedImage);
    let nested_group = find_group(&connected_group.items, nested_id).unwrap();
    assert_eq!(nested_group.kind, FrameGroupKind::Composition);
    assert_eq!((nested_group.width, nested_group.height), (800, 450));
    assert_eq!(nested_group.background_color, nested_background);
    assert!(nested_group.items.is_empty());
}

#[test]
fn merge_skips_a_disabled_first_input_and_normalizes_the_first_produced_wire_at_runtime() {
    let (mut project, composition_id, track_id) = project_with_composition();

    let inactive_clip = Clip::new("disabled first", 0.0, 2.0);
    let inactive_clip_id = inactive_clip.id;
    project.add_clip(inactive_clip);
    project
        .attach_clip_to_track(track_id, inactive_clip_id)
        .unwrap();
    project.get_clip_mut(inactive_clip_id).unwrap().blend_mode = BlendMode::Multiply;
    let inactive_node_id = add_node(
        &mut project,
        NodeContainer::Clip(inactive_clip_id),
        solid_node("inactive source"),
    );
    project.get_node_mut(inactive_node_id).unwrap().enabled = false;
    project
        .set_output_node(
            NodeContainer::Clip(inactive_clip_id),
            Some(inactive_node_id),
        )
        .unwrap();

    let active_clip = Clip::new("active second", 0.0, 2.0);
    let active_clip_id = active_clip.id;
    project.add_clip(active_clip);
    project
        .attach_clip_to_track(track_id, active_clip_id)
        .unwrap();
    project.get_clip_mut(active_clip_id).unwrap().blend_mode = BlendMode::Overlay;
    let active_node_id = add_node(
        &mut project,
        NodeContainer::Clip(active_clip_id),
        solid_node("active source"),
    );
    project
        .set_output_node(NodeContainer::Clip(active_clip_id), Some(active_node_id))
        .unwrap();

    let merge_id = add_node(
        &mut project,
        NodeContainer::Composition(composition_id),
        Node::new("merge", NodeContent::Merge),
    );
    project
        .set_output_node(NodeContainer::Composition(composition_id), Some(merge_id))
        .unwrap();
    let target = address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
    let inactive_connection_id = project
        .connect_ports(
            address(PortOwner::Clip(inactive_clip_id), IMAGE_OUTPUT_PORT),
            target.clone(),
        )
        .unwrap();
    let active_connection_id = project
        .connect_ports(
            address(PortOwner::Clip(active_clip_id), IMAGE_OUTPUT_PORT),
            target,
        )
        .unwrap();
    project
        .set_connection_blend_mode(inactive_connection_id, BlendMode::Add)
        .unwrap();
    project
        .set_connection_blend_mode(active_connection_id, BlendMode::Screen)
        .unwrap();

    let project_before_render = project.clone();
    let serialized_connections_before = serde_json::to_value(&project.connections).unwrap();

    let rendered = frame(&project, 0);
    let merge = find_group(&rendered.items, merge_id).unwrap();
    assert_eq!(merge.items.len(), 1);
    let FrameItem::Group(active_wrapper) = &merge.items[0] else {
        panic!("a produced Merge input must be wrapped as a connected image");
    };
    assert_eq!(active_wrapper.source_id, active_connection_id);
    assert_eq!(active_wrapper.blend_mode, BlendMode::Normal);
    assert!(find_group(&active_wrapper.items, active_node_id).is_some());
    assert_eq!(object_source_ids(&merge.items), vec![active_node_id]);
    assert_eq!(project, project_before_render);
    assert_eq!(
        serde_json::to_value(&project.connections).unwrap(),
        serialized_connections_before,
        "base-layer normalization is runtime-only and must not rewrite wire blend metadata",
    );
    assert_eq!(
        project
            .connections
            .iter()
            .find(|connection| connection.id == inactive_connection_id)
            .unwrap()
            .blend_mode,
        BlendMode::Add,
    );
    assert_eq!(
        project
            .connections
            .iter()
            .find(|connection| connection.id == active_connection_id)
            .unwrap()
            .blend_mode,
        BlendMode::Screen,
    );

    // At ten seconds both Clips are inactive, so Merge and the root
    // Composition materialize as an empty background-only frame.
    assert!(frame(&project, 300).items.is_empty());
}

#[test]
fn disabled_and_out_of_range_nodes_never_expose_preview_source_identity() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip = Clip::new("short clip", 1.0, 1.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();
    let node_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("visual"),
    );
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(node_id))
        .unwrap();

    assert!(object_source_ids(&frame(&project, 0).items).is_empty());
    assert_eq!(object_source_ids(&frame(&project, 30).items), vec![node_id]);
    project.get_node_mut(node_id).unwrap().enabled = false;
    assert!(object_source_ids(&frame(&project, 30).items).is_empty());
}

#[test]
fn malformed_serialized_variadic_orders_are_reported_without_repairing_the_model() {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "clip");
    let source_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("source"),
    );
    let other_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("other"),
    );
    let merge_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        Node::new("merge", NodeContent::Merge),
    );
    let target = address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
    project.connections = vec![
        ProjectConnection::new(
            address(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
            target.clone(),
            4,
        ),
        ProjectConnection::new(
            address(PortOwner::Node(other_id), IMAGE_OUTPUT_PORT),
            target.clone(),
            4,
        ),
    ];

    assert!(
        project
            .validate_connections()
            .contains(&ProjectGraphError::DuplicateConnectionOrder { target, order: 4 })
    );
    assert_eq!(project.connections[0].order, 4);
    assert_eq!(project.connections[1].order, 4);
}
