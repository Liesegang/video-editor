use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use library::animation::EasingFunction;
use library::cache::CacheManager;
use library::editor::project_service::ProjectManager;
use library::framing::get_frame_from_project;
use library::model::frame::Image;
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameGroup, FrameGroupKind, FrameItem};
use library::model::frame::frame::FrameInfo;
use library::model::project::{
    Composition, EvalOutput, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer,
    NodeGraphBundle, PortAddress, PortDataType, PortDefinition, PortExposure, PortOwner, PortSide,
    Project, ProjectConnection, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, TIME_PORT,
};
use library::model::property::{Keyframe, Property, PropertyValue, Vec2};
use library::model::{Clip, Node, NodeContent};
use library::plugin::{
    EFFECT_APPLY_OPERATION, EFFECT_CATEGORY, EffectPlugin, FrameEvaluationContext,
    OperationDescriptor, OperationDescriptorError, Plugin, PluginManager, ResolvedNodeInputs,
    property_port_key,
};
use library::rendering::renderer::RenderOutput;
use library::{RenderService, SkiaRenderer};
use ordered_float::OrderedFloat;
use uuid::Uuid;

const WIDTH: u64 = 16;
const HEIGHT: u64 = 8;
const FPS: f64 = 10.0;

fn output_port(key: &str, data_type: PortDataType) -> PortDefinition {
    PortDefinition::output(
        key,
        "Output",
        data_type,
        PortSide::Right,
        PortExposure::Graph,
    )
}

fn set_constant(node: &mut Node, key: &str, value: PropertyValue) {
    node.properties
        .set(key.to_string(), Property::constant(value));
}

fn vec2(x: f64, y: f64) -> PropertyValue {
    PropertyValue::Vec2(Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    })
}

fn image_wire(from: Uuid, to: Uuid) -> ProjectConnection {
    ProjectConnection::new(
        PortAddress::new(PortOwner::Node(from), IMAGE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(to), IMAGE_INPUT_PORT),
        0,
    )
}

fn shape_wire(from: Uuid, to: Uuid) -> ProjectConnection {
    ProjectConnection::new(
        PortAddress::new(PortOwner::Node(from), SHAPE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(to), SHAPE_INPUT_PORT),
        0,
    )
}

fn setup_project() -> (Project, Uuid, Uuid) {
    let mut project = Project::new("effect graph");
    let (mut composition, track) = Composition::new("main", WIDTH, HEIGHT, FPS, 10.0);
    composition.background_color = Color::black();
    let composition_id = composition.id;
    let track_id = track.id;
    project.add_track(track);
    project.add_composition(composition);
    (project, composition_id, track_id)
}

fn project_with_graph(graph: NodeGraphBundle, start_time: f64, duration: f64) -> (Project, Uuid) {
    let (mut project, _composition_id, track_id) = setup_project();
    let clip = Clip::new("effect clip", start_time, duration);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();
    project
        .insert_node_graph(NodeContainer::Clip(clip_id), graph)
        .unwrap();
    (project, clip_id)
}

fn evaluate(project: &Project, plugins: &Arc<PluginManager>, frame_number: u64) -> FrameInfo {
    get_frame_from_project(
        project,
        0,
        frame_number,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        plugins,
    )
    .unwrap()
}

fn preview(project: &Project, plugins: &Arc<PluginManager>, frame_number: u64) -> Image {
    let frame = evaluate(project, plugins, frame_number);
    let renderer = SkiaRenderer::new(
        frame.width as u32,
        frame.height as u32,
        frame.background_color.clone(),
        false,
        None,
        None,
    )
    .unwrap();
    let mut service = RenderService::new(renderer, plugins.clone(), Arc::new(CacheManager::new()));
    match service.render_from_frame_info(&frame).unwrap() {
        RenderOutput::Image(image) => image,
        RenderOutput::Texture(_) => panic!("CPU renderer unexpectedly returned a texture"),
    }
}

fn find_group(items: &[FrameItem], source_id: Uuid) -> Option<&FrameGroup> {
    items.iter().find_map(|item| match item {
        FrameItem::Group(group) if group.source_id == source_id => Some(group),
        FrameItem::Group(group) => find_group(&group.items, source_id),
        FrameItem::Object(_) => None,
    })
}

