use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result, bail};
use library::cache::CacheManager;
use library::editor::project_service::ProjectManager;
use library::framing::get_frame_from_project;
use library::model::frame::Image;
use library::model::frame::color::Color;
use library::model::frame::draw_type::PathEffect;
use library::model::frame::entity::{FrameContent, FrameItem};
use library::model::frame::frame::FrameInfo;
use library::model::project::{
    NodeGraphBundle, PortAddress, PortOwner, ProjectConnection, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::model::property::{Property, PropertyValue};
use library::model::{Clip, Composition, Node, NodeContainer, Project};
use library::plugin::PluginManager;
use library::rendering::renderer::RenderOutput;
use library::{RenderService, SkiaRenderer};
use uuid::Uuid;

const WIDTH: u64 = 160;
const HEIGHT: u64 = 96;
const FPS: f64 = 10.0;
const PATH: &str = "M12 18 C45 2 82 48 145 12 L145 76 L22 76";

fn set(node: &mut Node, key: &str, value: PropertyValue) -> Result<()> {
    node.set_property(key.to_string(), Property::constant(value))
        .map_err(anyhow::Error::msg)
}

fn dash(plugins: &PluginManager) -> Result<Node> {
    let mut node = plugins.create_path_effect_operation_node("dash")?;
    set(
        &mut node,
        "intervals",
        PropertyValue::String("13 5".to_string()),
    )?;
    set(&mut node, "phase", 2.0.into())?;
    Ok(node)
}

fn trim(plugins: &PluginManager) -> Result<Node> {
    let mut node = plugins.create_path_effect_operation_node("trim")?;
    set(&mut node, "start", 0.08.into())?;
    set(&mut node, "end", 0.81.into())?;
    Ok(node)
}

fn setup_project(
    source: Node,
    path_effects: Vec<Node>,
    plugins: &Arc<PluginManager>,
) -> Result<(Project, Uuid)> {
    let mut stroke = plugins.create_style_operation_node("stroke")?;
    set(&mut stroke, "width", 5.0.into())?;
    set(&mut stroke, "color", PropertyValue::Color(Color::white()))?;
    set(
        &mut stroke,
        "dash_array",
        PropertyValue::String(String::new()),
    )?;
    let style_id = stroke.id;

    let mut nodes = vec![source];
    let mut connections = Vec::new();
    let mut upstream_id = nodes[0].id;
    for effect in path_effects {
        connections.push(ProjectConnection::new(
            PortAddress::new(PortOwner::Node(upstream_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(effect.id), SHAPE_INPUT_PORT),
            0,
        ));
        upstream_id = effect.id;
        nodes.push(effect);
    }
    connections.push(ProjectConnection::new(
        PortAddress::new(PortOwner::Node(upstream_id), SHAPE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(style_id), SHAPE_INPUT_PORT),
        0,
    ));
    nodes.push(stroke);

    let mut project = Project::new("explicit Path Effect graph");
    let (mut composition, track) = Composition::new("main", WIDTH, HEIGHT, FPS, 2.0);
    composition.background_color = Color::black();
    let track_id = track.id;
    let clip = Clip::new("path", 0.0, 2.0);
    let clip_id = clip.id;
    project.add_track(track)?;
    project.add_composition(composition)?;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project.insert_node_graph(
        NodeContainer::Clip(clip_id),
        NodeGraphBundle::new(nodes, connections, Some(style_id)),
    )?;
    Ok((project, style_id))
}

fn shape_source(plugins: Arc<PluginManager>) -> Result<Node> {
    ProjectManager::new(Arc::new(RwLock::new(Project::new("factory"))), plugins)
        .create_shape_node(PATH, WIDTH, HEIGHT, WIDTH, HEIGHT)
        .context("create Shape generator")
}

fn text_source(plugins: Arc<PluginManager>) -> Result<Node> {
    ProjectManager::new(Arc::new(RwLock::new(Project::new("factory"))), plugins)
        .create_text_node("PATH", "Arial", WIDTH, HEIGHT)
        .context("create Text generator")
}

fn evaluate(project: &Project, plugins: &Arc<PluginManager>) -> Result<FrameInfo> {
    get_frame_from_project(
        project,
        0,
        0,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        plugins,
    )
    .context("evaluate Path Effect graph")
}

fn first_content(items: &[FrameItem]) -> Option<&FrameContent> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(object) => Some(&object.content),
        FrameItem::Group(group) => first_content(&group.items),
    })
}

fn rendered_path_effects(frame: &FrameInfo) -> Result<&[PathEffect]> {
    let Some(FrameContent::Shape { path_effects, .. }) = first_content(&frame.items) else {
        bail!("Path Effect graph did not produce Shape frame content");
    };
    Ok(path_effects)
}

fn render(project: &Project, plugins: &Arc<PluginManager>) -> Result<Image> {
    let frame = evaluate(project, plugins)?;
    let renderer = SkiaRenderer::new(
        frame.width as u32,
        frame.height as u32,
        frame.background_color.clone(),
        false,
        None,
        None,
    )?;
    let mut service =
        RenderService::new(renderer, Arc::clone(plugins), Arc::new(CacheManager::new()));
    match service.render_from_frame_info(&frame)? {
        RenderOutput::Image(image) => Ok(image),
        RenderOutput::Texture(_) => bail!("CPU renderer returned a texture"),
    }
}

