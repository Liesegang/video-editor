use anyhow::{Context, Result};
use library::model::frame::color::Color;
use library::model::project::{
    IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeGraphBundle, PortAddress, PortOwner,
    ProjectConnection, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::model::{Clip, Composition, Node, NodeContainer, NodeContent, Project};
use library::plugin::PluginManager;
use uuid::Uuid;

use super::{FPS, HEIGHT, WIDTH, set, vec2};

pub(super) fn project_with_shape_graph(
    source: Node,
    shape_operations: Vec<Node>,
    styles: Vec<Node>,
) -> Result<(Project, Uuid)> {
    assert!(
        !styles.is_empty(),
        "Shape graphs need an explicit Style boundary"
    );
    let mut project = Project::new("creative render e2e");
    let (mut composition, track) =
        Composition::new("main", u64::from(WIDTH), u64::from(HEIGHT), FPS, 2.0);
    composition.background_color = Color::black();
    let track_id = track.id;
    let source_id = source.id;
    let clip = Clip::new("creative clip", 0.0, 2.0);
    let clip_id = clip.id;

    project
        .add_track(track)
        .expect("container structural Merge insertion must succeed");
    project
        .add_composition(composition)
        .expect("container structural Merge insertion must succeed");
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;

    let mut root_transform = PluginManager::default().create_shape_transform_operation_node()?;
    set(&mut root_transform, "position", vec2(8.0, 8.0))?;
    set(&mut root_transform, "anchor", vec2(0.0, 0.0))?;

    let mut nodes = vec![source];
    let mut connections = Vec::new();
    let mut shape_output_id = source_id;
    for operation in std::iter::once(root_transform).chain(shape_operations) {
        connections.push(ProjectConnection::new(
            PortAddress::new(PortOwner::Node(shape_output_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(operation.id), SHAPE_INPUT_PORT),
            0,
        ));
        shape_output_id = operation.id;
        nodes.push(operation);
    }

    let mut image_outputs = Vec::new();
    for style in styles {
        connections.push(ProjectConnection::new(
            PortAddress::new(PortOwner::Node(shape_output_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(style.id), SHAPE_INPUT_PORT),
            0,
        ));
        image_outputs.push(style.id);
        nodes.push(style);
    }
    let output_id = if image_outputs.len() == 1 {
        image_outputs[0]
    } else {
        let merge = Node::new_merge("Style Merge");
        let merge_id = merge.id;
        for (order, style_id) in image_outputs.into_iter().enumerate() {
            connections.push(ProjectConnection::new(
                PortAddress::new(PortOwner::Node(style_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
                order as i64,
            ));
        }
        nodes.push(merge);
        merge_id
    };
    project.insert_node_graph(
        NodeContainer::Clip(clip_id),
        NodeGraphBundle::new(nodes, connections, Some(output_id)),
    )?;
    Ok((project, source_id))
}

pub(super) fn transform_node_id(project: &Project) -> Result<Uuid> {
    project
        .nodes
        .values()
        .find_map(|node| match node.content() {
            NodeContent::PluginOperation(operation)
                if operation.category == library::plugin::TRANSFORM_CATEGORY =>
            {
                Some(node.id)
            }
            _ => None,
        })
        .context("Shape graph must contain a root Transform")
}
