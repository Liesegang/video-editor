use std::sync::{Arc, RwLock};

use anyhow::{Context, Result as AnyResult, bail};
use library::LibraryError;
use library::animation::EasingFunction;
use library::editor::project_service::ProjectManager;
use library::framing::get_frame_from_project;
use library::model::frame::entity::{FrameGroup, FrameGroupKind, FrameItem, FrameObject};
use library::model::frame::runtime_shape::RuntimeShapeGeometry;
use library::model::frame::transform::{Position, Scale, Transform};
use library::model::project::{
    Composition, EvalOutput, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer,
    NodeGraphBundle, PortAddress, PortDataType, PortDirection, PortOwner, Project,
    ProjectConnection, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, TIME_PORT,
};
use library::model::property::{Keyframe, Property, PropertyValue, Vec2};
use library::model::{Clip, GeneratorContent, Node, NodeContent};
use library::plugin::{
    EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY, FrameEvaluationContext,
    IMAGE_TRANSFORM_COMPONENT_ID, PluginManager, SHAPE_TRANSFORM_COMPONENT_ID,
    STYLE_APPLY_OPERATION, STYLE_CATEGORY, TRANSFORM_APPLY_OPERATION, TRANSFORM_CATEGORY,
    property_port_key,
};
use ordered_float::OrderedFloat;
use uuid::Uuid;

const WIDTH: u64 = 320;
const HEIGHT: u64 = 180;
const FPS: f64 = 10.0;

fn operation_id(graph: &NodeGraphBundle, category: &str, component_id: &str) -> AnyResult<Uuid> {
    graph
        .nodes
        .iter()
        .find_map(|node| match node.content() {
            NodeContent::PluginOperation(operation)
                if operation.category == category && operation.component_id == component_id =>
            {
                Some(node.id)
            }
            _ => None,
        })
        .with_context(|| format!("graph has no {category}/{component_id} operation"))
}

fn generator_id(graph: &NodeGraphBundle, kind: GeneratorContent) -> AnyResult<Uuid> {
    graph
        .nodes
        .iter()
        .find_map(|node| match node.content() {
            NodeContent::Generator(actual) if *actual == kind => Some(node.id),
            _ => None,
        })
        .context("graph has no requested generator")
}

fn property_value<'a>(node: &'a Node, key: &str) -> AnyResult<&'a PropertyValue> {
    node.properties()
        .get(key)
        .and_then(Property::value)
        .with_context(|| format!("Node {} has no constant {key}", node.id))
}

fn set_property(node: &mut Node, key: &str, property: Property) -> AnyResult<()> {
    node.set_property(key.to_string(), property)
        .map_err(anyhow::Error::msg)
}

fn insert_stray_property(node: &mut Node, key: &str, property: Property) -> AnyResult<()> {
    let mut value = serde_json::to_value(&*node)?;
    value["properties"][key] = serde_json::to_value(property)?;
    *node = serde_json::from_value(value)?;
    Ok(())
}

fn project_with_graph(graph: NodeGraphBundle) -> AnyResult<Project> {
    let mut project = Project::new("Transform graph");
    let (composition, track) = Composition::new("main", WIDTH, HEIGHT, FPS, 3.0);
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let clip = Clip::new("graph", 0.0, 3.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project.insert_node_graph(NodeContainer::Clip(clip_id), graph)?;
    Ok(project)
}

fn evaluate(
    project: &Project,
    plugins: &Arc<PluginManager>,
    frame_number: u64,
) -> Result<library::model::frame::frame::FrameInfo, LibraryError> {
    get_frame_from_project(
        project,
        0,
        frame_number,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        plugins,
    )
}

fn first_object(items: &[FrameItem]) -> Option<&FrameObject> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(object) => Some(object),
        FrameItem::Group(group) => first_object(&group.items),
    })
}

fn group_by_source(items: &[FrameItem], source_id: Uuid) -> Option<&FrameGroup> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(_) => None,
        FrameItem::Group(group) if group.source_id == source_id => Some(group),
        FrameItem::Group(group) => group_by_source(&group.items, source_id),
    })
}

