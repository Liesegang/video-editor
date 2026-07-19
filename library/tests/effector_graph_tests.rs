use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use library::animation::EasingFunction;
use library::cache::CacheManager;
use library::core::ensemble::effectors::{
    EffectorElementContext, OpacityMode, evaluate_configured_transform,
};
use library::core::ensemble::target::EffectorTarget;
use library::core::ensemble::types::EffectorConfig;
use library::editor::project_service::ProjectManager;
use library::framing::get_frame_from_project;
use library::model::frame::Image;
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameContent, FrameItem};
use library::model::frame::frame::FrameInfo;
use library::model::project::{
    Composition, EFFECTOR_OUTPUT_PORT, EFFECTORS_INPUT_PORT, EvalOutput, NodeContainer,
    NodeGraphBundle, PortAddress, PortDataType, PortDirection, PortMultiplicity, PortOwner,
    PortSide, Project, ProjectConnection, TIME_PORT,
};
use library::model::property::{
    Keyframe, Property, PropertyDefinition, PropertyMap, PropertyValue,
};
use library::model::{Clip, EffectorInstance, Node, NodeContent};
use library::plugin::{
    EFFECTOR_CATEGORY, EFFECTOR_PRODUCE_OPERATION, EffectorPlugin, FrameEvaluationContext, Plugin,
    PluginManager, ResolvedNodeInputs, property_port_key, property_ui_type_to_port_data_type,
};
use library::rendering::renderer::RenderOutput;
use library::{RenderService, SkiaRenderer};
use skia_safe::Point;
use uuid::Uuid;

const WIDTH: u64 = 128;
const HEIGHT: u64 = 80;
const FPS: f64 = 10.0;

fn set_constant(node: &mut Node, key: &str, value: PropertyValue) {
    node.properties
        .set(key.to_string(), Property::constant(value));
}

fn effector_wire(from: Uuid, to: Uuid, order: i64) -> ProjectConnection {
    ProjectConnection::new(
        PortAddress::new(PortOwner::Node(from), EFFECTOR_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(to), EFFECTORS_INPUT_PORT),
        order,
    )
}

fn setup_project() -> (Project, Uuid, Uuid) {
    let mut project = Project::new("effector graph");
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
    let clip = Clip::new("effector clip", start_time, duration);
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
fn descriptors_factories_and_text_shape_consumers_have_complete_typed_contracts() {
    let plugins = Arc::new(PluginManager::default());
    let mut available = plugins.get_available_effectors();
    available.sort();
    assert_eq!(
        available,
        ["opacity", "randomize", "step_delay", "transform"]
    );

    for component_id in available {
        let descriptor = plugins
            .operation_descriptor(EFFECTOR_CATEGORY, &component_id, EFFECTOR_PRODUCE_OPERATION)
            .unwrap();
        let node = plugins
            .create_effector_operation_node(&component_id)
            .unwrap();
        let NodeContent::PluginOperation(operation) = &node.content else {
            panic!("Effector factory must create a plugin operation")
        };
        assert_eq!(operation.category, EFFECTOR_CATEGORY);
        assert_eq!(operation.component_id, component_id);
        assert_eq!(operation.operation, EFFECTOR_PRODUCE_OPERATION);
        assert_eq!(operation.declared_ports, descriptor.declared_ports());
        assert!(node.effectors.is_empty());
        for definition in descriptor.properties() {
            assert_eq!(
                node.properties
                    .get(definition.name())
                    .and_then(Property::value),
                Some(definition.default_value())
            );
            let port = operation
                .declared_ports
                .iter()
                .find(|port| port.key == property_port_key(definition.name()))
                .unwrap();
            assert_eq!(port.direction, PortDirection::Input);
            assert_eq!(
                port.data_type,
                property_ui_type_to_port_data_type(definition.ui_type())
            );
        }
        let output = operation
            .declared_ports
            .iter()
            .find(|port| port.key == EFFECTOR_OUTPUT_PORT)
            .unwrap();
        assert_eq!(output.direction, PortDirection::Output);
        assert_eq!(output.side, PortSide::Right);
        assert_eq!(output.data_type, PortDataType::Effector);
    }

    let transform = plugins.create_effector_operation_node("transform").unwrap();
    for key in ["tx", "ty", "scale_x", "scale_y", "rotation", "target"] {
        assert!(transform.properties.get(key).is_some(), "missing {key}");
    }
    let opacity = plugins.create_effector_operation_node("opacity").unwrap();
    for key in ["opacity", "mode", "target"] {
        assert!(opacity.properties.get(key).is_some(), "missing {key}");
    }

    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins,
    );
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
    for consumer in [text_id, shape_id] {
        let input = project
            .port_definition(
                &PortAddress::new(PortOwner::Node(consumer), EFFECTORS_INPUT_PORT),
                PortDirection::Input,
            )
            .unwrap();
        assert_eq!(input.data_type, PortDataType::Effector);
        assert_eq!(input.multiplicity, PortMultiplicity::Variadic);
        assert_eq!(input.side, PortSide::Left);
    }
}

