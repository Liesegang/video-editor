use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use library::animation::EasingFunction;
use library::editor::project_service::ProjectManager;
use library::framing::get_frame_from_project;
use library::model::frame::color::Color;
use library::model::frame::draw_type::DrawStyle;
use library::model::frame::entity::{FrameContent, FrameItem, StyleConfig};
use library::model::frame::runtime_shape::RuntimeShapeGeometry;
use library::model::project::{
    Composition, EvalOutput, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer, NodeGraphBundle,
    PortAddress, PortDataType, PortDefinition, PortDirection, PortExposure, PortOwner, PortSide,
    Project, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, TIME_PORT,
};
use library::model::property::{
    Keyframe, Property, PropertyDefinition, PropertyMap, PropertyUiType, PropertyValue, Vec2,
};
use library::model::{Clip, GeneratorContent, Node, NodeContent};
use library::plugin::{
    FrameEvaluationContext, OperationDescriptor, OperationDescriptorError, Plugin, PluginManager,
    ResolvedNodeInputs, STYLE_APPLY_OPERATION, STYLE_CATEGORY, StylePlugin, property_port_key,
    property_ui_type_to_port_data_type,
};
use ordered_float::OrderedFloat;
use uuid::Uuid;

const WIDTH: u64 = 320;
const HEIGHT: u64 = 180;
const FPS: f64 = 10.0;

fn output_port(key: &str, label: &str, data_type: PortDataType) -> PortDefinition {
    PortDefinition::output(key, label, data_type, PortSide::Right, PortExposure::Graph)
}

fn setup_project() -> (Project, Uuid, Uuid) {
    let mut project = Project::new("style graph");
    let (composition, track) = Composition::new("main", WIDTH, HEIGHT, FPS, 10.0);
    let composition_id = composition.id;
    let track_id = track.id;
    project.add_track(track);
    project.add_composition(composition);
    (project, composition_id, track_id)
}

fn project_with_graph(graph: NodeGraphBundle, start_time: f64, duration: f64) -> Project {
    let (mut project, _composition_id, track_id) = setup_project();
    let clip = Clip::new("graph", start_time, duration);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();
    project
        .insert_node_graph(NodeContainer::Clip(clip_id), graph)
        .unwrap();
    project
}

fn frame(
    project: &Project,
    plugins: &Arc<PluginManager>,
    frame_number: u64,
) -> library::model::frame::frame::FrameInfo {
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

fn first_content(items: &[FrameItem]) -> Option<&FrameContent> {
    items.iter().find_map(|item| match item {
        FrameItem::Object(object) => Some(&object.content),
        FrameItem::Group(group) => first_content(&group.items),
    })
}

fn content_styles(content: &FrameContent) -> &[StyleConfig] {
    match content {
        FrameContent::Text { styles, .. } | FrameContent::Shape { styles, .. } => styles,
        other => panic!("expected styled content, got {other:?}"),
    }
}

fn draw_styles(
    project: &Project,
    plugins: &Arc<PluginManager>,
    frame_number: u64,
) -> Vec<DrawStyle> {
    let rendered = frame(project, plugins, frame_number);
    fn collect(items: &[FrameItem], styles: &mut Vec<DrawStyle>) {
        for item in items {
            match item {
                FrameItem::Object(object) => styles.extend(
                    content_styles(&object.content)
                        .iter()
                        .map(|style| style.style.clone()),
                ),
                FrameItem::Group(group) => collect(&group.items, styles),
            }
        }
    }
    let mut styles = Vec::new();
    collect(&rendered.items, &mut styles);
    styles
}

fn style_kinds(styles: &[DrawStyle]) -> Vec<&'static str> {
    styles
        .iter()
        .map(|style| match style {
            DrawStyle::Fill { .. } => "fill",
            DrawStyle::Stroke { .. } => "stroke",
        })
        .collect()
}

fn operation_component(node: &Node) -> Option<&str> {
    match node.content() {
        NodeContent::PluginOperation(operation) => Some(&operation.component_id),
        _ => None,
    }
}