#[test]
fn transform_and_style_have_one_explicit_property_authority() -> AnyResult<()> {
    let plugins = PluginManager::default();
    let detached = ["position", "rotation", "scale", "anchor", "opacity"];
    for kind in ["text", "shape", "solid", "sksl", "image", "video"] {
        let definitions = plugins
            .get_entity_converter(kind)
            .with_context(|| format!("{kind} converter is missing"))?
            .get_property_definitions(WIDTH, HEIGHT, 100, 50);
        for key in detached {
            assert!(
                definitions
                    .iter()
                    .all(|definition| definition.name() != key),
                "{kind} generator still embeds {key}"
            );
        }
    }

    let descriptor = plugins.operation_descriptor(
        TRANSFORM_CATEGORY,
        SHAPE_TRANSFORM_COMPONENT_ID,
        TRANSFORM_APPLY_OPERATION,
    )?;
    assert_eq!(descriptor.label(), "Shape Transform");
    assert_eq!(
        descriptor
            .properties()
            .iter()
            .map(|definition| definition.name())
            .collect::<Vec<_>>(),
        vec!["position", "rotation", "scale", "anchor"]
    );
    assert!(
        descriptor
            .properties()
            .iter()
            .all(|definition| definition.name() != "opacity")
    );
    for (key, direction, data_type) in [
        (TIME_PORT, PortDirection::Input, PortDataType::Number),
        (SHAPE_INPUT_PORT, PortDirection::Input, PortDataType::Shape),
        (
            SHAPE_OUTPUT_PORT,
            PortDirection::Output,
            PortDataType::Shape,
        ),
    ] {
        assert!(descriptor.declared_ports().iter().any(|port| {
            port.key == key && port.direction == direction && port.data_type == data_type
        }));
    }
    for definition in descriptor.properties() {
        assert!(
            descriptor
                .declared_ports()
                .iter()
                .any(|port| port.key == property_port_key(definition.name()))
        );
    }
    let transform = plugins.create_shape_transform_operation_node()?;
    assert_eq!(transform.name, "Shape Transform");
    assert_eq!(transform.properties().iter().count(), 4);

    let image_descriptor = plugins.operation_descriptor(
        TRANSFORM_CATEGORY,
        IMAGE_TRANSFORM_COMPONENT_ID,
        TRANSFORM_APPLY_OPERATION,
    )?;
    assert_eq!(image_descriptor.label(), "Image Transform");
    assert_eq!(image_descriptor.properties().len(), 4);
    assert!(image_descriptor.declared_ports().iter().any(|port| {
        port.key == library::model::project::IMAGE_INPUT_PORT
            && port.direction == PortDirection::Input
            && port.data_type == PortDataType::Image
    }));
    assert!(image_descriptor.declared_ports().iter().any(|port| {
        port.key == IMAGE_OUTPUT_PORT
            && port.direction == PortDirection::Output
            && port.data_type == PortDataType::Image
    }));
    assert!(
        image_descriptor
            .declared_ports()
            .iter()
            .all(|port| { port.key != SHAPE_INPUT_PORT && port.key != SHAPE_OUTPUT_PORT })
    );

    for component_id in ["fill", "stroke"] {
        let style =
            plugins.operation_descriptor(STYLE_CATEGORY, component_id, STYLE_APPLY_OPERATION)?;
        assert!(
            style
                .properties()
                .iter()
                .any(|definition| definition.name() == "opacity")
        );
        assert!(style.declared_ports().iter().any(|port| {
            port.key == IMAGE_OUTPUT_PORT
                && port.direction == PortDirection::Output
                && port.data_type == PortDataType::Image
        }));
    }
    assert_eq!(
        plugins
            .operation_descriptor(EFFECTOR_CATEGORY, "transform", EFFECTOR_APPLY_OPERATION,)?
            .label(),
        "Transform Modulation"
    );
    assert_eq!(
        plugins
            .operation_descriptor(EFFECTOR_CATEGORY, "opacity", EFFECTOR_APPLY_OPERATION)?
            .label(),
        "Opacity Modulation"
    );

    Ok(())
}

