use super::*;
use crate::editor::PropertyOwner;
use crate::error::LibraryError;
use crate::model::frame::color::Color;
use crate::model::project::{
    NodeContainer, NodeGraphBundle, PortAddress, PortDataType, PortDirection, PortOwner, Project,
    SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, TIME_PORT,
};
use crate::model::property::{
    KeyframeUpdate, Property, PropertyDefinition, PropertyUiType, PropertyValue,
};
use crate::model::{
    Clip, Composition, GeneratorContent, Node, NodeContent, native_node_descriptor_for_node,
};
use crate::plugin::PluginManager;
use crate::plugin::entity_converter::measure_text_size;
use ordered_float::OrderedFloat;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

fn assert_converter_properties(
    manager: &ProjectManager,
    node: &Node,
    converter_kind: &str,
    dimensions: (u64, u64, u64, u64),
) {
    let Some(converter) = manager.plugin_manager.get_entity_converter(converter_kind) else {
        panic!("{converter_kind} converter should be registered");
    };
    let definitions =
        converter.get_property_definitions(dimensions.0, dimensions.1, dimensions.2, dimensions.3);
    for definition in definitions {
        assert!(
            node.properties().get(definition.name()).is_some(),
            "{} factory omitted converter property {}",
            converter_kind,
            definition.name()
        );
    }
}

fn assert_property_value(node: &Node, key: &str, expected: PropertyValue) {
    assert_eq!(
        node.properties()
            .get(key)
            .and_then(Property::value)
            .cloned(),
        Some(expected),
        "authoritative property {key} must match GeneratorContent"
    );
}

#[test]
fn project_manager_exposes_identity_based_keyframe_edits() {
    let mut project = Project::new("keyframe service");
    let node = PluginManager::default()
        .create_style_operation_node("fill")
        .expect("Fill Style should be registered");
    let node_id = node.id;
    project.add_node(node);
    let shared = Arc::new(RwLock::new(project));
    let manager = ProjectManager::new(Arc::clone(&shared), Arc::new(PluginManager::default()));
    let owner = PropertyOwner::Node(node_id);

    let id = manager
        .add_keyframe_with_id(
            owner,
            "opacity",
            1.0,
            PropertyValue::Number(OrderedFloat(1.0)),
            Some(crate::animation::EasingFunction::EaseOutQuad),
        )
        .expect("service should promote an initialized constant property");
    manager
        .update_keyframe_by_id(
            owner,
            "opacity",
            id,
            KeyframeUpdate {
                time: Some(2.0),
                value: Some(PropertyValue::Number(OrderedFloat(0.75))),
                ..Default::default()
            },
        )
        .expect("service should update the identified key");

    {
        let read = shared.read().expect("project should remain readable");
        let keyframe = read
            .get_node(node_id)
            .and_then(|node| node.properties().get("opacity"))
            .and_then(|property| property.keyframe_by_id(id))
            .expect("identified key should exist");
        assert_eq!(keyframe.time, OrderedFloat(2.0));
        assert_eq!(keyframe.value, PropertyValue::Number(OrderedFloat(0.75)));
        assert_eq!(
            keyframe.easing,
            crate::animation::EasingFunction::EaseOutQuad
        );
    }

    let saved = manager.save_project().expect("project should serialize");
    let loaded = Project::load(&saved).expect("project should deserialize");
    let loaded_keyframe = loaded
        .get_node(node_id)
        .and_then(|node| node.properties().get("opacity"))
        .and_then(|property| property.keyframe_by_id(id))
        .expect("save/load should preserve keyframe identity");
    assert_eq!(loaded_keyframe.time, OrderedFloat(2.0));
    assert_eq!(
        loaded_keyframe.easing,
        crate::animation::EasingFunction::EaseOutQuad
    );

    manager
        .remove_keyframe_by_id(owner, "opacity", id)
        .expect("service should remove the identified key");
    let read = shared.read().expect("project should remain readable");
    let property = read
        .get_node(node_id)
        .and_then(|node| node.properties().get("opacity"))
        .expect("property should remain as a constant");
    assert_eq!(property.evaluator, "constant");
    assert_eq!(
        property.value(),
        Some(&PropertyValue::Number(OrderedFloat(0.75)))
    );
}

