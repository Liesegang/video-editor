#[path = "creative_render_tests/shape_graph.rs"]
mod shape_graph;
mod support;

use anyhow::{Context, Result, anyhow, bail};
use std::sync::{Arc, RwLock};

use library::cache::CacheManager;
use library::core::ensemble::target::EffectorTarget;
use library::core::ensemble::types::{EffectorConfig, EnsembleData};
use library::editor::project_service::GeneratorNodeRequest;
use library::editor::{ProjectModel, ProjectService};
use library::framing::get_frame_from_project;
use library::model::frame::Image;
use library::model::frame::color::Color;
use library::model::frame::draw_type::{DrawStyle, PathEffect};
use library::model::frame::entity::{FrameContent, FrameItem, SkSLColorDomain, StyleConfig};
use library::model::frame::frame::FrameInfo;
use library::model::path::parse_legacy_svg_path_data;
use library::model::project::{
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, NodeGraphBundle, PortAddress, PortOwner,
    ProjectConnection, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::model::property::{Property, PropertyValue, Vec2};
use library::model::{Clip, Composition, Node, NodeContainer, NodeContent, Project};
use library::plugin::{ExportSettings, PluginManager};
use library::rendering::renderer::{Affine2D, RenderOutput, Renderer, TextRasterRequest};
use library::{RenderDestination, RenderService, SkiaRenderer};
use ordered_float::OrderedFloat;
use uuid::Uuid;

use shape_graph::{project_with_shape_graph, transform_node_id};
use support::generator_node_for_canvas;

const WIDTH: u32 = 128;
const HEIGHT: u32 = 80;
const FPS: f64 = 10.0;

fn set(node: &mut Node, key: &str, value: PropertyValue) -> Result<()> {
    node.set_property(key.to_string(), Property::constant(value))
        .map_err(|error| anyhow!(error))
}

fn vec2(x: f64, y: f64) -> PropertyValue {
    PropertyValue::Vec2(Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    })
}

fn fill(plugins: &PluginManager, color: Color) -> Result<Node> {
    let mut node = plugins.create_style_operation_node("fill")?;
    set(&mut node, "color", PropertyValue::Color(color))?;
    Ok(node)
}

fn stroke(plugins: &PluginManager, color: Color, width: f64, dash_array: &str) -> Result<Node> {
    let mut node = plugins.create_style_operation_node("stroke")?;
    set(&mut node, "color", PropertyValue::Color(color))?;
    set(&mut node, "width", width.into())?;
    set(
        &mut node,
        "dash_array",
        PropertyValue::String(dash_array.to_string()),
    )?;
    Ok(node)
}

fn base_node(name: &str, request: GeneratorNodeRequest) -> Result<Node> {
    let node = generator_node_for_canvas(
        name,
        request,
        u64::from(WIDTH),
        u64::from(HEIGHT),
        u64::from(WIDTH),
        u64::from(HEIGHT),
    );
    Ok(node)
}

fn text_node(text: &str) -> Result<Node> {
    let mut node = base_node(
        "text",
        GeneratorNodeRequest::Text {
            text: text.to_string(),
            font: "Arial".to_string(),
        },
    )?;
    set(&mut node, "text", PropertyValue::String(text.to_string()))?;
    set(
        &mut node,
        "font_family",
        PropertyValue::String("Arial".to_string()),
    )?;
    set(&mut node, "size", 30.0.into())?;
    Ok(node)
}

fn default_text_styles(plugins: &PluginManager) -> Result<Vec<Node>> {
    Ok(vec![
        fill(
            plugins,
            Color {
                r: 230,
                g: 25,
                b: 20,
                a: 255,
            },
        )?,
        stroke(
            plugins,
            Color {
                r: 20,
                g: 70,
                b: 240,
                a: 255,
            },
            2.0,
            "5 2",
        )?,
    ])
}