#[test]
fn graph_order_keyframes_and_scalar_overrides_produce_one_ensemble_and_roundtrip() {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_text_graph("ORDER", "Arial", WIDTH, HEIGHT)
        .unwrap();
    let consumer_id = graph.output_node_id.unwrap();
    let mut transform = plugins.create_effector_operation_node("transform").unwrap();
    transform.properties.set(
        "tx".into(),
        Property::keyframe(vec![
            Keyframe::new(0.0, 0.0.into(), EasingFunction::Linear),
            Keyframe::new(1.0, 20.0.into(), EasingFunction::Linear),
        ]),
    );
    set_constant(
        &mut transform,
        "target",
        PropertyValue::String("Char".into()),
    );
    let opacity = plugins.create_effector_operation_node("opacity").unwrap();
    let transform_id = transform.id;
    let opacity_id = opacity.id;
    graph.nodes.extend([transform, opacity]);
    graph.connections.extend([
        effector_wire(transform_id, consumer_id, 0),
        effector_wire(opacity_id, consumer_id, 1),
    ]);
    let (mut project, clip_id) = project_with_graph(graph, 0.0, 2.0);
    project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(opacity_id), property_port_key("opacity")),
        )
        .unwrap();

    let rendered = evaluate(&project, &plugins, 5);
    let FrameContent::Text {
        ensemble: Some(ensemble),
        ..
    } = first_content(&rendered.items).unwrap()
    else {
        panic!("wired Effectors must produce EnsembleData")
    };
    assert_eq!(ensemble.effector_configs.len(), 2);
    assert!(matches!(
        &ensemble.effector_configs[0],
        EffectorConfig::Transform {
            translate,
            target: EffectorTarget::Char,
            ..
        } if (translate.0 - 10.0).abs() < f32::EPSILON
    ));
    assert!(matches!(
        &ensemble.effector_configs[1],
        EffectorConfig::Opacity {
            target_opacity,
            mode: OpacityMode::Set,
            target: EffectorTarget::Block,
        } if (target_opacity - 0.5).abs() < f32::EPSILON
    ));
    assert!(project.get_node(consumer_id).unwrap().effectors.is_empty());

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
fn missing_invalid_unknown_and_scalar_no_output_never_restore_embedded_effectors() {
    let plugins = Arc::new(PluginManager::default());
    let opacity = plugins.create_effector_operation_node("opacity").unwrap();
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
        plugins.evaluate_effector_operation(
            &context,
            "opacity",
            opacity.id,
            &PropertyMap::new(),
            0.0
        ),
        EvalOutput::NoOutput
    );

    let mut invalid_mode = opacity.properties.clone();
    invalid_mode.set(
        "mode".into(),
        Property::keyframe(vec![Keyframe::new(
            0.0,
            PropertyValue::String("outside-options".into()),
            EasingFunction::Linear,
        )]),
    );
    assert_eq!(
        plugins.evaluate_effector_operation(&context, "opacity", opacity.id, &invalid_mode, 0.0),
        EvalOutput::NoOutput
    );

    let mut scalar = ResolvedNodeInputs::default();
    scalar
        .properties
        .insert("opacity".into(), EvalOutput::NoOutput);
    let scalar_context = FrameEvaluationContext {
        resolved_inputs: Some(&scalar),
        ..context
    };
    assert_eq!(
        plugins.evaluate_effector_operation(
            &scalar_context,
            "opacity",
            opacity.id,
            &opacity.properties,
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
        .effectors
        .push(plugins.create_effector_instance("transform").unwrap());
    let mut unknown = plugins.create_effector_operation_node("opacity").unwrap();
    let unknown_id = unknown.id;
    let NodeContent::PluginOperation(operation) = &mut unknown.content else {
        panic!()
    };
    operation.component_id = "unavailable-effector".into();
    graph.nodes.push(unknown);
    graph
        .connections
        .push(effector_wire(unknown_id, consumer_id, 0));
    let (project, _) = project_with_graph(graph, 0.0, 2.0);
    let rendered = evaluate(&project, &plugins, 0);
    let FrameContent::Text { ensemble, .. } = first_content(&rendered.items).unwrap() else {
        panic!()
    };
    assert!(
        ensemble.is_none(),
        "wired NoOutput must not restore the embedded legacy Transform"
    );
    assert_eq!(Project::load(&project.save().unwrap()).unwrap(), project);
}

struct CountingEffectorPlugin {
    evaluations: Arc<AtomicUsize>,
}

impl Plugin for CountingEffectorPlugin {
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

impl EffectorPlugin for CountingEffectorPlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        Vec::new()
    }

    fn convert(
        &self,
        _context: &FrameEvaluationContext,
        _instance: &EffectorInstance,
        _eval_time: f64,
    ) -> Option<EffectorConfig> {
        self.evaluations.fetch_add(1, Ordering::SeqCst);
        Some(EffectorConfig::Opacity {
            target_opacity: 100.0,
            mode: OpacityMode::Set,
            target: EffectorTarget::Block,
        })
    }
}