#[test]
fn effect_descriptor_factory_materializes_defaults_and_distinct_image_ports() {
    let plugins = PluginManager::default();
    for (component_id, _, _) in plugins.get_available_effects() {
        let descriptor = plugins
            .operation_descriptor(EFFECT_CATEGORY, &component_id, EFFECT_APPLY_OPERATION)
            .unwrap();
        let node = plugins.create_effect_operation_node(&component_id).unwrap();
        let NodeContent::PluginOperation(operation) = &node.content else {
            panic!("Effect factory must create a plugin operation");
        };
        assert_eq!(operation.category, EFFECT_CATEGORY);
        assert_eq!(operation.operation, EFFECT_APPLY_OPERATION);
        assert_eq!(operation.component_id, component_id);
        assert!(node.effects.is_empty());
        for definition in descriptor.properties() {
            assert_eq!(
                node.properties
                    .get(definition.name())
                    .and_then(Property::value),
                Some(definition.default_value())
            );
            assert!(
                operation
                    .declared_ports
                    .iter()
                    .any(|port| port.key == property_port_key(definition.name()))
            );
        }
        let image_input = operation
            .declared_ports
            .iter()
            .find(|port| port.key == IMAGE_INPUT_PORT)
            .unwrap();
        assert_eq!(
            image_input.direction,
            library::model::project::PortDirection::Input
        );
        assert_eq!(image_input.side, PortSide::Left);
        assert_eq!(image_input.data_type, PortDataType::Image);
        let image_output = operation
            .declared_ports
            .iter()
            .find(|port| port.key == IMAGE_OUTPUT_PORT)
            .unwrap();
        assert_eq!(
            image_output.direction,
            library::model::project::PortDirection::Output
        );
        assert_eq!(image_output.side, PortSide::Right);
        assert_eq!(image_output.data_type, PortDataType::Image);
    }

    let collision = OperationDescriptor::new(
        "test",
        "same-key",
        "same-key.v1",
        "Same Key",
        Vec::new(),
        [
            PortDefinition::input("io", "Input", PortDataType::Image),
            output_port("io", PortDataType::Image),
        ],
    );
    assert!(matches!(
        collision,
        Err(OperationDescriptorError::PortCollision { .. })
    ));
}

#[test]
fn effect_chain_uses_wiring_order_and_evaluates_keyframes_and_scalar_overrides() {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let source = manager
        .create_solid_node(Color::white(), WIDTH, HEIGHT)
        .unwrap();
    let mut blur = plugins.create_effect_operation_node("blur").unwrap();
    let dilate = plugins.create_effect_operation_node("dilate").unwrap();
    blur.properties.set(
        "sigma_x".into(),
        Property::keyframe(vec![
            Keyframe::new(0.0, 0.0.into(), EasingFunction::Linear),
            Keyframe::new(1.0, 10.0.into(), EasingFunction::Linear),
        ]),
    );
    let source_id = source.id;
    let blur_id = blur.id;
    let dilate_id = dilate.id;
    let graph = NodeGraphBundle::new(
        vec![source, blur, dilate],
        vec![
            image_wire(source_id, blur_id),
            image_wire(blur_id, dilate_id),
        ],
        Some(dilate_id),
    );
    let (mut project, clip_id) = project_with_graph(graph, 0.0, 2.0);
    project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(blur_id), property_port_key("sigma_y")),
        )
        .unwrap();

    let rendered = evaluate(&project, &plugins, 5);
    let outer = find_group(&rendered.items, dilate_id).unwrap();
    assert_eq!(outer.kind, FrameGroupKind::Effect);
    assert_eq!(outer.effects[0].effect_type, "dilate");
    let inner = find_group(&outer.items, blur_id).unwrap();
    assert_eq!(inner.kind, FrameGroupKind::Effect);
    assert_eq!(inner.effects[0].effect_type, "blur");
    assert_eq!(
        inner.effects[0].properties["sigma_x"],
        PropertyValue::Number(OrderedFloat(5.0))
    );
    assert_eq!(
        inner.effects[0].properties["sigma_y"],
        PropertyValue::Number(OrderedFloat(0.5))
    );

    let saved = project.save().unwrap();
    let loaded = Project::load(&saved).unwrap();
    assert_eq!(loaded, project);
    assert!(loaded.validation_issues().is_empty());
}

