use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use library::animation::EasingFunction;
use library::cache::CacheManager;
use library::core::ensemble::decorators::{BackplateShape, BackplateTarget};
use library::core::ensemble::types::{DecoratorConfig, EnsembleData};
use library::editor::project_service::ProjectManager;
use library::framing::get_frame_from_project;
use library::model::frame::Image;
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameContent, FrameItem};
use library::model::frame::frame::FrameInfo;
use library::model::project::{
    Composition, EvalOutput, NodeContainer, NodeGraphBundle, PortAddress, PortDataType,
    PortDirection, PortMultiplicity, PortOwner, PortSide, Project, ProjectConnection,
    SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, TIME_PORT,
};
use library::model::property::{
    Keyframe, Property, PropertyDefinition, PropertyMap, PropertyValue,
};
use library::model::{Clip, Node, NodeContent};
use library::plugin::{
    DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY, DecoratorPlugin, FrameEvaluationContext,
    OperationDescriptor, OperationDescriptorError, Plugin, PluginManager, ResolvedNodeInputs,
    property_port_key, property_ui_type_to_port_data_type,
};
use library::rendering::renderer::{Affine2D, RenderOutput, Renderer, ShapeRasterRequest};
use library::{RenderService, SkiaRenderer};
use uuid::Uuid;

const WIDTH: u64 = 128;
const HEIGHT: u64 = 80;
const FPS: f64 = 10.0;

fn set_constant(node: &mut Node, key: &str, value: PropertyValue) {
    assert!(
        node.set_property(key.to_string(), Property::constant(value))
            .is_ok(),
        "operation descriptor must initialize {key}"
    );
}

fn shape_wire(from: Uuid, to: Uuid) -> ProjectConnection {
    ProjectConnection::new(
        PortAddress::new(PortOwner::Node(from), SHAPE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(to), SHAPE_INPUT_PORT),
        0,
    )
}

fn shape_source_id(graph: &NodeGraphBundle) -> Uuid {
    graph
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
        .expect("shape source")
        .id
}

fn insert_decorator_chain(graph: &mut NodeGraphBundle, decorator_ids: &[Uuid]) {
    let source_id = shape_source_id(graph);
    let mut targets = Vec::new();
    graph.connections.retain(|connection| {
        let is_shape_fanout = connection.from
            == PortAddress::new(PortOwner::Node(source_id), SHAPE_OUTPUT_PORT)
            && connection.to.port == SHAPE_INPUT_PORT;
        if is_shape_fanout {
            targets.push(connection.to.clone());
        }
        !is_shape_fanout
    });
    assert!(!targets.is_empty(), "factory must expose a Shape consumer");

    let mut upstream = source_id;
    for decorator_id in decorator_ids {
        graph.connections.push(shape_wire(upstream, *decorator_id));
        upstream = *decorator_id;
    }
    for target in targets {
        graph.connections.push(ProjectConnection::new(
            PortAddress::new(PortOwner::Node(upstream), SHAPE_OUTPUT_PORT),
            target,
            0,
        ));
    }
}

fn setup_project() -> (Project, Uuid, Uuid) {
    let mut project = Project::new("decorator graph");
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
    let clip = Clip::new("decorator clip", start_time, duration);
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

fn first_content(items: &[FrameItem]) -> Option<&FrameContent> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(object) => Some(&object.content),
        FrameItem::Group(group) => first_content(&group.items),
    })
}

