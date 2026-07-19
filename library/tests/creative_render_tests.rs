mod support;

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use library::cache::CacheManager;
use library::core::ensemble::target::EffectorTarget;
use library::core::ensemble::types::{DecoratorConfig, EffectorConfig, EnsembleData};
use library::editor::project_service::GeneratorNodeRequest;
use library::framing::get_frame_from_project;
use library::model::frame::Image;
use library::model::frame::color::Color;
use library::model::frame::draw_type::{DrawStyle, PathEffect};
use library::model::frame::entity::{FrameContent, FrameItem, StyleConfig};
use library::model::frame::frame::FrameInfo;
use library::model::project::{
    IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeGraphBundle, PortAddress, PortOwner,
    ProjectConnection, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::model::property::{Property, PropertyValue, Vec2};
use library::model::{Clip, Composition, Node, NodeContainer, NodeContent, Project};
use library::plugin::{ExportSettings, LoadPlugin, LoadRequest, NativeImageLoader, PluginManager};
use library::rendering::renderer::{Affine2D, RenderOutput, Renderer, TextRasterRequest};
use library::{ExportService, ProjectModel, ProjectService, RenderService, SkiaRenderer};
use ordered_float::OrderedFloat;
use uuid::Uuid;

use support::generator_node_for_canvas;

const WIDTH: u32 = 128;
const HEIGHT: u32 = 80;
const FPS: f64 = 10.0;

fn set(node: &mut Node, key: &str, value: PropertyValue) {
    node.set_property(key.to_string(), Property::constant(value));
}

fn vec2(x: f64, y: f64) -> PropertyValue {
    PropertyValue::Vec2(Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    })
}

fn rgba(r: u8, g: u8, b: u8) -> PropertyValue {
    PropertyValue::Color(Color { r, g, b, a: 255 })
}

fn fill(plugins: &PluginManager, color: Color) -> Node {
    let mut node = plugins.create_style_operation_node("fill").unwrap();
    set(&mut node, "color", PropertyValue::Color(color));
    node
}

fn stroke(plugins: &PluginManager, color: Color, width: f64, dash_array: &str) -> Node {
    let mut node = plugins.create_style_operation_node("stroke").unwrap();
    set(&mut node, "color", PropertyValue::Color(color));
    set(&mut node, "width", width.into());
    set(
        &mut node,
        "dash_array",
        PropertyValue::String(dash_array.to_string()),
    );
    node
}

fn base_node(name: &str, request: GeneratorNodeRequest) -> Node {
    let mut node = generator_node_for_canvas(
        name,
        request,
        u64::from(WIDTH),
        u64::from(HEIGHT),
        u64::from(WIDTH),
        u64::from(HEIGHT),
    );
    set(&mut node, "position", vec2(8.0, 8.0));
    set(&mut node, "anchor", vec2(0.0, 0.0));
    node
}

fn text_node(text: &str) -> Node {
    let mut node = base_node(
        "text",
        GeneratorNodeRequest::Text {
            text: text.to_string(),
            font: "Arial".to_string(),
        },
    );
    set(&mut node, "text", PropertyValue::String(text.to_string()));
    set(
        &mut node,
        "font_family",
        PropertyValue::String("Arial".to_string()),
    );
    set(&mut node, "size", 30.0.into());
    node
}

fn default_text_styles(plugins: &PluginManager) -> Vec<Node> {
    vec![
        fill(
            plugins,
            Color {
                r: 230,
                g: 25,
                b: 20,
                a: 255,
            },
        ),
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
        ),
    ]
}

