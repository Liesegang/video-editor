mod support;

use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use library::editor::project_service::GeneratorNodeRequest;
use library::framing::get_frame_from_project;
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameGroup, FrameItem};
use library::model::project::{
    Composition, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer, PortAddress, PortOwner,
    Project,
};
use library::model::{BlendMode, Clip, Node};
use library::plugin::PluginManager;
use uuid::Uuid;

use support::generator_node_for_canvas;

fn project_with_clip() -> Result<(Project, Uuid)> {
    let mut project = Project::new("blend catalog");
    let (composition, track) = Composition::new("main", 320, 180, 30.0, 10.0);
    let track_id = track.id;
    project.add_track(track)?;
    project.add_composition(composition)?;
    let clip = Clip::new("blend clip", 0.0, 10.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    Ok((project, clip_id))
}

fn attach_node(project: &mut Project, clip_id: Uuid, node: Node) -> Result<Uuid> {
    let node_id = node.id;
    project.add_node(node);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
        .map_err(|error| anyhow!(error))?;
    Ok(node_id)
}

fn solid(name: &str, color: Color) -> Node {
    generator_node_for_canvas(
        name,
        GeneratorNodeRequest::Solid { color },
        320,
        180,
        320,
        180,
    )
}

fn find_group(items: &[FrameItem], source_id: Uuid) -> Option<&FrameGroup> {
    items.iter().find_map(|item| match item {
        FrameItem::Group(group) if group.source_id == source_id => Some(group),
        FrameItem::Group(group) => find_group(&group.items, source_id),
        FrameItem::Object(_) => None,
    })
}

#[test]
fn all_29_merge_connection_modes_roundtrip_in_authoritative_order() -> Result<()> {
    let (mut project, clip_id) = project_with_clip()?;
    let merge_id = attach_node(&mut project, clip_id, Node::new_merge("all modes"))?;
    let mut connection_ids = Vec::new();
    for (index, mode) in BlendMode::ALL.into_iter().enumerate() {
        let source_id = attach_node(
            &mut project,
            clip_id,
            solid(&format!("source {index}"), Color::black()),
        )?;
        let connection_id = project.connect_ports(
            PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )?;
        project.set_connection_blend_mode(connection_id, mode)?;
        connection_ids.push(connection_id);
    }

    let json = project.save()?;
    assert!(!json.contains(r#""blend_mode":"Add""#));
    let loaded = Project::load(&json)?;
    let loaded_modes = connection_ids
        .iter()
        .map(|id| {
            loaded
                .connections
                .iter()
                .find(|connection| connection.id == *id)
                .context("roundtripped Merge connection must exist")
                .map(|connection| connection.blend_mode)
        })
        .collect::<Result<Vec<_>>>()?;
    assert_eq!(loaded_modes, BlendMode::ALL);
    Ok(())
}

#[test]
fn first_produced_merge_layer_preserves_clear_and_dissolve_only() -> Result<()> {
    let (mut project, clip_id) = project_with_clip()?;
    let source_id = attach_node(
        &mut project,
        clip_id,
        solid(
            "source",
            Color {
                r: 190,
                g: 60,
                b: 20,
                a: 128,
            },
        ),
    )?;
    let merge_id = attach_node(&mut project, clip_id, Node::new_merge("merge"))?;
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(merge_id))
        .map_err(|error| anyhow!(error))?;
    let connection_id = project.connect_ports(
        PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
    )?;
    let plugins = Arc::new(PluginManager::default());

    for (authored, runtime) in [
        (BlendMode::Clear, BlendMode::Clear),
        (BlendMode::Dissolve, BlendMode::Dissolve),
        (BlendMode::LinearBurn, BlendMode::Normal),
    ] {
        project.set_connection_blend_mode(connection_id, authored)?;
        let rendered = get_frame_from_project(
            &project,
            0,
            0,
            1.0,
            None,
            &plugins.get_property_evaluators(),
            &plugins,
        )?;
        let merge = find_group(&rendered.items, merge_id).context("Merge group must render")?;
        let FrameItem::Group(wrapper) = &merge.items[0] else {
            bail!("Merge input must remain isolated");
        };
        assert_eq!(wrapper.source_id, connection_id);
        assert_eq!(wrapper.blend_mode, runtime, "authored {authored:?}");
    }
    Ok(())
}
