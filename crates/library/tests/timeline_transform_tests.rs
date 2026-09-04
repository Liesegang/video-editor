mod support;

use anyhow::{Context, Result, anyhow, bail};
use library::editor::project_service::MediaNodeRequest;
use library::framing::get_frame_from_project;
use library::model::frame::entity::{FrameContent, FrameGroup, FrameItem};
use library::model::project::{
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, NodeContainer, PortAddress, PortOwner, Project, TIME_PORT,
};
use library::model::property::{Property, PropertyValue};
use library::model::{Asset, AssetKind, Clip, Composition, Node};
use library::plugin::{PluginManager, property_port_key};
use ordered_float::OrderedFloat;
use std::sync::Arc;
use uuid::Uuid;

use support::media_node_for_canvas;

fn add_node(project: &mut Project, container: NodeContainer, node: Node) -> Result<Uuid> {
    let id = node.id;
    project.add_node(node);
    project
        .attach_node_to_container(container, id)
        .map_err(|error| anyhow!(error))?;
    Ok(id)
}

fn frame(project: &Project, frame_number: u64) -> Result<library::model::frame::frame::FrameInfo> {
    let plugins = Arc::new(PluginManager::default());
    Ok(get_frame_from_project(
        project,
        0,
        frame_number,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        &plugins,
    )?)
}

fn find_group(items: &[FrameItem], source_id: Uuid) -> Option<&FrameGroup> {
    items.iter().find_map(|item| match item {
        FrameItem::Group(group) if group.source_id == source_id => Some(group),
        FrameItem::Group(group) => find_group(&group.items, source_id),
        FrameItem::Object(_) => None,
    })
}

fn first_content(items: &[FrameItem]) -> Option<&FrameContent> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(object) => Some(&object.content),
        FrameItem::Group(group) => first_content(&group.items),
    })
}

#[test]
fn clip_time_drives_an_explicit_image_transform_without_mutating_the_media_source() -> Result<()> {
    let mut project = Project::new("timeline transform");
    let (composition, track) = Composition::new("main", 320, 180, 30.0, 10.0);
    let track_id = track.id;
    project.add_track(track)?;
    project.add_composition(composition)?;

    let mut asset = Asset::new("video", "fixture.mp4", AssetKind::Video);
    asset.fps = Some(10.0);
    let asset_id = asset.id;
    project.assets.push(asset);

    let mut clip = Clip::new("timed", 2.0, 4.0);
    clip.trim_in = OrderedFloat(1.0);
    clip.time_stretch = OrderedFloat(2.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;

    let source = media_node_for_canvas(
        "video",
        MediaNodeRequest::Video {
            asset_id,
            file_path: "fixture.mp4".to_string(),
            stream_index: None,
            audio_stream_index: None,
        },
        320,
        180,
        320,
        180,
    );
    let source_id = add_node(&mut project, NodeContainer::Clip(clip_id), source)?;
    let mut transform = PluginManager::default().create_image_transform_operation_node()?;
    transform
        .set_property(
            "rotation".into(),
            Property::constant(PropertyValue::Number(OrderedFloat(100.0))),
        )
        .map_err(anyhow::Error::msg)?;
    let transform_id = add_node(&mut project, NodeContainer::Clip(clip_id), transform)?;
    project.connect_ports(
        PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(transform_id), IMAGE_INPUT_PORT),
    )?;
    project.set_output_node(NodeContainer::Clip(clip_id), Some(transform_id))?;
    project.connect_ports(
        PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
        PortAddress::new(PortOwner::Node(transform_id), property_port_key("rotation")),
    )?;

    assert_eq!(frame(&project, 30)?.object_count(), 0);
    let rendered = frame(&project, 90)?;
    let FrameContent::Video {
        source_time,
        surface,
        ..
    } = first_content(&rendered.items).context("video frame content must exist")?
    else {
        bail!("expected video output");
    };
    assert!((*source_time - 3.0).abs() < 1e-9);
    assert_eq!(surface.transform, Default::default());
    let group = find_group(&rendered.items, transform_id).context("Image Transform must render")?;
    assert!((group.transform.rotation - 3.0).abs() < 1e-9);
    Ok(())
}