#[test]
fn unknown_missing_input_and_scalar_no_output_effects_are_safe_no_output() {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let missing = plugins.create_effect_operation_node("blur").unwrap();
    let missing_id = missing.id;
    let (project, _) = project_with_graph(
        NodeGraphBundle::new(vec![missing], Vec::new(), Some(missing_id)),
        0.0,
        2.0,
    );
    assert!(evaluate(&project, &plugins, 0).items.is_empty());

    let source = manager
        .create_solid_node(Color::white(), WIDTH, HEIGHT)
        .unwrap();
    let mut unknown = plugins.create_effect_operation_node("blur").unwrap();
    let source_id = source.id;
    let unknown_id = unknown.id;
    let NodeContent::PluginOperation(operation) = &mut unknown.content else {
        panic!()
    };
    operation.component_id = "unavailable-effect".into();
    let (project, _) = project_with_graph(
        NodeGraphBundle::new(
            vec![source, unknown],
            vec![image_wire(source_id, unknown_id)],
            Some(unknown_id),
        ),
        0.0,
        2.0,
    );
    assert!(evaluate(&project, &plugins, 0).items.is_empty());
    assert_eq!(Project::load(&project.save().unwrap()).unwrap(), project);

    let blur = plugins.create_effect_operation_node("blur").unwrap();
    let descriptor = plugins
        .operation_descriptor(EFFECT_CATEGORY, "blur", EFFECT_APPLY_OPERATION)
        .unwrap();
    let (composition, _) = Composition::new("main", WIDTH, HEIGHT, FPS, 2.0);
    let project = Project::new("scalar NoOutput");
    let evaluators = plugins.get_property_evaluators();
    let operation_effect = |key: &str, input| {
        let mut inputs = ResolvedNodeInputs::default();
        inputs.properties.insert(key.into(), input);
        let context = FrameEvaluationContext {
            project: &project,
            composition: &composition,
            property_evaluators: &evaluators,
            plugin_manager: &plugins,
            resolved_inputs: Some(&inputs),
        };
        context.build_operation_effect("blur", descriptor.properties(), &blur.properties, 0.0)
    };
    assert!(operation_effect("sigma_x", EvalOutput::NoOutput).is_none());
    assert!(
        operation_effect(
            "sigma_x",
            EvalOutput::Produced(PropertyValue::String("wrong".into()))
        )
        .is_none()
    );
    assert!(operation_effect("sigma_x", EvalOutput::Produced((-1.0).into())).is_none());
    assert!(
        operation_effect(
            "tile_mode",
            EvalOutput::Produced(PropertyValue::String("outside-options".into()))
        )
        .is_none()
    );

    let mut invalid_keyframe = blur.properties.clone();
    invalid_keyframe.set(
        "tile_mode".into(),
        Property::keyframe(vec![Keyframe::new(
            0.0,
            PropertyValue::String("outside-options".into()),
            EasingFunction::Linear,
        )]),
    );
    let context = FrameEvaluationContext {
        project: &project,
        composition: &composition,
        property_evaluators: &evaluators,
        plugin_manager: &plugins,
        resolved_inputs: None,
    };
    assert!(
        context
            .build_operation_effect("blur", descriptor.properties(), &invalid_keyframe, 0.0)
            .is_none()
    );
}

struct PostCompositeProbe {
    calls: Arc<AtomicUsize>,
}

impl Plugin for PostCompositeProbe {
    fn id(&self) -> &str {
        "post_composite_probe"
    }

    fn name(&self) -> String {
        "Post Composite Probe".into()
    }

    fn category(&self) -> String {
        "Test".into()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EffectPlugin for PostCompositeProbe {
    fn apply(
        &self,
        input: &RenderOutput,
        _params: &HashMap<String, PropertyValue>,
        _gpu_context: Option<&mut library::rendering::skia_utils::GpuContext>,
    ) -> Result<RenderOutput, library::LibraryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let RenderOutput::Image(image) = input else {
            return Err(library::LibraryError::Render(
                "probe requires a CPU image".into(),
            ));
        };
        let has_red = image
            .data
            .chunks_exact(4)
            .any(|pixel| pixel[0] > 180 && pixel[2] < 80 && pixel[3] > 0);
        let has_blue = image
            .data
            .chunks_exact(4)
            .any(|pixel| pixel[2] > 180 && pixel[0] < 80 && pixel[3] > 0);
        let replacement = if has_red && has_blue {
            [0, 255, 0]
        } else {
            [255, 0, 255]
        };
        let mut data = image.data.clone();
        for pixel in data.chunks_exact_mut(4) {
            if pixel[3] > 0 {
                pixel[..3].copy_from_slice(&replacement);
            }
        }
        Ok(RenderOutput::Image(Image::new(
            image.width,
            image.height,
            data,
        )))
    }

    fn properties(&self) -> Vec<library::model::property::PropertyDefinition> {
        Vec::new()
    }
}

fn half_solid(manager: &ProjectManager, color: Color, x: f64) -> Node {
    let mut node = manager.create_solid_node(color, WIDTH, HEIGHT).unwrap();
    set_constant(&mut node, "anchor", vec2(0.0, 0.0));
    set_constant(&mut node, "scale", vec2(50.0, 100.0));
    set_constant(&mut node, "position", vec2(x, 0.0));
    node
}

#[test]
fn merge_is_composited_before_effect_and_effect_is_applied_exactly_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let plugins = Arc::new(PluginManager::default());
    plugins.register_effect(Arc::new(PostCompositeProbe {
        calls: calls.clone(),
    }));
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let red = half_solid(
        &manager,
        Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        },
        0.0,
    );
    let blue = half_solid(
        &manager,
        Color {
            r: 0,
            g: 0,
            b: 255,
            a: 255,
        },
        WIDTH as f64 / 2.0,
    );
    let merge = Node::new("Merge", NodeContent::Merge);
    let effect = plugins
        .create_effect_operation_node("post_composite_probe")
        .unwrap();
    let red_id = red.id;
    let blue_id = blue.id;
    let merge_id = merge.id;
    let effect_id = effect.id;
    let merge_target = PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
    let graph = NodeGraphBundle::new(
        vec![red, blue, merge, effect],
        vec![
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(red_id), IMAGE_OUTPUT_PORT),
                merge_target.clone(),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(blue_id), IMAGE_OUTPUT_PORT),
                merge_target,
                1,
            ),
            image_wire(merge_id, effect_id),
        ],
        Some(effect_id),
    );
    let (project, _) = project_with_graph(graph, 0.0, 2.0);
    let frame = evaluate(&project, &plugins, 0);
    let effect_group = find_group(&frame.items, effect_id).unwrap();
    assert_eq!(effect_group.kind, FrameGroupKind::Effect);
    assert_eq!(
        find_group(&effect_group.items, merge_id).unwrap().kind,
        FrameGroupKind::Merge
    );

    let image = preview(&project, &plugins, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let green_pixels = image
        .data
        .chunks_exact(4)
        .filter(|pixel| pixel[1] > 200 && pixel[0] < 30 && pixel[2] < 30)
        .count();
    assert!(green_pixels >= (WIDTH * HEIGHT) as usize - HEIGHT as usize);
}