fn set_constant(node: &mut Node, key: &str, value: PropertyValue) {
    assert!(
        node.set_property(key.to_string(), Property::constant(value))
            .is_ok(),
        "operation descriptor must initialize {key}"
    );
}

#[test]
fn style_descriptors_materialize_all_defaults_and_namespaced_typed_ports() {
    let plugins = PluginManager::default();
    for component_id in ["fill", "stroke"] {
        let descriptor = plugins
            .operation_descriptor(STYLE_CATEGORY, component_id, STYLE_APPLY_OPERATION)
            .unwrap();
        let node = plugins.create_style_operation_node(component_id).unwrap();
        let NodeContent::PluginOperation(operation) = node.content() else {
            panic!("factory must create a plugin operation");
        };
        assert_eq!(operation.category, STYLE_CATEGORY);
        assert_eq!(operation.component_id, component_id);
        assert_eq!(operation.operation, STYLE_APPLY_OPERATION);
        assert_eq!(operation.declared_ports, descriptor.declared_ports());
        assert_eq!(node.name, descriptor.label());

        for definition in descriptor.properties() {
            assert_eq!(
                node.properties()
                    .get(definition.name())
                    .and_then(Property::value),
                Some(definition.default_value())
            );
            let key = property_port_key(definition.name());
            let port = operation
                .declared_ports
                .iter()
                .find(|port| port.key == key)
                .expect("every property must have one port");
            assert_eq!(port.direction, PortDirection::Input);
            assert_eq!(
                port.data_type,
                property_ui_type_to_port_data_type(definition.ui_type())
            );
            assert!(
                operation
                    .declared_ports
                    .iter()
                    .all(|port| port.key != definition.name()),
                "unprefixed property aliases are forbidden"
            );
        }
        let shape_input = operation
            .declared_ports
            .iter()
            .find(|port| port.key == SHAPE_INPUT_PORT)
            .unwrap();
        assert_eq!(shape_input.direction, PortDirection::Input);
        assert_eq!(shape_input.data_type, PortDataType::Shape);
        let image_output = operation
            .declared_ports
            .iter()
            .find(|port| port.key == IMAGE_OUTPUT_PORT)
            .unwrap();
        assert_eq!(image_output.direction, PortDirection::Output);
        assert_eq!(image_output.data_type, PortDataType::Image);
        assert_eq!(image_output.side, PortSide::Right);
    }
}

#[test]
fn operation_descriptor_rejects_malformed_properties_and_terminal_ports() {
    let float = |name: &str, min: f64, max: f64, step: f64, default: f64| {
        PropertyDefinition::new(
            name,
            PropertyUiType::Float {
                min,
                max,
                step,
                suffix: String::new(),
                min_hard_limit: true,
                max_hard_limit: true,
            },
            "Value",
            default.into(),
        )
    };
    let make = |properties, outputs| {
        OperationDescriptor::new("test", "component", "test.v1", "Test", properties, outputs)
    };

    assert!(matches!(
        make(
            vec![
                float("amount", 0.0, 1.0, 0.1, 0.5),
                float("amount", 0.0, 1.0, 0.1, 0.5)
            ],
            vec![output_port("result", "Result", PortDataType::Number)]
        ),
        Err(OperationDescriptorError::DuplicateProperty { .. })
    ));
    assert!(matches!(
        make(
            vec![float("amount", 1.0, 0.0, 0.0, 2.0)],
            vec![output_port("result", "Result", PortDataType::Number)]
        ),
        Err(OperationDescriptorError::InvalidProperty { .. })
    ));
    let dropdown = PropertyDefinition::new(
        "mode",
        PropertyUiType::Dropdown {
            options: vec!["A".into(), "A".into()],
        },
        "Mode",
        PropertyValue::String("B".into()),
    );
    assert!(matches!(
        make(
            vec![dropdown],
            vec![output_port("result", "Result", PortDataType::Number)]
        ),
        Err(OperationDescriptorError::InvalidProperty { .. })
    ));
    let non_finite_vec = PropertyDefinition::new(
        "point",
        PropertyUiType::Vec2 {
            suffix: String::new(),
        },
        "Point",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(f64::NAN),
            y: OrderedFloat(0.0),
        }),
    );
    assert!(matches!(
        make(
            vec![non_finite_vec],
            vec![output_port("result", "Result", PortDataType::Vec2)]
        ),
        Err(OperationDescriptorError::InvalidProperty { .. })
    ));
    assert!(matches!(
        make(
            Vec::new(),
            vec![
                output_port("result", "Result", PortDataType::Number),
                output_port("result", "Duplicate", PortDataType::Number),
            ]
        ),
        Err(OperationDescriptorError::PortCollision { .. })
    ));
    assert!(matches!(
        make(
            Vec::new(),
            vec![output_port("bad key", "Result", PortDataType::Number)]
        ),
        Err(OperationDescriptorError::InvalidOperationPortKey { .. })
    ));
}

