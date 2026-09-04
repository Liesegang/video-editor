use std::sync::Arc;

use anyhow::{Context, Result, Result as AnyResult, bail, ensure};
use library::cache::CacheManager;
use library::framing::get_frame_from_project;
use library::model::frame::Image;
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameContent, FrameItem};
use library::model::frame::frame::FrameInfo;
use library::model::project::{
    Composition, NodeContainer, NodeGraphBundle, PortAddress, PortDataType, PortDirection,
    PortOwner, Project, ProjectConnection, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::model::property::{Property, PropertyValue};
use library::model::{Clip, Node, NodeContent};
use library::plugin::{PluginManager, TRANSFORM_CATEGORY};
use library::rendering::renderer::{Affine2D, RenderOutput};
use library::{RenderService, SkiaRenderer};
use uuid::Uuid;

pub(super) const WIDTH: u64 = 128;
pub(super) const HEIGHT: u64 = 80;
pub(super) const FPS: f64 = 10.0;

pub(super) fn set_constant(node: &mut Node, key: &str, value: PropertyValue) {
    assert!(
        node.set_property(key.to_string(), Property::constant(value))
            .is_ok(),
        "operation descriptor must initialize {key}"
    );
}

pub(super) fn setup_project() -> (Project, Uuid, Uuid) {
    let mut project = Project::new("effector graph");
    let (mut composition, track) = Composition::new("main", WIDTH, HEIGHT, FPS, 10.0);
    composition.background_color = Color::black();
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

pub(super) fn project_with_graph(
    graph: NodeGraphBundle,
    start_time: f64,
    duration: f64,
) -> AnyResult<(Project, Uuid)> {
    let (mut project, _composition_id, track_id) = setup_project();
    let clip = Clip::new("effector clip", start_time, duration);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project
        .insert_node_graph(NodeContainer::Clip(clip_id), graph)
        .context("insert Effector graph into Clip")?;
    Ok((project, clip_id))
}

pub(super) fn evaluate_result(
    project: &Project,
    plugins: &Arc<PluginManager>,
    frame_number: u64,
) -> Result<FrameInfo, library::LibraryError> {
    get_frame_from_project(
        project,
        0,
        frame_number,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        plugins,
    )
}

pub(super) fn evaluate(
    project: &Project,
    plugins: &Arc<PluginManager>,
    frame_number: u64,
) -> AnyResult<FrameInfo> {
    evaluate_result(project, plugins, frame_number).context("evaluate Effector graph frame")
}

pub(super) fn preview(
    project: &Project,
    plugins: &Arc<PluginManager>,
    frame_number: u64,
) -> AnyResult<Image> {
    let frame = evaluate(project, plugins, frame_number)?;
    render_frame(&frame, plugins)
}

pub(super) fn render_frame(frame: &FrameInfo, plugins: &Arc<PluginManager>) -> AnyResult<Image> {
    let renderer = SkiaRenderer::new(
        frame.width as u32,
        frame.height as u32,
        frame.background_color.clone(),
        false,
        None,
        None,
    )
    .context("create CPU renderer")?;
    let mut service = RenderService::new(renderer, plugins.clone(), Arc::new(CacheManager::new()));
    match service.render_from_frame_info(frame)? {
        RenderOutput::Image(image) => Ok(image),
        RenderOutput::Working(_) => bail!("unmanaged renderer returned Project pixels"),
        RenderOutput::Texture(_) => bail!("CPU renderer unexpectedly returned a texture"),
    }
}

pub(super) fn first_content(items: &[FrameItem]) -> Option<&FrameContent> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(object) => Some(&object.content),
        FrameItem::Group(group) => first_content(&group.items),
    })
}

pub(super) fn first_object(
    items: &[FrameItem],
) -> Option<&library::model::frame::entity::FrameObject> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(object) => Some(object),
        FrameItem::Group(group) => first_object(&group.items),
    })
}

pub(super) fn group_effect_time(items: &[FrameItem], source_id: Uuid) -> Option<f64> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(_) => None,
        FrameItem::Group(group) => (group.source_id == source_id)
            .then(|| group.effect_time.into_inner())
            .or_else(|| group_effect_time(&group.items, source_id)),
    })
}

pub(super) fn collect_projected_bounds(
    items: &[FrameItem],
    parent: Affine2D,
    bounds: &mut Option<(f64, f64, f64, f64)>,
) -> AnyResult<()> {
    for item in items {
        match item {
            FrameItem::Object(object) => {
                let local = object.content_bounds.with_context(|| {
                    format!(
                        "rendered object {} omitted Preview bounds",
                        object.source_node_id
                    )
                })?;
                let transform = parent.compose(Affine2D::from(object.content.transform()));
                let (x, y, width, height) = local.as_tuple();
                for (local_x, local_y) in [
                    (x, y),
                    (x + width, y),
                    (x + width, y + height),
                    (x, y + height),
                ] {
                    let (mapped_x, mapped_y) =
                        transform.map_point(f64::from(local_x), f64::from(local_y));
                    *bounds = Some(bounds.map_or(
                        (mapped_x, mapped_y, mapped_x, mapped_y),
                        |(left, top, right, bottom)| {
                            (
                                left.min(mapped_x),
                                top.min(mapped_y),
                                right.max(mapped_x),
                                bottom.max(mapped_y),
                            )
                        },
                    ));
                }
            }
            FrameItem::Group(group) => collect_projected_bounds(
                &group.items,
                parent.compose(Affine2D::from(&group.transform)),
                bounds,
            )?,
        }
    }
    Ok(())
}