fn project_with_shape_graph(
    source: Node,
    shape_operations: Vec<Node>,
    styles: Vec<Node>,
) -> (Project, Uuid) {
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

    project.add_track(track);
    project.add_composition(composition);
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();

    let mut nodes = vec![source];
    let mut connections = Vec::new();
    let mut shape_output_id = source_id;
    for operation in shape_operations {
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
    project
        .insert_node_graph(
            NodeContainer::Clip(clip_id),
            NodeGraphBundle::new(nodes, connections, Some(output_id)),
        )
        .unwrap();
    (project, source_id)
}

fn project_with_image_node(node: Node) -> (Project, Uuid) {
    let mut project = Project::new("creative render e2e");
    let (mut composition, track) =
        Composition::new("main", u64::from(WIDTH), u64::from(HEIGHT), FPS, 2.0);
    composition.background_color = Color::black();
    let track_id = track.id;
    let node_id = node.id;
    let clip = Clip::new("creative clip", 0.0, 2.0);
    let clip_id = clip.id;
    project.add_track(track);
    project.add_composition(composition);
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();
    project
        .insert_node_graph(
            NodeContainer::Clip(clip_id),
            NodeGraphBundle::with_output_node(node),
        )
        .unwrap();
    (project, node_id)
}

fn evaluate(
    project: &Project,
    frame_number: u64,
    plugins: &Arc<PluginManager>,
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

fn preview(
    project: &Project,
    frame_number: u64,
    plugins: &Arc<PluginManager>,
) -> Result<Image, library::LibraryError> {
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
    match service.render_from_frame_info(&frame)? {
        RenderOutput::Image(image) => Ok(image),
        RenderOutput::Texture(_) => panic!("CPU renderer unexpectedly returned a texture"),
    }
}

fn first_content(items: &[FrameItem]) -> Option<&FrameContent> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(object) => Some(&object.content),
        FrameItem::Group(group) => first_content(&group.items),
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

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("creative-render-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn assert_preview_matches_export(
    project: &Project,
    frame_number: u64,
    plugins: &Arc<PluginManager>,
) {
    let expected = preview(project, frame_number, plugins).unwrap();
    let directory = TempDirectory::new();
    let stem = directory.0.join("frame");
    let model = ProjectModel::new(Arc::new(project.clone()), 0).unwrap();
    let renderer = SkiaRenderer::new(WIDTH, HEIGHT, Color::black(), false, None, None).unwrap();
    let mut render_service =
        RenderService::new(renderer, Arc::clone(plugins), Arc::new(CacheManager::new()));
    let mut exporter = ExportService::new(
        Arc::clone(plugins),
        "png_export".to_string(),
        Arc::new(ExportSettings::for_dimensions(WIDTH, HEIGHT, FPS)),
        1,
    );
    exporter
        .render_range(
            &mut render_service,
            &model,
            frame_number..frame_number + 1,
            stem.to_str().unwrap(),
        )
        .unwrap();
    exporter.shutdown().unwrap();

    let path = format!("{}_{frame_number:03}.png", stem.to_string_lossy());
    let exported = NativeImageLoader::new()
        .load(&LoadRequest::Image { path }, &CacheManager::new())
        .unwrap()
        .image;
    assert_eq!((exported.width, exported.height), (WIDTH, HEIGHT));
    assert_eq!(exported.data, expected.data);
}

fn assert_round_trip(project: &Project, frame_number: u64, plugins: &Arc<PluginManager>) {
    let json = project.save().unwrap();
    assert!(!json.contains("schema_version"));
    let loaded = Project::load(&json).unwrap();
    assert_eq!(
        evaluate(project, frame_number, plugins).unwrap().items,
        evaluate(&loaded, frame_number, plugins).unwrap().items
    );
    assert_eq!(
        preview(project, frame_number, plugins).unwrap().data,
        preview(&loaded, frame_number, plugins).unwrap().data
    );
}

fn effector(plugins: &PluginManager, kind: &str) -> Node {
    plugins.create_effector_operation_node(kind).unwrap()
}

fn decorator(plugins: &PluginManager, target: &str) -> Node {
    let mut node = plugins
        .create_decorator_operation_node("backplate")
        .unwrap();
    set(
        &mut node,
        "target",
        PropertyValue::String(target.to_string()),
    );
    set(
        &mut node,
        "shape",
        PropertyValue::String("RoundRect".to_string()),
    );
    set(&mut node, "color", rgba(20, 170, 60));
    set(&mut node, "padding", 2.0.into());
    set(&mut node, "radius", 3.0.into());
    node
}

#[test]
fn text_converter_styles_transform_round_trip_and_export_are_real_pixels() {
    let plugins = Arc::new(PluginManager::default());
    let mut node = text_node("TEXT");
    set(&mut node, "position", vec2(14.0, 11.0));
    set(&mut node, "scale", vec2(90.0, 110.0));
    set(&mut node, "rotation", 4.0.into());
    set(&mut node, "opacity", 80.0.into());
    let (project, node_id) =
        project_with_shape_graph(node, Vec::new(), default_text_styles(&plugins));

    let frame = evaluate(&project, 0, &plugins).unwrap();
    let FrameContent::Text {
        text,
        font,
        size,
        styles,
        ensemble,
        transform,
        ..
    } = first_content(&frame.items).unwrap()
    else {
        panic!("text converter did not produce FrameContent::Text");
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
    assert_eq!(transform.opacity, 0.8);

    let standard = preview(&project, 0, &plugins).unwrap();
    assert!(
        dominant_pixels(&standard, 0) > 10,
        "fill pixels disappeared"
    );
    assert!(
        dominant_pixels(&standard, 2) > 10,
        "stroke pixels disappeared"
    );

    let (ensemble_project, _) = project_with_shape_graph(
        project.get_node(node_id).unwrap().clone(),
        vec![effector(&plugins, "transform")],
        default_text_styles(&plugins),
    );
    let ensemble_image = preview(&ensemble_project, 0, &plugins).unwrap();
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
        moved.get_node_mut(node_id).unwrap(),
        "position",
        vec2(30.0, 16.0),
    );
    let moved_image = preview(&moved, 0, &plugins).unwrap();
    assert_ne!(hash(&standard), hash(&moved_image));
    assert!(colored_centroid(&moved_image).unwrap().0 > colored_centroid(&standard).unwrap().0);

    assert_round_trip(&ensemble_project, 0, &plugins);
    assert_preview_matches_export(&project, 0, &plugins);
}

#[test]
fn shape_converter_fill_stroke_path_effect_transform_and_invalid_paths_are_explicit() {
    let plugins = Arc::new(PluginManager::default());
    let path = "M 0 0 L 42 0 L 42 27 L 0 27 Z";
    let mut node = base_node(
        "shape",
        GeneratorNodeRequest::Shape {
            path: path.to_string(),
        },
    );
    set(&mut node, "path", PropertyValue::String(path.to_string()));
    set(&mut node, "position", vec2(22.0, 18.0));
    set(&mut node, "rotation", 8.0.into());
    set(&mut node, "opacity", 90.0.into());
    set(
        &mut node,
        "path_effect",
        PropertyValue::String("Corner".to_string()),
    );
    set(&mut node, "path_effect_radius", 5.0.into());
    let styles = vec![
        fill(
            &plugins,
            Color {
                r: 20,
                g: 220,
                b: 40,
                a: 255,
            },
        ),
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
        ),
    ];
    let (project, node_id) = project_with_shape_graph(node, Vec::new(), styles);

    let frame = evaluate(&project, 0, &plugins).unwrap();
    let FrameContent::Shape {
        path: converted_path,
        styles,
        path_effects,
        transform,
        ..
    } = first_content(&frame.items).unwrap()
    else {
        panic!("shape converter did not produce FrameContent::Shape");
    };
    assert_eq!(converted_path, path);
    assert_eq!(styles.len(), 1);
    assert_eq!(path_effects, &[PathEffect::Corner { radius: 5.0 }]);
    assert_eq!((transform.position.x, transform.position.y), (22.0, 18.0));
    assert_eq!(transform.rotation, 8.0);

    let rendered = preview(&project, 0, &plugins).unwrap();
    assert!(
        dominant_pixels(&rendered, 1) > 100,
        "shape Fill was not rendered"
    );
    assert!(
        dominant_pixels(&rendered, 0) > 20,
        "shape Stroke was not rendered"
    );

    let mut no_effect = project.clone();
    set(
        no_effect.get_node_mut(node_id).unwrap(),
        "path_effect",
        PropertyValue::String("None".to_string()),
    );
    assert_ne!(
        hash(&rendered),
        hash(&preview(&no_effect, 0, &plugins).unwrap()),
        "Corner path effect did not change pixels"
    );

    let mut moved = project.clone();
    set(
        moved.get_node_mut(node_id).unwrap(),
        "position",
        vec2(44.0, 28.0),
    );
    let moved = preview(&moved, 0, &plugins).unwrap();
    let original_center = colored_centroid(&rendered).unwrap();
    let moved_center = colored_centroid(&moved).unwrap();
    assert!(moved_center.0 > original_center.0 + 10.0);
    assert!(moved_center.1 > original_center.1 + 4.0);

    for malformed in ["", "this is not SVG path data"] {
        let mut invalid = project.clone();
        set(
            invalid.get_node_mut(node_id).unwrap(),
            "path",
            PropertyValue::String(malformed.to_string()),
        );
        assert!(evaluate(&invalid, 0, &plugins).unwrap().items.is_empty());
        assert_eq!(light_sum(&preview(&invalid, 0, &plugins).unwrap()), 0);
    }

    assert_round_trip(&project, 0, &plugins);
    assert_preview_matches_export(&project, 0, &plugins);
}

#[test]
fn sksl_converter_uses_runtime_time_and_matches_png_export() {
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
    );
    set(
        &mut node,
        "shader",
        PropertyValue::String(shader.to_string()),
    );
    set(&mut node, "width", 96.0.into());
    set(&mut node, "height", 54.0.into());
    set(&mut node, "position", vec2(10.0, 9.0));
    let (project, _) = project_with_image_node(node);

    let frame = evaluate(&project, 0, &plugins).unwrap();
    let FrameContent::SkSL {
        shader: converted,
        resolution,
        transform,
        ..
    } = first_content(&frame.items).unwrap()
    else {
        panic!("SkSL converter did not produce FrameContent::SkSL");
    };
    assert_eq!(converted, shader);
    assert_eq!(*resolution, (96.0, 54.0));
    assert_eq!((transform.position.x, transform.position.y), (10.0, 9.0));

    let first = preview(&project, 0, &plugins).unwrap();
    let late = preview(&project, 9, &plugins).unwrap();
    assert!(light_sum(&first) > 0);
    assert_ne!(
        hash(&first),
        hash(&late),
        "iTime did not reach the real SkSL renderer"
    );

    assert_round_trip(&project, 9, &plugins);
    assert_preview_matches_export(&project, 9, &plugins);
}

