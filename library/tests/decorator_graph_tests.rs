use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use library::animation::EasingFunction;
use library::cache::CacheManager;
use library::core::ensemble::decorators::{BackplateShape, BackplateTarget};
use library::core::ensemble::types::DecoratorConfig;
use library::editor::project_service::ProjectManager;
use library::framing::get_frame_from_project;
use library::model::frame::Image;
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameContent, FrameItem};
use library::model::frame::frame::FrameInfo;
use library::model::project::{
    Composition, DECORATOR_OUTPUT_PORT, DECORATORS_INPUT_PORT, EvalOutput, NodeContainer,
    NodeGraphBundle, PortAddress, PortDataType, PortDirection, PortMultiplicity, PortOwner,
    PortSide, Project, ProjectConnection, TIME_PORT,
};
use library::model::property::{
    Keyframe, Property, PropertyDefinition, PropertyMap, PropertyValue,
};
use library::model::{Clip, DecoratorInstance, Node, NodeContent};
use library::plugin::{
    DECORATOR_CATEGORY, DECORATOR_PRODUCE_OPERATION, DecoratorPlugin, FrameEvaluationContext,
    Plugin, PluginManager, ResolvedNodeInputs, property_port_key,
    property_ui_type_to_port_data_type,
};
use library::rendering::renderer::RenderOutput;
use library::{RenderService, SkiaRenderer};
use uuid::Uuid;

const WIDTH: u64 = 128;
const HEIGHT: u64 = 80;
const FPS: f64 = 10.0;

fn set_constant(node: &mut Node, key: &str, value: PropertyValue) {
    node.properties
        .set(key.to_string(), Property::constant(value));
}