pub(super) fn alpha_bounds(image: &Image) -> Option<(f64, f64, f64, f64)> {
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    for (index, pixel) in image.data.chunks_exact(4).enumerate() {
        if pixel[3] == 0 {
            continue;
        }
        let x = (index % image.width as usize) as f64;
        let y = (index / image.width as usize) as f64;
        bounds = Some(bounds.map_or((x, y, x + 1.0, y + 1.0), |current| {
            (
                current.0.min(x),
                current.1.min(y),
                current.2.max(x + 1.0),
                current.3.max(y + 1.0),
            )
        }));
    }
    bounds
}

pub(super) fn assert_alpha_inside_preview_bounds(
    frame: &FrameInfo,
    image: &Image,
) -> AnyResult<()> {
    let mut preview = None;
    collect_projected_bounds(&frame.items, Affine2D::IDENTITY, &mut preview)?;
    let preview = preview.context("frame must expose evaluated Preview bounds")?;
    let alpha = alpha_bounds(image).context("fixture must render non-transparent pixels")?;
    assert!(
        alpha.0 >= preview.0 && alpha.1 >= preview.1,
        "alpha starts outside Preview bounds: alpha={alpha:?}, preview={preview:?}"
    );
    assert!(
        alpha.2 <= preview.2 && alpha.3 <= preview.3,
        "alpha ends outside Preview bounds: alpha={alpha:?}, preview={preview:?}"
    );
    assert!(
        preview.2 - preview.0 < frame.width as f64 && preview.3 - preview.1 < frame.height as f64,
        "regression must not pass through a full-composition fallback: {preview:?}"
    );
    Ok(())
}

pub(super) fn assert_clean_straight_rgba(image: &Image) {
    let mut visible = 0_usize;
    let mut straight_partial = false;
    for pixel in image.data.chunks_exact(4) {
        if pixel[3] == 0 {
            assert_eq!(pixel, &[0, 0, 0, 0], "transparent RGB carried color");
            continue;
        }
        visible += 1;
        if pixel[3] < 240
            && pixel[..3]
                .iter()
                .any(|channel| u16::from(*channel) > u16::from(pixel[3]) + 24)
        {
            straight_partial = true;
        }
    }
    assert!(visible > 0, "the explicit graph rendered no visible pixels");
    assert!(
        straight_partial,
        "partially transparent colors appear premultiplied instead of straight RGBA"
    );
}

fn shape_wire(from: Uuid, to: Uuid) -> ProjectConnection {
    ProjectConnection::new(
        PortAddress::new(PortOwner::Node(from), SHAPE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(to), SHAPE_INPUT_PORT),
        0,
    )
}

pub(super) fn insert_effector_chain(
    graph: &mut NodeGraphBundle,
    effector_ids: &[Uuid],
) -> Result<()> {
    let source_id = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(
                    library::model::GeneratorContent::Text
                        | library::model::GeneratorContent::Shape
                )
            )
        })
        .context("graph has no Shape source")?
        .id;
    let mut terminal_id = source_id;
    loop {
        let outgoing = graph
            .connections
            .iter()
            .filter(|connection| {
                connection.from == PortAddress::new(PortOwner::Node(terminal_id), SHAPE_OUTPUT_PORT)
                    && connection.to.port == SHAPE_INPUT_PORT
            })
            .collect::<Vec<_>>();
        let [connection] = outgoing.as_slice() else {
            break;
        };
        let PortOwner::Node(next_id) = connection.to.owner else {
            break;
        };
        let next_produces_shape = graph.nodes.iter().any(|node| {
            node.id == next_id
                && matches!(
                    node.content(),
                    NodeContent::PluginOperation(operation)
                        if operation.declared_ports.iter().any(|port| {
                            port.key == SHAPE_OUTPUT_PORT
                                && port.direction == PortDirection::Output
                                && port.data_type == PortDataType::Shape
                        })
                )
        });
        if !next_produces_shape {
            break;
        }
        terminal_id = next_id;
    }

    let mut targets = Vec::new();
    graph.connections.retain(|connection| {
        let is_shape_fanout = connection.from
            == PortAddress::new(PortOwner::Node(terminal_id), SHAPE_OUTPUT_PORT)
            && connection.to.port == SHAPE_INPUT_PORT;
        if is_shape_fanout {
            targets.push(connection.to.clone());
        }
        !is_shape_fanout
    });
    ensure!(!targets.is_empty(), "factory must expose a Shape consumer");
    let mut upstream = terminal_id;
    for effector_id in effector_ids {
        graph.connections.push(shape_wire(upstream, *effector_id));
        upstream = *effector_id;
    }
    for target in targets {
        graph.connections.push(ProjectConnection::new(
            PortAddress::new(PortOwner::Node(upstream), SHAPE_OUTPUT_PORT),
            target,
            0,
        ));
    }
    Ok(())
}

pub(super) fn root_transform_id(graph: &NodeGraphBundle) -> Result<Uuid> {
    graph
        .nodes
        .iter()
        .find_map(|node| match node.content() {
            NodeContent::PluginOperation(operation) if operation.category == TRANSFORM_CATEGORY => {
                Some(node.id)
            }
            _ => None,
        })
        .context("factory graph has no root Transform")
}
