use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};
use library::editor::project_service::{DEFAULT_SHAPE_PATH, ProjectManager};
use library::framing::get_frame_from_project;
use library::model::frame::color::Color;
use library::model::frame::draw_type::DrawStyle;
use library::model::frame::entity::{FrameContent, FrameItem, FrameObject};
use library::model::frame::frame::FrameInfo;
use library::model::path::{FillRule, PathContour, PathPoint, PathSegment, PathValue};
use library::model::project::connection::{DATA_VALUE_OUTPUT_PORT, DATA_VALUE_PROPERTY};
use library::model::project::{
    Composition, NodeContainer, NodeGraphBundle, PortAddress, PortOwner, Project,
    ProjectConnection, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::model::property::{ColorSpaceRef, ColorValue, Property, PropertyValue};
use library::model::{Clip, DataContent, GeneratorContent, Node, NodeContent};
use library::plugin::{PluginManager, property_port_key};

const WIDTH: u64 = 160;
const HEIGHT: u64 = 90;

fn manager(plugins: Arc<PluginManager>) -> ProjectManager {
    ProjectManager::new(
        Arc::new(RwLock::new(Project::new("canonical consumer factory"))),
        plugins,
    )
}

fn project_with_graph(graph: NodeGraphBundle) -> Result<Project> {
    let mut project = Project::new("canonical consumer graph");
    let (composition, track) = Composition::new("Main", WIDTH, HEIGHT, 30.0, 2.0);
    let track_id = track.id;
    project.add_track(track)?;
    project.add_composition(composition)?;
    let clip = Clip::new("Graph", 0.0, 2.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project.insert_node_graph(NodeContainer::Clip(clip_id), graph)?;
    Ok(project)
}

fn frame(project: &Project, plugins: &Arc<PluginManager>) -> Result<FrameInfo> {
    get_frame_from_project(
        project,
        0,
        0,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        plugins,
    )
    .context("evaluate canonical consumer graph")
}

fn objects(items: &[FrameItem]) -> Vec<&FrameObject> {
    fn collect<'a>(items: &'a [FrameItem], output: &mut Vec<&'a FrameObject>) {
        for item in items {
            match item {
                FrameItem::Object(object) => output.push(object),
                FrameItem::Group(group) => collect(&group.items, output),
            }
        }
    }
    let mut output = Vec::new();
    collect(items, &mut output);
    output
}

fn color_data(value: ColorValue) -> Result<Node> {
    let mut node = Node::new_data("Canonical Color", DataContent::Color);
    node.set_property(
        DATA_VALUE_PROPERTY.to_string(),
        Property::constant(PropertyValue::ColorValue(value)),
    )
    .map_err(anyhow::Error::msg)?;
    Ok(node)
}

fn path_data(value: PathValue) -> Result<Node> {
    let mut node = Node::new_data("Canonical Path", DataContent::Path);
    node.set_property(
        DATA_VALUE_PROPERTY.to_string(),
        Property::constant(PropertyValue::Path(value)),
    )
    .map_err(anyhow::Error::msg)?;
    Ok(node)
}

fn with_legacy_property(
    project: &Project,
    node_id: uuid::Uuid,
    key: &str,
    value: PropertyValue,
) -> Result<Project> {
    let mut json = serde_json::to_value(project)?;
    let node = json
        .get_mut("nodes")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|nodes| nodes.get_mut(&node_id.to_string()))
        .with_context(|| format!("serialized Project has no Node {node_id}"))?;
    let slot = node
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|properties| properties.get_mut(key))
        .and_then(|property| property.get_mut("properties"))
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|properties| properties.get_mut("value"))
        .with_context(|| format!("serialized Node {node_id} has no property value {key}"))?;
    *slot = serde_json::to_value(value)?;
    serde_json::from_value(json).context("load explicit pre-v1 property representation")
}

fn solid_graph(factory: &ProjectManager, value: ColorValue) -> Result<NodeGraphBundle> {
    let color = color_data(value)?;
    let solid = factory.create_solid_node(Color::white(), WIDTH, HEIGHT)?;
    let output = solid.id;
    let connection = ProjectConnection::new(
        PortAddress::new(PortOwner::Node(color.id), DATA_VALUE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(solid.id), "color"),
        0,
    );
    Ok(NodeGraphBundle::new(
        vec![color, solid],
        vec![connection],
        Some(output),
    ))
}