struct AudioDefaultsProbe;

impl crate::plugin::Plugin for AudioDefaultsProbe {
    fn id(&self) -> &str {
        "test.audio-defaults-probe"
    }

    fn name(&self) -> String {
        "Audio Defaults Probe".to_string()
    }

    fn category(&self) -> String {
        "Converter".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl crate::plugin::EntityConverterPlugin for AudioDefaultsProbe {
    fn supports_kind(&self, kind: &str) -> bool {
        kind == "audio"
    }

    fn convert_entity(
        &self,
        _evaluator: &crate::plugin::FrameEvaluationContext,
        _layer: &Node,
        _time: f64,
    ) -> Option<crate::model::frame::entity::FrameObject> {
        None
    }

    fn get_property_definitions(
        &self,
        _canvas_width: u64,
        _canvas_height: u64,
        _clip_width: u64,
        _clip_height: u64,
    ) -> Vec<PropertyDefinition> {
        vec![PropertyDefinition::new(
            "probe_profile",
            PropertyUiType::Text,
            "Probe Profile",
            PropertyValue::String("registered-default".to_string()),
        )]
    }
}

struct InvalidImageMetadataProbe;

impl crate::plugin::Plugin for InvalidImageMetadataProbe {
    fn id(&self) -> &str {
        "test.invalid-image-metadata"
    }

    fn name(&self) -> String {
        "Invalid Image Metadata".to_string()
    }

    fn category(&self) -> String {
        "Converter".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl crate::plugin::EntityConverterPlugin for InvalidImageMetadataProbe {
    fn supports_kind(&self, kind: &str) -> bool {
        kind == "image"
    }

    fn convert_entity(
        &self,
        _evaluator: &crate::plugin::FrameEvaluationContext,
        _layer: &Node,
        _time: f64,
    ) -> Option<crate::model::frame::entity::FrameObject> {
        None
    }

    fn get_property_definitions(
        &self,
        _canvas_width: u64,
        _canvas_height: u64,
        _clip_width: u64,
        _clip_height: u64,
    ) -> Vec<PropertyDefinition> {
        vec![PropertyDefinition::new(
            "broken_scale",
            PropertyUiType::Float {
                min: 0.0,
                max: 10.0,
                step: 0.0,
                suffix: String::new(),
                min_hard_limit: true,
                max_hard_limit: true,
            },
            "Broken Scale",
            PropertyValue::Number(OrderedFloat(1.0)),
        )]
    }
}

#[test]
fn audio_media_factory_materializes_registered_optional_converter_defaults() {
    let plugins = Arc::new(PluginManager::default());
    plugins.register_entity_converter_plugin(Arc::new(AudioDefaultsProbe));
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("audio media factory"))),
        plugins,
    );

    let node = manager
        .create_media_node(
            "Audio",
            MediaNodeRequest::Audio {
                asset_id: Uuid::new_v4(),
                file_path: "sound.wav".to_string(),
                audio_stream_index: Some(2),
            },
            1920,
            1080,
            0,
            0,
        )
        .expect("an optional registered audio converter should participate in authoring");

    assert_eq!(
        node.properties()
            .get("probe_profile")
            .and_then(Property::value),
        Some(&PropertyValue::String("registered-default".to_string()))
    );
    assert_eq!(
        node.properties().get("file_path").and_then(Property::value),
        Some(&PropertyValue::String("sound.wav".to_string()))
    );
}

#[test]
fn media_factory_rejects_invalid_converter_property_metadata() {
    let plugins = Arc::new(PluginManager::new());
    plugins.register_entity_converter_plugin(Arc::new(InvalidImageMetadataProbe));
    let manager = ProjectManager::new(
        Arc::new(RwLock::new(Project::new("invalid media metadata"))),
        plugins,
    );

    let error = manager
        .create_media_node(
            "Image",
            MediaNodeRequest::Image {
                asset_id: Uuid::new_v4(),
                file_path: "broken.png".to_string(),
            },
            1920,
            1080,
            64,
            64,
        )
        .expect_err("zero Float step must be rejected before Node creation");

    assert!(matches!(
        error,
        LibraryError::Validation(message)
            if message.contains("broken_scale")
                && message.contains("step must be greater than zero")
    ));
}

#[test]
fn generator_factories_materialize_every_converter_default_and_content_value()
-> Result<(), Box<dyn std::error::Error>> {
    let shared = Arc::new(RwLock::new(Project::new("generator factories")));
    let manager = ProjectManager::new(Arc::clone(&shared), Arc::new(PluginManager::default()));
    let canvas = (1920, 1080);

    let text = "Factory text";
    let font = "Arial";
    let (text_width, text_height) = measure_text_size(text, font, 100.0);
    let Ok(text_node) = manager.create_text_node(text, font, canvas.0, canvas.1) else {
        panic!("text node factory should succeed");
    };
    assert_eq!(
        text_node.content(),
        &NodeContent::Generator(GeneratorContent::Text)
    );
    assert_converter_properties(
        &manager,
        &text_node,
        "text",
        (canvas.0, canvas.1, text_width as u64, text_height as u64),
    );
    assert_property_value(&text_node, "text", PropertyValue::String(text.to_string()));
    assert_property_value(
        &text_node,
        "font_family",
        PropertyValue::String(font.to_string()),
    );

    let path = "M 0 0 H 120 V 80 H 0 Z";
    let Ok(shape_node) = manager.create_shape_node(path, canvas.0, canvas.1, 120, 80) else {
        panic!("shape node factory should succeed");
    };
    assert_eq!(
        shape_node.content(),
        &NodeContent::Generator(GeneratorContent::Shape)
    );
    assert_converter_properties(
        &manager,
        &shape_node,
        "shape",
        (canvas.0, canvas.1, 120, 80),
    );
    let path_value = crate::model::path::parse_legacy_svg_path_data(path)?;
    assert_property_value(&shape_node, "path", PropertyValue::Path(path_value.clone()));

    let shader = "half4 main(float2 p) { return half4(1); }";
    let Ok(sksl_node) = manager.create_sksl_node(shader, canvas.0, canvas.1) else {
        panic!("SkSL node factory should succeed");
    };
    assert_eq!(
        sksl_node.content(),
        &NodeContent::Generator(GeneratorContent::SkSL)
    );
    assert_converter_properties(
        &manager,
        &sksl_node,
        "sksl",
        (canvas.0, canvas.1, canvas.0, canvas.1),
    );
    assert_property_value(
        &sksl_node,
        "shader",
        PropertyValue::String(shader.to_string()),
    );

    let color = Color {
        r: 12,
        g: 34,
        b: 56,
        a: 78,
    };
    let Ok(solid_node) = manager.create_solid_node(color.clone(), canvas.0, canvas.1) else {
        panic!("solid node factory should succeed");
    };
    assert_eq!(
        solid_node.content(),
        &NodeContent::Generator(GeneratorContent::Solid)
    );
    assert_converter_properties(
        &manager,
        &solid_node,
        "solid",
        (canvas.0, canvas.1, canvas.0, canvas.1),
    );
    let color_value = crate::model::property::ColorValue::from_straight_srgba8(&color);
    assert_property_value(&solid_node, "color", PropertyValue::ColorValue(color_value));

    for node in [&text_node, &shape_node, &sksl_node, &solid_node] {
        for detached_property in ["position", "scale", "rotation", "anchor", "opacity"] {
            assert!(
                node.properties().get(detached_property).is_none(),
                "{} must not embed {detached_property}; use Transform/Style",
                node.name
            );
        }
    }
    for (node, expected_kind) in [
        (&text_node, "Text"),
        (&shape_node, "Shape"),
        (&sksl_node, "SkSL"),
        (&solid_node, "Solid"),
    ] {
        let Ok(serialized) = serde_json::to_value(node) else {
            panic!("{} should serialize", node.name);
        };
        assert_eq!(serialized["content"]["data"], expected_kind);
        assert!(serialized["content"]["data"].is_string());
    }

    let text_id = text_node.id;
    let shape_id = shape_node.id;
    let sksl_id = sksl_node.id;
    let solid_id = solid_node.id;
    let mut project = Project::new("generator roundtrip");
    for node in [text_node, shape_node, sksl_node, solid_node] {
        project.add_node(node);
    }
    let Ok(saved) = project.save() else {
        panic!("generator project should save");
    };
    let Ok(loaded) = Project::load(&saved) else {
        panic!("generator project should load");
    };
    let loaded_expectations = [
        (text_id, "text", PropertyValue::String(text.to_string())),
        (shape_id, "path", PropertyValue::Path(path_value)),
        (sksl_id, "shader", PropertyValue::String(shader.to_string())),
    ];
    for (node_id, property_key, expected) in loaded_expectations {
        let Some(node) = loaded.get_node(node_id) else {
            panic!("generator {node_id} should survive save/load");
        };
        assert_property_value(node, property_key, expected);
    }
    let loaded_solid_color = loaded
        .get_node(solid_id)
        .and_then(|node| node.properties().get("color"))
        .and_then(Property::value)
        .and_then(|value| match value {
            PropertyValue::ColorValue(value) => value.try_to_renderer_srgba8().ok(),
            PropertyValue::Color(value) => Some(value.clone()),
            _ => None,
        });
    assert_eq!(loaded_solid_color, Some(color));
    Ok(())
}

#[test]
fn converter_backed_generator_properties_have_typed_catalog_inputs() {
    let shared = Arc::new(RwLock::new(Project::new("generator catalog contract")));
    let manager = ProjectManager::new(Arc::clone(&shared), Arc::new(PluginManager::default()));
    let nodes = [
        manager
            .create_text_node("Text", DEFAULT_TEXT_FONT, 1920, 1080)
            .expect("Text factory should succeed"),
        manager
            .create_shape_node(DEFAULT_SHAPE_PATH, 1920, 1080, 100, 100)
            .expect("Shape factory should succeed"),
        manager
            .create_sksl_node(DEFAULT_SKSL_SHADER, 1920, 1080)
            .expect("SkSL factory should succeed"),
        manager
            .create_solid_node(Color::white(), 1920, 1080)
            .expect("Solid factory should succeed"),
    ];

    for node in nodes {
        let descriptor = native_node_descriptor_for_node(&node)
            .expect("every native Generator must have a catalog descriptor");
        let mut property_names = node
            .properties()
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        property_names.sort_unstable();
        let mut input_names = descriptor
            .ports()
            .iter()
            .filter(|port| port.direction == PortDirection::Input && port.key != TIME_PORT)
            .map(|port| port.key.as_str())
            .collect::<Vec<_>>();
        input_names.sort_unstable();
        assert_eq!(
            input_names,
            property_names,
            "{} catalog inputs drifted from its converter-backed properties",
            descriptor.catalog_id()
        );

        for port in descriptor
            .ports()
            .iter()
            .filter(|port| port.direction == PortDirection::Input && port.key != TIME_PORT)
        {
            let value = node
                .properties()
                .get(&port.key)
                .and_then(Property::value)
                .expect("catalog property input must reference a materialized property");
            let expected_type = match value {
                PropertyValue::Path(_) => PortDataType::Path,
                PropertyValue::String(_) => PortDataType::String,
                PropertyValue::Number(_) => PortDataType::Number,
                PropertyValue::Color(_) | PropertyValue::ColorValue(_) => PortDataType::Color,
                _ => panic!(
                    "{} property {} has no asserted graph type for {value:?}",
                    descriptor.catalog_id(),
                    port.key
                ),
            };
            assert_eq!(
                port.data_type,
                expected_type,
                "{}.{} catalog type drifted from its runtime property",
                descriptor.catalog_id(),
                port.key
            );
        }
    }
}

#[test]
fn generator_clip_factories_wrap_bare_sources_in_explicit_image_graphs() {
    let shared = Arc::new(RwLock::new(Project::new("shared generator factory")));
    let manager = ProjectManager::new(Arc::clone(&shared), Arc::new(PluginManager::default()));

    let Ok(direct_text) = manager.create_text_node("same", DEFAULT_TEXT_FONT, 640, 480) else {
        panic!("direct text factory should succeed");
    };
    let Ok(text_bundle) = manager.create_text_clip("same", 0.0, 1.0, 640, 480) else {
        panic!("text clip factory should succeed");
    };
    let clip_text = text_bundle
        .graph
        .nodes
        .iter()
        .find(|node| node.content() == direct_text.content())
        .expect("text clip must retain the bare Shape source");
    assert_eq!(clip_text.properties(), direct_text.properties());
    assert!(matches!(
        text_bundle.primary_node().map(Node::content),
        Some(NodeContent::PluginOperation(_))
    ));
    assert_eq!(text_bundle.graph.nodes.len(), 3);
    assert_eq!(text_bundle.graph.connections.len(), 2);
    assert!(
        text_bundle
            .graph
            .connections
            .iter()
            .all(|connection| connection.order == 0)
    );

    let Ok(direct_shape) = manager.create_shape_node(DEFAULT_SHAPE_PATH, 640, 480, 100, 100) else {
        panic!("direct shape factory should succeed");
    };
    let Ok(shape_bundle) = manager.create_shape_clip(0.0, 1.0, 640, 480) else {
        panic!("shape clip factory should succeed");
    };
    let clip_shape = shape_bundle
        .graph
        .nodes
        .iter()
        .find(|node| node.content() == direct_shape.content())
        .expect("shape clip must retain the bare Shape source");
    assert_eq!(clip_shape.properties(), direct_shape.properties());
    assert!(matches!(
        shape_bundle.primary_node().map(Node::content),
        Some(NodeContent::Merge)
    ));
    assert_eq!(shape_bundle.graph.nodes.len(), 5);
    assert_eq!(
        shape_bundle
            .graph
            .connections
            .iter()
            .map(|connection| connection.order)
            .collect::<Vec<_>>(),
        vec![0, 0, 0, 0, 1]
    );

    let Ok(direct_sksl) = manager.create_sksl_node(DEFAULT_SKSL_SHADER, 640, 480) else {
        panic!("direct SkSL factory should succeed");
    };
    let Ok(sksl_bundle) = manager.create_sksl_clip(0.0, 1.0, 640, 480) else {
        panic!("SkSL clip factory should succeed");
    };
    let clip_sksl = sksl_bundle
        .graph
        .nodes
        .iter()
        .find(|node| node.content() == direct_sksl.content())
        .expect("SkSL clip must retain the bare Image source");
    assert_eq!(clip_sksl.properties(), direct_sksl.properties());
    assert!(matches!(
        sksl_bundle.primary_node().map(Node::content),
        Some(NodeContent::PluginOperation(_))
    ));
    assert_eq!(sksl_bundle.graph.nodes.len(), 2);
    assert_eq!(sksl_bundle.graph.connections.len(), 1);
}

fn manager_with_empty_clip() -> (Arc<RwLock<Project>>, ProjectManager, Uuid, Uuid) {
    let mut project = Project::new("shape splice");
    let (composition, track) = Composition::new("main", 640, 480, 30.0, 5.0);
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let clip = Clip::new("clip", 0.0, 5.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();
    let shared = Arc::new(RwLock::new(project));
    let manager = ProjectManager::new(Arc::clone(&shared), Arc::new(PluginManager::default()));
    (shared, manager, track_id, clip_id)
}

#[test]
fn add_shape_operation_accepts_a_dangling_source() {
    let (shared, manager, _, clip_id) = manager_with_empty_clip();
    let source = manager
        .create_text_node("draft", DEFAULT_TEXT_FONT, 640, 480)
        .unwrap();
    let source_id = source.id;
    shared
        .write()
        .unwrap()
        .insert_node_graph(
            NodeContainer::Clip(clip_id),
            NodeGraphBundle::new(vec![source], Vec::new(), None),
        )
        .unwrap();

    manager.add_effector(source_id, "opacity").unwrap();
    let project = shared.read().unwrap();
    let operation = project
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::PluginOperation(operation)
                    if operation.category == "effector" && operation.component_id == "opacity"
            )
        })
        .unwrap();
    assert_eq!(
        project.find_node_container(operation.id),
        Some(NodeContainer::Clip(clip_id))
    );
    let inserted_connection = project
        .connections
        .iter()
        .find(|connection| {
            connection.from.owner == PortOwner::Node(source_id)
                && connection.to.owner == PortOwner::Node(operation.id)
        })
        .expect("shape operation must receive the dangling source");
    assert_eq!(
        inserted_connection.from,
        PortAddress::new(PortOwner::Node(source_id), SHAPE_OUTPUT_PORT)
    );
    assert_eq!(
        inserted_connection.to,
        PortAddress::new(PortOwner::Node(operation.id), SHAPE_INPUT_PORT)
    );
}

