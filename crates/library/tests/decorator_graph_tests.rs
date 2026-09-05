use std::sync::{Arc, RwLock};

use anyhow::{Context, Result, anyhow, bail};
use library::cache::CacheManager;
use library::core::ensemble::decorators::{BackplateFit, BackplateShape, BackplateTarget};
use library::core::ensemble::effectors::OpacityMode;
use library::core::ensemble::target::EffectorTarget;
use library::core::ensemble::types::{DecoratorConfig, EffectorConfig};
use library::editor::project_service::ProjectManager;
use library::framing::get_frame_from_project;
use library::model::frame::Image;
use library::model::frame::color::Color;
use library::model::frame::draw_type::DrawStyle;
use library::model::frame::effect::ImageEffect;
use library::model::frame::entity::{FrameContent, FrameItem};
use library::model::frame::runtime_shape::{
    RuntimeBounds, RuntimePathPart, RuntimePathShape, RuntimeShape, RuntimeShapeGeometry,
};
use library::model::frame::transform::Transform;
use library::model::project::{
    BACKGROUND_SHAPE_INPUT_PORT, Composition, EvalOutput, NodeContainer, NodeGraphBundle,
    PortAddress, PortDataType, PortDirection, PortMultiplicity, PortOwner, PortSide, Project,
    ProjectConnection, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::model::property::{Property, PropertyValue};
use library::model::{Clip, Node};
use library::plugin::{
    DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY, FrameEvaluationContext, PluginManager,
    property_port_key,
};
use library::rendering::renderer::{Affine2D, RenderOutput, Renderer, ShapeRasterRequest};
use library::{RenderService, SkiaRenderer};
use ordered_float::OrderedFloat;
use uuid::Uuid;

const WIDTH: u64 = 180;
const HEIGHT: u64 = 100;
const FPS: f64 = 10.0;
const TRIANGLE: &str = "M 0 0 L 1 0 L 0.5 1 Z";

fn set(node: &mut Node, key: &str, value: PropertyValue) -> Result<()> {
    node.set_property(key.to_string(), Property::constant(value))
        .map_err(anyhow::Error::msg)
}

fn first_object(items: &[FrameItem]) -> Option<&library::model::frame::entity::FrameObject> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(object) => Some(object),
        FrameItem::Group(group) => first_object(&group.items),
        FrameItem::Transition(transition) => {
            first_object(std::slice::from_ref(&transition.from.item))
                .or_else(|| first_object(std::slice::from_ref(&transition.to.item)))
        }
    })
}

fn evaluate(project: &Project, plugins: &Arc<PluginManager>) -> Result<Vec<FrameItem>> {
    Ok(get_frame_from_project(
        project,
        0,
        0,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        plugins,
    )?
    .items)
}

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
        RenderOutput::Working(_) => bail!("unmanaged renderer returned Project pixels"),
        RenderOutput::Texture(_) => bail!("CPU renderer returned a texture"),
    }
}

struct GraphFixture {
    project: Project,
    backplate_id: Uuid,
    fill_id: Uuid,
}