#[test]
fn raster_clip_factories_wrap_neutral_sources_in_image_transform_nodes() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("raster factory"))),
        plugins,
    );
    let asset_id = Uuid::new_v4();

    let bundles = [
        manager.create_image_clip(asset_id, "image.png", 0.0, 1.0, WIDTH as u32, HEIGHT as u32)?,
        manager.create_video_clip(
            asset_id,
            "video.mp4",
            0.0,
            1.0,
            0.0,
            1.0,
            WIDTH as u32,
            HEIGHT as u32,
        )?,
        manager.create_sksl_clip(0.0, 1.0, WIDTH as u32, HEIGHT as u32)?,
    ];

    for bundle in bundles {
        assert_eq!(bundle.graph.nodes.len(), 2);
        assert_eq!(bundle.graph.connections.len(), 1);
        let source = bundle
            .graph
            .nodes
            .iter()
            .find(|node| !matches!(node.content(), NodeContent::PluginOperation(_)))
            .context("raster graph has no source")?;
        for key in ["position", "rotation", "scale", "anchor", "opacity"] {
            assert!(
                source.properties().get(key).is_none(),
                "{} source still owns {key}",
                source.name
            );
        }
        let transform_id = operation_id(
            &bundle.graph,
            TRANSFORM_CATEGORY,
            IMAGE_TRANSFORM_COMPONENT_ID,
        )?;
        assert_eq!(bundle.graph.output_node_id, Some(transform_id));
        assert!(bundle.graph.connections.iter().any(|connection| {
            connection.from == PortAddress::new(PortOwner::Node(source.id), IMAGE_OUTPUT_PORT)
                && connection.to
                    == PortAddress::new(PortOwner::Node(transform_id), IMAGE_INPUT_PORT)
        }));
        let transform = bundle
            .graph
            .nodes
            .iter()
            .find(|node| node.id == transform_id)
            .context("Image Transform is missing")?;
        assert_eq!(
            property_value(transform, "position")?,
            &library::plugin::transforms::vec2_value(160.0, 90.0)
        );
        assert_eq!(
            property_value(transform, "anchor")?,
            &library::plugin::transforms::vec2_value(160.0, 90.0)
        );
    }
    Ok(())
}

#[test]
fn raster_sources_ignore_stray_spatial_properties() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("legacy raster"))),
        plugins.clone(),
    );
    let source =
        manager.create_solid_node(library::model::frame::color::Color::white(), WIDTH, HEIGHT)?;
    let source_id = source.id;

    let mut persisted = serde_json::to_value(source)?;
    for (key, value) in [
        (
            "position",
            library::plugin::transforms::vec2_value(70.0, 40.0),
        ),
        ("anchor", library::plugin::transforms::vec2_value(10.0, 5.0)),
        (
            "scale",
            library::plugin::transforms::vec2_value(125.0, 80.0),
        ),
        ("rotation", 15.0.into()),
        ("opacity", 50.0.into()),
    ] {
        persisted["properties"][key] = serde_json::to_value(Property::constant(value))?;
    }
    let source_with_stray_properties: Node = serde_json::from_value(persisted)?;
    let project = project_with_graph(NodeGraphBundle::with_output_node(
        source_with_stray_properties,
    ))?;
    let rendered = evaluate(&project, &plugins, 0)?;
    let object = first_object(&rendered.items).context("neutral raster source is missing")?;
    assert_eq!(object.source_node_id, source_id);
    assert_eq!(object.spatial_transform_node_id, None);
    assert_eq!(object.spatial_transform.as_ref(), &Transform::default());
    Ok(())
}

