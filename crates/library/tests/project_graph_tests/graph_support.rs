use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use library::editor::project_service::GeneratorNodeRequest;
use library::framing::get_frame_from_project;
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameGroup, FrameItem};
use library::model::project::{
    Composition, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer, PortAddress, PortDataType,
    PortDefinition, PortExposure, PortOwner, PortSide, Project,
};
use library::model::{Clip, Node};
use library::plugin::PluginManager;
use uuid::Uuid;

use super::support::generator_node_for_canvas;

pub(super) fn project_with_composition() -> (Project, Uuid, Uuid) {
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

pub(super) fn add_clip(project: &mut Project, track_id: Uuid, name: &str) -> Result<Uuid> {
    let clip = Clip::new(name, 0.0, 10.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    Ok(clip_id)
}

pub(super) fn solid_node(name: &str) -> Node {
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

pub(super) fn rewrite_persisted_node(node: &mut Node, update: impl FnOnce(&mut serde_json::Value)) {
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

pub(super) fn graph_output(key: &str, label: &str, data_type: PortDataType) -> PortDefinition {
    PortDefinition::output(key, label, data_type, PortSide::Right, PortExposure::Graph)
}

pub(super) fn plugin_operation_node(
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

pub(super) fn add_node(
    project: &mut Project,
    container: NodeContainer,
    node: Node,
) -> Result<Uuid> {
    let node_id = node.id;
    project.add_node(node);
    project
        .attach_node_to_container(container, node_id)
        .map_err(|error| anyhow!(error))?;
    Ok(node_id)
}

pub(super) fn address(owner: PortOwner, port: &str) -> PortAddress {
    PortAddress::new(owner, port)
}

pub(super) fn structural_merge_id(project: &Project, container: NodeContainer) -> Result<Uuid> {
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

pub(super) fn connect_source_to_structural_merge(
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

pub(super) fn bind_downstream_merge(
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

pub(super) fn frame(
    project: &Project,
    frame_number: u64,
) -> Result<library::model::frame::frame::FrameInfo> {
    frame_for_composition(project, 0, frame_number)
}

pub(super) fn frame_for_composition(
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

pub(super) fn find_group(items: &[FrameItem], source_id: Uuid) -> Option<&FrameGroup> {
    items.iter().find_map(|item| match item {
        FrameItem::Group(group) if group.source_id == source_id => Some(group),
        FrameItem::Group(group) => find_group(&group.items, source_id),
        FrameItem::Object(_) => None,
        FrameItem::Transition(transition) => {
            find_group(std::slice::from_ref(&transition.from.item), source_id)
                .or_else(|| find_group(std::slice::from_ref(&transition.to.item), source_id))
        }
    })
}

pub(super) fn object_source_ids(items: &[FrameItem]) -> Vec<Uuid> {
    fn collect(items: &[FrameItem], ids: &mut Vec<Uuid>) {
        for item in items {
            match item {
                FrameItem::Object(object) => ids.push(object.source_node_id),
                FrameItem::Group(group) => collect(&group.items, ids),
                FrameItem::Transition(transition) => {
                    collect(std::slice::from_ref(&transition.from.item), ids);
                    collect(std::slice::from_ref(&transition.to.item), ids);
                }
            }
        }
    }
    let mut ids = Vec::new();
    collect(items, &mut ids);
    ids
}