fn project_with_image_graph(
    graph: NodeGraphBundle,
    content_node_id: Uuid,
) -> Result<(Project, Uuid)> {
    let mut project = Project::new("creative render e2e");
    let (mut composition, track) =
        Composition::new("main", u64::from(WIDTH), u64::from(HEIGHT), FPS, 2.0);
    composition.background_color = Color::black();
    let track_id = track.id;
    let clip = Clip::new("creative clip", 0.0, 2.0);
    let clip_id = clip.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project.insert_node_graph(NodeContainer::Clip(clip_id), graph)?;
    Ok((project, content_node_id))
}

fn find_group(
    items: &[FrameItem],
    source_id: Uuid,
) -> Option<&library::model::frame::entity::FrameGroup> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(_) => None,
        FrameItem::Group(group) if group.source_id == source_id => Some(group),
        FrameItem::Group(group) => find_group(&group.items, source_id),
        FrameItem::Transition(transition) => {
            find_group(std::slice::from_ref(&transition.from.item), source_id)
                .or_else(|| find_group(std::slice::from_ref(&transition.to.item), source_id))
        }
    })
}

fn evaluate(
    project: &Project,
    frame_number: u64,
    plugins: &Arc<PluginManager>,
) -> Result<FrameInfo> {
    Ok(get_frame_from_project(
        project,
        0,
        frame_number,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        plugins,
    )?)
}

fn preview(project: &Project, frame_number: u64, plugins: &Arc<PluginManager>) -> Result<Image> {
    let frame = evaluate(project, frame_number, plugins)?;
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
    match service.render_project_frame(project, &frame, RenderDestination::Preview)? {
        RenderOutput::Image(image) => Ok(image),
        RenderOutput::Working(_) => bail!("Project renderer returned unterminated working pixels"),
        RenderOutput::Texture(_) => bail!("CPU renderer unexpectedly returned a texture"),
    }
}

fn first_content(items: &[FrameItem]) -> Option<&FrameContent> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(object) => Some(&object.content),
        FrameItem::Group(group) => first_content(&group.items),
        FrameItem::Transition(transition) => {
            first_content(std::slice::from_ref(&transition.from.item))
                .or_else(|| first_content(std::slice::from_ref(&transition.to.item)))
        }
    })
}