#[test]
fn execution_contract_ignores_labels_but_rejects_typed_breakage() {
    let descriptor = OperationDescriptor::style(
        "test",
        "Test",
        vec![PropertyDefinition::new(
            "opacity",
            PropertyUiType::Float {
                min: 0.0,
                max: 1.0,
                step: 0.1,
                suffix: String::new(),
                min_hard_limit: true,
                max_hard_limit: true,
            },
            "Opacity",
            1.0.into(),
        )],
    )
    .unwrap();
    let mut persisted = descriptor.declared_ports().to_vec();
    persisted.reverse();
    for port in &mut persisted {
        port.label = format!("renamed {}", port.label);
        port.side = PortSide::Left;
    }
    assert!(descriptor.is_execution_compatible_with_ports(&persisted));

    persisted[0].data_type = PortDataType::Boolean;
    assert!(!descriptor.is_execution_compatible_with_ports(&persisted));
}

#[test]
fn graph_factories_have_stable_orders_positions_and_no_embedded_style_authority() {
    let plugins = Arc::new(PluginManager::default());
    let project = Arc::new(RwLock::new(Project::new("factory")));
    let manager = ProjectManager::new(project, plugins);

    let text = manager
        .create_text_graph("hello", "Arial", WIDTH, HEIGHT)
        .unwrap();
    assert_eq!(text.nodes.len(), 2);
    let text_consumer = text
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(GeneratorContent::Text)
            )
        })
        .unwrap();
    let fill = text
        .nodes
        .iter()
        .find(|node| operation_component(node) == Some("fill"))
        .unwrap();
    assert_eq!(text_consumer.ui_position, [0.0, 0.0]);
    assert_eq!(fill.ui_position, [360.0, 0.0]);
    assert_eq!(text.output_node_id, Some(fill.id));
    assert_eq!(text.connections.len(), 1);
    assert_eq!(text.connections[0].order, 0);
    assert_eq!(text.connections[0].from.port, SHAPE_OUTPUT_PORT);
    assert_eq!(text.connections[0].to.port, SHAPE_INPUT_PORT);

    let shape = manager
        .create_shape_graph("M0 0 L10 0 L10 10 Z", WIDTH, HEIGHT, 10, 10)
        .unwrap();
    let shape_consumer = shape
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(GeneratorContent::Shape)
            )
        })
        .unwrap();
    assert_eq!(shape_consumer.ui_position, [0.0, 110.0]);
    let merge = shape
        .nodes
        .iter()
        .find(|node| matches!(node.content(), NodeContent::Merge))
        .unwrap();
    assert_eq!(shape.output_node_id, Some(merge.id));
    assert_eq!(shape.nodes.len(), 4);
    assert_eq!(shape.connections.len(), 4);
    let mut ordered = shape
        .connections
        .iter()
        .filter(|connection| connection.to.port == MERGE_IMAGES_PORT)
        .collect::<Vec<_>>();
    ordered.sort_by_key(|connection| connection.order);
    assert_eq!(
        ordered
            .iter()
            .map(|connection| {
                let PortOwner::Node(source) = connection.from.owner else {
                    panic!()
                };
                operation_component(shape.nodes.iter().find(|node| node.id == source).unwrap())
                    .unwrap()
            })
            .collect::<Vec<_>>(),
        vec!["fill", "stroke"]
    );
    assert_eq!(
        ordered
            .iter()
            .map(|connection| connection.order)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );

    let expected = draw_styles(
        &project_with_graph(shape.clone(), 0.0, 2.0),
        &Arc::new(PluginManager::default()),
        0,
    );
    assert_eq!(style_kinds(&expected), vec!["fill", "stroke"]);

    let mut reversed_storage = shape.clone();
    reversed_storage.connections.reverse();
    let rendered = draw_styles(
        &project_with_graph(reversed_storage, 0.0, 2.0),
        &Arc::new(PluginManager::default()),
        0,
    );
    assert_eq!(style_kinds(&rendered), vec!["fill", "stroke"]);

    let mut swapped_order = shape;
    for connection in &mut swapped_order.connections {
        if connection.to.port == MERGE_IMAGES_PORT {
            connection.order = 1 - connection.order;
        }
    }
    let rendered = draw_styles(
        &project_with_graph(swapped_order, 0.0, 2.0),
        &Arc::new(PluginManager::default()),
        0,
    );
    assert_eq!(style_kinds(&rendered), vec!["stroke", "fill"]);
}