fn graph_fixture(plugins: &Arc<PluginManager>, with_background: bool) -> Result<GraphFixture> {
    let factory = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("backplate factory"))),
        plugins.clone(),
    );
    let mut text = factory.create_text_node("AB", "Arial", WIDTH, HEIGHT)?;
    let mut background = factory.create_shape_node(TRIANGLE, WIDTH, HEIGHT, 1, 1)?;
    let mut backplate = plugins.create_decorator_operation_node("backplate")?;
    set(&mut backplate, "padding", 4.0.into())?;
    set(
        &mut backplate,
        "offset",
        PropertyValue::Vec2(library::model::property::Vec2 {
            x: OrderedFloat(3.0),
            y: OrderedFloat(-2.0),
        }),
    )?;
    let mut fill = plugins.create_style_operation_node("fill")?;
    let color = Color {
        r: 12,
        g: 190,
        b: 72,
        a: 255,
    };
    set(&mut fill, "color", PropertyValue::Color(color))?;

    text.ui_position = [0.0, 0.0];
    background.ui_position = [0.0, 180.0];
    backplate.ui_position = [300.0, 80.0];
    fill.ui_position = [600.0, 80.0];
    let text_id = text.id;
    let background_id = background.id;
    let backplate_id = backplate.id;
    let fill_id = fill.id;
    let mut connections = vec![
        ProjectConnection::new(
            PortAddress::new(PortOwner::Node(text_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(backplate_id), SHAPE_INPUT_PORT),
            0,
        ),
        ProjectConnection::new(
            PortAddress::new(PortOwner::Node(backplate_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(fill_id), SHAPE_INPUT_PORT),
            0,
        ),
    ];
    if with_background {
        connections.push(ProjectConnection::new(
            PortAddress::new(PortOwner::Node(background_id), SHAPE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(backplate_id), BACKGROUND_SHAPE_INPUT_PORT),
            0,
        ));
    }

    let graph = NodeGraphBundle::new(
        vec![text, background, backplate, fill],
        connections,
        Some(fill_id),
    );
    let mut project = Project::new("geometry-only backplate");
    let (mut composition, track) = Composition::new("main", WIDTH, HEIGHT, FPS, 2.0);
    composition.background_color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let track_id = track.id;
    project.add_track(track)?;
    project.add_composition(composition)?;
    let clip = Clip::new("title", 0.0, 2.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project.insert_node_graph(NodeContainer::Clip(clip_id), graph)?;
    Ok(GraphFixture {
        project,
        backplate_id,
        fill_id,
    })
}

fn runtime_shapes(plugins: &Arc<PluginManager>) -> Result<(RuntimeShape, RuntimeShape)> {
    let factory = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("shape factory"))),
        plugins.clone(),
    );
    let text = factory.create_text_node("A\nBC", "Arial", WIDTH, HEIGHT)?;
    let background = factory.create_shape_node(TRIANGLE, WIDTH, HEIGHT, 1, 1)?;
    let project = Project::new("runtime shape");
    let (composition, _) = Composition::new("main", WIDTH, HEIGHT, FPS, 2.0);
    let evaluators = plugins.get_property_evaluators();
    let context = FrameEvaluationContext {
        project: &project,
        composition: &composition,
        property_evaluators: &evaluators,
        plugin_manager: plugins,
        resolved_inputs: None,
    };
    let text = plugins
        .get_entity_converter("text")
        .context("Text converter missing")?
        .convert_shape(&context, &text, 0.0)
        .context("Text conversion produced no Shape")?;
    let background = plugins
        .get_entity_converter("shape")
        .context("Shape converter missing")?
        .convert_shape(&context, &background, 0.0)
        .context("Path conversion produced no Shape")?;
    Ok((text, background))
}

fn runtime_path_shape(path: &str, part_ids: &[u64]) -> Result<RuntimeShape> {
    let parsed = skia_safe::Path::from_svg(path).context("test path is invalid")?;
    let bounds = parsed.compute_tight_bounds();
    let bounds = RuntimeBounds::new(bounds.left, bounds.top, bounds.right, bounds.bottom);
    let source_id = Uuid::new_v4();
    let ids = if part_ids.is_empty() {
        vec![source_id.as_u128() as u64]
    } else {
        part_ids.to_vec()
    };
    Ok(RuntimeShape {
        source_id,
        geometry: RuntimeShapeGeometry::Path(RuntimePathShape {
            path: path.to_string(),
            canonical_path: None,
            bounds,
            path_effects: Vec::new(),
            parts: ids
                .into_iter()
                .map(|stable_id| RuntimePathPart {
                    path: path.to_string(),
                    canonical_path: None,
                    bounds,
                    stable_id,
                    block_group_id: 10,
                    line_group_id: 20,
                    line_index: 0,
                    opacity: 1.0,
                })
                .collect(),
        }),
        spatial_transform_node_id: None,
        spatial_transform: Transform::default(),
        modulation_transform: Transform::default(),
        transform: Transform::default(),
        effects: Vec::new(),
        effector_configs: Vec::new(),
        decorator_configs: Vec::new(),
    })
}