fn hash(image: &Image) -> u64 {
    image.data.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn light_sum(image: &Image) -> u64 {
    image
        .data
        .chunks_exact(4)
        .map(|pixel| u64::from(pixel[0]) + u64::from(pixel[1]) + u64::from(pixel[2]))
        .sum()
}

fn dominant_pixels(image: &Image, channel: usize) -> usize {
    image
        .data
        .chunks_exact(4)
        .filter(|pixel| {
            let value = u16::from(pixel[channel]);
            let other_a = u16::from(pixel[(channel + 1) % 3]);
            let other_b = u16::from(pixel[(channel + 2) % 3]);
            value > 40 && value > other_a + 30 && value > other_b + 30
        })
        .count()
}

fn colored_centroid(image: &Image) -> Option<(f64, f64)> {
    let mut x_total = 0_u64;
    let mut y_total = 0_u64;
    let mut count = 0_u64;
    for (index, pixel) in image.data.chunks_exact(4).enumerate() {
        if pixel[0] > 8 || pixel[1] > 8 || pixel[2] > 8 {
            x_total += (index as u32 % image.width) as u64;
            y_total += (index as u32 / image.width) as u64;
            count += 1;
        }
    }
    (count > 0).then_some((x_total as f64 / count as f64, y_total as f64 / count as f64))
}

fn assert_preview_matches_export_render(
    project: &Project,
    frame_number: u64,
    plugins: &Arc<PluginManager>,
) -> Result<()> {
    let expected = preview(project, frame_number, plugins)?;
    let model = ProjectModel::new(Arc::new(project.clone()), 0)?;
    let renderer = SkiaRenderer::new(WIDTH, HEIGHT, Color::black(), false, None, None)?;
    let mut render_service =
        RenderService::new(renderer, Arc::clone(plugins), Arc::new(CacheManager::new()));
    let settings = ExportSettings::from_project(model.project().as_ref(), model.composition())?;
    let exported =
        render_service.render_export_frame(&model, settings.frame_time(frame_number)?)?;
    let exported = exported.image();
    assert_eq!((exported.width, exported.height), (WIDTH, HEIGHT));
    assert_eq!(exported.data, expected.data);
    Ok(())
}

fn assert_round_trip(
    project: &Project,
    frame_number: u64,
    plugins: &Arc<PluginManager>,
) -> Result<()> {
    let json = project.save()?;
    assert!(!json.contains("schema_version"));
    let loaded = Project::load(&json)?;
    assert_eq!(
        evaluate(project, frame_number, plugins)?.items,
        evaluate(&loaded, frame_number, plugins)?.items
    );
    assert_eq!(
        preview(project, frame_number, plugins)?.data,
        preview(&loaded, frame_number, plugins)?.data
    );
    Ok(())
}

fn effector(plugins: &PluginManager, kind: &str) -> Result<Node> {
    Ok(plugins.create_effector_operation_node(kind)?)
}

#[test]
fn text_converter_styles_transform_round_trip_and_export_are_real_pixels() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let node = text_node("TEXT")?;
    let mut styles = default_text_styles(&plugins)?;
    for style in &mut styles {
        set(style, "opacity", 0.8.into())?;
    }
    let (mut project, node_id) = project_with_shape_graph(node, Vec::new(), styles)?;
    let transform_id = transform_node_id(&project)?;
    let transform_node = project
        .get_node_mut(transform_id)
        .context("Text root Transform must exist")?;
    set(transform_node, "position", vec2(14.0, 11.0))?;
    set(transform_node, "scale", vec2(90.0, 110.0))?;
    set(transform_node, "rotation", 4.0.into())?;

    let frame = evaluate(&project, 0, &plugins)?;
    let FrameContent::Text {
        text,
        font,
        size,
        styles,
        ensemble,
        transform,
        ..
    } = first_content(&frame.items).context("text frame content must exist")?
    else {
        bail!("text converter did not produce FrameContent::Text");
    };
    assert_eq!(text, "TEXT");
    assert_eq!(font, "Arial");
    assert_eq!(*size, 30.0);
    assert_eq!(styles.len(), 1);
    assert!(matches!(styles[0].style, DrawStyle::Fill { .. }));
    assert!(ensemble.is_none());
    assert_eq!((transform.position.x, transform.position.y), (14.0, 11.0));
    assert_eq!((transform.scale.x, transform.scale.y), (0.9, 1.1));
    assert_eq!(transform.rotation, 4.0);
    assert_eq!(
        transform.opacity, 1.0,
        "base alpha must not live on Transform"
    );
    let DrawStyle::Fill { ref color, .. } = styles[0].style else {
        bail!("first Style branch must remain Fill");
    };
    assert_eq!(color.a, 204, "static opacity must be evaluated by Style");

    let standard = preview(&project, 0, &plugins)?;
    assert!(
        dominant_pixels(&standard, 0) > 10,
        "fill pixels disappeared"
    );
    assert!(
        dominant_pixels(&standard, 2) > 10,
        "stroke pixels disappeared"
    );

    let (ensemble_project, _) = project_with_shape_graph(
        project
            .get_node(node_id)
            .context("text source Node must exist")?
            .clone(),
        vec![effector(&plugins, "transform")?],
        default_text_styles(&plugins)?,
    )?;
    let ensemble_image = preview(&ensemble_project, 0, &plugins)?;
    assert!(
        dominant_pixels(&ensemble_image, 0) > 10,
        "enabling Ensemble discarded the Fill style"
    );
    assert!(
        dominant_pixels(&ensemble_image, 2) > 10,
        "enabling Ensemble discarded the Stroke style"
    );

    let mut moved = project.clone();
    set(
        moved
            .get_node_mut(transform_id)
            .context("moved Text root Transform must exist")?,
        "position",
        vec2(30.0, 16.0),
    )?;
    let moved_image = preview(&moved, 0, &plugins)?;
    assert_ne!(hash(&standard), hash(&moved_image));
    assert!(
        colored_centroid(&moved_image)
            .context("moved text must have colored pixels")?
            .0
            > colored_centroid(&standard)
                .context("standard text must have colored pixels")?
                .0
    );

    assert_round_trip(&ensemble_project, 0, &plugins)?;
    assert_preview_matches_export_render(&project, 0, &plugins)?;
    Ok(())
}