fn decorator_wire(from: Uuid, to: Uuid, order: i64) -> ProjectConnection {
    ProjectConnection::new(
        PortAddress::new(PortOwner::Node(from), DECORATOR_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(to), DECORATORS_INPUT_PORT),
        order,
    )
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
fn descriptor_factory_is_complete_and_only_text_exposes_the_typed_consumer() {
    let plugins = Arc::new(PluginManager::default());
    assert_eq!(plugins.get_available_decorators(), ["backplate"]);
    let descriptor = plugins
        .operation_descriptor(DECORATOR_CATEGORY, "backplate", DECORATOR_PRODUCE_OPERATION)
        .unwrap();
    let decorator = plugins
        .create_decorator_operation_node("backplate")
        .unwrap();
    let NodeContent::PluginOperation(operation) = &decorator.content else {
        panic!()
    };
    assert_eq!(operation.category, DECORATOR_CATEGORY);
    assert_eq!(operation.component_id, "backplate");
    assert_eq!(operation.operation, DECORATOR_PRODUCE_OPERATION);
    assert_eq!(operation.declared_ports, descriptor.declared_ports());
    assert!(decorator.decorators.is_empty());
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
                .properties
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
    let output = operation
        .declared_ports
        .iter()
        .find(|port| port.key == DECORATOR_OUTPUT_PORT)
        .unwrap();
    assert_eq!(output.direction, PortDirection::Output);
    assert_eq!(output.side, PortSide::Right);
    assert_eq!(output.data_type, PortDataType::Decorator);

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
    let text_input = project
        .port_definition(
            &PortAddress::new(PortOwner::Node(text_id), DECORATORS_INPUT_PORT),
            PortDirection::Input,
        )
        .unwrap();
    assert_eq!(text_input.data_type, PortDataType::Decorator);
    assert_eq!(text_input.multiplicity, PortMultiplicity::Variadic);
    assert!(
        project
            .port_definition(
                &PortAddress::new(PortOwner::Node(shape_id), DECORATORS_INPUT_PORT),
                PortDirection::Input,
            )
            .is_none(),
        "Shape must not advertise a Decorator lane before it renders one"
    );
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
    let consumer_id = graph.output_node_id.unwrap();
    let mut first = plugins
        .create_decorator_operation_node("backplate")
        .unwrap();
    first.properties.set(
        "padding".into(),
        Property::keyframe(vec![
            Keyframe::new(0.0, 0.0.into(), EasingFunction::Linear),
            Keyframe::new(1.0, 10.0.into(), EasingFunction::Linear),
        ]),
    );
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
    graph.connections.extend([
        decorator_wire(first_id, consumer_id, 0),
        decorator_wire(second_id, consumer_id, 1),
    ]);
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
    assert!(project.get_node(consumer_id).unwrap().decorators.is_empty());

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

    let mut invalid_shape = backplate.properties.clone();
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
            &backplate.properties,
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
    let consumer_id = graph.output_node_id.unwrap();
    graph
        .nodes
        .iter_mut()
        .find(|node| node.id == consumer_id)
        .unwrap()
        .decorators
        .push(plugins.create_decorator_instance("backplate").unwrap());
    let mut unknown = plugins
        .create_decorator_operation_node("backplate")
        .unwrap();
    let unknown_id = unknown.id;
    let NodeContent::PluginOperation(operation) = &mut unknown.content else {
        panic!()
    };
    operation.component_id = "unavailable-decorator".into();
    graph.nodes.push(unknown);
    graph
        .connections
        .push(decorator_wire(unknown_id, consumer_id, 0));
    let (project, _) = project_with_graph(graph, 0.0, 2.0);
    let rendered = evaluate(&project, &plugins, 0);
    let FrameContent::Text { ensemble, .. } = first_content(&rendered.items).unwrap() else {
        panic!()
    };
    assert!(
        ensemble.is_none(),
        "wired NoOutput must not restore the embedded legacy Backplate"
    );
    assert_eq!(Project::load(&project.save().unwrap()).unwrap(), project);
}

struct CountingDecoratorPlugin {
    evaluations: Arc<AtomicUsize>,
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

    fn convert(
        &self,
        _context: &FrameEvaluationContext,
        _instance: &DecoratorInstance,
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
fn inactive_decorator_operation_is_not_evaluated() {
    let evaluations = Arc::new(AtomicUsize::new(0));
    let plugins = Arc::new(PluginManager::default());
    plugins.register_decorator_plugin(Arc::new(CountingDecoratorPlugin {
        evaluations: evaluations.clone(),
    }));
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_text_graph("inactive", "Arial", WIDTH, HEIGHT)
        .unwrap();
    let consumer_id = graph.output_node_id.unwrap();
    let counting = plugins.create_decorator_operation_node("counting").unwrap();
    let counting_id = counting.id;
    graph.nodes.push(counting);
    graph
        .connections
        .push(decorator_wire(counting_id, consumer_id, 0));
    let (project, _) = project_with_graph(graph, 5.0, 2.0);

    assert!(evaluate(&project, &plugins, 0).items.is_empty());
    assert_eq!(evaluations.load(Ordering::SeqCst), 0);
    assert!(first_content(&evaluate(&project, &plugins, 50).items).is_some());
    assert_eq!(evaluations.load(Ordering::SeqCst), 1);
}

#[test]
fn graph_backplate_pixels_match_legacy_embedded_decorator_pixels() {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let configure = |properties: &mut PropertyMap| {
        properties.set(
            "target".into(),
            Property::constant(PropertyValue::String("Char".into())),
        );
        properties.set(
            "shape".into(),
            Property::constant(PropertyValue::String("RoundRect".into())),
        );
        properties.set(
            "color".into(),
            Property::constant(PropertyValue::Color(Color {
                r: 20,
                g: 180,
                b: 60,
                a: 255,
            })),
        );
        properties.set("padding".into(), Property::constant(3.0.into()));
        properties.set("radius".into(), Property::constant(2.0.into()));
    };

    let mut legacy = manager
        .create_text_node("PARITY", "Arial", WIDTH, HEIGHT)
        .unwrap();
    let mut legacy_backplate = plugins.create_decorator_instance("backplate").unwrap();
    configure(&mut legacy_backplate.properties);
    legacy.decorators.push(legacy_backplate);
    let (legacy_project, _) =
        project_with_graph(NodeGraphBundle::with_output_node(legacy), 0.0, 2.0);

    let mut graph = manager
        .create_text_graph("PARITY", "Arial", WIDTH, HEIGHT)
        .unwrap();
    let consumer_id = graph.output_node_id.unwrap();
    let mut backplate = plugins
        .create_decorator_operation_node("backplate")
        .unwrap();
    configure(&mut backplate.properties);
    let backplate_id = backplate.id;
    graph.nodes.push(backplate);
    graph
        .connections
        .push(decorator_wire(backplate_id, consumer_id, 0));
    let (graph_project, _) = project_with_graph(graph, 0.0, 2.0);

    let legacy_frame = evaluate(&legacy_project, &plugins, 0);
    let graph_frame = evaluate(&graph_project, &plugins, 0);
    let FrameContent::Text {
        ensemble: Some(legacy_ensemble),
        ..
    } = first_content(&legacy_frame.items).unwrap()
    else {
        panic!()
    };
    let FrameContent::Text {
        ensemble: Some(graph_ensemble),
        ..
    } = first_content(&graph_frame.items).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        graph_ensemble.decorator_configs,
        legacy_ensemble.decorator_configs
    );
    assert_eq!(
        preview(&graph_project, &plugins, 0).data,
        preview(&legacy_project, &plugins, 0).data
    );
    assert!(
        graph_project
            .get_node(consumer_id)
            .unwrap()
            .decorators
            .is_empty()
    );
}