#[test]
fn descriptor_factory_and_text_shape_sources_have_complete_typed_contracts() {
    let plugins = Arc::new(PluginManager::default());
    assert_eq!(plugins.get_available_decorators(), ["backplate"]);
    let descriptor = plugins
        .operation_descriptor(DECORATOR_CATEGORY, "backplate", DECORATOR_APPLY_OPERATION)
        .unwrap();
    let decorator = plugins
        .create_decorator_operation_node("backplate")
        .unwrap();
    let NodeContent::PluginOperation(operation) = decorator.content() else {
        panic!()
    };
    assert_eq!(operation.category, DECORATOR_CATEGORY);
    assert_eq!(operation.component_id, "backplate");
    assert_eq!(operation.operation, DECORATOR_APPLY_OPERATION);
    assert_eq!(operation.declared_ports, descriptor.declared_ports());
    assert_eq!(
        descriptor
            .properties()
            .iter()
            .map(PropertyDefinition::name)
            .collect::<Vec<_>>(),
        ["target", "shape", "color", "padding", "radius"]
    );
    for definition in descriptor.properties() {
        assert_eq!(
            decorator
                .properties()
                .get(definition.name())
                .and_then(Property::value),
            Some(definition.default_value())
        );
        let input = operation
            .declared_ports
            .iter()
            .find(|port| port.key == property_port_key(definition.name()))
            .unwrap();
        assert_eq!(input.direction, PortDirection::Input);
        assert_eq!(
            input.data_type,
            property_ui_type_to_port_data_type(definition.ui_type())
        );
    }
    let input = operation
        .declared_ports
        .iter()
        .find(|port| port.key == SHAPE_INPUT_PORT)
        .unwrap();
    assert_eq!(input.direction, PortDirection::Input);
    assert_eq!(input.data_type, PortDataType::Shape);
    assert_eq!(input.multiplicity, PortMultiplicity::Single);
    let output = operation
        .declared_ports
        .iter()
        .find(|port| port.key == SHAPE_OUTPUT_PORT)
        .unwrap();
    assert_eq!(output.direction, PortDirection::Output);
    assert_eq!(output.side, PortSide::Right);
    assert_eq!(output.data_type, PortDataType::Shape);

    let manager = ProjectManager::new(Arc::new(RwLock::new(Project::new("factory"))), plugins);
    let text = manager
        .create_text_node("typed", "Arial", WIDTH, HEIGHT)
        .unwrap();
    let shape = manager
        .create_shape_node("M0 0 L10 0 L10 10 Z", WIDTH, HEIGHT, 10, 10)
        .unwrap();
    let (mut project, composition_id, _) = setup_project();
    let text_id = text.id;
    let shape_id = shape.id;
    project.add_node(text);
    project.add_node(shape);
    project
        .attach_node_to_container(NodeContainer::Composition(composition_id), text_id)
        .unwrap();
    project
        .attach_node_to_container(NodeContainer::Composition(composition_id), shape_id)
        .unwrap();
    for source in [text_id, shape_id] {
        let output = project
            .port_definition(
                &PortAddress::new(PortOwner::Node(source), SHAPE_OUTPUT_PORT),
                PortDirection::Output,
            )
            .unwrap();
        assert_eq!(output.data_type, PortDataType::Shape);
        assert_eq!(output.multiplicity, PortMultiplicity::Single);
        assert_eq!(output.side, PortSide::Right);
    }
}

#[test]
fn graph_order_keyframes_and_scalar_overrides_build_decorators_and_roundtrip() {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_text_graph("ORDER", "Arial", WIDTH, HEIGHT)
        .unwrap();
    let mut first = plugins
        .create_decorator_operation_node("backplate")
        .unwrap();
    first
        .set_property(
            "padding".into(),
            Property::keyframe(vec![
                Keyframe::new(0.0, 0.0.into(), EasingFunction::Linear),
                Keyframe::new(1.0, 10.0.into(), EasingFunction::Linear),
            ]),
        )
        .expect("backplate descriptor initializes padding");
    set_constant(&mut first, "target", PropertyValue::String("Char".into()));
    set_constant(
        &mut first,
        "shape",
        PropertyValue::String("RoundRect".into()),
    );
    let second = plugins
        .create_decorator_operation_node("backplate")
        .unwrap();
    let first_id = first.id;
    let second_id = second.id;
    graph.nodes.extend([first, second]);
    insert_decorator_chain(&mut graph, &[first_id, second_id]);
    let (mut project, clip_id) = project_with_graph(graph, 0.0, 2.0);
    project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(second_id), property_port_key("radius")),
        )
        .unwrap();

    let rendered = evaluate(&project, &plugins, 5);
    let FrameContent::Text {
        ensemble: Some(ensemble),
        ..
    } = first_content(&rendered.items).unwrap()
    else {
        panic!()
    };
    assert_eq!(ensemble.decorator_configs.len(), 2);
    assert!(matches!(
        &ensemble.decorator_configs[0],
        DecoratorConfig::Backplate {
            target: BackplateTarget::Char,
            shape: BackplateShape::RoundedRect,
            padding,
            ..
        } if (padding.0 - 5.0).abs() < f32::EPSILON
            && padding.0 == padding.1
            && padding.1 == padding.2
            && padding.2 == padding.3
    ));
    assert!(matches!(
        &ensemble.decorator_configs[1],
        DecoratorConfig::Backplate {
            target: BackplateTarget::Block,
            shape: BackplateShape::Rect,
            corner_radius,
            ..
        } if (corner_radius - 0.5).abs() < f32::EPSILON
    ));

    let saved = project.save().unwrap();
    assert!(!saved.contains("schema_version"));
    let loaded = Project::load(&saved).unwrap();
    assert_eq!(loaded, project);
    assert!(loaded.validation_issues().is_empty());
    assert_eq!(
        first_content(&evaluate(&loaded, &plugins, 5).items),
        first_content(&rendered.items)
    );
}