fn rasterize_geometry(path: &str) -> Result<Image> {
    let style = library::model::frame::entity::StyleConfig {
        id: Uuid::new_v4(),
        style: DrawStyle::Fill {
            color: Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            offset: 0.0,
        },
    };
    let mut renderer = SkiaRenderer::new(
        64,
        48,
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        },
        false,
        None,
        None,
    )?;
    match renderer.rasterize_shape_layer(ShapeRasterRequest {
        path_data: path,
        canonical_path: None,
        parts: &[],
        styles: std::slice::from_ref(&style),
        path_effects: &[],
        ensemble: None,
        transform: Affine2D::IDENTITY,
    })? {
        RenderOutput::Image(image) => Ok(image),
        RenderOutput::Working(_) => bail!("unmanaged renderer returned Project pixels"),
        RenderOutput::Texture(_) => bail!("CPU renderer returned a texture"),
    }
}

fn alpha_at(image: &Image, x: usize, y: usize) -> u8 {
    image.data[(y * image.width as usize + x) * 4 + 3]
}

#[test]
fn descriptor_has_two_typed_shape_inputs_and_no_appearance_properties() -> Result<()> {
    let plugins = PluginManager::default();
    let descriptor =
        plugins.operation_descriptor(DECORATOR_CATEGORY, "backplate", DECORATOR_APPLY_OPERATION)?;
    assert_eq!(
        descriptor
            .properties()
            .iter()
            .map(library::model::property::PropertyDefinition::name)
            .collect::<Vec<_>>(),
        ["target", "padding", "offset", "fit"]
    );
    for forbidden in ["color", "opacity", "stroke", "shape", "radius"] {
        assert!(
            descriptor
                .properties()
                .iter()
                .all(|property| property.name() != forbidden),
            "Backplate must not own appearance property {forbidden}"
        );
    }
    let ports = descriptor.declared_ports();
    for (key, label) in [
        (SHAPE_INPUT_PORT, "Target"),
        (BACKGROUND_SHAPE_INPUT_PORT, "Background"),
    ] {
        let port = ports
            .iter()
            .find(|port| port.key == key)
            .with_context(|| format!("missing {key}"))?;
        assert_eq!(port.label, label);
        assert_eq!(port.direction, PortDirection::Input);
        assert_eq!(port.data_type, PortDataType::Shape);
        assert_eq!(port.multiplicity, PortMultiplicity::Single);
    }
    let output = ports
        .iter()
        .find(|port| port.key == SHAPE_OUTPUT_PORT)
        .context("missing Shape output")?;
    assert_eq!(output.direction, PortDirection::Output);
    assert_eq!(output.side, PortSide::Right);
    assert_eq!(output.data_type, PortDataType::Shape);

    let node = plugins.create_decorator_operation_node("backplate")?;
    for property in descriptor.properties() {
        assert_eq!(
            node.properties()
                .get(property.name())
                .and_then(Property::value),
            Some(property.default_value())
        );
        assert!(ports.iter().any(|port| {
            port.key == property_port_key(property.name()) && port.direction == PortDirection::Input
        }));
    }
    Ok(())
}

