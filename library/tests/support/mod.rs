use anyhow::{Context, Result};
use library::editor::project_service::{GeneratorNodeRequest, MediaNodeRequest, ProjectManager};
use library::model::project::{
    PortDataType, PortDefinition, PortDirection, PortExposure, PortSide, ProjectGraphError,
};
use library::model::{Node, NodeContainer, Project};
use library::plugin::PluginManager;
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