#[test]
fn factories_build_centered_generator_transform_style_topologies() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(Arc::new(RwLock::new(Project::new("factory"))), plugins);

    let text = manager.create_text_graph("hello", "Arial", WIDTH, HEIGHT)?;
    let text_id = generator_id(&text, GeneratorContent::Text)?;
    let text_transform_id = operation_id(&text, TRANSFORM_CATEGORY, SHAPE_TRANSFORM_COMPONENT_ID)?;
    let text_fill_id = operation_id(&text, STYLE_CATEGORY, "fill")?;
    assert_eq!(text.output_node_id, Some(text_fill_id));
    assert_eq!(text.nodes.len(), 3);
    assert_eq!(text.connections.len(), 2);
    assert!(text.connections.iter().any(|connection| {
        connection.from == PortAddress::new(PortOwner::Node(text_id), SHAPE_OUTPUT_PORT)
            && connection.to
                == PortAddress::new(PortOwner::Node(text_transform_id), SHAPE_INPUT_PORT)
    }));
    assert!(text.connections.iter().any(|connection| {
        connection.from == PortAddress::new(PortOwner::Node(text_transform_id), SHAPE_OUTPUT_PORT)
            && connection.to == PortAddress::new(PortOwner::Node(text_fill_id), SHAPE_INPUT_PORT)
    }));
    let text_transform = text
        .nodes
        .iter()
        .find(|node| node.id == text_transform_id)
        .context("Text Transform is missing")?;
    assert_eq!(
        property_value(text_transform, "position")?,
        &library::plugin::transforms::vec2_value(160.0, 90.0)
    );
    let (text_width, text_height) =
        library::plugin::entity_converter::measure_text_size("hello", "Arial", 100.0);
    assert_eq!(
        property_value(text_transform, "anchor")?,
        &library::plugin::transforms::vec2_value(
            f64::from(text_width.trunc()) / 2.0,
            f64::from(text_height.trunc()) / 2.0,
        )
    );

    let shape = manager.create_shape_graph("M0 0 H40 V20 H0 Z", WIDTH, HEIGHT, 40, 20)?;
    let shape_id = generator_id(&shape, GeneratorContent::Shape)?;
    let shape_transform_id =
        operation_id(&shape, TRANSFORM_CATEGORY, SHAPE_TRANSFORM_COMPONENT_ID)?;
    let fill_id = operation_id(&shape, STYLE_CATEGORY, "fill")?;
    let stroke_id = operation_id(&shape, STYLE_CATEGORY, "stroke")?;
    let merge_id = shape
        .nodes
        .iter()
        .find(|node| matches!(node.content(), NodeContent::Merge))
        .context("Shape graph has no Merge")?
        .id;
    assert_eq!(shape.output_node_id, Some(merge_id));
    assert_eq!(shape.nodes.len(), 5);
    assert_eq!(shape.connections.len(), 5);
    assert!(shape.connections.iter().any(|connection| {
        connection.from == PortAddress::new(PortOwner::Node(shape_id), SHAPE_OUTPUT_PORT)
            && connection.to
                == PortAddress::new(PortOwner::Node(shape_transform_id), SHAPE_INPUT_PORT)
    }));
    for style_id in [fill_id, stroke_id] {
        assert!(shape.connections.iter().any(|connection| {
            connection.from
                == PortAddress::new(PortOwner::Node(shape_transform_id), SHAPE_OUTPUT_PORT)
                && connection.to == PortAddress::new(PortOwner::Node(style_id), SHAPE_INPUT_PORT)
        }));
    }
    let shape_transform = shape
        .nodes
        .iter()
        .find(|node| node.id == shape_transform_id)
        .context("Shape Transform is missing")?;
    assert_eq!(
        property_value(shape_transform, "position")?,
        &library::plugin::transforms::vec2_value(160.0, 90.0)
    );
    assert_eq!(
        property_value(shape_transform, "anchor")?,
        &library::plugin::transforms::vec2_value(20.0, 10.0)
    );
    Ok(())
}