#[test]
fn ensemble_step_delay_randomize_and_independent_crud_use_one_runtime_path() {
    let plugins = Arc::new(PluginManager::default());
    let source = text_node("ABCD");
    let mut step = effector(&plugins, "step_delay");
    set(&mut step, "delay", 0.2.into());
    set(&mut step, "duration", 0.2.into());
    set(&mut step, "from_opacity", 0.0.into());
    set(&mut step, "to_opacity", 100.0.into());
    set(
        &mut step,
        "target",
        PropertyValue::String("Block".to_string()),
    );
    let step_id = step.id;
    let (project, node_id) = project_with_shape_graph(
        source.clone(),
        vec![step.clone()],
        default_text_styles(&plugins),
    );

    let frame = evaluate(&project, 4, &plugins).unwrap();
    let FrameContent::Text {
        ensemble: Some(ensemble),
        styles,
        ..
    } = first_content(&frame.items).unwrap()
    else {
        panic!("the explicit Shape Effector did not produce EnsembleData");
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
        panic!("StepDelay plugin produced the wrong config variant");
    };
    assert!((delay_per_element - 0.2).abs() < f32::EPSILON);
    assert!((duration - 0.2).abs() < f32::EPSILON);
    assert_eq!(from_opacity, 0.0);
    assert_eq!(to_opacity, 100.0);
    assert_eq!(target, EffectorTarget::Block);

    let start = preview(&project, 0, &plugins).unwrap();
    let middle = preview(&project, 4, &plugins).unwrap();
    let end = preview(&project, 10, &plugins).unwrap();
    assert_eq!(light_sum(&start), 0);
    assert!(light_sum(&middle) > light_sum(&start));
    assert!(light_sum(&end) > light_sum(&middle));

    let mut random = effector(&plugins, "randomize");
    set(&mut random, "seed", 7.0.into());
    set(&mut random, "translate_range", 4.0.into());
    set(&mut random, "rotate_range", 8.0.into());
    set(&mut random, "scale_range", 0.35.into());
    set(
        &mut random,
        "target",
        PropertyValue::String("Char".to_string()),
    );
    let random_id = random.id;
    let (randomized, _) = project_with_shape_graph(
        source.clone(),
        vec![step.clone(), random.clone()],
        default_text_styles(&plugins),
    );
    let random_a = preview(&randomized, 10, &plugins).unwrap();
    let random_b = preview(&randomized, 10, &plugins).unwrap();
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
        changed_seed.get_node_mut(random_id).unwrap(),
        "seed",
        8.0.into(),
    );
    assert_ne!(
        hash(&random_a),
        hash(&preview(&changed_seed, 10, &plugins).unwrap()),
        "changing Randomize seed did not change the rendered characters"
    );

    let mut scale_random = random;
    set(&mut scale_random, "translate_range", 0.0.into());
    set(&mut scale_random, "rotate_range", 0.0.into());
    let (scale_only, _) = project_with_shape_graph(
        source,
        vec![step, scale_random],
        default_text_styles(&plugins),
    );
    assert_ne!(
        hash(&end),
        hash(&preview(&scale_only, 10, &plugins).unwrap()),
        "Randomize scale_range was ignored"
    );

    let shared = Arc::new(RwLock::new(project.clone()));
    let service = ProjectService::new(Arc::clone(&shared), Arc::clone(&plugins));
    service.add_effector(node_id, "opacity").unwrap();
    service.add_decorator(node_id, "backplate").unwrap();
    let locked = shared.read().unwrap();
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
        .expect("add_effector must author a standalone operation Node");
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
        .expect("add_decorator must author a standalone operation Node");
    assert!(locked.connections.iter().any(|connection| {
        connection.from == PortAddress::new(PortOwner::Node(opacity_node.id), SHAPE_OUTPUT_PORT)
            && connection.to
                == PortAddress::new(PortOwner::Node(backplate_node.id), SHAPE_INPUT_PORT)
    }));
    drop(locked);

    assert_round_trip(&randomized, 10, &plugins);
    assert_preview_matches_export(&project, 10, &plugins);
}

