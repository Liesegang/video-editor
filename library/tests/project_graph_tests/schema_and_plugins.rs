use anyhow::{Context, Result, anyhow, bail};
use library::animation::EasingFunction;
use library::model::project::{
    DURATION_PORT, FPS_PORT, FRAME_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer,
    NodeGraphBundle, PortDataType, PortDefinition, PortDirection, PortOwner, Project,
    ProjectConnection, ProjectGraphError, RESOLUTION_PORT, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
    TIME_PORT,
};
use library::model::property::{Keyframe, Property, PropertyValue};
use library::model::{BlendMode, Node, NodeContent};
use ordered_float::OrderedFloat;
use uuid::Uuid;

use super::graph_support::{
    add_clip, add_node, address, frame, graph_output, plugin_operation_node,
    project_with_composition, rewrite_persisted_node, solid_node,
};

fn insert_persisted_property(node: &mut Node, key: &str, property: Property) {
    let encoded_property = serde_json::to_value(property);
    assert!(encoded_property.is_ok(), "test Property must serialize");
    let encoded_property = encoded_property.unwrap_or(serde_json::Value::Null);
    rewrite_persisted_node(node, |encoded| {
        encoded["properties"][key] = encoded_property;
    });
}

#[test]
fn direct_pre_v1_schema_roundtrips_without_version_or_legacy_node_timing() -> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "timeline placement")?;
    let node_id = add_node(
        &mut project,
        NodeContainer::Clip(clip_id),
        solid_node("leaf"),
    )?;
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(node_id))
        .map_err(|error| anyhow!(error))?;
    let json = project.save()?;
    let value: serde_json::Value = serde_json::from_str(&json)?;
    let root = value
        .as_object()
        .context("Project JSON root must be an object")?;
    assert!(!root.contains_key("schema_version"));
    assert!(!root.contains_key("migration"));
    assert!(root.contains_key("compositions"));
    assert!(root.contains_key("tracks"));
    assert!(root.contains_key("clips"));
    assert!(root.contains_key("nodes"));

    let serialized_node = value["nodes"]
        .get(node_id.to_string())
        .context("serialized Node must exist")?;
    assert!(serialized_node.get("start_time").is_none());
    assert!(serialized_node.get("duration").is_none());
    assert!(serialized_node.get("trim_in").is_none());
    assert!(serialized_node.get("time_stretch").is_none());
    assert!(
        value["clips"]
            .get(clip_id.to_string())
            .context("serialized Clip must exist")?
            .get("start_time")
            .is_some()
    );

    let loaded = Project::load(&json)?;
    assert_eq!(loaded, project);
    assert!(loaded.validate_containment().is_empty());
    Ok(())
}

#[test]
fn wire_blend_is_required_and_distinct_fanout_values_roundtrip_losslessly() -> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "fanout")?;
    let container = NodeContainer::Clip(clip_id);
    let source_id = add_node(&mut project, container, solid_node("shared source"))?;
    let first_merge_id = add_node(&mut project, container, Node::new_merge("first merge"))?;
    let second_merge_id = add_node(&mut project, container, Node::new_merge("second merge"))?;
    let source = address(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT);
    let first_wire = project.connect_ports(
        source.clone(),
        address(PortOwner::Node(first_merge_id), MERGE_IMAGES_PORT),
    )?;
    let second_wire = project.connect_ports(
        source,
        address(PortOwner::Node(second_merge_id), MERGE_IMAGES_PORT),
    )?;
    project.set_connection_blend_mode(first_wire, BlendMode::LinearDodge)?;
    project.set_connection_blend_mode(second_wire, BlendMode::Multiply)?;

    let json = project.save()?;
    let value: serde_json::Value = serde_json::from_str(&json)?;
    let serialized_connections = value["connections"]
        .as_array()
        .context("serialized connections must be an array")?;
    let serialized_blend = |id: Uuid| -> Result<&str> {
        serialized_connections
            .iter()
            .find(|connection| connection["id"] == id.to_string())
            .context("serialized connection must exist")?["blend_mode"]
            .as_str()
            .context("serialized blend mode must be a string")
    };
    assert_eq!(serialized_blend(first_wire)?, "LinearDodge");
    assert_eq!(serialized_blend(second_wire)?, "Multiply");
    assert_eq!(Project::load(&json)?, project);

    let mut missing_field = value;
    missing_field["connections"]
        .as_array_mut()
        .context("serialized connections must be an array")?
        .first_mut()
        .context("serialized connections must not be empty")?
        .as_object_mut()
        .context("serialized connection must be an object")?
        .remove("blend_mode");
    let malformed = serde_json::to_string(&missing_field)?;
    let error = match Project::load(&malformed) {
        Ok(_) => bail!("Project without wire blend mode unexpectedly loaded"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("missing field `blend_mode`"),
        "pre-v1 Projects with an old connection shape must be rejected explicitly: {error}",
    );
    Ok(())
}