#[test]
fn descriptor_effect_pixels_match_legacy_embedded_effect_pixels() {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut source = manager
        .create_shape_node("M0 0 L4 0 L4 4 L0 4 Z", WIDTH, HEIGHT, 4, 4)
        .unwrap();
    let mut legacy_effect = plugins.get_default_effect_config("blur").unwrap();
    legacy_effect.properties.set(
        "sigma_x".into(),
        Property::constant(PropertyValue::Number(OrderedFloat(1.5))),
    );
    legacy_effect.properties.set(
        "sigma_y".into(),
        Property::constant(PropertyValue::Number(OrderedFloat(1.5))),
    );
    source.effects.push(legacy_effect);
    let legacy_fill = plugins.create_style_operation_node("fill").unwrap();
    let source_id = source.id;
    let legacy_fill_id = legacy_fill.id;
    let (legacy_project, _) = project_with_graph(
        NodeGraphBundle::new(
            vec![source.clone(), legacy_fill],
            vec![shape_wire(source_id, legacy_fill_id)],
            Some(legacy_fill_id),
        ),
        0.0,
        2.0,
    );

    source.effects.clear();
    let fill = plugins.create_style_operation_node("fill").unwrap();
    let mut effect = plugins.create_effect_operation_node("blur").unwrap();
    set_constant(&mut effect, "sigma_x", 1.5.into());
    set_constant(&mut effect, "sigma_y", 1.5.into());
    let fill_id = fill.id;
    let effect_id = effect.id;
    let (graph_project, _) = project_with_graph(
        NodeGraphBundle::new(
            vec![source, fill, effect],
            vec![
                shape_wire(source_id, fill_id),
                image_wire(fill_id, effect_id),
            ],
            Some(effect_id),
        ),
        0.0,
        2.0,
    );

    let legacy = preview(&legacy_project, &plugins, 0);
    let graph = preview(&graph_project, &plugins, 0);
    assert_eq!(graph.width, legacy.width);
    assert_eq!(graph.height, legacy.height);
    assert_eq!(graph.data, legacy.data);
}

#[test]
fn inactive_effect_operation_never_invokes_plugin() {
    let calls = Arc::new(AtomicUsize::new(0));
    let plugins = Arc::new(PluginManager::default());
    plugins.register_effect(Arc::new(PostCompositeProbe {
        calls: calls.clone(),
    }));
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let source = manager
        .create_solid_node(Color::white(), WIDTH, HEIGHT)
        .unwrap();
    let effect = plugins
        .create_effect_operation_node("post_composite_probe")
        .unwrap();
    let source_id = source.id;
    let effect_id = effect.id;
    let (project, _) = project_with_graph(
        NodeGraphBundle::new(
            vec![source, effect],
            vec![image_wire(source_id, effect_id)],
            Some(effect_id),
        ),
        5.0,
        2.0,
    );

    let _ = preview(&project, &plugins, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let _ = preview(&project, &plugins, 50);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
