use super::*;
use crate::model::property::Property;
use crate::plugin::PluginManager;

fn operation_with_ports(ports: Vec<PortDefinition>) -> Node {
    let node = PluginManager::default()
        .create_path_effect_operation_node("trim")
        .expect("built-in Trim Path factory must exist");
    let mut persisted = serde_json::to_value(node).expect("operation must serialize");
    persisted["content"]["data"]["declared_ports"] =
        serde_json::to_value(ports).expect("ports must serialize");
    serde_json::from_value(persisted).expect("persisted operation ports must load")
}

fn number_definition() -> PropertyDefinition {
    PropertyDefinition::new(
        "amount",
        PropertyUiType::Float {
            min: 0.0,
            max: 100.0,
            step: 1.0,
            suffix: String::new(),
            min_hard_limit: true,
            max_hard_limit: true,
        },
        "Amount",
        PropertyValue::Number(OrderedFloat(50.0)),
    )
}

#[test]
fn generator_completion_requires_the_exact_constant_definition_contract() -> Result<(), String> {
    let definitions = vec![number_definition()];
    let complete = PropertyMap::from_definitions(&definitions);
    assert!(
        Node::new_generator("complete", GeneratorContent::Solid, &definitions, complete,).is_ok()
    );

    let missing = match Node::new_generator(
        "missing",
        GeneratorContent::Solid,
        &definitions,
        PropertyMap::new(),
    ) {
        Ok(_) => return Err("missing Generator property was accepted".to_string()),
        Err(error) => error,
    };
    assert!(missing.contains("omitted declared property 'amount'"));

    let mut unknown = PropertyMap::from_definitions(&definitions);
    unknown.set(
        "typo".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(1.0))),
    );
    let unknown =
        match Node::new_generator("unknown", GeneratorContent::Solid, &definitions, unknown) {
            Ok(_) => return Err("undeclared Generator property was accepted".to_string()),
            Err(error) => error,
        };
    assert!(unknown.contains("undeclared property 'typo'"));

    let mut dynamic = PropertyMap::from_definitions(&definitions);
    dynamic.set(
        "amount".to_string(),
        Property::expression("time".to_string(), PropertyValue::from(0.0)),
    );
    let dynamic =
        match Node::new_generator("dynamic", GeneratorContent::Solid, &definitions, dynamic) {
            Ok(_) => return Err("dynamic Generator initial value was accepted".to_string()),
            Err(error) => error,
        };
    assert!(dynamic.contains("not a constant value"));

    let mut invalid = PropertyMap::from_definitions(&definitions);
    invalid.set(
        "amount".to_string(),
        Property::constant(PropertyValue::String("wrong".to_string())),
    );
    let invalid =
        match Node::new_generator("invalid", GeneratorContent::Solid, &definitions, invalid) {
            Ok(_) => return Err("invalid Generator property value was accepted".to_string()),
            Err(error) => error,
        };
    assert!(invalid.contains("is invalid"));
    Ok(())
}

#[test]
fn sparse_pre_v1_generator_still_deserializes_losslessly() -> Result<(), serde_json::Error> {
    let mut sparse = serde_json::to_value(Node::new_merge("persisted sparse generator"))?;
    sparse["content"] = serde_json::json!({ "type": "Generator", "data": "Text" });
    sparse["properties"] = serde_json::json!({});
    let json = serde_json::to_string(&sparse)?;
    let loaded: Node = serde_json::from_str(&json)?;

    assert_eq!(
        loaded.content(),
        &NodeContent::Generator(GeneratorContent::Text)
    );
    assert!(loaded.properties().iter().next().is_none());
    Ok(())
}

#[test]
fn pre_v1_time_modulo_json_has_no_fmod_alias() -> Result<(), serde_json::Error> {
    let mut legacy = serde_json::to_value(Node::new_fmod("legacy value kind"))?;
    legacy["content"]["data"] = serde_json::Value::String("TimeModulo".to_string());
    let error = serde_json::from_value::<Node>(legacy).unwrap_err();
    assert!(error.to_string().contains("unknown variant `TimeModulo`"));
    Ok(())
}

#[test]
fn bypass_state_round_trips_and_missing_pre_v1_state_defaults_off() -> Result<(), serde_json::Error>
{
    let mut node = Node::new_add("persisted bypass");
    node.bypassed = true;
    let encoded = serde_json::to_value(&node)?;
    assert_eq!(serde_json::from_value::<Node>(encoded.clone())?, node);

    let mut without_bypass = encoded;
    without_bypass
        .as_object_mut()
        .expect("Node serializes as an object")
        .remove("bypassed");
    assert!(!serde_json::from_value::<Node>(without_bypass)?.bypassed);
    Ok(())
}