#[test]
fn char_line_block_emit_stable_semantic_parts_from_custom_shape() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let (text, background) = runtime_shapes(&plugins)?;
    let RuntimeShapeGeometry::Text(text_metadata) = &text.geometry else {
        bail!("Text converter returned Path")
    };
    let expected_char_ids = text_metadata
        .elements
        .iter()
        .map(|element| element.element_group_id)
        .collect::<Vec<_>>();

    for (target, expected_count) in [
        (BackplateTarget::Char, 3),
        (BackplateTarget::Line, 2),
        (BackplateTarget::Block, 1),
    ] {
        let output = text.clone().into_backplate_geometry(
            Uuid::new_v4(),
            background.clone(),
            DecoratorConfig::Backplate {
                target,
                padding: (2.0, 2.0, 2.0, 2.0),
                offset: (0.0, 0.0),
                fit: BackplateFit::Stretch,
            },
            0.0,
        )?;
        let RuntimeShapeGeometry::Path(path) = output.geometry else {
            bail!("Backplate output was not Path Shape")
        };
        assert_eq!(path.parts.len(), expected_count);
        assert!(path.parts.iter().all(|part| !part.path.is_empty()));
        if target == BackplateTarget::Char {
            assert_eq!(
                path.parts
                    .iter()
                    .map(|part| part.stable_id)
                    .collect::<Vec<_>>(),
                expected_char_ids
            );
        }
    }
    Ok(())
}

#[test]
fn arbitrary_triangle_is_fitted_and_offset_without_becoming_a_rectangle() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let (text, background) = runtime_shapes(&plugins)?;
    let base = text.clone().into_backplate_geometry(
        Uuid::new_v4(),
        background.clone(),
        DecoratorConfig::Backplate {
            target: BackplateTarget::Block,
            padding: (0.0, 0.0, 0.0, 0.0),
            offset: (0.0, 0.0),
            fit: BackplateFit::Contain,
        },
        0.0,
    )?;
    let shifted = text.into_backplate_geometry(
        Uuid::new_v4(),
        background,
        DecoratorConfig::Backplate {
            target: BackplateTarget::Block,
            padding: (4.0, 4.0, 4.0, 4.0),
            offset: (9.0, -3.0),
            fit: BackplateFit::Contain,
        },
        0.0,
    )?;
    let RuntimeShapeGeometry::Path(base) = base.geometry else {
        bail!("base output was not Path")
    };
    let RuntimeShapeGeometry::Path(shifted) = shifted.geometry else {
        bail!("shifted output was not Path")
    };
    let parsed = skia_safe::Path::from_svg(&shifted.path).context("generated path is invalid")?;
    assert_eq!(
        parsed.count_points(),
        4,
        "custom triangle contour was replaced by built-in geometry"
    );
    assert!(shifted.bounds.width() > base.bounds.width());
    let base_center = (
        (base.bounds.left + base.bounds.right) * 0.5,
        (base.bounds.top + base.bounds.bottom) * 0.5,
    );
    let shifted_center = (
        (shifted.bounds.left + shifted.bounds.right) * 0.5,
        (shifted.bounds.top + shifted.bounds.bottom) * 0.5,
    );
    assert!((shifted_center.0 - base_center.0 - 9.0).abs() < 0.01);
    assert!((shifted_center.1 - base_center.1 + 3.0).abs() < 0.01);
    Ok(())
}

#[test]
fn stretch_contain_and_cover_have_distinct_bounds_and_cover_pixels_are_cropped() -> Result<()> {
    let target = runtime_path_shape("M 10 10 L 50 10 L 50 30 L 10 30 Z", &[])?;
    let background = runtime_path_shape("M 0 0 L 10 0 L 5 10 Z", &[])?;
    let evaluate_fit = |fit| -> Result<RuntimePathShape> {
        let output = target.clone().into_backplate_geometry(
            Uuid::new_v4(),
            background.clone(),
            DecoratorConfig::Backplate {
                target: BackplateTarget::Block,
                padding: (0.0, 0.0, 0.0, 0.0),
                offset: (0.0, 0.0),
                fit,
            },
            0.0,
        )?;
        let RuntimeShapeGeometry::Path(path) = output.geometry else {
            bail!("Backplate output was not Path")
        };
        Ok(path)
    };

    let stretch = evaluate_fit(BackplateFit::Stretch)?;
    let contain = evaluate_fit(BackplateFit::Contain)?;
    let cover = evaluate_fit(BackplateFit::Cover)?;
    assert!((stretch.bounds.width() - 40.0).abs() < 0.01);
    assert!((stretch.bounds.height() - 20.0).abs() < 0.01);
    assert!((contain.bounds.width() - 20.0).abs() < 0.01);
    assert!((contain.bounds.height() - 20.0).abs() < 0.01);
    assert!(cover.bounds.left >= 10.0 - 0.01);
    assert!(cover.bounds.top >= 10.0 - 0.01);
    assert!(cover.bounds.right <= 50.0 + 0.01);
    assert!(cover.bounds.bottom <= 30.0 + 0.01);

    let stretch_pixels = rasterize_geometry(&stretch.path)?;
    let contain_pixels = rasterize_geometry(&contain.path)?;
    let cover_pixels = rasterize_geometry(&cover.path)?;
    assert!(alpha_at(&stretch_pixels, 14, 12) > 0);
    assert_eq!(alpha_at(&contain_pixels, 14, 12), 0);
    assert_eq!(alpha_at(&cover_pixels, 14, 12), 0);
    assert!(alpha_at(&cover_pixels, 25, 12) > 0);
    assert_eq!(
        alpha_at(&cover_pixels, 25, 5),
        0,
        "Cover must clip overscaled geometry outside the destination"
    );
    Ok(())
}