#[test]
fn inactive_effector_operation_is_not_evaluated() {
    let evaluations = Arc::new(AtomicUsize::new(0));
    let plugins = Arc::new(PluginManager::default());
    plugins.register_effector_plugin(Arc::new(CountingEffectorPlugin {
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
    let counting = plugins.create_effector_operation_node("counting").unwrap();
    let counting_id = counting.id;
    graph.nodes.push(counting);
    graph
        .connections
        .push(effector_wire(counting_id, consumer_id, 0));
    let (project, _) = project_with_graph(graph, 5.0, 2.0);

    assert!(evaluate(&project, &plugins, 0).items.is_empty());
    assert_eq!(evaluations.load(Ordering::SeqCst), 0);
    assert!(first_content(&evaluate(&project, &plugins, 50).items).is_some());
    assert_eq!(evaluations.load(Ordering::SeqCst), 1);
}

#[test]
fn descriptor_graph_render_matches_legacy_embedded_effector_pixels() {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );

    let mut legacy = manager
        .create_text_node("PARITY", "Arial", WIDTH, HEIGHT)
        .unwrap();
    let mut legacy_transform = plugins.create_effector_instance("transform").unwrap();
    legacy_transform
        .properties
        .set("tx".into(), Property::constant(PropertyValue::from(4.0)));
    legacy_transform.properties.set(
        "rotation".into(),
        Property::constant(PropertyValue::from(7.0)),
    );
    legacy_transform.properties.set(
        "target".into(),
        Property::constant(PropertyValue::String("Char".into())),
    );
    let mut legacy_opacity = plugins.create_effector_instance("opacity").unwrap();
    legacy_opacity.properties.set(
        "opacity".into(),
        Property::constant(PropertyValue::from(70.0)),
    );
    legacy_opacity.properties.set(
        "mode".into(),
        Property::constant(PropertyValue::String("Multiply".into())),
    );
    legacy_opacity.properties.set(
        "target".into(),
        Property::constant(PropertyValue::String("Char".into())),
    );
    legacy.effectors = vec![legacy_transform, legacy_opacity];
    let (legacy_project, _) =
        project_with_graph(NodeGraphBundle::with_output_node(legacy), 0.0, 2.0);

    let mut graph = manager
        .create_text_graph("PARITY", "Arial", WIDTH, HEIGHT)
        .unwrap();
    let consumer_id = graph.output_node_id.unwrap();
    let mut transform = plugins.create_effector_operation_node("transform").unwrap();
    set_constant(&mut transform, "tx", 4.0.into());
    set_constant(&mut transform, "rotation", 7.0.into());
    set_constant(
        &mut transform,
        "target",
        PropertyValue::String("Char".into()),
    );
    let mut opacity = plugins.create_effector_operation_node("opacity").unwrap();
    set_constant(&mut opacity, "opacity", 70.0.into());
    set_constant(
        &mut opacity,
        "mode",
        PropertyValue::String("Multiply".into()),
    );
    set_constant(&mut opacity, "target", PropertyValue::String("Char".into()));
    let transform_id = transform.id;
    let opacity_id = opacity.id;
    graph.nodes.extend([transform, opacity]);
    graph.connections.extend([
        effector_wire(transform_id, consumer_id, 0),
        effector_wire(opacity_id, consumer_id, 1),
    ]);
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
        graph_ensemble.effector_configs,
        legacy_ensemble.effector_configs
    );
    assert_eq!(
        preview(&graph_project, &plugins, 0).data,
        preview(&legacy_project, &plugins, 0).data
    );
    assert!(
        graph_project
            .get_node(consumer_id)
            .unwrap()
            .effectors
            .is_empty()
    );
}

#[test]
fn graph_randomize_char_is_deterministic_and_seeded_by_element_identity() {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_text_graph("ABCDE", "Arial", WIDTH, HEIGHT)
        .unwrap();
    let consumer_id = graph.output_node_id.unwrap();
    let mut random = plugins.create_effector_operation_node("randomize").unwrap();
    set_constant(&mut random, "seed", 7.0.into());
    set_constant(&mut random, "translate_range", 8.0.into());
    set_constant(&mut random, "rotate_range", 12.0.into());
    set_constant(&mut random, "scale_range", 0.25.into());
    set_constant(&mut random, "target", PropertyValue::String("Char".into()));
    let random_id = random.id;
    graph.nodes.push(random);
    graph
        .connections
        .push(effector_wire(random_id, consumer_id, 0));
    let (project, _) = project_with_graph(graph, 0.0, 2.0);

    let image_a = preview(&project, &plugins, 0);
    let image_b = preview(&project, &plugins, 0);
    assert_eq!(image_a.data, image_b.data);

    let frame = evaluate(&project, &plugins, 0);
    let FrameContent::Text {
        ensemble: Some(ensemble),
        ..
    } = first_content(&frame.items).unwrap()
    else {
        panic!()
    };
    let first = evaluate_configured_transform(
        &ensemble.effector_configs,
        0.0,
        EffectorElementContext {
            global_index: 0,
            line_index: 0,
            line_char_index: 0,
            total_chars: 5,
            line_char_count: 5,
            char_center: Point::new(0.0, 0.0),
        },
    )
    .unwrap();
    let second = evaluate_configured_transform(
        &ensemble.effector_configs,
        0.0,
        EffectorElementContext {
            global_index: 1,
            line_index: 0,
            line_char_index: 1,
            total_chars: 5,
            line_char_count: 5,
            char_center: Point::new(0.0, 0.0),
        },
    )
    .unwrap();
    assert_ne!(
        first, second,
        "Char randomization must mix element identity"
    );

    let mut changed_seed = project;
    set_constant(
        changed_seed.get_node_mut(random_id).unwrap(),
        "seed",
        8.0.into(),
    );
    assert_ne!(image_a.data, preview(&changed_seed, &plugins, 0).data);
}

#[test]
fn shape_variadic_effector_input_applies_single_element_transform() {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_shape_graph("M0 0 L20 0 L20 20 L0 20 Z", WIDTH, HEIGHT, 20, 20)
        .unwrap();
    let shape_id = graph.output_node_id.unwrap();
    let mut transform = plugins.create_effector_operation_node("transform").unwrap();
    set_constant(&mut transform, "tx", 8.0.into());
    set_constant(&mut transform, "ty", 3.0.into());
    let transform_id = transform.id;
    let mut opacity = plugins.create_effector_operation_node("opacity").unwrap();
    set_constant(&mut opacity, "opacity", 50.0.into());
    let opacity_id = opacity.id;
    graph.nodes.extend([transform, opacity]);
    graph.connections.extend([
        effector_wire(transform_id, shape_id, 0),
        effector_wire(opacity_id, shape_id, 1),
    ]);
    let (project, _) = project_with_graph(graph, 0.0, 2.0);

    let rendered = evaluate(&project, &plugins, 0);
    let FrameContent::Shape { transform, .. } = first_content(&rendered.items).unwrap() else {
        panic!()
    };
    assert_eq!((transform.position.x, transform.position.y), (72.0, 43.0));
    assert!((transform.opacity - 0.5).abs() < f64::EPSILON);
    assert!(project.get_node(shape_id).unwrap().effectors.is_empty());
}
