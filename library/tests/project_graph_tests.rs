#[path = "project_graph_tests/connections_and_outputs.rs"]
mod connections_and_outputs;
#[path = "project_graph_tests/container_ports.rs"]
mod container_ports;
#[path = "project_graph_tests/graph_transactions.rs"]
mod graph_transactions;
#[path = "project_graph_tests/rendering.rs"]
mod rendering;
#[path = "project_graph_tests/reparenting.rs"]
mod reparenting;
#[path = "project_graph_tests/schema_and_plugins.rs"]
mod schema_and_plugins;
#[path = "project_graph_tests/timing_and_order.rs"]
mod timing_and_order;

mod support;

use anyhow::{Context, Result, anyhow, bail};
use std::sync::Arc;

use library::animation::EasingFunction;
use library::cache::CacheManager;
use library::editor::project_service::GeneratorNodeRequest;
use library::framing::get_frame_from_project;
use library::model::frame::Image;
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameGroup, FrameGroupKind, FrameItem};
use library::model::project::{
    AUDIO_OUTPUT_PORT, Composition, CompositionSettingsError, DURATION_PORT, FMOD_X_INPUT_PORT,
    FPS_PORT, FRAME_PORT, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT,
    NUMBER_RESULT_OUTPUT_PORT, NodeContainer, NodeGraphBundle, PortAddress, PortDataType,
    PortDefinition, PortDirection, PortExposure, PortMultiplicity, PortOwner, PortSide, Project,
    ProjectConnection, ProjectGraphError, RESOLUTION_PORT, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
    TIME_PORT,
};
use library::model::property::{Keyframe, Property, PropertyValue};
use library::model::{
    Asset, AssetKind, BlendMode, Clip, CompositionInstanceContent, Node, NodeContent, Track,
};
use library::plugin::PluginManager;
use library::rendering::renderer::RenderOutput;
use library::{RenderService, SkiaRenderer};
use ordered_float::OrderedFloat;
use uuid::Uuid;

use support::{assert_external_container_output, generator_node_for_canvas};

fn project_with_composition() -> (Project, Uuid, Uuid) {
    let mut project = Project::new("direct graph");
    let (composition, track) = Composition::new("main", 320, 180, 30.0, 10.0);
    let composition_id = composition.id;
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    (project, composition_id, track_id)
}