#[test]
fn transformed_and_text_background_shapes_are_consumed_as_geometry() -> Result<()> {
    let target = runtime_path_shape("M 10 10 L 50 10 L 50 30 L 10 30 Z", &[])?;
    let plain_background = runtime_path_shape("M 0 0 L 10 0 L 5 10 Z", &[])?;
    let mut transformed_background = plain_background.clone();
    transformed_background.set_root_transform(
        Uuid::new_v4(),
        Transform {
            rotation: 90.0,
            ..Transform::default()
        },
    )?;
    let config = DecoratorConfig::Backplate {
        target: BackplateTarget::Block,
        padding: (0.0, 0.0, 0.0, 0.0),
        offset: (0.0, 0.0),
        fit: BackplateFit::Stretch,
    };
    let plain = target.clone().into_backplate_geometry(
        Uuid::new_v4(),
        plain_background,
        config.clone(),
        0.0,
    )?;
    let transformed = target.clone().into_backplate_geometry(
        Uuid::new_v4(),
        transformed_background,
        config.clone(),
        0.0,
    )?;
    let RuntimeShapeGeometry::Path(plain) = plain.geometry else {
        bail!("plain output was not Path")
    };
    let RuntimeShapeGeometry::Path(transformed) = transformed.geometry else {
        bail!("transformed output was not Path")
    };
    assert_ne!(plain.path, transformed.path);
    assert_ne!(
        rasterize_geometry(&plain.path)?.data,
        rasterize_geometry(&transformed.path)?.data,
        "background spatial Transform must affect emitted geometry"
    );

    let plugins = Arc::new(PluginManager::default());
    let (text_background, _) = runtime_shapes(&plugins)?;
    let text_output =
        target.into_backplate_geometry(Uuid::new_v4(), text_background, config, 0.0)?;
    let RuntimeShapeGeometry::Path(text_output) = text_output.geometry else {
        bail!("Text background was not converted to Path")
    };
    assert!(!text_output.path.is_empty());
    assert!(text_output.bounds.width() > 0.0);
    assert!(text_output.bounds.height() > 0.0);
    Ok(())
}