#[test]
fn unknown_plugin_operation_roundtrips_identity_ports_properties_keyframes_and_wires() -> Result<()>
{
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "unknown operation graph")?;

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
    insert_persisted_property(
        &mut shape,
        "strength",
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
        .map_err(|error| anyhow!(error))?;
    assert!(project.validate_connections().is_empty());

    let json = project.save()?;
    let serialized: serde_json::Value = serde_json::from_str(&json)?;
    let operation_data = &serialized["nodes"][shape_id.to_string()]["content"]["data"];
    assert_eq!(
        operation_data["component_id"],
        serde_json::Value::String("dev.example.future-shape".to_string())
    );
    assert!(operation_data.get("plugin_id").is_none());
    let loaded = Project::load(&json)?;
    assert_eq!(loaded, project);
    let loaded_connection = loaded
        .connections
        .iter()
        .find(|connection| connection.id == connection_id)
        .context("unknown operation connection must round-trip")?;
    assert_eq!(loaded_connection.order, 7);
    let loaded_shape = loaded
        .get_node(shape_id)
        .context("loaded shape operation Node must exist")?;
    let NodeContent::PluginOperation(operation) = loaded_shape.content() else {
        bail!("unknown operation must remain a PluginOperation");
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
        loaded_shape.properties(),
        project
            .get_node(shape_id)
            .context("source shape operation Node must exist")?
            .properties()
    );
    assert!(loaded.validate_connections().is_empty());
    assert!(
        frame(&loaded, 0)?.items.is_empty(),
        "an unavailable operation must produce NoOutput instead of failing the frame"
    );
    Ok(())
}

#[test]
fn plugin_operation_declared_ports_are_required_persisted_project_data() -> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "required ports")?;
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
    project.insert_node_graph(
        NodeContainer::Clip(clip_id),
        NodeGraphBundle::new(vec![operation], Vec::new(), Some(operation_id)),
    )?;

    let mut json: serde_json::Value = serde_json::from_str(&project.save()?)?;
    json["nodes"][operation_id.to_string()]["content"]["data"]
        .as_object_mut()
        .context("serialized operation data must be an object")?
        .remove("declared_ports");
    let malformed = serde_json::to_string(&json)?;

    let error = match Project::load(&malformed) {
        Ok(_) => bail!("operation without declared ports unexpectedly loaded"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("declared_ports"));
    Ok(())
}

#[test]
fn plugin_and_leaf_nodes_expose_only_declared_or_consumed_ports() -> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "non-image helper")?;
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
    insert_persisted_property(
        &mut operation,
        "authored_but_not_connectable",
        Property::constant(PropertyValue::Boolean(true)),
    );
    let operation_id = operation.id;
    project.insert_node_graph(
        NodeContainer::Clip(clip_id),
        NodeGraphBundle::new(vec![operation], Vec::new(), None),
    )?;

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
    )?;
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
    Ok(())
}

#[test]
fn typed_shape_plugin_connections_reject_cycles() -> Result<()> {
    let (mut project, _composition_id, track_id) = project_with_composition();
    let clip_id = add_clip(&mut project, track_id, "typed cycle")?;
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
    project.insert_node_graph(
        NodeContainer::Clip(clip_id),
        NodeGraphBundle::new(vec![first, second], Vec::new(), None),
    )?;

    project.connect_ports(
        address(PortOwner::Node(first_id), SHAPE_OUTPUT_PORT),
        address(PortOwner::Node(second_id), SHAPE_INPUT_PORT),
    )?;
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
    assert!(project.connections.iter().any(|connection| {
        connection.from.owner == PortOwner::Node(first_id)
            && connection.to.owner == PortOwner::Node(second_id)
    }));
    assert!(project.validate_connections().is_empty());
    Ok(())
}