#[test]
fn image_transforms_wrap_complete_image_subtrees_and_compose_as_nested_groups() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager.create_shape_graph("M0 0 H40 V20 H0 Z", WIDTH, HEIGHT, 40, 20)?;
    let generator_id = generator_id(&graph, GeneratorContent::Shape)?;
    let shape_transform_id =
        operation_id(&graph, TRANSFORM_CATEGORY, SHAPE_TRANSFORM_COMPONENT_ID)?;
    let merge_id = graph
        .output_node_id
        .context("Shape graph has no Merge output")?;
    let merge = graph
        .nodes
        .iter_mut()
        .find(|node| node.id == merge_id)
        .context("Shape graph Merge is missing")?;
    insert_stray_property(
        merge,
        "position",
        Property::constant(library::plugin::transforms::vec2_value(999.0, 999.0)),
    )?;

    let mut upstream_transform = plugins.create_image_transform_operation_node()?;
    set_property(
        &mut upstream_transform,
        "position",
        Property::constant(library::plugin::transforms::vec2_value(12.0, 8.0)),
    )?;
    let upstream_transform_id = upstream_transform.id;
    let mut downstream_transform = plugins.create_image_transform_operation_node()?;
    set_property(
        &mut downstream_transform,
        "rotation",
        Property::constant(15.0.into()),
    )?;
    let downstream_transform_id = downstream_transform.id;
    graph
        .nodes
        .extend([upstream_transform, downstream_transform]);
    graph.connections.extend([
        ProjectConnection::new(
            PortAddress::new(PortOwner::Node(merge_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(upstream_transform_id), IMAGE_INPUT_PORT),
            0,
        ),
        ProjectConnection::new(
            PortAddress::new(PortOwner::Node(upstream_transform_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(downstream_transform_id), IMAGE_INPUT_PORT),
            0,
        ),
    ]);
    graph.output_node_id = Some(downstream_transform_id);
    let project = project_with_graph(graph)?;
    let frame = evaluate(&project, &plugins, 0)?;

    let downstream = group_by_source(&frame.items, downstream_transform_id)
        .context("downstream Image Transform group is missing")?;
    assert_eq!(downstream.kind, FrameGroupKind::ImageTransform);
    assert_eq!(downstream.transform.rotation, 15.0);
    assert_eq!((downstream.width, downstream.height), (WIDTH, HEIGHT));
    let upstream = group_by_source(&downstream.items, upstream_transform_id)
        .context("upstream Image Transform group is not nested")?;
    assert_eq!(upstream.kind, FrameGroupKind::ImageTransform);
    assert_eq!(
        (upstream.transform.position.x, upstream.transform.position.y),
        (12.0, 8.0)
    );
    assert_eq!(upstream.items.len(), 1);
    let merge = group_by_source(&upstream.items, merge_id)
        .context("Image Transform discarded the complete Merge subtree")?;
    assert_eq!(merge.kind, FrameGroupKind::Merge);
    assert_eq!(merge.transform, Transform::default());

    let object = first_object(&downstream.items).context("nested image has no object")?;
    assert_eq!(object.source_node_id, generator_id);
    assert_eq!(
        object.spatial_transform_node_id,
        Some(shape_transform_id),
        "Image Transform must preserve Shape Transform provenance"
    );
    Ok(())
}

#[test]
fn root_transform_keyframes_drive_the_whole_shape_and_preview_owner() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager.create_shape_graph("M0 0 H20 V10 H0 Z", WIDTH, HEIGHT, 20, 10)?;
    let transform_id = operation_id(&graph, TRANSFORM_CATEGORY, SHAPE_TRANSFORM_COMPONENT_ID)?;
    let transform = graph
        .nodes
        .iter_mut()
        .find(|node| node.id == transform_id)
        .context("root Transform is missing")?;
    set_property(
        transform,
        "position",
        Property::keyframe(vec![
            Keyframe::new(
                0.0,
                PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(30.0),
                    y: OrderedFloat(20.0),
                }),
                EasingFunction::Linear,
            ),
            Keyframe::new(
                1.0,
                PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(70.0),
                    y: OrderedFloat(40.0),
                }),
                EasingFunction::Linear,
            ),
        ]),
    )?;
    set_property(
        transform,
        "rotation",
        Property::keyframe(vec![
            Keyframe::new(0.0, 0.0.into(), EasingFunction::Linear),
            Keyframe::new(1.0, 90.0.into(), EasingFunction::Linear),
        ]),
    )?;
    let shape_id = generator_id(&graph, GeneratorContent::Shape)?;
    let project = project_with_graph(graph)?;

    for (frame_number, expected_position, expected_rotation) in
        [(0, (30.0, 20.0), 0.0), (10, (70.0, 40.0), 90.0)]
    {
        let rendered = evaluate(&project, &plugins, frame_number)?;
        let object = first_object(&rendered.items).context("Shape frame has no object")?;
        assert_eq!(object.source_node_id, shape_id);
        assert_eq!(object.spatial_transform_node_id, Some(transform_id));
        assert_eq!(
            (
                object.spatial_transform.position.x,
                object.spatial_transform.position.y
            ),
            expected_position
        );
        assert_eq!(object.spatial_transform.rotation, expected_rotation);
        assert_eq!(
            object.spatial_transform.anchor,
            Position { x: 10.0, y: 5.0 }
        );
        assert_eq!(
            object.spatial_transform.as_ref(),
            object.content.transform()
        );
    }
    Ok(())
}