#[test]
fn background_pending_configs_fail_closed_but_root_state_remains_supported() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let target = runtime_path_shape("M 10 10 H 50 V 30 H 10 Z", &[])?;
    let (mut text_background, _) = runtime_shapes(&plugins)?;
    let config = DecoratorConfig::Backplate {
        target: BackplateTarget::Block,
        padding: (0.0, 0.0, 0.0, 0.0),
        offset: (0.0, 0.0),
        fit: BackplateFit::Stretch,
    };
    text_background
        .effector_configs
        .push(EffectorConfig::Opacity {
            target_opacity: 50.0,
            mode: OpacityMode::Set,
            target: EffectorTarget::Char,
        });
    let error = target
        .clone()
        .into_backplate_geometry(Uuid::new_v4(), text_background, config.clone(), 0.0)
        .expect_err("pending Text Effector config must be rejected");
    assert!(error.to_string().contains("pending Effector configs"));

    let (mut text_background, _) = runtime_shapes(&plugins)?;
    text_background.decorator_configs.push(config.clone());
    let error = target
        .clone()
        .into_backplate_geometry(Uuid::new_v4(), text_background, config.clone(), 0.0)
        .expect_err("pending Text Decorator config must be rejected");
    assert!(error.to_string().contains("pending Decorator configs"));

    let mut background = runtime_path_shape("M 0 0 L 10 0 L 5 10 Z", &[])?;
    let effect = ImageEffect {
        effect_type: "blur".to_string(),
        properties: std::collections::HashMap::new(),
    };
    background.effects.push(effect.clone());
    background.set_root_transform(
        Uuid::new_v4(),
        Transform {
            rotation: 90.0,
            opacity: 0.4,
            ..Transform::default()
        },
    )?;
    let output = target.into_backplate_geometry(Uuid::new_v4(), background, config, 0.0)?;
    assert_eq!(output.effects, [effect]);
    let RuntimeShapeGeometry::Path(path) = output.geometry else {
        bail!("Backplate root-state output was not Path")
    };
    assert!(
        path.parts
            .iter()
            .all(|part| (part.opacity - 0.4).abs() < 0.001)
    );
    Ok(())
}

#[test]
fn negative_padding_and_overlap_order_are_explicit_and_deterministic() -> Result<()> {
    let target = runtime_path_shape("M 10 10 L 50 10 L 50 30 L 10 30 Z", &[9, 3])?;
    let background = runtime_path_shape("M 0 0 L 10 0 L 5 10 Z", &[])?;
    let config = DecoratorConfig::Backplate {
        target: BackplateTarget::Char,
        padding: (-2.0, -4.0, -2.0, -4.0),
        offset: (0.0, 0.0),
        fit: BackplateFit::Stretch,
    };
    let first = target.clone().into_backplate_geometry(
        Uuid::new_v4(),
        background.clone(),
        config.clone(),
        0.0,
    )?;
    let second = target.into_backplate_geometry(Uuid::new_v4(), background, config, 0.0)?;
    let RuntimeShapeGeometry::Path(first) = first.geometry else {
        bail!("first output was not Path")
    };
    let RuntimeShapeGeometry::Path(second) = second.geometry else {
        bail!("second output was not Path")
    };
    assert!((first.bounds.left - 14.0).abs() < 0.01);
    assert!((first.bounds.top - 12.0).abs() < 0.01);
    assert!(first.bounds.right <= 46.0 + 0.01);
    assert!(first.bounds.bottom <= 28.0 + 0.01);
    assert_eq!(
        first
            .parts
            .iter()
            .map(|part| part.stable_id)
            .collect::<Vec<_>>(),
        [9, 3],
        "authored overlap order is retained"
    );
    assert_eq!(first.path, second.path);
    assert_eq!(first.parts, second.parts);
    Ok(())
}