#[test]
fn bypass_capability_requires_supported_unambiguous_ports_for_every_output() {
    for data_type in [
        PortDataType::Image,
        PortDataType::Shape,
        PortDataType::List,
        PortDataType::Numeric,
        PortDataType::Number,
        PortDataType::Vec2,
        PortDataType::Vec3,
        PortDataType::Vec4,
        PortDataType::Audio,
    ] {
        let node = operation_with_ports(vec![
            PortDefinition::input("source", "Source", data_type),
            PortDefinition::output(
                "result",
                "Result",
                data_type,
                PortSide::Right,
                PortExposure::Graph,
            ),
        ]);
        assert!(node.supports_bypass(), "{data_type:?} must pass through");
        assert_eq!(node.bypass_input_for_output("result"), Some("source"));
    }

    let ambiguous = operation_with_ports(vec![
        PortDefinition::input("left", "Left", PortDataType::Image),
        PortDefinition::input("right", "Right", PortDataType::Image),
        PortDefinition::output(
            "result",
            "Result",
            PortDataType::Image,
            PortSide::Right,
            PortExposure::Graph,
        ),
    ]);
    assert_eq!(ambiguous.bypass_input_for_output("result"), None);
    assert!(!ambiguous.supports_bypass());
}

#[test]
fn particle_catalog_advertises_bypass_only_for_type_preserving_modifiers() {
    for catalog_id in [
        "native.particle.shape-location",
        "native.particle.initialize",
        "native.particle.gravity-force",
        "native.particle.drag-force",
    ] {
        let node = Node::new_catalog_node(catalog_id).expect("implemented Particle modifier");
        assert!(node.supports_bypass(), "{catalog_id}");
        assert_eq!(
            node.bypass_input_for_output("particles"),
            Some("particles"),
            "{catalog_id}"
        );
    }
    for catalog_id in ["native.particle.emitter", "native.particle.sprite-renderer"] {
        let node = Node::new_catalog_node(catalog_id).expect("implemented Particle endpoint");
        assert!(!node.supports_bypass(), "{catalog_id}");
    }
}

#[test]
fn authored_edits_cannot_extend_a_factory_property_contract() {
    let mut node = Node::new_fmod("sealed property contract");
    let unknown = Property::constant(PropertyValue::Number(OrderedFloat(2.0)));

    assert!(node.set_property("unknown".to_string(), unknown).is_err());
    assert!(!node.update_property_or_keyframe(
        "unknown",
        0.0,
        PropertyValue::Number(OrderedFloat(2.0)),
        None,
    ));
    assert!(
        node.upsert_keyframe_with_id(
            "unknown",
            0.0,
            PropertyValue::Number(OrderedFloat(2.0)),
            None,
        )
        .is_none()
    );
    assert!(node.properties().get("unknown").is_none());
    assert!(node.properties().get(FMOD_DIVISOR_INPUT_PORT).is_some());
    assert_eq!(
        ValueContent::Fmod.bypass_input_for_output(NUMBER_RESULT_OUTPUT_PORT),
        Some(FMOD_X_INPUT_PORT)
    );
}

#[test]
fn basic_numeric_factories_share_ports_and_use_safe_identity_defaults() {
    for (node, content, default_b) in [
        (Node::new_add("Add"), ValueContent::Add, 0.0),
        (Node::new_subtract("Subtract"), ValueContent::Subtract, 0.0),
        (Node::new_multiply("Multiply"), ValueContent::Multiply, 1.0),
        (Node::new_divide("Divide"), ValueContent::Divide, 1.0),
    ] {
        assert_eq!(node.content(), &NodeContent::Value(content));
        assert_eq!(
            node.properties()
                .get(NUMERIC_B_INPUT_PORT)
                .and_then(Property::value),
            Some(&PropertyValue::Number(OrderedFloat(default_b)))
        );
        assert_eq!(
            content
                .port_definitions()
                .iter()
                .map(|port| port.key.as_str())
                .collect::<Vec<_>>(),
            vec![
                NUMERIC_A_INPUT_PORT,
                NUMERIC_B_INPUT_PORT,
                NUMBER_RESULT_OUTPUT_PORT,
            ]
        );
        assert_eq!(
            content.bypass_input_for_output(NUMBER_RESULT_OUTPUT_PORT),
            Some(NUMERIC_A_INPUT_PORT)
        );
    }
}

