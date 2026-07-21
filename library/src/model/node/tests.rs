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