#[test]
fn shape_converter_fill_stroke_path_effect_transform_and_invalid_paths_are_explicit() -> Result<()>
{
    let plugins = Arc::new(PluginManager::default());
    let path = "M 0 0 L 42 0 L 42 27 L 0 27 Z";
    let mut node = base_node(
        "shape",
        GeneratorNodeRequest::Shape {
            path: path.to_string(),
        },
    )?;
    set(&mut node, "path", PropertyValue::String(path.to_string()))?;
    let mut corner = plugins.create_path_effect_operation_node("corner")?;
    set(&mut corner, "radius", 5.0.into())?;
    let corner_id = corner.id;
    let mut styles = vec![
        fill(
            &plugins,
            Color {
                r: 20,
                g: 220,
                b: 40,
                a: 255,
            },
        )?,
        stroke(
            &plugins,
            Color {
                r: 240,
                g: 25,
                b: 20,
                a: 255,
            },
            4.0,
            "5 3",
        )?,
    ];
    for style in &mut styles {
        set(style, "opacity", 0.9.into())?;
    }
    let (mut project, node_id) = project_with_shape_graph(node, vec![corner], styles)?;
    let transform_id = transform_node_id(&project)?;
    let transform_node = project
        .get_node_mut(transform_id)
        .context("Shape root Transform must exist")?;
    set(transform_node, "position", vec2(22.0, 18.0))?;
    set(transform_node, "rotation", 8.0.into())?;

    let frame = evaluate(&project, 0, &plugins)?;
    let FrameContent::Shape {
        path: converted_path,
        canonical_path: Some(converted_canonical_path),
        styles,
        path_effects,
        transform,
        ..
    } = first_content(&frame.items).context("shape frame content must exist")?
    else {
        bail!("shape converter did not produce FrameContent::Shape");
    };
    assert_eq!(
        parse_legacy_svg_path_data(converted_path)?,
        parse_legacy_svg_path_data(path)?
    );
    assert_eq!(converted_canonical_path, &parse_legacy_svg_path_data(path)?);
    assert_eq!(styles.len(), 1);
    assert_eq!(path_effects, &[PathEffect::Corner { radius: 5.0 }]);
    assert_eq!((transform.position.x, transform.position.y), (22.0, 18.0));
    assert_eq!(transform.rotation, 8.0);

    let rendered = preview(&project, 0, &plugins)?;
    assert!(
        dominant_pixels(&rendered, 1) > 100,
        "shape Fill was not rendered"
    );
    assert!(
        dominant_pixels(&rendered, 0) > 20,
        "shape Stroke was not rendered"
    );

    let mut no_effect = project.clone();
    let input = no_effect
        .connections
        .iter()
        .find(|connection| {
            connection.to == PortAddress::new(PortOwner::Node(corner_id), SHAPE_INPUT_PORT)
        })
        .context("Corner Path Effect input wire must exist")?
        .clone();
    let outgoing = no_effect
        .connections
        .iter()
        .filter(|connection| {
            connection.from == PortAddress::new(PortOwner::Node(corner_id), SHAPE_OUTPUT_PORT)
        })
        .map(|connection| (connection.id, connection.to.clone()))
        .collect::<Vec<_>>();
    for (connection_id, target) in outgoing {
        no_effect.reconnect_connection(connection_id, input.from.clone(), target)?;
    }
    assert!(no_effect.disconnect_connection(input.id));
    assert_ne!(
        hash(&rendered),
        hash(&preview(&no_effect, 0, &plugins)?),
        "Corner path effect did not change pixels"
    );

    let mut moved = project.clone();
    set(
        moved
            .get_node_mut(transform_id)
            .context("Shape root Transform must exist for position edit")?,
        "position",
        vec2(44.0, 28.0),
    )?;
    let moved = preview(&moved, 0, &plugins)?;
    let original_center = colored_centroid(&rendered).context("shape must render pixels")?;
    let moved_center = colored_centroid(&moved).context("moved shape must render pixels")?;
    assert!(moved_center.0 > original_center.0 + 10.0);
    assert!(moved_center.1 > original_center.1 + 4.0);

    for malformed in ["", "this is not SVG path data"] {
        let mut invalid = project.clone();
        set(
            invalid
                .get_node_mut(node_id)
                .context("shape Node must exist for malformed path edit")?,
            "path",
            PropertyValue::String(malformed.to_string()),
        )?;
        assert!(evaluate(&invalid, 0, &plugins)?.items.is_empty());
        assert_eq!(light_sum(&preview(&invalid, 0, &plugins)?), 0);
    }

    assert_round_trip(&project, 0, &plugins)?;
    assert_preview_matches_export_render(&project, 0, &plugins)?;
    Ok(())
}