fn image_hash(image: &Image) -> u64 {
    image.data.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[test]
fn explicit_nodes_apply_in_wire_order_and_reorder_preserves_authored_state() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let source = shape_source(Arc::clone(&plugins))?;
    assert_eq!(
        source
            .properties()
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>(),
        ["path"]
    );
    let dash = dash(&plugins)?;
    let trim = trim(&plugins)?;
    let dash_id = dash.id;
    let trim_id = trim.id;
    let (mut project, style_id) = setup_project(source, vec![dash, trim], &plugins)?;
    let style_input = PortAddress::new(PortOwner::Node(style_id), SHAPE_INPUT_PORT);

    assert_eq!(
        project.path_effect_chain_to(&style_input)?,
        [dash_id, trim_id]
    );
    assert_eq!(
        rendered_path_effects(&evaluate(&project, &plugins)?)?,
        [
            PathEffect::Dash {
                intervals: vec![13.0, 5.0],
                phase: 2.0,
            },
            PathEffect::Trim {
                start: 0.08,
                end: 0.81,
            },
        ]
    );
    let before_hash = image_hash(&render(&project, &plugins)?);
    let authored_nodes = [dash_id, trim_id]
        .into_iter()
        .map(|node_id| (node_id, project.get_node(node_id).unwrap().clone()))
        .collect::<Vec<_>>();
    let connection_ids = project
        .connections
        .iter()
        .map(|connection| connection.id)
        .collect::<HashSet<_>>();

    project.reorder_path_effect_chain(&[trim_id, dash_id])?;

    assert_eq!(
        project.path_effect_chain_to(&style_input)?,
        [trim_id, dash_id]
    );
    for (node_id, authored) in authored_nodes {
        assert_eq!(project.get_node(node_id), Some(&authored));
    }
    assert_eq!(
        project
            .connections
            .iter()
            .map(|connection| connection.id)
            .collect::<HashSet<_>>(),
        connection_ids
    );
    assert_eq!(
        rendered_path_effects(&evaluate(&project, &plugins)?)?,
        [
            PathEffect::Trim {
                start: 0.08,
                end: 0.81,
            },
            PathEffect::Dash {
                intervals: vec![13.0, 5.0],
                phase: 2.0,
            },
        ]
    );
    assert_ne!(
        image_hash(&render(&project, &plugins)?),
        before_hash,
        "rewiring Path Effect order did not change rendered pixels"
    );

    let serialized = project.save()?;
    assert!(!serialized.contains("path_effects"));
    assert!(!serialized.contains("path_effect_intervals"));
    let loaded = Project::load(&serialized)?;
    assert_eq!(loaded, project);
    assert_eq!(
        rendered_path_effects(&evaluate(&loaded, &plugins)?)?,
        rendered_path_effects(&evaluate(&project, &plugins)?)?
    );
    Ok(())
}

#[test]
fn text_shape_reports_the_missing_outline_boundary() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let corner = plugins.create_path_effect_operation_node("corner")?;
    let corner_id = corner.id;
    let (project, _) = setup_project(text_source(Arc::clone(&plugins))?, vec![corner], &plugins)?;
    let error = get_frame_from_project(
        &project,
        0,
        0,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        &plugins,
    )
    .expect_err("Text Path Effect must not silently discard glyph semantics");
    let message = error.to_string();
    assert!(message.contains(&corner_id.to_string()), "{message}");
    assert!(message.contains("Path geometry"), "{message}");
    assert!(message.contains("outline extraction"), "{message}");
    Ok(())
}

#[test]
fn bypassed_path_effect_routes_shape_without_descriptor_or_properties() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let mut corner = plugins.create_path_effect_operation_node("corner")?;
    corner.bypassed = true;
    assert!(corner.supports_bypass());
    let corner_id = corner.id;
    let mut persisted = serde_json::to_value(corner)?;
    persisted["content"]["data"]["component_id"] =
        serde_json::Value::String("unavailable-corner".to_string());
    let corner = serde_json::from_value(persisted)?;
    let (mut project, _) =
        setup_project(shape_source(Arc::clone(&plugins))?, vec![corner], &plugins)?;

    assert!(rendered_path_effects(&evaluate(&project, &plugins)?)?.is_empty());

    project.connections.retain(|connection| {
        !(connection.to.owner == PortOwner::Node(corner_id)
            && connection.to.port == SHAPE_INPUT_PORT)
    });
    assert!(evaluate(&project, &plugins)?.items.is_empty());
    Ok(())
}

#[test]
fn disabled_path_effect_keeps_the_global_no_output_contract() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let mut corner = plugins.create_path_effect_operation_node("corner")?;
    corner.bypassed = true;
    corner.enabled = false;
    let (project, _) = setup_project(shape_source(Arc::clone(&plugins))?, vec![corner], &plugins)?;
    assert!(evaluate(&project, &plugins)?.items.is_empty());
    Ok(())
}