#[test]
fn legacy_v1_backplate_keeps_one_shape_paint_time_appearance() -> Result<()> {
    let mut target = runtime_path_shape("M 20 20 L 30 20 L 30 30 L 20 30 Z", &[])?;
    target.push_decorator(DecoratorConfig::LegacyBackplate {
        target: BackplateTarget::Block,
        shape: BackplateShape::Rect,
        color: Color {
            r: 220,
            g: 10,
            b: 20,
            a: 255,
        },
        padding: (10.0, 10.0, 10.0, 10.0),
        corner_radius: 0.0,
    });
    let style = library::model::frame::entity::StyleConfig {
        id: Uuid::new_v4(),
        style: DrawStyle::Fill {
            color: Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            offset: 0.0,
        },
    };
    let object = target.into_styled_object(style, 0.0)?;
    let FrameContent::Shape {
        path,
        styles,
        path_effects,
        ensemble,
        transform,
        ..
    } = object.content
    else {
        bail!("legacy Backplate target was not Shape")
    };
    let ensemble = ensemble.context("legacy v1 config was not retained on its target Shape")?;
    let mut renderer = SkiaRenderer::new(
        48,
        48,
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        },
        false,
        None,
        None,
    )?;
    let RenderOutput::Image(image) = renderer.rasterize_shape_layer(ShapeRasterRequest {
        path_data: &path,
        canonical_path: None,
        parts: &[],
        styles: &styles,
        path_effects: &path_effects,
        ensemble: Some(&ensemble),
        transform: Affine2D::from(&transform),
    })?
    else {
        bail!("CPU renderer returned a texture")
    };
    let outside = &image.data[(12 * image.width as usize + 12) * 4..][..4];
    assert!(outside[0] > 180 && outside[1] < 60 && outside[3] > 0);
    let inside = &image.data[(25 * image.width as usize + 25) * 4..][..4];
    assert!(inside[0] > 220 && inside[1] > 220 && inside[2] > 220);
    Ok(())
}

#[test]
fn target_part_opacity_survives_until_style_rasterization() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let (mut text, mut background) = runtime_shapes(&plugins)?;
    background.effects.push(ImageEffect {
        effect_type: "grouped-opacity-fixture".to_string(),
        properties: Default::default(),
    });
    text.effector_configs.push(EffectorConfig::Opacity {
        target_opacity: 50.0,
        mode: OpacityMode::Set,
        target: EffectorTarget::Char,
    });
    let output = text.into_backplate_geometry(
        Uuid::new_v4(),
        background,
        DecoratorConfig::Backplate {
            target: BackplateTarget::Char,
            padding: (0.0, 0.0, 0.0, 0.0),
            offset: (0.0, 0.0),
            fit: BackplateFit::Stretch,
        },
        0.0,
    )?;
    let object = output.into_styled_object(
        library::model::frame::entity::StyleConfig {
            id: Uuid::new_v4(),
            style: DrawStyle::Fill {
                color: Color {
                    r: 20,
                    g: 40,
                    b: 60,
                    a: 200,
                },
                offset: 0.0,
            },
        },
        0.0,
    )?;
    let FrameContent::Shape {
        parts,
        styles,
        effects,
        ..
    } = object.content
    else {
        bail!("Backplate Style did not rasterize Shape geometry")
    };
    assert_eq!(parts.len(), 3, "all parts must share one renderer object");
    assert!(parts.iter().all(|part| part.opacity == OrderedFloat(0.5)));
    assert!(matches!(
        styles.as_slice(),
        [library::model::frame::entity::StyleConfig {
            style: DrawStyle::Fill {
                color: Color { a: 200, .. },
                ..
            },
            ..
        }]
    ));
    assert_eq!(
        effects.len(),
        1,
        "the Image effect must not be cloned per part"
    );
    Ok(())
}

#[test]
fn graph_rasterizes_only_through_downstream_style_and_roundtrips() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let fixture = graph_fixture(&plugins, true)?;
    assert!(fixture.project.validation_issues().is_empty());
    let items = evaluate(&fixture.project, &plugins)?;
    let object = first_object(&items).context("Backplate graph produced no object")?;
    assert_eq!(object.source_node_id, fixture.backplate_id);
    let FrameContent::Shape {
        path,
        styles,
        ensemble,
        ..
    } = &object.content
    else {
        bail!("Backplate did not cross Style as Shape geometry")
    };
    assert!(
        ensemble.is_none(),
        "Backplate leaked to paint-time decorators"
    );
    assert!(!path.is_empty());
    assert!(styles.iter().any(|style| {
        style.id == fixture.fill_id
            && matches!(
                style.style,
                DrawStyle::Fill {
                    color: Color {
                        r: 12,
                        g: 190,
                        b: 72,
                        a: 255
                    },
                    ..
                }
            )
    }));

    let image = preview(&fixture.project, &plugins)?;
    assert!(
        image.data.chunks_exact(4).any(|pixel| {
            pixel[1] > 120 && pixel[1] > pixel[0].saturating_add(60) && pixel[3] > 0
        })
    );

    let saved = fixture.project.save()?;
    assert!(!saved.contains("schema_version"));
    let loaded = Project::load(&saved)?;
    assert_eq!(loaded, fixture.project);
    assert_eq!(preview(&loaded, &plugins)?.data, image.data);
    Ok(())
}