#[test]
fn sksl_converter_uses_runtime_time_and_matches_export_render() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let shader = r#"
half4 main(float2 fragCoord) {
    float2 uv = fragCoord / iResolution.xy;
    float3 color = 0.5 + 0.5 * cos(iTime + uv.xyx * 4.0 + float3(0.0, 2.0, 4.0));
    return half4(color, 1.0);
}
"#;
    let mut node = base_node(
        "sksl",
        GeneratorNodeRequest::SkSL {
            shader: shader.to_string(),
        },
    )?;
    set(
        &mut node,
        "shader",
        PropertyValue::String(shader.to_string()),
    )?;
    set(&mut node, "width", 96.0.into())?;
    set(&mut node, "height", 54.0.into())?;
    let node_id = node.id;
    let mut transform = plugins.create_image_transform_operation_node()?;
    set(&mut transform, "position", vec2(10.0, 9.0))?;
    let transform_id = transform.id;
    let (project, _) = project_with_image_graph(
        NodeGraphBundle::new(
            vec![node, transform],
            vec![ProjectConnection::new(
                PortAddress::new(PortOwner::Node(node_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(transform_id), IMAGE_INPUT_PORT),
                0,
            )],
            Some(transform_id),
        ),
        node_id,
    )?;

    let frame = evaluate(&project, 0, &plugins)?;
    let FrameContent::SkSL {
        shader: converted,
        resolution,
        color_domain,
        transform,
        ..
    } = first_content(&frame.items).context("SkSL frame content must exist")?
    else {
        bail!("SkSL converter did not produce FrameContent::SkSL");
    };
    assert_eq!(converted, shader);
    assert_eq!(*resolution, (96.0, 54.0));
    assert_eq!(*color_domain, SkSLColorDomain::ProjectWorkingLinear);
    assert_eq!((transform.position.x, transform.position.y), (0.0, 0.0));
    let image_transform =
        find_group(&frame.items, transform_id).context("SkSL Image Transform group must exist")?;
    assert_eq!(
        (
            image_transform.transform.position.x,
            image_transform.transform.position.y
        ),
        (10.0, 9.0)
    );

    let first = preview(&project, 0, &plugins)?;
    let late = preview(&project, 9, &plugins)?;
    assert!(light_sum(&first) > 0);
    assert_ne!(
        hash(&first),
        hash(&late),
        "iTime did not reach the real SkSL renderer"
    );

    assert_round_trip(&project, 9, &plugins)?;
    assert_preview_matches_export_render(&project, 9, &plugins)?;
    Ok(())
}