#[test]
fn every_native_value_has_one_complete_unique_descriptor_contract() {
    let mut keys = std::collections::HashSet::new();
    let mut labels = std::collections::HashSet::new();
    let mut symbols = std::collections::HashSet::new();

    assert_eq!(ValueContent::ALL.len(), 5);
    for content in ValueContent::ALL {
        assert!(keys.insert(content.operation_key()));
        assert!(labels.insert(content.label()));
        assert!(symbols.insert(content.symbol()));

        let node = Node::new_value(content.label(), content);
        assert_eq!(node.content(), &NodeContent::Value(content));
        assert_eq!(
            node.properties(),
            &PropertyMap::from_definitions(content.property_definitions())
        );

        let ports = content.port_definitions();
        assert_eq!(ports.len(), 3);
        assert!(ports.iter().any(|port| {
            port.direction == crate::model::project::PortDirection::Input
                && port.key == content.primary_input()
        }));
        assert!(ports.iter().any(|port| {
            port.direction == crate::model::project::PortDirection::Input
                && port.key == content.secondary_input()
        }));
        assert!(ports.iter().any(|port| {
            port.direction == crate::model::project::PortDirection::Output
                && port.key == NUMBER_RESULT_OUTPUT_PORT
        }));
        assert_eq!(
            content.bypass_input_for_output(NUMBER_RESULT_OUTPUT_PORT),
            Some(content.primary_input())
        );
        let _ = content.numeric_operation();
    }
}

#[test]
fn every_list_catalog_factory_is_complete_typed_and_roundtrips() {
    use crate::model::project::connection::{LIST_INDEX_INPUT_PORT, LIST_ITEMS_INPUT_PORT};
    use crate::model::project::{PortDataType, PortDirection, PortMultiplicity};

    for operation in ListContent::ALL {
        let descriptor = native_node_descriptor(operation.catalog_id()).unwrap();
        assert_eq!(descriptor.label(), operation.label());
        assert_eq!(
            descriptor.runtime_status(),
            NativeNodeRuntimeStatus::Implemented
        );
        assert_eq!(descriptor.factory(), NativeNodeFactory::List(operation),);
        let node = Node::new_catalog_node(operation.catalog_id()).unwrap();
        assert_eq!(node.content(), &NodeContent::List(operation));
        assert_eq!(
            node.properties(),
            &PropertyMap::from_definitions(operation.property_definitions())
        );
        assert!(!node.supports_bypass());
        let restored: Node = serde_json::from_str(&serde_json::to_string(&node).unwrap()).unwrap();
        assert_eq!(restored, node);
    }

    let make = native_node_descriptor(ListContent::Make.catalog_id()).unwrap();
    assert!(make.ports().iter().any(|port| {
        port.key == LIST_ITEMS_INPUT_PORT
            && port.direction == PortDirection::Input
            && port.data_type == PortDataType::Any
            && port.multiplicity == PortMultiplicity::Variadic
    }));
    let get = Node::new_catalog_node(ListContent::GetItem.catalog_id()).unwrap();
    assert_eq!(
        get.properties()
            .get(LIST_INDEX_INPUT_PORT)
            .and_then(Property::value),
        Some(&PropertyValue::Integer(0))
    );
}