#[test]
fn add_shape_operation_preserves_every_fanout_connection_identity_and_order() {
    let (shared, manager, _, clip_id) = manager_with_empty_clip();
    let graph = manager
        .create_shape_graph(DEFAULT_SHAPE_PATH, 640, 480, 100, 100)
        .unwrap();
    let source_id = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(GeneratorContent::Shape)
            )
        })
        .unwrap()
        .id;
    let transform_id = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::PluginOperation(operation)
                    if operation.category == crate::plugin::TRANSFORM_CATEGORY
            )
        })
        .expect("Shape graph must contain Transform")
        .id;
    let originals = graph
        .connections
        .iter()
        .filter(|connection| connection.from.owner == PortOwner::Node(transform_id))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(originals.len(), 2);
    shared
        .write()
        .unwrap()
        .insert_node_graph(NodeContainer::Clip(clip_id), graph)
        .unwrap();

    manager.add_effector(source_id, "transform").unwrap();
    let project = shared.read().unwrap();
    for original in originals {
        let rewired = project
            .connections
            .iter()
            .find(|connection| connection.id == original.id)
            .unwrap();
        assert_eq!(rewired.to, original.to);
        assert_eq!(rewired.order, original.order);
        assert_ne!(rewired.from, original.from);
        assert_eq!(rewired.from.port, SHAPE_OUTPUT_PORT);
    }
}