#[test]
fn missing_invalid_unknown_and_scalar_no_output_do_not_restore_legacy_decorators() {
    let plugins = Arc::new(PluginManager::default());
    let backplate = plugins
        .create_decorator_operation_node("backplate")
        .unwrap();
    let (composition, _) = Composition::new("main", WIDTH, HEIGHT, FPS, 2.0);
    let project = Project::new("validation");
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
            backplate.id,
            &PropertyMap::new(),
            0.0
        ),
        EvalOutput::NoOutput
    );

    let mut invalid_shape = backplate.properties().clone();
    invalid_shape.set(
        "shape".into(),
        Property::keyframe(vec![Keyframe::new(
            0.0,
            PropertyValue::String("outside-options".into()),
            EasingFunction::Linear,
        )]),
    );
    assert_eq!(
        plugins.evaluate_decorator_operation(
            &context,
            "backplate",
            backplate.id,
            &invalid_shape,
            0.0
        ),
        EvalOutput::NoOutput
    );

    let mut scalar = ResolvedNodeInputs::default();
    scalar
        .properties
        .insert("padding".into(), EvalOutput::NoOutput);
    let scalar_context = FrameEvaluationContext {
        resolved_inputs: Some(&scalar),
        ..context
    };
    assert_eq!(
        plugins.evaluate_decorator_operation(
            &scalar_context,
            "backplate",
            backplate.id,
            backplate.properties(),
            0.0
        ),
        EvalOutput::NoOutput
    );

    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_text_graph("unknown", "Arial", WIDTH, HEIGHT)
        .unwrap();
    let unknown = plugins
        .create_decorator_operation_node("backplate")
        .unwrap();
    let unknown_id = unknown.id;
    let mut unknown_json = serde_json::to_value(unknown).unwrap();
    unknown_json["content"]["data"]["component_id"] =
        serde_json::Value::String("unavailable-decorator".to_string());
    let unknown: Node = serde_json::from_value(unknown_json).unwrap();
    graph.nodes.push(unknown);
    insert_decorator_chain(&mut graph, &[unknown_id]);
    let (project, _) = project_with_graph(graph, 0.0, 2.0);
    let rendered = evaluate(&project, &plugins, 0);
    assert!(
        rendered.items.is_empty(),
        "a required unknown Shape operation must make the branch NoOutput"
    );
    assert_eq!(Project::load(&project.save().unwrap()).unwrap(), project);
}

struct CountingDecoratorPlugin {
    evaluations: Arc<AtomicUsize>,
    descriptors: Arc<AtomicUsize>,
}

impl Plugin for CountingDecoratorPlugin {
    fn id(&self) -> &str {
        "counting"
    }

    fn name(&self) -> String {
        "Counting".into()
    }

    fn category(&self) -> String {
        "Test".into()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl DecoratorPlugin for CountingDecoratorPlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        Vec::new()
    }

    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        self.descriptors.fetch_add(1, Ordering::SeqCst);
        OperationDescriptor::decorator(self.id(), self.name(), self.properties())
    }

    fn evaluate_source(
        &self,
        _context: &FrameEvaluationContext,
        _source_id: Uuid,
        _properties: &PropertyMap,
        _eval_time: f64,
    ) -> Option<DecoratorConfig> {
        self.evaluations.fetch_add(1, Ordering::SeqCst);
        Some(DecoratorConfig::Backplate {
            target: BackplateTarget::Block,
            shape: BackplateShape::Rect,
            color: Color::black(),
            padding: (0.0, 0.0, 0.0, 0.0),
            corner_radius: 0.0,
        })
    }
}