#[test]
fn timeline_backplate_authoring_preserves_foreground_and_adds_background_pixels() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let shared = Arc::new(RwLock::new(Project::new("semantic Backplate")));
    let manager = ProjectManager::new(shared.clone(), plugins.clone());
    let graph = manager.create_text_graph("A", "Arial", WIDTH, HEIGHT)?;
    let text_id = graph
        .nodes
        .iter()
        .find(|node| matches!(node.content(), library::model::NodeContent::Generator(_)))
        .context("Text graph has no generator")?
        .id;
    {
        let mut project = shared
            .write()
            .map_err(|error| anyhow!("test Project lock is poisoned: {error}"))?;
        let (mut composition, track) = Composition::new("main", WIDTH, HEIGHT, FPS, 2.0);
        composition.background_color = Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        };
        let track_id = track.id;
        project.add_track(track)?;
        project.add_composition(composition)?;
        let clip = Clip::new("title", 0.0, 2.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id)?;
        project.insert_node_graph(NodeContainer::Clip(clip_id), graph)?;
    }
    let before = {
        let project = shared
            .read()
            .map_err(|error| anyhow!("test Project lock is poisoned: {error}"))?;
        preview(&project, &plugins)?
    };
    manager.add_decorator(text_id, "backplate")?;
    let after = {
        let project = shared
            .read()
            .map_err(|error| anyhow!("test Project lock is poisoned: {error}"))?;
        preview(&project, &plugins)?
    };
    let visible = |image: &Image| {
        image
            .data
            .chunks_exact(4)
            .filter(|pixel| pixel[3] != 0)
            .count()
    };
    assert!(
        visible(&after) > visible(&before),
        "semantic Backplate did not add background coverage"
    );
    assert!(
        after
            .data
            .chunks_exact(4)
            .any(|pixel| { pixel[0] > 180 && pixel[1] > 180 && pixel[2] > 180 && pixel[3] > 0 }),
        "foreground text disappeared behind the Backplate branch"
    );
    Ok(())
}

#[test]
fn missing_background_shape_is_no_output_not_implicit_colored_rect() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let fixture = graph_fixture(&plugins, false)?;
    assert!(evaluate(&fixture.project, &plugins)?.is_empty());
    Ok(())
}

#[test]
fn descriptor_evaluation_rejects_missing_properties_and_accepts_layout_only_config() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let node = plugins.create_decorator_operation_node("backplate")?;
    let (composition, _) = Composition::new("main", WIDTH, HEIGHT, FPS, 1.0);
    let project = Project::new("config");
    let evaluators = plugins.get_property_evaluators();
    let context = FrameEvaluationContext {
        project: &project,
        composition: &composition,
        property_evaluators: &evaluators,
        plugin_manager: &plugins,
        resolved_inputs: None,
    };
    assert_eq!(
        plugins.evaluate_decorator_operation(
            &context,
            "backplate",
            node.id,
            &Default::default(),
            0.0,
        ),
        EvalOutput::NoOutput
    );
    assert_eq!(
        plugins.evaluate_decorator_operation(
            &context,
            "backplate",
            node.id,
            node.properties(),
            0.0,
        ),
        EvalOutput::Produced(DecoratorConfig::Backplate {
            target: BackplateTarget::Block,
            padding: (0.0, 0.0, 0.0, 0.0),
            offset: (0.0, 0.0),
            fit: BackplateFit::Stretch,
        })
    );
    Ok(())
}