#[test]
fn every_color_catalog_factory_initializes_typed_defaults_and_roundtrips() {
    use crate::model::project::{PortDataType, PortDirection};

    for operation in ColorContent::ALL {
        let descriptor = native_node_descriptor(operation.catalog_id()).unwrap();
        assert_eq!(descriptor.label(), operation.label());
        assert_eq!(descriptor.category(), "Color");
        assert_eq!(
            descriptor.runtime_status(),
            NativeNodeRuntimeStatus::Implemented
        );
        assert_eq!(descriptor.factory(), NativeNodeFactory::Color(operation));
        let node = Node::new_catalog_node(operation.catalog_id()).unwrap();
        assert_eq!(node.content(), &NodeContent::Color(operation));
        assert_eq!(
            node.properties(),
            &PropertyMap::from_definitions(operation.property_definitions())
        );
        assert_eq!(
            node.supports_bypass(),
            operation == ColorContent::ConvertSpace
        );
        assert_eq!(
            node.bypass_input_for_output(COLOR_VALUE_PORT),
            (operation == ColorContent::ConvertSpace).then_some(COLOR_VALUE_PORT)
        );
        assert!(descriptor.ports().iter().any(|port| {
            port.direction == PortDirection::Output
                && matches!(
                    port.data_type,
                    PortDataType::Color | PortDataType::Number | PortDataType::String
                )
        }));
        let restored: Node = serde_json::from_str(&serde_json::to_string(&node).unwrap()).unwrap();
        assert_eq!(restored, node);
    }

    let compose = Node::new_catalog_node(ColorContent::Compose.catalog_id()).unwrap();
    assert_eq!(
        compose
            .properties()
            .get(COLOR_SPACE_PORT)
            .and_then(Property::value),
        Some(&PropertyValue::String("srgb".to_string()))
    );
    let mix = Node::new_catalog_node(ColorContent::Mix.catalog_id()).unwrap();
    assert_eq!(
        mix.properties()
            .get(COLOR_MIX_FACTOR_PORT)
            .and_then(Property::value),
        Some(&PropertyValue::Number(OrderedFloat(0.5)))
    );
    let convert = Node::new_catalog_node(ColorContent::ConvertSpace.catalog_id()).unwrap();
    assert_eq!(
        convert
            .properties()
            .get(COLOR_TARGET_SPACE_PORT)
            .and_then(Property::value),
        Some(&PropertyValue::String("linear-srgb".to_string()))
    );
}

#[test]
fn every_data_catalog_factory_is_complete_typed_and_roundtrips_losslessly() {
    use crate::model::path::{FillRule, PathValue};
    use crate::model::project::connection::DATA_VALUE_OUTPUT_PORT;
    use crate::model::project::{PortDataType, PortDirection};
    use crate::model::property::{ColorSpaceRef, ColorValue};

    for data in DataContent::ALL {
        let descriptor = native_node_descriptor(data.catalog_id()).unwrap();
        assert_eq!(descriptor.label(), data.label());
        assert_eq!(
            descriptor.runtime_status(),
            NativeNodeRuntimeStatus::Implemented
        );
        assert_eq!(descriptor.factory(), NativeNodeFactory::Data(data));
        assert_eq!(descriptor.ports().len(), 1);
        assert!(descriptor.ports().iter().any(|port| {
            port.key == DATA_VALUE_OUTPUT_PORT
                && port.direction == PortDirection::Output
                && port.data_type
                    == match data {
                        DataContent::Color => PortDataType::Color,
                        DataContent::Path => PortDataType::Path,
                    }
        }));
        let node = Node::new_catalog_node(data.catalog_id()).unwrap();
        assert_eq!(node.content(), &NodeContent::Data(data));
        assert_eq!(
            node.properties(),
            &PropertyMap::from_definitions(data.property_definitions())
        );
        assert!(!node.supports_bypass());
        let restored: Node = serde_json::from_str(&serde_json::to_string(&node).unwrap()).unwrap();
        assert_eq!(restored, node);
    }

    let color = DataContent::Color.property_definitions()[0].default_value();
    assert!(
        matches!(color, PropertyValue::ColorValue(value) if value.color_space() == &ColorSpaceRef::srgb())
    );
    let path = DataContent::Path.property_definitions()[0].default_value();
    assert_eq!(
        path,
        &PropertyValue::Path(PathValue::empty(FillRule::NonZero))
    );
    assert!(ColorValue::new(ColorSpaceRef::srgb(), [-1.0, 2.0, 3.0, 0.5]).is_ok());
}

#[test]
fn every_path_operation_catalog_factory_has_an_executable_typed_route() {
    use crate::model::project::connection::{PATH_OUTPUT_PORT, PATHS_INPUT_PORT};
    use crate::model::project::{PortDataType, PortDirection};

    for operation in PathOperationContent::ALL {
        let descriptor = native_node_descriptor(operation.catalog_id()).unwrap();
        assert_eq!(descriptor.label(), operation.label());
        assert_eq!(
            descriptor.runtime_status(),
            NativeNodeRuntimeStatus::Implemented
        );
        assert_eq!(descriptor.factory(), NativeNodeFactory::Path(operation));
        let node = Node::new_catalog_node(operation.catalog_id()).unwrap();
        assert_eq!(node.content(), &NodeContent::Path(operation));
        assert_eq!(
            node.properties(),
            &PropertyMap::from_definitions(operation.property_definitions())
        );
        assert!(!node.supports_bypass());
        let restored: Node = serde_json::from_str(&serde_json::to_string(&node).unwrap()).unwrap();
        assert_eq!(restored, node);
    }

    let union = native_node_descriptor(PathOperationContent::Union.catalog_id()).unwrap();
    assert!(union.ports().iter().any(|port| {
        port.key == PATHS_INPUT_PORT
            && port.direction == PortDirection::Input
            && port.data_type == PortDataType::List
    }));
    assert!(union.ports().iter().any(|port| {
        port.key == PATH_OUTPUT_PORT
            && port.direction == PortDirection::Output
            && port.data_type == PortDataType::Path
    }));
}