#[test]
fn disabled_and_inactive_decorator_operations_short_circuit_before_plugin_work() {
    let evaluations = Arc::new(AtomicUsize::new(0));
    let descriptors = Arc::new(AtomicUsize::new(0));
    let plugins = Arc::new(PluginManager::default());
    plugins.register_decorator_plugin(Arc::new(CountingDecoratorPlugin {
        evaluations: evaluations.clone(),
        descriptors: descriptors.clone(),
    }));
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_text_graph("inactive", "Arial", WIDTH, HEIGHT)
        .unwrap();
    let mut counting = plugins.create_decorator_operation_node("counting").unwrap();
    counting.enabled = false;
    let counting_id = counting.id;
    graph.nodes.push(counting);
    insert_decorator_chain(&mut graph, &[counting_id]);
    let descriptor_baseline = descriptors.load(Ordering::SeqCst);
    let (mut project, _) = project_with_graph(graph, 0.0, 2.0);

    assert!(evaluate(&project, &plugins, 0).items.is_empty());
    assert_eq!(evaluations.load(Ordering::SeqCst), 0);
    assert_eq!(
        descriptors.load(Ordering::SeqCst),
        descriptor_baseline,
        "disabled Shape operations must not look up a plugin descriptor"
    );

    project.get_node_mut(counting_id).unwrap().enabled = true;
    assert!(first_content(&evaluate(&project, &plugins, 0).items).is_some());
    assert_eq!(evaluations.load(Ordering::SeqCst), 1);

    let inactive_graph = {
        let mut graph = manager
            .create_text_graph("inactive", "Arial", WIDTH, HEIGHT)
            .unwrap();
        let counting = plugins.create_decorator_operation_node("counting").unwrap();
        let counting_id = counting.id;
        graph.nodes.push(counting);
        insert_decorator_chain(&mut graph, &[counting_id]);
        graph
    };
    let (inactive, _) = project_with_graph(inactive_graph, 5.0, 2.0);
    assert!(evaluate(&inactive, &plugins, 0).items.is_empty());
    assert_eq!(evaluations.load(Ordering::SeqCst), 1);
}

#[test]
fn graph_backplate_pixels_are_stable_across_project_roundtrip() {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let configure = |node: &mut Node| {
        node.set_property(
            "target".into(),
            Property::constant(PropertyValue::String("Char".into())),
        )
        .expect("backplate descriptor initializes target");
        node.set_property(
            "shape".into(),
            Property::constant(PropertyValue::String("RoundRect".into())),
        )
        .expect("backplate descriptor initializes shape");
        node.set_property(
            "color".into(),
            Property::constant(PropertyValue::Color(Color {
                r: 20,
                g: 180,
                b: 60,
                a: 255,
            })),
        )
        .expect("backplate descriptor initializes color");
        node.set_property("padding".into(), Property::constant(3.0.into()))
            .expect("backplate descriptor initializes padding");
        node.set_property("radius".into(), Property::constant(2.0.into()))
            .expect("backplate descriptor initializes radius");
    };

    let mut graph = manager
        .create_text_graph("PARITY", "Arial", WIDTH, HEIGHT)
        .unwrap();
    let mut backplate = plugins
        .create_decorator_operation_node("backplate")
        .unwrap();
    configure(&mut backplate);
    let backplate_id = backplate.id;
    graph.nodes.push(backplate);
    insert_decorator_chain(&mut graph, &[backplate_id]);
    let (graph_project, _) = project_with_graph(graph, 0.0, 2.0);

    let graph_frame = evaluate(&graph_project, &plugins, 0);
    let FrameContent::Text {
        ensemble: Some(graph_ensemble),
        ..
    } = first_content(&graph_frame.items).unwrap()
    else {
        panic!()
    };
    assert_eq!(graph_ensemble.decorator_configs.len(), 1);
    let expected = preview(&graph_project, &plugins, 0);
    assert!(expected.data.iter().any(|channel| *channel != 0));

    let loaded = Project::load(&graph_project.save().unwrap()).unwrap();
    assert_eq!(loaded, graph_project);
    assert_eq!(
        preview(&loaded, &plugins, 0).data,
        expected.data,
        "the explicit Shape Decorator graph must survive serialization"
    );
}