#[test]
fn transform_preserves_generator_and_text_group_identity() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let graph = manager.create_text_graph("AA\nAA", "Arial", WIDTH, HEIGHT)?;
    let text_id = generator_id(&graph, GeneratorContent::Text)?;
    let transform_id = operation_id(&graph, TRANSFORM_CATEGORY, SHAPE_TRANSFORM_COMPONENT_ID)?;
    let text_node = graph
        .nodes
        .iter()
        .find(|node| node.id == text_id)
        .context("Text generator is missing")?;
    let (composition, _) = Composition::new("main", WIDTH, HEIGHT, FPS, 3.0);
    let project = Project::new("identity");
    let evaluators = plugins.get_property_evaluators();
    let context = FrameEvaluationContext {
        project: &project,
        composition: &composition,
        property_evaluators: &evaluators,
        plugin_manager: &plugins,
        resolved_inputs: None,
    };
    let mut shape = plugins
        .get_entity_converter("text")
        .context("Text converter is missing")?
        .convert_shape(&context, text_node, 0.0)
        .context("Text converter produced no Shape")?;
    let before = match &shape.geometry {
        RuntimeShapeGeometry::Text(text) => text
            .elements
            .iter()
            .map(|element| {
                (
                    element.element_group_id,
                    element.line_group_id,
                    element.block_group_id,
                )
            })
            .collect::<Vec<_>>(),
        RuntimeShapeGeometry::Path(_) => bail!("Text converter produced Path geometry"),
    };
    shape.set_root_transform(
        transform_id,
        Transform {
            position: Position { x: 20.0, y: 30.0 },
            rotation: 15.0,
            scale: Scale { x: 1.2, y: 0.8 },
            anchor: Position { x: 5.0, y: 6.0 },
            opacity: 1.0,
        },
    )?;
    assert_eq!(shape.source_id, text_id);
    assert_eq!(shape.spatial_transform_node_id, Some(transform_id));
    let after = match &shape.geometry {
        RuntimeShapeGeometry::Text(text) => text
            .elements
            .iter()
            .map(|element| {
                (
                    element.element_group_id,
                    element.line_group_id,
                    element.block_group_id,
                )
            })
            .collect::<Vec<_>>(),
        RuntimeShapeGeometry::Path(_) => bail!("Transform replaced Text geometry"),
    };
    assert_eq!(after, before);
    Ok(())
}

