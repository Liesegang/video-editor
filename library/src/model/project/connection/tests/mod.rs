use super::*;
use crate::editor::project_service::{GeneratorNodeRequest, test_generator_node};
use crate::model::project::{Composition, Project, ProjectGraphError};
use crate::model::{BlendMode, Clip, CompositionInstanceContent, Node, NodeContainer, NodeContent};
use crate::plugin::PluginManager;
use uuid::Uuid;

fn add_node(project: &mut Project, container: NodeContainer, name: &str) -> Uuid {
    let node = Node::new_merge(name);
    let node_id = node.id;
    project.add_node(node);
    project
        .attach_node_to_container(container, node_id)
        .unwrap();
    node_id
}

fn add_single_image_node(project: &mut Project, container: NodeContainer, name: &str) -> Uuid {
    let mut node = PluginManager::default()
        .create_image_transform_operation_node()
        .unwrap();
    node.name = name.to_string();
    let node_id = node.id;
    project.add_node(node);
    project
        .attach_node_to_container(container, node_id)
        .unwrap();
    node_id
}

fn attach_authored_node(
    project: &mut Project,
    container: NodeContainer,
    node: Node,
) -> Result<Uuid, ProjectGraphError> {
    let node_id = node.id;
    project.add_node(node);
    project.attach_node_to_container(container, node_id)?;
    Ok(node_id)
}

fn project_with_detached_clip(name: &str, start_time: f64, duration: f64) -> (Project, Uuid) {
    let mut project = Project::new("semantic graph");
    let clip = Clip::new(name, start_time, duration);
    let clip_id = clip.id;
    project.add_clip(clip);
    (project, clip_id)
}

mod mutation;
mod outputs;
mod ports;
mod semantics;