#[test]
fn text_and_shape_clip_graphs_roundtrip_with_explicit_raster_boundaries() {
    let plugins = Arc::new(PluginManager::default());
    let shared = Arc::new(RwLock::new(setup_project().0));
    let manager = ProjectManager::new(shared.clone(), plugins.clone());
    let composition_id = shared.read().unwrap().compositions[0].id;
    let track_id = shared.read().unwrap().compositions[0].track_ids[0];

    let text_bundle = manager
        .create_text_clip("hello", 0.0, 2.0, WIDTH as u32, HEIGHT as u32)
        .unwrap();
    assert!(text_bundle.clip.node_ids.is_empty());
    assert!(text_bundle.graph.output_node().is_some());
    manager
        .add_clip_to_track(composition_id, track_id, text_bundle, None)
        .unwrap();
    let saved = shared.read().unwrap().save().unwrap();
    let loaded = Project::load(&saved).unwrap();
    assert!(loaded.validation_issues().is_empty());
    assert_eq!(loaded, *shared.read().unwrap());

    let graph_text = manager
        .create_text_graph("hello", "Arial", WIDTH, HEIGHT)
        .unwrap();
    assert_eq!(
        style_kinds(&draw_styles(
            &project_with_graph(graph_text, 0.0, 2.0),
            &plugins,
            0
        )),
        vec!["fill"]
    );

    let graph_shape = manager
        .create_shape_graph("M0 0 L10 0 L10 10 Z", WIDTH, HEIGHT, 10, 10)
        .unwrap();
    assert_eq!(
        style_kinds(&draw_styles(
            &project_with_graph(graph_shape, 0.0, 2.0),
            &plugins,
            0
        )),
        vec!["fill", "stroke"]
    );
}

#[test]
fn editing_style_constants_keyframes_and_connected_scalars_changes_render_only() {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let graph = manager
        .create_text_graph("hello", "Arial", WIDTH, HEIGHT)
        .unwrap();
    let fill_id = graph
        .nodes
        .iter()
        .find(|node| operation_component(node) == Some("fill"))
        .unwrap()
        .id;
    let source = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(GeneratorContent::Text)
            )
        })
        .unwrap()
        .clone();
    let mut project = project_with_graph(graph, 0.0, 2.0);
    let clip_id = project.find_node_container(fill_id).unwrap().id();

    let fill = project.get_node_mut(fill_id).unwrap();
    set_constant(
        fill,
        "color",
        PropertyValue::Color(Color {
            r: 200,
            g: 20,
            b: 10,
            a: 200,
        }),
    );
    set_constant(fill, "opacity", 0.5.into());
    let rendered = draw_styles(&project, &plugins, 0);
    assert!(matches!(
        rendered.as_slice(),
        [DrawStyle::Fill {
            color: Color { r: 200, a: 100, .. },
            ..
        }]
    ));

    project
        .get_node_mut(fill_id)
        .unwrap()
        .set_property(
            "opacity".into(),
            Property::keyframe(vec![
                Keyframe::new(0.0, 0.2.into(), EasingFunction::Linear),
                Keyframe::new(1.0, 0.8.into(), EasingFunction::Linear),
            ]),
        )
        .expect("fill descriptor initializes opacity");
    let at_start = draw_styles(&project, &plugins, 0);
    let at_one_second = draw_styles(&project, &plugins, 10);
    assert!(matches!(
        at_start.as_slice(),
        [DrawStyle::Fill {
            color: Color { a: 40, .. },
            ..
        }]
    ));
    assert!(matches!(
        at_one_second.as_slice(),
        [DrawStyle::Fill {
            color: Color { a: 160, .. },
            ..
        }]
    ));

    project
        .connect_ports(
            PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
            PortAddress::new(PortOwner::Node(fill_id), property_port_key("opacity")),
        )
        .unwrap();
    let connected = draw_styles(&project, &plugins, 5);
    assert!(matches!(
        connected.as_slice(),
        [DrawStyle::Fill {
            color: Color { a: 100, .. },
            ..
        }]
    ));
    assert_eq!(project.get_node(source.id), Some(&source));
}

