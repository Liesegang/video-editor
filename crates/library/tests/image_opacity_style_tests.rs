use std::sync::{Arc, RwLock};

use anyhow::{Context, Result, anyhow, bail};
use library::cache::CacheManager;
use library::editor::project_service::{GeneratorNodeRequest, ProjectManager};
use library::framing::get_frame_from_project;
use library::model::frame::Image;
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameGroup, FrameGroupKind, FrameItem};
use library::model::project::{
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, NodeGraphBundle, PortAddress, PortDataType, PortDirection,
    PortOwner, ProjectConnection,
};
use library::model::property::{Property, PropertyValue};
use library::model::{Clip, Composition, NodeContainer, Project};
use library::plugin::{IMAGE_OPACITY_STYLE_COMPONENT_ID, PluginManager};
use library::rendering::renderer::RenderOutput;
use library::{RenderService, SkiaRenderer};

const WIDTH: u32 = 32;
const HEIGHT: u32 = 24;

fn project_with_opacity(opacity: f64) -> Result<(Project, Arc<PluginManager>, uuid::Uuid)> {
    let plugins = Arc::new(PluginManager::default());
    let detached = Arc::new(RwLock::new(Project::new("factory")));
    let manager = ProjectManager::new(detached, Arc::clone(&plugins));
    let source = manager.create_generator_node(
        GeneratorNodeRequest::Solid {
            color: Color {
                r: 240,
                g: 80,
                b: 20,
                a: 255,
            },
        },
        u64::from(WIDTH),
        u64::from(HEIGHT),
        u64::from(WIDTH),
        u64::from(HEIGHT),
    )?;
    let mut style = plugins.create_image_opacity_style_operation_node()?;
    style
        .set_property(
            "opacity".to_string(),
            Property::constant(PropertyValue::from(opacity)),
        )
        .map_err(|error| anyhow!(error))?;
    let source_id = source.id;
    let style_id = style.id;

    let mut project = Project::new("Image Opacity Style");
    let (mut composition, track) =
        Composition::new("main", u64::from(WIDTH), u64::from(HEIGHT), 30.0, 1.0);
    composition.background_color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let track_id = track.id;
    project.add_track(track)?;
    project.add_composition(composition)?;
    let clip = Clip::new("solid", 0.0, 1.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project.insert_node_graph(
        NodeContainer::Clip(clip_id),
        NodeGraphBundle::new(
            vec![source, style],
            vec![ProjectConnection::new(
                PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(style_id), IMAGE_INPUT_PORT),
                0,
            )],
            Some(style_id),
        ),
    )?;
    Ok((project, plugins, style_id))
}

fn find_group(items: &[FrameItem], source_id: uuid::Uuid) -> Option<&FrameGroup> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(_) => None,
        FrameItem::Group(group) if group.source_id == source_id => Some(group),
        FrameItem::Group(group) => find_group(&group.items, source_id),
    })
}

fn render(project: &Project, plugins: &Arc<PluginManager>) -> Result<Image> {
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
    let mut service =
        RenderService::new(renderer, Arc::clone(plugins), Arc::new(CacheManager::new()));
    match service.render_from_frame_info(&frame)? {
        RenderOutput::Image(image) => Ok(image),
        RenderOutput::Working(_) => bail!("unmanaged renderer returned Project pixels"),
        RenderOutput::Texture(_) => bail!("CPU renderer unexpectedly returned a texture"),
    }
}

#[test]
fn image_opacity_is_a_typed_style_and_not_a_transform_property() -> Result<()> {
    let plugins = PluginManager::default();
    let style = plugins.create_image_opacity_style_operation_node()?;
    let descriptor = plugins.operation_descriptor(
        "style",
        IMAGE_OPACITY_STYLE_COMPONENT_ID,
        "style.apply.v1",
    )?;
    assert_eq!(descriptor.properties().len(), 1);
    assert_eq!(descriptor.properties()[0].name(), "opacity");
    let image_input = descriptor
        .declared_ports()
        .iter()
        .find(|port| port.key == IMAGE_INPUT_PORT)
        .context("Image Opacity must have an Image input")?;
    assert_eq!(image_input.direction, PortDirection::Input);
    assert_eq!(image_input.data_type, PortDataType::Image);
    assert!(style.properties().get("opacity").is_some());
    for forbidden in ["position", "rotation", "scale", "anchor"] {
        assert!(style.properties().get(forbidden).is_none());
    }

    let transform = plugins.create_image_transform_operation_node()?;
    assert!(transform.properties().get("opacity").is_none());
    Ok(())
}

#[test]
fn frame_graph_applies_image_style_alpha_once_with_identity_geometry() -> Result<()> {
    let (project, plugins, style_id) = project_with_opacity(0.25)?;
    let frame = get_frame_from_project(
        &project,
        0,
        0,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        &plugins,
    )?;
    let group = find_group(&frame.items, style_id).context("Image Style group must exist")?;
    assert_eq!(group.kind, FrameGroupKind::ImageStyle);
    assert_eq!(group.transform.opacity, 0.25);
    assert_eq!(group.transform.position.x, 0.0);
    assert_eq!(group.transform.position.y, 0.0);
    assert_eq!(group.transform.scale.x, 1.0);
    assert_eq!(group.transform.scale.y, 1.0);
    assert_eq!(group.transform.rotation, 0.0);
    Ok(())
}

#[test]
fn real_renderer_reduces_straight_raster_alpha_without_moving_pixels() -> Result<()> {
    let (opaque_project, opaque_plugins, _) = project_with_opacity(1.0)?;
    let (quarter_project, quarter_plugins, _) = project_with_opacity(0.25)?;
    let opaque = render(&opaque_project, &opaque_plugins)?;
    let quarter = render(&quarter_project, &quarter_plugins)?;
    assert_eq!(
        (opaque.width, opaque.height),
        (quarter.width, quarter.height)
    );

    let opaque_pixel = &opaque.data[(opaque.data.len() / 2)..][..4];
    let quarter_pixel = &quarter.data[(quarter.data.len() / 2)..][..4];
    assert!(opaque_pixel[0] > 200 && opaque_pixel[3] > 240);
    assert!(
        quarter_pixel[..3]
            .iter()
            .zip(&opaque_pixel[..3])
            .all(|(quarter, opaque)| quarter.abs_diff(*opaque) <= 1),
        "opaque={opaque_pixel:?}, quarter={quarter_pixel:?}"
    );
    assert!((55..=75).contains(&quarter_pixel[3]), "{quarter_pixel:?}");
    Ok(())
}
