use anyhow::{Context, Result};
use library::editor::project_service::{GeneratorNodeRequest, MediaNodeRequest, ProjectManager};
use library::model::project::{
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, NodeGraphBundle, PortAddress, PortDataType,
    PortDefinition, PortDirection, PortExposure, PortOwner, PortSide, ProjectConnection,
    ProjectGraphError,
};
use library::model::property::{Property, PropertyValue, Vec2};
use library::model::{Asset, Clip, Composition, Node, NodeContainer, Project};
use library::plugin::PluginManager;
use ordered_float::OrderedFloat;
use std::sync::{Arc, RwLock};

#[allow(
    dead_code,
    reason = "each integration-test crate compiles this shared helper independently"
)]
pub fn assert_external_container_output(
    ports: &[PortDefinition],
    key: &str,
    data_type: PortDataType,
) -> Result<()> {
    let output = ports
        .iter()
        .find(|port| port.key == key && port.direction == PortDirection::Output)
        .with_context(|| format!("{key} output port must exist"))?;
    assert_eq!(output.side, PortSide::Right);
    assert_eq!(output.exposure, PortExposure::External);
    assert_eq!(output.data_type, data_type);
    Ok(())
}

#[allow(
    dead_code,
    reason = "each integration-test crate compiles this shared helper independently"
)]
pub fn generator_node(name: &str, request: GeneratorNodeRequest) -> Node {
    generator_node_for_canvas(name, request, 1920, 1080, 1920, 1080)
}

pub fn generator_node_for_canvas(
    name: &str,
    request: GeneratorNodeRequest,
    canvas_width: u64,
    canvas_height: u64,
    clip_width: u64,
    clip_height: u64,
) -> Node {
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new(
            "integration test generator factory",
        ))),
        Arc::new(PluginManager::default()),
    );
    let result = manager.create_generator_node(
        request,
        canvas_width,
        canvas_height,
        clip_width,
        clip_height,
    );
    assert!(
        result.is_ok(),
        "built-in Generator converter must create a complete test Node: {result:?}"
    );
    let mut node = result.unwrap_or_else(|_| Node::new_merge("invalid Generator test fallback"));
    node.name = name.to_string();
    node
}

#[allow(
    dead_code,
    reason = "each integration-test crate compiles this shared helper independently"
)]
pub fn media_node_for_canvas(
    name: &str,
    request: MediaNodeRequest,
    canvas_width: u64,
    canvas_height: u64,
    media_width: u64,
    media_height: u64,
) -> Node {
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("integration test media factory"))),
        Arc::new(PluginManager::default()),
    );
    let result = manager.create_media_node(
        name,
        request,
        canvas_width,
        canvas_height,
        media_width,
        media_height,
    );
    assert!(
        result.is_ok(),
        "built-in Media factory must create a complete test Node: {result:?}"
    );
    result.unwrap_or_else(|_| Node::new_merge("invalid Media test fallback"))
}

#[allow(
    dead_code,
    reason = "each integration-test crate compiles this shared helper independently"
)]
pub fn media_project_with_asset(asset: Asset) -> Result<(Project, uuid::Uuid)> {
    let mut project = Project::new("embedded audio integration");
    let (composition, track) = Composition::new("main", 12, 8, 12.0, 2.0);
    let track_id = track.id;
    let asset_id = asset.id;
    let file_path = asset.path.clone();
    let media_width = u64::from(asset.width.unwrap_or(12));
    let media_height = u64::from(asset.height.unwrap_or(8));
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project.assets.push(asset);

    let clip = Clip::new("padded media clip", 0.0, 2.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    let node = media_node_for_canvas(
        "embedded audio video",
        MediaNodeRequest::Video {
            asset_id,
            file_path,
            stream_index: None,
            audio_stream_index: None,
            outputs: library::model::MediaOutputSelection::ImageAndAudio,
        },
        12,
        8,
        media_width,
        media_height,
    );
    let node_id = node.id;
    project.add_node(node);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
        .map_err(|error| anyhow::anyhow!(error))?;
    bind_av_output(&mut project, NodeContainer::Clip(clip_id), node_id)
        .map_err(|error| anyhow::anyhow!(error))?;
    Ok((project, asset_id))
}

#[allow(
    dead_code,
    reason = "each integration-test crate compiles this shared helper independently"
)]
pub fn transformed_image_graph(
    plugin_manager: &PluginManager,
    source: Node,
    position: [f64; 2],
    anchor: [f64; 2],
) -> Result<(NodeGraphBundle, uuid::Uuid)> {
    let source_id = source.id;
    let mut transform = plugin_manager.create_image_transform_operation_node()?;
    for (key, value) in [("position", position), ("anchor", anchor)] {
        transform
            .set_property(
                key.to_string(),
                Property::constant(PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(value[0]),
                    y: OrderedFloat(value[1]),
                })),
            )
            .map_err(anyhow::Error::msg)?;
    }
    let transform_id = transform.id;
    Ok((
        NodeGraphBundle::new(
            vec![source, transform],
            vec![ProjectConnection::new(
                PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(transform_id), IMAGE_INPUT_PORT),
                0,
            )],
            Some(transform_id),
        ),
        transform_id,
    ))
}

#[allow(
    dead_code,
    reason = "each integration-test crate compiles this shared helper independently"
)]
pub fn attach_audio_output(
    project: &mut Project,
    container: NodeContainer,
    node_id: uuid::Uuid,
) -> Result<(), ProjectGraphError> {
    project.attach_node_to_container(container, node_id)?;
    project.set_audio_output_node(container, Some(node_id))
}

#[allow(
    dead_code,
    reason = "each integration-test crate compiles this shared helper independently"
)]
pub fn bind_av_output(
    project: &mut Project,
    container: NodeContainer,
    node_id: uuid::Uuid,
) -> Result<(), ProjectGraphError> {
    project.set_output_node(container, Some(node_id))?;
    project.set_audio_output_node(container, Some(node_id))
}

#[allow(
    dead_code,
    reason = "each integration-test crate compiles this shared helper independently"
)]
pub fn channel_energy(samples: &[f32], channel: usize) -> f32 {
    samples
        .chunks_exact(2)
        .map(|frame| frame[channel] * frame[channel])
        .sum::<f32>()
        / (samples.len() / 2).max(1) as f32
}

#[allow(
    dead_code,
    reason = "each integration-test crate compiles this shared helper independently"
)]
pub fn positive_zero_crossings(samples: &[f32], channel: usize) -> usize {
    samples
        .chunks_exact(2)
        .map(|frame| frame[channel])
        .zip(samples.chunks_exact(2).skip(1).map(|frame| frame[channel]))
        .filter(|(before, after)| *before <= 0.0 && *after > 0.0)
        .count()
}