#[test]
fn path_backplates_render_one_stable_element_before_style() {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let render_target = |target: &str| {
        let mut graph = manager
            .create_shape_graph("M 30 20 H 90 V 55 H 30 Z", WIDTH, HEIGHT, 60, 35)
            .unwrap();
        let mut backplate = plugins
            .create_decorator_operation_node("backplate")
            .unwrap();
        set_constant(
            &mut backplate,
            "target",
            PropertyValue::String(target.into()),
        );
        set_constant(
            &mut backplate,
            "shape",
            PropertyValue::String("Rect".into()),
        );
        set_constant(
            &mut backplate,
            "color",
            PropertyValue::Color(Color {
                r: 12,
                g: 220,
                b: 35,
                a: 255,
            }),
        );
        set_constant(&mut backplate, "padding", 7.0.into());
        let backplate_id = backplate.id;
        graph.nodes.push(backplate);
        insert_decorator_chain(&mut graph, &[backplate_id]);
        let (project, _) = project_with_graph(graph, 0.0, 2.0);
        let frame = evaluate(&project, &plugins, 0);
        let FrameContent::Shape {
            ensemble: Some(ensemble),
            ..
        } = first_content(&frame.items).unwrap()
        else {
            panic!("Path Decorator must survive the Shape -> Image boundary")
        };
        assert_eq!(ensemble.decorator_configs.len(), 1);
        let image = preview(&project, &plugins, 0);
        let green = image
            .data
            .chunks_exact(4)
            .filter(|pixel| {
                u16::from(pixel[1]) > 150
                    && u16::from(pixel[1]) > u16::from(pixel[0]) + 80
                    && u16::from(pixel[1]) > u16::from(pixel[2]) + 80
            })
            .count();
        assert!(green > 100, "the padded Path backplate was not rasterized");
        image
    };

    let character = render_target("Char");
    let line = render_target("Line");
    let block = render_target("Block");
    assert_eq!(character.data, line.data);
    assert_eq!(line.data, block.data);
}

#[test]
fn rounded_path_backplate_is_drawn_once_without_alpha_overdraw() {
    let ensemble = EnsembleData {
        enabled: true,
        effector_configs: Vec::new(),
        decorator_configs: vec![DecoratorConfig::Backplate {
            target: BackplateTarget::Block,
            shape: BackplateShape::RoundedRect,
            color: Color {
                r: 40,
                g: 120,
                b: 220,
                a: 128,
            },
            padding: (4.0, 4.0, 4.0, 4.0),
            corner_radius: 6.0,
        }],
        patches: Default::default(),
    };
    let mut renderer = SkiaRenderer::new(
        WIDTH as u32,
        HEIGHT as u32,
        Color::black(),
        false,
        None,
        None,
    )
    .unwrap();
    let RenderOutput::Image(image) = renderer
        .rasterize_shape_layer(ShapeRasterRequest {
            path_data: "M 20 20 H 60 V 40 H 20 Z",
            styles: &[],
            path_effects: &[],
            ensemble: Some(&ensemble),
            transform: Affine2D::IDENTITY,
        })
        .unwrap()
    else {
        panic!("CPU renderer unexpectedly returned a texture")
    };
    let center = ((30 * WIDTH + 40) * 4) as usize;
    assert_eq!(image.data[center + 3], 128);
}

#[test]
fn path_backplate_parts_target_is_explicitly_unsupported() {
    let ensemble = EnsembleData {
        enabled: true,
        effector_configs: Vec::new(),
        decorator_configs: vec![DecoratorConfig::Backplate {
            target: BackplateTarget::Parts,
            shape: BackplateShape::Rect,
            color: Color::white(),
            padding: (0.0, 0.0, 0.0, 0.0),
            corner_radius: 0.0,
        }],
        patches: Default::default(),
    };
    let mut renderer = SkiaRenderer::new(
        WIDTH as u32,
        HEIGHT as u32,
        Color::black(),
        false,
        None,
        None,
    )
    .unwrap();
    let error = renderer
        .rasterize_shape_layer(ShapeRasterRequest {
            path_data: "M 10 10 H 40 V 30 H 10 Z",
            styles: &[],
            path_effects: &[],
            ensemble: Some(&ensemble),
            transform: Affine2D::IDENTITY,
        })
        .unwrap_err();
    assert!(error.to_string().contains("BackplateTarget::Parts"));
}