#[test]
fn add_shape_operation_stops_at_a_cross_container_boundary() {
    let (shared, manager, track_id, clip_id) = manager_with_empty_clip();
    let source = manager
        .create_text_node("cross", DEFAULT_TEXT_FONT, 640, 480)
        .unwrap();
    let source_id = source.id;
    let downstream = manager
        .plugin_manager
        .create_effector_operation_node("transform")
        .unwrap();
    let downstream_id = downstream.id;
    {
        let mut project = shared.write().unwrap();
        project
            .insert_node_graph(
                NodeContainer::Clip(clip_id),
                NodeGraphBundle::new(vec![source], Vec::new(), None),
            )
            .unwrap();
        project
            .insert_node_graph(
                NodeContainer::Track(track_id),
                NodeGraphBundle::new(vec![downstream], Vec::new(), None),
            )
            .unwrap();
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(source_id), SHAPE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(downstream_id), SHAPE_INPUT_PORT),
            )
            .unwrap();
    }
    let original = shared
        .read()
        .unwrap()
        .connections
        .iter()
        .find(|connection| {
            connection.from.owner == PortOwner::Node(source_id)
                && connection.to.owner == PortOwner::Node(downstream_id)
        })
        .expect("cross-container shape connection must exist")
        .clone();

    manager.add_effector(source_id, "opacity").unwrap();
    let project = shared.read().unwrap();
    let inserted = project
        .nodes
        .values()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::PluginOperation(operation)
                    if operation.category == "effector"
                        && operation.component_id == "opacity"
            )
        })
        .unwrap();
    assert_eq!(
        project.find_node_container(inserted.id),
        Some(NodeContainer::Clip(clip_id))
    );
    let rewired = project
        .connections
        .iter()
        .find(|connection| connection.id == original.id)
        .unwrap();
    assert_eq!(rewired.to, original.to);
    assert_eq!(rewired.order, original.order);
    assert_eq!(rewired.from.owner, PortOwner::Node(inserted.id));
}