#[test]
fn multiline_backplates_cover_char_line_block_and_follow_transforms() {
    let plugins = Arc::new(PluginManager::default());
    let mut hashes = Vec::new();
    for target in ["Char", "Line", "Block"] {
        let node = text_node("A\nBBB");
        let (project, _) = project_with_shape_graph(
            node,
            vec![decorator(&plugins, target)],
            default_text_styles(&plugins),
        );
        let rendered = preview(&project, 0, &plugins).unwrap();
        assert!(dominant_pixels(&rendered, 1) > 20);
        hashes.push(hash(&rendered));
        assert_round_trip(&project, 0, &plugins);
    }
    hashes.sort_unstable();
    hashes.dedup();
    assert_eq!(
        hashes.len(),
        3,
        "Char/Line/Block backplates collapsed to one geometry"
    );

    for target in ["Line", "Block"] {
        let base = text_node("A\nBBB");
        let backplate = decorator(&plugins, target);
        let (base_project, _) = project_with_shape_graph(
            base.clone(),
            vec![backplate.clone()],
            vec![fill(&plugins, Color::black())],
        );
        let base_image = preview(&base_project, 0, &plugins).unwrap();

        let mut transform = effector(&plugins, "transform");
        set(&mut transform, "tx", 24.0.into());
        set(&mut transform, "ty", 5.0.into());
        set(&mut transform, "rotation", 6.0.into());
        let (moved, _) = project_with_shape_graph(
            base,
            vec![backplate, transform],
            vec![fill(&plugins, Color::black())],
        );
        let moved_image = preview(&moved, 0, &plugins).unwrap();
        let base_center = colored_centroid(&base_image).unwrap();
        let moved_center = colored_centroid(&moved_image).unwrap();
        assert!(
            moved_center.0 > base_center.0 + 15.0,
            "{target} backplate did not follow the effector transform"
        );
        assert_ne!(hash(&base_image), hash(&moved_image));
    }
}

