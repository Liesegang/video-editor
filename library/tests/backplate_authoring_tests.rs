use std::sync::{Arc, RwLock};

use anyhow::{Context, Result, anyhow};
use library::cache::CacheManager;
use library::editor::project_service::ProjectManager;
use library::framing::get_frame_from_project;
use library::model::frame::Image;
use library::model::frame::color::Color;
use library::model::project::{
    Composition, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer, PortAddress, PortOwner,
    Project,
};
use library::model::{BlendMode, Clip, NodeContent};
use library::plugin::PluginManager;
use library::rendering::renderer::RenderOutput;
use library::{RenderService, SkiaRenderer};

fn preview(project: &Project, plugins: &Arc<PluginManager>) -> Result<Image> {
    let frame = get_frame_from_project(
        project,
        0,
        0,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        plugins,
    )?;
    let renderer = SkiaRenderer::new(
        frame.width as u32,
        frame.height as u32,
        frame.background_color.clone(),
        false,
        None,
        None,
    )?;
    let mut service = RenderService::new(renderer, plugins.clone(), Arc::new(CacheManager::new()));
    match service.render_from_frame_info(&frame)? {
        RenderOutput::Image(image) => Ok(image),
        RenderOutput::Texture(_) => anyhow::bail!("CPU renderer returned a texture"),
    }
}

fn center_pixel(image: &Image) -> Result<[u8; 4]> {
    let offset = ((image.height / 2 * image.width + image.width / 2) * 4) as usize;
    image
        .data
        .get(offset..offset + 4)
        .context("image has no complete RGBA center pixel")?
        .try_into()
        .map_err(anyhow::Error::from)
}

#[test]
fn outer_merge_copies_root_blend_without_rewriting_existing_fanout_wires() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let shared = Arc::new(RwLock::new(Project::new("Backplate blend preservation")));
    let manager = ProjectManager::new(shared.clone(), plugins.clone());
    let graph = manager.create_shape_graph("M 0 0 H 40 V 40 H 0 Z", 180, 100, 40, 40)?;
    let old_output_id = graph.output_node_id.context("Shape graph has no output")?;
    let shape_id = graph
        .nodes
        .iter()
        .find(|node| matches!(node.content(), NodeContent::Generator(_)))
        .context("Shape graph has no generator")?
        .id;
    let (mut composition, track) = Composition::new("main", 180, 100, 10.0, 2.0);
    composition.background_color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let track_id = track.id;
    let clip = Clip::new("shape", 0.0, 2.0);
    let clip_id = clip.id;
    let (old_connections, old_node_ids) = {
        let mut project = shared
            .write()
            .map_err(|error| anyhow!("test Project lock is poisoned: {error}"))?;
        project.add_track(track)?;
        project.add_composition(composition)?;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id)?;
        project.insert_node_graph(NodeContainer::Clip(clip_id), graph)?;
        project
            .get_node_mut(old_output_id)
            .context("old output disappeared")?
            .blend_mode = BlendMode::Multiply;
        (
            project.connections.clone(),
            project
                .get_clip(clip_id)
                .context("test Clip disappeared")?
                .node_ids
                .clone(),
        )
    };
    assert!(
        old_connections.iter().any(|candidate| {
            old_connections
                .iter()
                .filter(|connection| connection.from == candidate.from)
                .count()
                == 2
        }),
        "Shape graph must exercise a real Fill/Stroke fan-out",
    );
    let before = {
        let project = shared
            .read()
            .map_err(|error| anyhow!("test Project lock is poisoned: {error}"))?;
        preview(&project, &plugins)?
    };

    manager.add_decorator(shape_id, "backplate")?;
    let project = shared
        .read()
        .map_err(|error| anyhow!("test Project lock is poisoned: {error}"))?;
    assert_eq!(
        &project.connections[..old_connections.len()],
        old_connections
    );
    let clip = project.get_clip(clip_id).context("test Clip disappeared")?;
    assert!(clip.node_ids.starts_with(&old_node_ids));
    let merge_id = clip
        .output_node_id
        .context("Backplate Merge is not output")?;
    let foreground = project
        .connections
        .iter()
        .find(|connection| {
            connection.from == PortAddress::new(PortOwner::Node(old_output_id), IMAGE_OUTPUT_PORT)
                && connection.to == PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT)
        })
        .context("old output is not connected to new Merge")?;
    assert_eq!(foreground.order, 1);
    assert_eq!(foreground.blend_mode, BlendMode::Multiply);
    assert_eq!(
        project.get_node(old_output_id).map(|node| node.blend_mode),
        Some(BlendMode::Multiply)
    );
    let after = preview(&project, &plugins)?;
    let before_center = center_pixel(&before)?;
    assert!(before_center[0..3].iter().all(|channel| *channel > 180));
    assert_eq!(center_pixel(&after)?, [0, 0, 0, 255]);
    Ok(())
}