#[test]
fn ensemble_step_delay_randomize_and_independent_crud_use_one_runtime_path() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let source = text_node("ABCD")?;
    let mut step = effector(&plugins, "step_delay")?;
    set(&mut step, "delay", 0.2.into())?;
    set(&mut step, "duration", 0.2.into())?;
    set(&mut step, "from_opacity", 0.0.into())?;
    set(&mut step, "to_opacity", 100.0.into())?;
    set(
        &mut step,
        "target",
        PropertyValue::String("Block".to_string()),
    )?;
    let step_id = step.id;
    let (project, node_id) = project_with_shape_graph(
        source.clone(),
        vec![step.clone()],
        default_text_styles(&plugins)?,
    )?;

    let frame = evaluate(&project, 4, &plugins)?;
    let FrameContent::Text {
        ensemble: Some(ensemble),
        styles,
        ..
    } = first_content(&frame.items).context("ensemble text frame content must exist")?
    else {
        bail!("the explicit Shape Effector did not produce EnsembleData");
    };
    assert_eq!(styles.len(), 1);
    assert_eq!(ensemble.effector_configs.len(), 1);
    let EffectorConfig::StepDelay {
        delay_per_element,
        duration,
        from_opacity,
        to_opacity,
        target,
    } = ensemble.effector_configs[0]
    else {
        bail!("StepDelay plugin produced the wrong config variant");
    };
    assert!((delay_per_element - 0.2).abs() < f32::EPSILON);
    assert!((duration - 0.2).abs() < f32::EPSILON);
    assert_eq!(from_opacity, 0.0);
    assert_eq!(to_opacity, 100.0);
    assert_eq!(target, EffectorTarget::Block);

    let start = preview(&project, 0, &plugins)?;
    let middle = preview(&project, 4, &plugins)?;
    let end = preview(&project, 10, &plugins)?;
    assert_eq!(light_sum(&start), 0);
    assert!(light_sum(&middle) > light_sum(&start));
    assert!(light_sum(&end) > light_sum(&middle));

    let mut random = effector(&plugins, "randomize")?;
    set(&mut random, "seed", 7.0.into())?;
    set(&mut random, "translate_range", 4.0.into())?;
    set(&mut random, "rotate_range", 8.0.into())?;
    set(&mut random, "scale_range", 0.35.into())?;
    set(
        &mut random,
        "target",
        PropertyValue::String("Char".to_string()),
    )?;
    let random_id = random.id;
    let (randomized, _) = project_with_shape_graph(
        source.clone(),
        vec![step.clone(), random.clone()],
        default_text_styles(&plugins)?,
    )?;
    let random_a = preview(&randomized, 10, &plugins)?;
    let random_b = preview(&randomized, 10, &plugins)?;
    assert_eq!(
        random_a.data, random_b.data,
        "fixed seed was not deterministic"
    );
    assert_ne!(
        hash(&end),
        hash(&random_a),
        "Char Randomize rendered identically to standard text"
    );

    let mut changed_seed = randomized.clone();
    set(
        changed_seed
            .get_node_mut(random_id)
            .context("Randomize operation Node must exist")?,
        "seed",
        8.0.into(),
    )?;
    assert_ne!(
        hash(&random_a),
        hash(&preview(&changed_seed, 10, &plugins)?),
        "changing Randomize seed did not change the rendered characters"
    );

    let mut scale_random = random;
    set(&mut scale_random, "translate_range", 0.0.into())?;
    set(&mut scale_random, "rotate_range", 0.0.into())?;
    let (scale_only, _) = project_with_shape_graph(
        source,
        vec![step, scale_random],
        default_text_styles(&plugins)?,
    )?;
    assert_ne!(
        hash(&end),
        hash(&preview(&scale_only, 10, &plugins)?),
        "Randomize scale_range was ignored"
    );

    let shared = Arc::new(RwLock::new(project.clone()));
    let service = ProjectService::new(Arc::clone(&shared), Arc::clone(&plugins));
    service.add_effector(node_id, "opacity")?;
    service.add_decorator(node_id, "backplate")?;
    let locked = shared
        .read()
        .map_err(|error| anyhow!("project lock poisoned: {error}"))?;
    assert!(locked.get_node(node_id).is_some());
    let opacity_node = locked
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::PluginOperation(operation)
                    if operation.category == "effector" && operation.component_id == "opacity"
            )
        })
        .context("add_effector must author a standalone operation Node")?;
    assert!(locked.connections.iter().any(|connection| {
        connection.from == PortAddress::new(PortOwner::Node(step_id), SHAPE_OUTPUT_PORT)
            && connection.to == PortAddress::new(PortOwner::Node(opacity_node.id), SHAPE_INPUT_PORT)
    }));
    let backplate_node = locked
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::PluginOperation(operation)
                    if operation.category == "decorator" && operation.component_id == "backplate"
            )
        })
        .context("add_decorator must author a standalone operation Node")?;
    assert!(locked.connections.iter().any(|connection| {
        connection.from == PortAddress::new(PortOwner::Node(opacity_node.id), SHAPE_OUTPUT_PORT)
            && connection.to
                == PortAddress::new(PortOwner::Node(backplate_node.id), SHAPE_INPUT_PORT)
    }));
    drop(locked);

    assert_round_trip(&randomized, 10, &plugins)?;
    assert_preview_matches_export_render(&project, 10, &plugins)?;
    Ok(())
}