#[test]
fn path_modulation_is_component_wise_and_independent_of_root_wire_order() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let make_graph = |modulation_before_root: bool| -> AnyResult<(NodeGraphBundle, Uuid, Uuid)> {
        let mut graph = manager.create_shape_graph("M0 0 H20 V10 H0 Z", WIDTH, HEIGHT, 20, 10)?;
        let shape_id = generator_id(&graph, GeneratorContent::Shape)?;
        let root_id = operation_id(&graph, TRANSFORM_CATEGORY, SHAPE_TRANSFORM_COMPONENT_ID)?;
        let mut translate = plugins.create_effector_operation_node("transform")?;
        set_property(&mut translate, "tx", Property::constant(8.0.into()))?;
        set_property(&mut translate, "ty", Property::constant(3.0.into()))?;
        set_property(&mut translate, "rotation", Property::constant(12.0.into()))?;
        set_property(&mut translate, "scale_x", Property::constant(1.25.into()))?;
        set_property(&mut translate, "scale_y", Property::constant(0.8.into()))?;
        let translate_id = translate.id;
        let mut opacity = plugins.create_effector_operation_node("opacity")?;
        set_property(&mut opacity, "opacity", Property::constant(50.0.into()))?;
        set_property(
            &mut opacity,
            "mode",
            Property::constant(PropertyValue::String("Set".into())),
        )?;
        let opacity_id = opacity.id;
        graph.nodes.extend([translate, opacity]);

        if modulation_before_root {
            let upstream = graph
                .connections
                .iter_mut()
                .find(|connection| {
                    connection.from
                        == PortAddress::new(PortOwner::Node(shape_id), SHAPE_OUTPUT_PORT)
                        && connection.to
                            == PortAddress::new(PortOwner::Node(root_id), SHAPE_INPUT_PORT)
                })
                .context("Shape-to-Transform wire is missing")?;
            upstream.to = PortAddress::new(PortOwner::Node(translate_id), SHAPE_INPUT_PORT);
            graph.connections.extend([
                ProjectConnection::new(
                    PortAddress::new(PortOwner::Node(translate_id), SHAPE_OUTPUT_PORT),
                    PortAddress::new(PortOwner::Node(opacity_id), SHAPE_INPUT_PORT),
                    0,
                ),
                ProjectConnection::new(
                    PortAddress::new(PortOwner::Node(opacity_id), SHAPE_OUTPUT_PORT),
                    PortAddress::new(PortOwner::Node(root_id), SHAPE_INPUT_PORT),
                    0,
                ),
            ]);
        } else {
            let mut style_targets = Vec::new();
            graph.connections.retain(|connection| {
                let is_root_style_wire = connection.from
                    == PortAddress::new(PortOwner::Node(root_id), SHAPE_OUTPUT_PORT)
                    && connection.to.port == SHAPE_INPUT_PORT;
                if is_root_style_wire {
                    style_targets.push(connection.to.clone());
                }
                !is_root_style_wire
            });
            assert_eq!(style_targets.len(), 2);
            graph.connections.extend([
                ProjectConnection::new(
                    PortAddress::new(PortOwner::Node(root_id), SHAPE_OUTPUT_PORT),
                    PortAddress::new(PortOwner::Node(translate_id), SHAPE_INPUT_PORT),
                    0,
                ),
                ProjectConnection::new(
                    PortAddress::new(PortOwner::Node(translate_id), SHAPE_OUTPUT_PORT),
                    PortAddress::new(PortOwner::Node(opacity_id), SHAPE_INPUT_PORT),
                    0,
                ),
            ]);
            for target in style_targets {
                graph.connections.push(ProjectConnection::new(
                    PortAddress::new(PortOwner::Node(opacity_id), SHAPE_OUTPUT_PORT),
                    target,
                    0,
                ));
            }
        }
        Ok((graph, shape_id, root_id))
    };

    let evaluate_order = |before_root| -> AnyResult<(Transform, Transform)> {
        let (graph, shape_id, root_id) = make_graph(before_root)?;
        let project = project_with_graph(graph)?;
        let rendered = evaluate(&project, &plugins, 0)?;
        let object = first_object(&rendered.items).context("Path frame has no object")?;
        assert_eq!(object.source_node_id, shape_id);
        assert_eq!(object.spatial_transform_node_id, Some(root_id));
        Ok((
            object.spatial_transform.as_ref().clone(),
            object.content.transform().clone(),
        ))
    };
    let (before_spatial, before_final) = evaluate_order(true)?;
    let (after_spatial, after_final) = evaluate_order(false)?;
    assert_eq!(before_spatial, after_spatial);
    assert_eq!(before_final, after_final);
    assert_eq!(
        (before_final.position.x, before_final.position.y),
        (168.0, 93.0)
    );
    assert_eq!(before_final.rotation, 12.0);
    assert!((before_final.scale.x - 1.25).abs() < 1e-9);
    assert!((before_final.scale.y - 0.8).abs() < 1e-6);
    assert_eq!(before_final.opacity, 0.5);
    assert_eq!(before_spatial.opacity, 1.0);
    Ok(())
}