#[test]
fn effector_block_line_and_char_targets_are_distinct_in_multiline_pixels() {
    let plugins = Arc::new(PluginManager::default());
    let render_target = |target: &str| {
        let node = text_node("AB\nCD");
        let mut step = effector(&plugins, "step_delay");
        set(&mut step, "delay", 0.3.into());
        set(&mut step, "duration", 0.1.into());
        set(&mut step, "from_opacity", 0.0.into());
        set(&mut step, "to_opacity", 100.0.into());
        set(
            &mut step,
            "target",
            PropertyValue::String(target.to_string()),
        );
        let (project, _) =
            project_with_shape_graph(node, vec![step], default_text_styles(&plugins));
        preview(&project, 2, &plugins).unwrap()
    };

    let block = render_target("Block");
    let line = render_target("Line");
    let character = render_target("Char");
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
}

#[test]
fn empty_text_is_safe_missing_text_is_validation_and_parts_is_render_error() {
    let plugins = Arc::new(PluginManager::default());
    let empty_node = text_node("");
    let (empty_project, _) =
        project_with_shape_graph(empty_node, Vec::new(), default_text_styles(&plugins));
    let empty = preview(&empty_project, 0, &plugins).unwrap();
    assert_eq!(light_sum(&empty), 0);

    let complete_node = text_node("missing");
    let mut missing_json = serde_json::to_value(complete_node).unwrap();
    missing_json["properties"] = serde_json::json!({});
    let missing_node: Node = serde_json::from_value(missing_json).unwrap();
    let (missing_project, _) =
        project_with_shape_graph(missing_node, Vec::new(), default_text_styles(&plugins));
    assert!(
        evaluate(&missing_project, 0, &plugins)
            .unwrap()
            .items
            .is_empty()
    );

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
    let mut renderer = SkiaRenderer::new(WIDTH, HEIGHT, Color::black(), false, None, None).unwrap();
    let error = renderer
        .rasterize_text_layer(TextRasterRequest {
            text: "A",
            size: 30.0,
            font_name: "Arial",
            styles: &styles,
            ensemble: Some(&ensemble),
            transform: Affine2D::IDENTITY,
            current_time: 0.0,
        })
        .unwrap_err();
    assert!(error.to_string().contains("EffectorTarget::Parts"));

    let decorator_parts = EnsembleData {
        enabled: true,
        effector_configs: Vec::new(),
        decorator_configs: vec![DecoratorConfig::Backplate {
            target: library::core::ensemble::decorators::BackplateTarget::Parts,
            shape: library::core::ensemble::decorators::BackplateShape::Rect,
            color: Color::white(),
            padding: (0.0, 0.0, 0.0, 0.0),
            corner_radius: 0.0,
        }],
        patches: Default::default(),
    };
    let error = renderer
        .rasterize_text_layer(TextRasterRequest {
            text: "A",
            size: 30.0,
            font_name: "Arial",
            styles: &styles,
            ensemble: Some(&decorator_parts),
            transform: Affine2D::IDENTITY,
            current_time: 0.0,
        })
        .unwrap_err();
    assert!(error.to_string().contains("BackplateTarget::Parts"));
}