#[test]
fn canonical_data_ui_types_reject_lossy_legacy_substitutions() {
    use crate::model::frame::color::Color;
    use crate::model::path::{FillRule, PathValue};
    use crate::model::property::{ColorSpaceRef, ColorValue};

    let tagged = PropertyValue::ColorValue(
        ColorValue::new(
            ColorSpaceRef::new("scene_linear").unwrap(),
            [-0.5, 2.0, 0.0, 0.5],
        )
        .unwrap(),
    );
    let legacy = PropertyValue::Color(Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    });
    let path = PropertyValue::Path(PathValue::empty(FillRule::EvenOdd));
    assert!(tagged.is_compatible_with(&PropertyUiType::ColorValue));
    assert!(!tagged.is_compatible_with(&PropertyUiType::Color));
    assert!(legacy.is_compatible_with(&PropertyUiType::Color));
    assert!(!legacy.is_compatible_with(&PropertyUiType::ColorValue));
    assert!(path.is_compatible_with(&PropertyUiType::Path));
    assert!(!path.is_compatible_with(&PropertyUiType::MultilineText));
}

#[test]
fn module_creation_capability_is_owned_by_semantic_catalog_factories() {
    let particle = native_node_descriptor("native.particle.emitter").unwrap();
    assert_eq!(particle.factory(), NativeNodeFactory::NativeOperation);
    assert!(particle.supports_general_module_creation());
    assert!(particle.supports_host_module_creation());

    let particle_placeholder = native_node_descriptor("native.particle.spawn-burst").unwrap();
    assert_eq!(
        particle_placeholder.factory(),
        NativeNodeFactory::TypedPlaceholder
    );
    assert!(!particle_placeholder.supports_general_module_creation());
    assert!(!particle_placeholder.supports_host_module_creation());

    let transition_mix = native_node_descriptor(TRANSITION_IMAGE_MIX_NODE_ID).unwrap();
    assert_eq!(transition_mix.factory(), NativeNodeFactory::HostOperation);
    assert!(!transition_mix.supports_general_module_creation());
    assert!(transition_mix.supports_host_module_creation());
}

#[test]
fn native_particle_descriptor_rejects_schema_and_typed_value_drift() {
    let descriptor = native_node_descriptor("native.particle.emitter").unwrap();
    let emitter = Node::new_catalog_node(descriptor.catalog_id()).unwrap();
    descriptor
        .validate_native_properties(emitter.properties())
        .unwrap();

    let mut missing = emitter.properties().clone();
    missing.remove("rate");
    let error = descriptor.validate_native_properties(&missing).unwrap_err();
    assert!(
        error.contains("missing required Property 'rate'"),
        "{error}"
    );

    let mut unknown = emitter.properties().clone();
    unknown.set(
        "surprise".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(1.0))),
    );
    let error = descriptor.validate_native_properties(&unknown).unwrap_err();
    assert!(error.contains("unknown Property 'surprise'"), "{error}");

    let mut wrong_type = emitter.properties().clone();
    wrong_type.set(
        "rate".to_string(),
        Property::constant(PropertyValue::String("fast".to_string())),
    );
    let error = descriptor
        .validate_native_properties(&wrong_type)
        .unwrap_err();
    assert!(error.contains("Property 'rate' expects"), "{error}");

    let mut non_finite = emitter.properties().clone();
    non_finite.set(
        "rate".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(f64::NAN))),
    );
    let error = descriptor
        .validate_native_properties(&non_finite)
        .unwrap_err();
    assert!(error.contains("Property 'rate' must be finite"), "{error}");

    let mut outside_hard_bounds = emitter.properties().clone();
    outside_hard_bounds.set(
        "rate".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(-1.0))),
    );
    let error = descriptor
        .validate_native_properties(&outside_hard_bounds)
        .unwrap_err();
    assert!(error.contains("cannot be less than 0"), "{error}");
}