#[test]
fn text_converter_exposes_grapheme_source_ranges_without_claiming_glyph_metadata() {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let graph = manager
        .create_text_graph("A日e\u{301}", "Arial", WIDTH, HEIGHT)
        .unwrap();
    let node = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(GeneratorContent::Text)
            )
        })
        .unwrap();
    let (composition, _) = Composition::new("main", WIDTH, HEIGHT, FPS, 2.0);
    let project = Project::new("bounds");
    let evaluators = plugins.get_property_evaluators();
    let context = FrameEvaluationContext {
        project: &project,
        composition: &composition,
        property_evaluators: &evaluators,
        plugin_manager: &plugins,
        resolved_inputs: None,
    };
    let converter = plugins.get_entity_converter("text").unwrap();
    let shape = converter.convert_shape(&context, node, 0.0).unwrap();
    let RuntimeShapeGeometry::Text(text) = shape.geometry else {
        panic!("text converter must produce RuntimeShapeGeometry::Text")
    };
    assert_eq!(text.elements.len(), 3);
    assert_eq!(text.elements[0].utf8_range, 0..1);
    assert_eq!(text.elements[1].utf8_range, 1..4);
    assert_eq!(text.elements[2].source, "e\u{301}");
    assert_eq!(text.elements[2].utf8_range, 4..7);
    assert_eq!(text.elements[2].utf16_range, 2..4);
    assert!(text.elements.iter().all(|element| element.advance > 0.0));
}