#[test]
fn color_value_drives_solid_through_the_project_graph() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let factory = manager(plugins.clone());
    let color = ColorValue::new(ColorSpaceRef::srgb(), [0.5, 0.25, 1.0, 0.5])?;
    let project = project_with_graph(solid_graph(&factory, color.clone())?)?;
    let rendered = frame(&project, &plugins)?;
    let rendered_objects = objects(&rendered.items);
    let object = rendered_objects
        .first()
        .context("Color -> Solid produced no FrameObject")?;
    let FrameContent::Shape { styles, .. } = &object.content else {
        anyhow::bail!("Solid did not produce Shape-backed image content");
    };
    assert_eq!(
        styles.first().map(|style| &style.style),
        Some(&DrawStyle::Fill {
            color: Color {
                r: 128,
                g: 64,
                b: 255,
                a: 128,
            },
            offset: 0.0,
        })
    );
    let stored = project
        .nodes
        .values()
        .find(|node| matches!(node.content(), NodeContent::Data(DataContent::Color)))
        .and_then(|node| node.properties().get(DATA_VALUE_PROPERTY))
        .and_then(Property::value);
    assert_eq!(stored, Some(&PropertyValue::ColorValue(color)));
    Ok(())
}

#[test]
fn unsupported_renderer_colors_produce_no_output_without_white_fallback() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let factory = manager(plugins.clone());
    for color in [
        ColorValue::new(ColorSpaceRef::srgb(), [-0.25, 2.0, 0.5, 1.0])?,
        ColorValue::new(
            ColorSpaceRef::new("scene_linear_ap1")?,
            [0.5, 0.5, 0.5, 1.0],
        )?,
    ] {
        let project = project_with_graph(solid_graph(&factory, color)?)?;
        let rendered = frame(&project, &plugins)?;
        assert!(
            objects(&rendered.items).is_empty(),
            "unsupported color crossed the u8 renderer boundary"
        );
    }
    Ok(())
}

#[test]
fn explicit_pre_v1_color_and_svg_read_adapters_remain_lossless() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let factory = manager(plugins.clone());
    let legacy_color = Color {
        r: 17,
        g: 128,
        b: 239,
        a: 64,
    };

    let solid = factory.create_solid_node(Color::white(), WIDTH, HEIGHT)?;
    let solid_id = solid.id;
    let solid_project = project_with_graph(NodeGraphBundle::with_output_node(solid))?;
    let legacy_solid = with_legacy_property(
        &solid_project,
        solid_id,
        "color",
        PropertyValue::Color(legacy_color.clone()),
    )?;
    let solid_frame = frame(&legacy_solid, &plugins)?;
    let solid_objects = objects(&solid_frame.items);
    let FrameContent::Shape { styles, .. } = &solid_objects
        .first()
        .context("legacy Solid produced no output")?
        .content
    else {
        anyhow::bail!("legacy Solid changed output type");
    };
    assert!(matches!(
        styles.first().map(|style| &style.style),
        Some(DrawStyle::Fill { color, .. }) if color == &legacy_color
    ));

    let shape = factory.create_shape_node("M 0 0 H 40 V 20 H 0 Z", WIDTH, HEIGHT, 40, 20)?;
    let fill = plugins.create_style_operation_node("fill")?;
    let graph = NodeGraphBundle::new(
        vec![shape.clone(), fill.clone()],
        vec![ProjectConnection::new(
            PortAddress::new(PortOwner::Node(shape.id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(fill.id), SHAPE_INPUT_PORT),
            0,
        )],
        Some(fill.id),
    );
    let canonical_project = project_with_graph(graph)?;
    let legacy_style = with_legacy_property(
        &canonical_project,
        fill.id,
        "color",
        PropertyValue::Color(legacy_color.clone()),
    )?;
    let legacy_style = with_legacy_property(
        &legacy_style,
        shape.id,
        "path",
        PropertyValue::String("M 2 3 Q 20 40 38 3 Z".to_string()),
    )?;
    let styled_frame = frame(&legacy_style, &plugins)?;
    let styled_objects = objects(&styled_frame.items);
    let FrameContent::Shape {
        canonical_path: Some(path),
        styles,
        ..
    } = &styled_objects
        .first()
        .context("legacy Shape -> Fill produced no output")?
        .content
    else {
        anyhow::bail!("legacy SVG was not adapted to canonical runtime geometry");
    };
    assert!(!path.contours().is_empty());
    assert!(matches!(
        styles.first().map(|style| &style.style),
        Some(DrawStyle::Fill { color, .. }) if color == &legacy_color
    ));
    Ok(())
}