fn add_clip(project: &mut Project, track_id: Uuid, name: &str) -> Result<Uuid> {
    let clip = Clip::new(name, 0.0, 10.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    Ok(clip_id)
}

fn solid_node(name: &str) -> Node {
    generator_node_for_canvas(
        name,
        GeneratorNodeRequest::Solid {
            color: Color::black(),
        },
        320,
        180,
        320,
        180,
    )
}

fn colored_solid_node(name: &str, color: Color) -> Node {
    let mut node = solid_node(name);
    assert!(
        node.set_property(
            "color".to_string(),
            Property::constant(PropertyValue::Color(color)),
        )
        .is_ok(),
        "solid factory must initialize color"
    );
    node
}

fn rewrite_persisted_node(node: &mut Node, update: impl FnOnce(&mut serde_json::Value)) {
    let encoded = serde_json::to_value(&*node);
    assert!(encoded.is_ok(), "test Node must serialize");
    let mut encoded = encoded.unwrap_or(serde_json::Value::Null);
    update(&mut encoded);

    let decoded = serde_json::from_value(encoded);
    assert!(decoded.is_ok(), "mutated test Node must deserialize");
    if let Ok(decoded) = decoded {
        *node = decoded;
    }
}

fn insert_persisted_property(node: &mut Node, key: &str, property: Property) {
    let encoded_property = serde_json::to_value(property);
    assert!(encoded_property.is_ok(), "test Property must serialize");
    let encoded_property = encoded_property.unwrap_or(serde_json::Value::Null);
    rewrite_persisted_node(node, |encoded| {
        encoded["properties"][key] = encoded_property;
    });
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
    let mut node = Node::new_merge(name);
    rewrite_persisted_node(&mut node, |persisted| {
        persisted["content"] = serde_json::json!({
            "type": "PluginOperation",
            "data": {
                "category": category,
                "component_id": component_id,
                "operation": operation,
                "declared_ports": declared_ports,
            }
        });
    });
    node
}

fn add_node(project: &mut Project, container: NodeContainer, node: Node) -> Result<Uuid> {
    let node_id = node.id;
    project.add_node(node);
    project
        .attach_node_to_container(container, node_id)
        .map_err(|error| anyhow!(error))?;
    Ok(node_id)
}

fn address(owner: PortOwner, port: &str) -> PortAddress {
    PortAddress::new(owner, port)
}

fn structural_merge_id(project: &Project, container: NodeContainer) -> Result<Uuid> {
    match container {
        NodeContainer::Composition(id) => project
            .get_composition(id)
            .map(|composition| composition.structural_merge_node_id)
            .with_context(|| format!("Composition {id} must exist")),
        NodeContainer::Track(id) => project
            .get_track(id)
            .map(|track| track.structural_merge_node_id)
            .with_context(|| format!("Track {id} must exist")),
        NodeContainer::Clip(id) => bail!("Clip {id} has no structural Merge"),
    }
}

fn connect_source_to_structural_merge(
    project: &mut Project,
    container: NodeContainer,
    source: PortOwner,
) -> Result<Uuid> {
    let merge_id = structural_merge_id(project, container)?;
    Ok(project.connect_ports(
        address(source, IMAGE_OUTPUT_PORT),
        address(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
    )?)
}

fn bind_downstream_merge(
    project: &mut Project,
    container: NodeContainer,
    output_node_id: Uuid,
) -> Result<Uuid> {
    let structural_id = structural_merge_id(project, container)?;
    let connection_id = project.connect_ports(
        address(PortOwner::Node(structural_id), IMAGE_OUTPUT_PORT),
        address(PortOwner::Node(output_node_id), MERGE_IMAGES_PORT),
    )?;
    project.set_output_node(container, Some(output_node_id))?;
    Ok(connection_id)
}

fn frame(project: &Project, frame_number: u64) -> Result<library::model::frame::frame::FrameInfo> {
    frame_for_composition(project, 0, frame_number)
}

fn frame_for_composition(
    project: &Project,
    composition_index: usize,
    frame_number: u64,
) -> Result<library::model::frame::frame::FrameInfo> {
    let plugins = Arc::new(PluginManager::default());
    Ok(get_frame_from_project(
        project,
        composition_index,
        frame_number,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        &plugins,
    )?)
}

fn preview(project: &Project) -> Result<Image> {
    let plugins = Arc::new(PluginManager::default());
    let frame = get_frame_from_project(
        project,
        0,
        0,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        &plugins,
    )?;
    let renderer = SkiaRenderer::new(
        frame.width as u32,
        frame.height as u32,
        frame.background_color.clone(),
        false,
        None,
        None,
    )?;
    let mut service = RenderService::new(
        renderer,
        Arc::clone(&plugins),
        Arc::new(CacheManager::new()),
    );
    match service.render_from_frame_info(&frame)? {
        RenderOutput::Image(image) => Ok(image),
        RenderOutput::Texture(_) => bail!("CPU renderer unexpectedly returned a texture"),
    }
}

fn center_pixel(image: &Image) -> [u8; 4] {
    let index = ((image.height / 2 * image.width + image.width / 2) * 4) as usize;
    [
        image.data[index],
        image.data[index + 1],
        image.data[index + 2],
        image.data[index + 3],
    ]
}

fn container_owner(container: NodeContainer) -> PortOwner {
    match container {
        NodeContainer::Composition(id) => PortOwner::Composition(id),
        NodeContainer::Track(id) => PortOwner::Track(id),
        NodeContainer::Clip(id) => PortOwner::Clip(id),
    }
}

fn container_output(project: &Project, container: NodeContainer) -> Result<Option<Uuid>> {
    match container {
        NodeContainer::Composition(id) => Ok(project
            .get_composition(id)
            .with_context(|| format!("Composition {id} must exist"))?
            .output_node_id),
        NodeContainer::Track(id) => Ok(project
            .get_track(id)
            .with_context(|| format!("Track {id} must exist"))?
            .output_node_id),
        NodeContainer::Clip(id) => Ok(project
            .get_clip(id)
            .with_context(|| format!("Clip {id} must exist"))?
            .output_node_id),
    }
}

fn find_group(items: &[FrameItem], source_id: Uuid) -> Option<&FrameGroup> {
    items.iter().find_map(|item| match item {
        FrameItem::Group(group) if group.source_id == source_id => Some(group),
        FrameItem::Group(group) => find_group(&group.items, source_id),
        FrameItem::Object(_) => None,
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