#[test]
fn unknown_missing_invalid_and_scalar_no_output_styles_are_safe() {
    let plugins = Arc::new(PluginManager::default());
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_text_graph("unknown", "Arial", WIDTH, HEIGHT)
        .unwrap();
    let fill_index = graph
        .nodes
        .iter()
        .position(|node| operation_component(node) == Some("fill"))
        .unwrap();
    let mut persisted = serde_json::to_value(&graph.nodes[fill_index]).unwrap();
    persisted["content"]["data"]["component_id"] =
        serde_json::Value::String("unavailable-style".into());
    graph.nodes[fill_index] = serde_json::from_value(persisted).unwrap();
    let project = project_with_graph(graph, 0.0, 2.0);
    let rendered = frame(&project, &plugins, 0);
    assert!(rendered.items.is_empty());
    let saved = project.save().unwrap();
    assert_eq!(Project::load(&saved).unwrap(), project);

    let fill = plugins.create_style_operation_node("fill").unwrap();
    let (composition, _) = Composition::new("main", WIDTH, HEIGHT, FPS, 2.0);
    let project = Project::new("scalar NoOutput");
    let evaluators = plugins.get_property_evaluators();
    let mut resolved = ResolvedNodeInputs::default();
    resolved
        .properties
        .insert("opacity".into(), EvalOutput::NoOutput);
    let context = FrameEvaluationContext {
        project: &project,
        composition: &composition,
        property_evaluators: &evaluators,
        plugin_manager: &plugins,
        resolved_inputs: Some(&resolved),
    };
    assert_eq!(
        plugins.evaluate_style_operation(&context, "fill", fill.id, fill.properties(), 0.0),
        EvalOutput::NoOutput
    );

    let context = FrameEvaluationContext {
        project: &project,
        composition: &composition,
        property_evaluators: &evaluators,
        plugin_manager: &plugins,
        resolved_inputs: None,
    };
    assert_eq!(
        plugins.evaluate_style_operation(&context, "fill", fill.id, &PropertyMap::new(), 0.0),
        EvalOutput::NoOutput,
        "a missing descriptor property must not use the plugin fallback"
    );

    let mut invalid_fill = fill.properties().clone();
    invalid_fill.set(
        "opacity".into(),
        Property::constant(PropertyValue::String("wrong-type".into())),
    );
    assert_eq!(
        plugins.evaluate_style_operation(&context, "fill", fill.id, &invalid_fill, 0.0),
        EvalOutput::NoOutput
    );

    let stroke = plugins.create_style_operation_node("stroke").unwrap();
    let mut invalid_keyframe = stroke.properties().clone();
    invalid_keyframe.set(
        "join".into(),
        Property::keyframe(vec![Keyframe::new(
            0.0,
            PropertyValue::String("outside-options".into()),
            EasingFunction::Linear,
        )]),
    );
    assert_eq!(
        plugins.evaluate_style_operation(&context, "stroke", stroke.id, &invalid_keyframe, 0.0),
        EvalOutput::NoOutput
    );

    let mut invalid_scalar = ResolvedNodeInputs::default();
    invalid_scalar.properties.insert(
        "join".into(),
        EvalOutput::Produced(PropertyValue::String("outside-options".into())),
    );
    let context = FrameEvaluationContext {
        project: &project,
        composition: &composition,
        property_evaluators: &evaluators,
        plugin_manager: &plugins,
        resolved_inputs: Some(&invalid_scalar),
    };
    assert_eq!(
        plugins.evaluate_style_operation(&context, "stroke", stroke.id, stroke.properties(), 0.0),
        EvalOutput::NoOutput
    );
}

struct CountingStylePlugin {
    evaluations: Arc<AtomicUsize>,
}

impl Plugin for CountingStylePlugin {
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

impl StylePlugin for CountingStylePlugin {
    fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
        OperationDescriptor::style("counting", "Counting", Vec::new())
    }

    fn evaluate_source(
        &self,
        _context: &FrameEvaluationContext,
        source_id: Uuid,
        _properties: &PropertyMap,
        _eval_time: f64,
    ) -> Option<StyleConfig> {
        self.evaluations.fetch_add(1, Ordering::SeqCst);
        Some(StyleConfig {
            id: source_id,
            style: DrawStyle::Fill {
                color: Color::white(),
                offset: 0.0,
            },
        })
    }
}

#[test]
fn inactive_clip_style_plugin_is_not_evaluated() {
    let evaluations = Arc::new(AtomicUsize::new(0));
    let plugins = Arc::new(PluginManager::default());
    plugins.register_style_plugin(Arc::new(CountingStylePlugin {
        evaluations: evaluations.clone(),
    }));
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("factory"))),
        plugins.clone(),
    );
    let mut graph = manager
        .create_text_graph("inactive", "Arial", WIDTH, HEIGHT)
        .unwrap();
    let old_fill = graph
        .nodes
        .iter()
        .position(|node| operation_component(node) == Some("fill"))
        .unwrap();
    let counting = plugins.create_style_operation_node("counting").unwrap();
    let counting_id = counting.id;
    graph.nodes[old_fill] = counting;
    graph.output_node_id = Some(counting_id);
    graph.connections[0].to = PortAddress::new(PortOwner::Node(counting_id), SHAPE_INPUT_PORT);
    let project = project_with_graph(graph, 5.0, 2.0);

    assert!(frame(&project, &plugins, 0).items.is_empty());
    assert_eq!(evaluations.load(Ordering::SeqCst), 0);
    assert!(first_content(&frame(&project, &plugins, 50).items).is_some());
    assert_eq!(evaluations.load(Ordering::SeqCst), 1);
}