#[test]
fn effector_block_line_and_char_targets_are_distinct_in_multiline_pixels() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let render_target = |target: &str| -> Result<Image> {
        let node = text_node("AB\nCD")?;
        let mut step = effector(&plugins, "step_delay")?;
        set(&mut step, "delay", 0.3.into())?;
        set(&mut step, "duration", 0.1.into())?;
        set(&mut step, "from_opacity", 0.0.into())?;
        set(&mut step, "to_opacity", 100.0.into())?;
        set(
            &mut step,
            "target",
            PropertyValue::String(target.to_string()),
        )?;
        let (project, _) =
            project_with_shape_graph(node, vec![step], default_text_styles(&plugins)?)?;
        preview(&project, 2, &plugins)
    };

    let block = render_target("Block")?;
    let line = render_target("Line")?;
    let character = render_target("Char")?;
    assert!(light_sum(&block) > 0);
    assert!(
        light_sum(&line) > light_sum(&block),
        "Line target did not restart StepDelay for the second line"
    );
    assert!(
        light_sum(&character) > light_sum(&line),
        "Char target did not animate every character independently"
    );
    assert_ne!(hash(&block), hash(&line));
    assert_ne!(hash(&line), hash(&character));
    Ok(())
}

#[test]
fn empty_text_is_safe_missing_text_is_validation_and_parts_is_render_error() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let empty_node = text_node("")?;
    let (empty_project, _) =
        project_with_shape_graph(empty_node, Vec::new(), default_text_styles(&plugins)?)?;
    let empty = preview(&empty_project, 0, &plugins)?;
    assert_eq!(light_sum(&empty), 0);

    let complete_node = text_node("missing")?;
    let mut missing_json = serde_json::to_value(complete_node)?;
    missing_json["properties"] = serde_json::json!({});
    let missing_node: Node = serde_json::from_value(missing_json)?;
    let (missing_project, _) =
        project_with_shape_graph(missing_node, Vec::new(), default_text_styles(&plugins)?)?;
    assert!(evaluate(&missing_project, 0, &plugins)?.items.is_empty());

    let styles = vec![StyleConfig {
        id: Uuid::new_v4(),
        style: DrawStyle::Fill {
            color: Color::white(),
            offset: 0.0,
        },
    }];
    let ensemble = EnsembleData {
        enabled: true,
        effector_configs: vec![EffectorConfig::Transform {
            translate: (0.0, 0.0),
            rotate: 0.0,
            scale: (1.0, 1.0),
            target: EffectorTarget::Parts,
        }],
        decorator_configs: Vec::new(),
        patches: Default::default(),
    };
    let mut renderer = SkiaRenderer::new(WIDTH, HEIGHT, Color::black(), false, None, None)?;
    let error = match renderer.rasterize_text_layer(TextRasterRequest {
        text: "A",
        size: 30.0,
        font_name: "Arial",
        styles: &styles,
        ensemble: Some(&ensemble),
        transform: Affine2D::IDENTITY,
        current_time: 0.0,
    }) {
        Ok(_) => bail!("Parts effector unexpectedly rasterized"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("EffectorTarget::Parts"));

    Ok(())
}