#[test]
fn unsupported_colors_do_not_fall_back_inside_fill_or_stroke() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let factory = manager(plugins.clone());
    let unsupported = [
        ColorValue::new(ColorSpaceRef::srgb(), [1.25, 0.5, 0.5, 1.0])?,
        ColorValue::new(ColorSpaceRef::new("display_p3")?, [0.5, 0.5, 0.5, 1.0])?,
    ];
    for component in ["fill", "stroke"] {
        for color in &unsupported {
            let color_node = color_data(color.clone())?;
            let shape =
                factory.create_shape_node("M 0 0 H 80 V 80 H 0 Z", WIDTH, HEIGHT, 80, 80)?;
            let style = plugins.create_style_operation_node(component)?;
            let graph = NodeGraphBundle::new(
                vec![color_node.clone(), shape.clone(), style.clone()],
                vec![
                    ProjectConnection::new(
                        PortAddress::new(PortOwner::Node(shape.id), SHAPE_OUTPUT_PORT),
                        PortAddress::new(PortOwner::Node(style.id), SHAPE_INPUT_PORT),
                        0,
                    ),
                    ProjectConnection::new(
                        PortAddress::new(PortOwner::Node(color_node.id), DATA_VALUE_OUTPUT_PORT),
                        PortAddress::new(PortOwner::Node(style.id), property_port_key("color")),
                        0,
                    ),
                ],
                Some(style.id),
            );
            let project = project_with_graph(graph)?;
            let rendered = frame(&project, &plugins)?;
            assert!(
                objects(&rendered.items).is_empty(),
                "{component} silently substituted a renderer color"
            );
        }
    }
    Ok(())
}

#[test]
fn path_and_color_values_drive_shape_fill_and_stroke_consumers() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let factory = manager(plugins.clone());
    let path = PathValue::new(
        FillRule::EvenOdd,
        vec![PathContour::new(
            PathPoint::new(0.0, 0.0),
            vec![
                PathSegment::conic(PathPoint::new(40.0, 80.0), PathPoint::new(80.0, 0.0), 0.375),
                PathSegment::line(PathPoint::new(0.0, 0.0)),
            ],
            true,
        )],
    )?;
    let color = ColorValue::new(ColorSpaceRef::srgb(), [0.5, 0.25, 1.0, 1.0])?;
    let path_node = path_data(path.clone())?;
    let color_node = color_data(color)?;
    let mut graph = factory.create_shape_graph(DEFAULT_SHAPE_PATH, WIDTH, HEIGHT, 80, 80)?;
    let shape_id = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(GeneratorContent::Shape)
            )
        })
        .map(|node| node.id)
        .context("Shape graph has no Shape consumer")?;
    let fill_id = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::PluginOperation(operation) if operation.component_id == "fill"
            )
        })
        .map(|node| node.id)
        .context("Shape graph has no Fill consumer")?;
    let stroke_id = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::PluginOperation(operation) if operation.component_id == "stroke"
            )
        })
        .map(|node| node.id)
        .context("Shape graph has no Stroke consumer")?;
    graph.connections.extend([
        ProjectConnection::new(
            PortAddress::new(PortOwner::Node(path_node.id), DATA_VALUE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(shape_id), "path"),
            0,
        ),
        ProjectConnection::new(
            PortAddress::new(PortOwner::Node(color_node.id), DATA_VALUE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(fill_id), property_port_key("color")),
            0,
        ),
        ProjectConnection::new(
            PortAddress::new(PortOwner::Node(color_node.id), DATA_VALUE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(stroke_id), property_port_key("color")),
            0,
        ),
    ]);
    graph.nodes.extend([path_node, color_node]);

    let project = project_with_graph(graph)?;
    assert!(project.validate_connections().is_empty());
    let rendered = frame(&project, &plugins)?;
    let rendered_objects = objects(&rendered.items);
    assert_eq!(rendered_objects.len(), 2);
    let mut saw_fill = false;
    let mut saw_stroke = false;
    for object in rendered_objects {
        let FrameContent::Shape {
            canonical_path: Some(rendered_path),
            styles,
            ..
        } = &object.content
        else {
            anyhow::bail!("Path -> Shape -> Style dropped canonical geometry");
        };
        assert_eq!(rendered_path, &path);
        match styles.first().map(|style| &style.style) {
            Some(DrawStyle::Fill { color, .. }) => {
                saw_fill = true;
                assert_eq!(
                    *color,
                    Color {
                        r: 128,
                        g: 64,
                        b: 255,
                        a: 255
                    }
                );
            }
            Some(DrawStyle::Stroke { color, .. }) => {
                saw_stroke = true;
                assert_eq!(
                    *color,
                    Color {
                        r: 128,
                        g: 64,
                        b: 255,
                        a: 255
                    }
                );
            }
            _ => anyhow::bail!("Style branch has no Fill or Stroke"),
        }
    }
    assert!(saw_fill && saw_stroke);
    Ok(())
}
