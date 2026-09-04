use std::sync::{Arc, RwLock};

use anyhow::{Context, Result as AnyResult, bail};
use library::editor::project_service::ProjectManager;
use library::model::frame::color::Color;
use library::model::frame::entity::FrameContent;
use library::model::project::{MERGE_IMAGES_PORT, Project};
use library::model::property::{PropertyValue, Vec2};
use library::model::{BlendMode, NodeContent};
use library::plugin::PluginManager;
use ordered_float::OrderedFloat;

use super::support::{
    HEIGHT, WIDTH, assert_alpha_inside_preview_bounds, assert_clean_straight_rgba, evaluate,
    first_object, insert_effector_chain, preview, project_with_graph, render_frame,
    root_transform_id, set_constant,
};

#[test]
fn explicit_shape_effector_style_merge_keeps_straight_alpha_and_bounds() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_shape_graph("M 0 0 H 30 V 20 H 0 Z", WIDTH, HEIGHT, 30, 20)
        .context("create Shape graph")?;
    let transform_id = root_transform_id(&graph)?;
    let transform = graph
        .nodes
        .iter_mut()
        .find(|node| node.id == transform_id)
        .context("root Transform is missing")?;
    set_constant(
        transform,
        "position",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(62.0),
            y: OrderedFloat(39.0),
        }),
    );
    set_constant(
        transform,
        "anchor",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(15.0),
            y: OrderedFloat(10.0),
        }),
    );
    set_constant(
        transform,
        "scale",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(125.0),
            y: OrderedFloat(80.0),
        }),
    );
    set_constant(transform, "rotation", 21.0.into());

    for node in &mut graph.nodes {
        let NodeContent::PluginOperation(operation) = node.content() else {
            continue;
        };
        match operation.component_id.as_str() {
            "fill" => {
                set_constant(
                    node,
                    "color",
                    PropertyValue::Color(Color {
                        r: 240,
                        g: 70,
                        b: 20,
                        a: 160,
                    }),
                );
                set_constant(node, "opacity", 0.75.into());
            }
            "stroke" => {
                set_constant(
                    node,
                    "color",
                    PropertyValue::Color(Color {
                        r: 20,
                        g: 80,
                        b: 245,
                        a: 176,
                    }),
                );
                set_constant(node, "opacity", 0.8.into());
            }
            _ => {}
        }
    }

    let mut opacity = plugins.create_effector_operation_node("opacity")?;
    set_constant(&mut opacity, "opacity", 65.0.into());
    set_constant(&mut opacity, "mode", PropertyValue::String("Set".into()));
    let chain = [opacity.id];
    graph.nodes.push(opacity);
    insert_effector_chain(&mut graph, &chain)?;
    let merge_wire = graph
        .connections
        .iter_mut()
        .find(|connection| connection.to.port == MERGE_IMAGES_PORT && connection.order == 1)
        .context("Shape factory must merge its Fill and Stroke branches")?;
    merge_wire.blend_mode = BlendMode::Screen;

    let (mut project, _) = project_with_graph(graph, 0.0, 2.0)?;
    project
        .compositions
        .first_mut()
        .context("project has no Composition")?
        .background_color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let frame = evaluate(&project, &plugins, 0)?;
    let rendered = render_frame(&frame, &plugins)?;
    assert_clean_straight_rgba(&rendered);
    assert_alpha_inside_preview_bounds(&frame, &rendered)?;

    let loaded = Project::load(&project.save()?)?;
    assert_eq!(rendered.data, preview(&loaded, &plugins, 0)?.data);
    Ok(())
}

#[test]
fn shape_variadic_effector_input_applies_single_element_transform() -> AnyResult<()> {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_shape_graph("M0 0 L20 0 L20 20 L0 20 Z", WIDTH, HEIGHT, 20, 20)
        .context("create Shape graph")?;
    let shape_id = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(library::model::GeneratorContent::Shape)
            )
        })
        .context("Shape graph has no Shape source")?
        .id;
    let root_transform_id = root_transform_id(&graph)?;
    let mut modulation = plugins.create_effector_operation_node("transform")?;
    set_constant(&mut modulation, "tx", 8.0.into());
    set_constant(&mut modulation, "ty", 3.0.into());
    let modulation_id = modulation.id;
    let mut opacity = plugins.create_effector_operation_node("opacity")?;
    set_constant(&mut opacity, "opacity", 50.0.into());
    let opacity_id = opacity.id;
    graph.nodes.extend([modulation, opacity]);
    insert_effector_chain(&mut graph, &[modulation_id, opacity_id])?;
    let (mut project, _) = project_with_graph(graph, 0.0, 2.0)?;

    let rendered = evaluate(&project, &plugins, 0)?;
    let object = first_object(&rendered.items).context("Shape frame has no object")?;
    let FrameContent::Shape { transform, .. } = &object.content else {
        bail!("Shape graph did not render Shape content");
    };
    assert_eq!(object.source_node_id, shape_id);
    assert_eq!(object.spatial_transform_node_id, Some(root_transform_id));
    assert_eq!(
        (
            object.spatial_transform.position.x,
            object.spatial_transform.position.y
        ),
        (64.0, 40.0),
        "Preview edits the root Transform, not the downstream Transform Modulation"
    );
    assert_eq!((transform.position.x, transform.position.y), (72.0, 43.0));
    assert!((transform.opacity - 0.5).abs() < f64::EPSILON);
    let before = preview(&project, &plugins, 0)?;
    set_constant(
        project
            .get_node_mut(root_transform_id)
            .context("Shape root Transform is missing")?,
        "position",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(70.0),
            y: OrderedFloat(44.0),
        }),
    );
    let moved = evaluate(&project, &plugins, 0)?;
    let moved_object = first_object(&moved.items).context("moved frame has no object")?;
    assert_eq!(
        (
            moved_object.spatial_transform.position.x,
            moved_object.spatial_transform.position.y
        ),
        (70.0, 44.0)
    );
    assert_eq!(
        (
            moved_object.content.transform().position.x,
            moved_object.content.transform().position.y
        ),
        (78.0, 47.0),
        "the unchanged Transform Modulation remains composed after the root Transform edit"
    );
    assert_ne!(
        before.data,
        preview(&project, &plugins, 0)?.data,
        "editing the root Transform must change real rendered pixels"
    );
    Ok(())
}