#[test]
fn disabled_transform_is_no_output_without_poisoning_other_merge_inputs() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut first = manager.create_text_graph("disabled", "Arial", WIDTH, HEIGHT)?;
    let first_transform_id =
        operation_id(&first, TRANSFORM_CATEGORY, SHAPE_TRANSFORM_COMPONENT_ID)?;
    first
        .nodes
        .iter_mut()
        .find(|node| node.id == first_transform_id)
        .context("first Transform is missing")?
        .enabled = false;
    let first_output = first.output_node_id.context("first graph has no output")?;

    let second = manager.create_text_graph("visible", "Arial", WIDTH, HEIGHT)?;
    let second_transform_id =
        operation_id(&second, TRANSFORM_CATEGORY, SHAPE_TRANSFORM_COMPONENT_ID)?;
    let second_output = second
        .output_node_id
        .context("second graph has no output")?;
    first.nodes.extend(second.nodes);
    first.connections.extend(second.connections);
    let merge = Node::new_merge("Merge");
    let merge_id = merge.id;
    first.nodes.push(merge);
    first.connections.extend([
        ProjectConnection::new(
            PortAddress::new(PortOwner::Node(first_output), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
            0,
        ),
        ProjectConnection::new(
            PortAddress::new(PortOwner::Node(second_output), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
            1,
        ),
    ]);
    first.output_node_id = Some(merge_id);
    let mut project = project_with_graph(first)?;

    assert!(
        first_object(&evaluate(&project, &plugins, 0)?.items).is_some(),
        "a disabled optional branch must not suppress the visible Merge input"
    );
    project
        .get_node_mut(second_transform_id)
        .context("second Transform is missing")?
        .enabled = false;
    assert!(evaluate(&project, &plugins, 0)?.items.is_empty());
    Ok(())
}

#[test]
fn multiple_absolute_transforms_are_rejected_instead_of_silently_overwriting() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager.create_text_graph("double", "Arial", WIDTH, HEIGHT)?;
    let first_transform_id =
        operation_id(&graph, TRANSFORM_CATEGORY, SHAPE_TRANSFORM_COMPONENT_ID)?;
    let fill_id = operation_id(&graph, STYLE_CATEGORY, "fill")?;
    let second_transform = plugins.create_shape_transform_operation_node()?;
    let second_transform_id = second_transform.id;
    graph.nodes.push(second_transform);
    let downstream = graph
        .connections
        .iter_mut()
        .find(|connection| {
            connection.from
                == PortAddress::new(PortOwner::Node(first_transform_id), SHAPE_OUTPUT_PORT)
                && connection.to == PortAddress::new(PortOwner::Node(fill_id), SHAPE_INPUT_PORT)
        })
        .context("Transform-to-Fill wire is missing")?;
    downstream.to = PortAddress::new(PortOwner::Node(second_transform_id), SHAPE_INPUT_PORT);
    graph.connections.push(ProjectConnection::new(
        PortAddress::new(PortOwner::Node(second_transform_id), SHAPE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(fill_id), SHAPE_INPUT_PORT),
        0,
    ));
    let project = project_with_graph(graph)?;

    let error = evaluate(&project, &plugins, 0).expect_err("two root Transforms must fail");
    assert!(matches!(
        error,
        LibraryError::Validation(message)
            if message.contains("requires an affine transform stack")
                && message.contains(&first_transform_id.to_string())
                && message.contains(&second_transform_id.to_string())
    ));
    Ok(())
}

#[test]
fn unavailable_transform_identity_is_safe_no_output_and_roundtrips_losslessly() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    for (field, unavailable) in [
        ("component_id", "future-transform"),
        ("operation", "transform.apply.future"),
    ] {
        let mut graph = manager.create_text_graph("future", "Arial", WIDTH, HEIGHT)?;
        let transform_id = operation_id(&graph, TRANSFORM_CATEGORY, SHAPE_TRANSFORM_COMPONENT_ID)?;
        let index = graph
            .nodes
            .iter()
            .position(|node| node.id == transform_id)
            .context("Transform is missing")?;
        let mut persisted = serde_json::to_value(&graph.nodes[index])?;
        persisted["content"]["data"][field] = serde_json::Value::String(unavailable.into());
        graph.nodes[index] = serde_json::from_value(persisted)?;
        let project = project_with_graph(graph)?;
        assert!(
            evaluate(&project, &plugins, 0)?.items.is_empty(),
            "unavailable {field} must safely produce NoOutput"
        );
        let saved = project.save()?;
        assert_eq!(Project::load(&saved)?, project);
    }
    Ok(())
}

#[test]
fn transform_evaluator_rejects_missing_or_scalar_no_output_properties() -> AnyResult<()> {
    let plugins = PluginManager::default();
    let transform = plugins.create_shape_transform_operation_node()?;
    let (composition, _) = Composition::new("main", WIDTH, HEIGHT, FPS, 3.0);
    let project = Project::new("NoOutput");
    let evaluators = plugins.get_property_evaluators();
    let context = FrameEvaluationContext {
        project: &project,
        composition: &composition,
        property_evaluators: &evaluators,
        plugin_manager: &plugins,
        resolved_inputs: None,
    };
    assert_eq!(
        plugins.evaluate_transform_operation(
            &context,
            SHAPE_TRANSFORM_COMPONENT_ID,
            &Default::default(),
            0.0,
        ),
        EvalOutput::NoOutput
    );

    let mut resolved = library::plugin::ResolvedNodeInputs::default();
    resolved
        .properties
        .insert("position".into(), EvalOutput::NoOutput);
    let context = FrameEvaluationContext {
        project: &project,
        composition: &composition,
        property_evaluators: &evaluators,
        plugin_manager: &plugins,
        resolved_inputs: Some(&resolved),
    };
    assert_eq!(
        plugins.evaluate_transform_operation(
            &context,
            SHAPE_TRANSFORM_COMPONENT_ID,
            transform.properties(),
            0.0,
        ),
        EvalOutput::NoOutput
    );
    Ok(())
}
