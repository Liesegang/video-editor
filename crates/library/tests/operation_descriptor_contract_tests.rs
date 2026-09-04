use library::model::project::{
    IMAGE_OUTPUT_PORT, PortDataType, PortDefinition, PortDirection, PortExposure, PortMultiplicity,
    PortSide, SHAPE_INPUT_PORT, TIME_PORT,
};
use library::model::property::{PropertyDefinition, PropertyUiType, PropertyValue};
use library::plugin::{
    OperationDescriptor, OperationDescriptorError, PluginManager, STYLE_APPLY_OPERATION,
    STYLE_CATEGORY, property_port_key,
};

fn output(key: &str, label: &str, data_type: PortDataType) -> PortDefinition {
    PortDefinition::output(key, label, data_type, PortSide::Right, PortExposure::Graph)
}

fn descriptor_with_ports(
    ports: impl IntoIterator<Item = PortDefinition>,
) -> Result<OperationDescriptor, OperationDescriptorError> {
    OperationDescriptor::new(
        "external",
        "ordering-probe",
        "ordering-probe.v1",
        "Ordering Probe",
        vec![PropertyDefinition::new(
            "enabled",
            PropertyUiType::Bool,
            "Enabled",
            PropertyValue::Boolean(true),
        )],
        ports,
    )
}

#[test]
fn public_constructor_normalizes_time_properties_payloads_and_outputs_stably() {
    let descriptor = descriptor_with_ports([
        output("result-a", "Result A", PortDataType::Image),
        PortDefinition::input("payload-a", "Payload A", PortDataType::Image),
        PortDefinition::input(TIME_PORT, "Time", PortDataType::Number),
        output("result-b", "Result B", PortDataType::Number),
        PortDefinition::input("payload-b", "Payload B", PortDataType::Number),
    ])
    .expect("a valid external operation contract should normalize");

    let ports = descriptor.declared_ports();
    assert_eq!(
        ports
            .iter()
            .map(|port| port.key.clone())
            .collect::<Vec<_>>(),
        [
            TIME_PORT.to_string(),
            property_port_key("enabled"),
            "payload-a".to_string(),
            "payload-b".to_string(),
            "result-a".to_string(),
            "result-b".to_string(),
        ]
    );
    assert!(
        ports[..4]
            .iter()
            .all(|port| port.direction == PortDirection::Input)
    );
    assert!(
        ports[4..]
            .iter()
            .all(|port| port.direction == PortDirection::Output)
    );
}

#[test]
fn public_constructor_rejects_every_malformed_time_contract() {
    let wrong_direction = output(TIME_PORT, "Time", PortDataType::Number);
    let wrong_type = PortDefinition::input(TIME_PORT, "Time", PortDataType::String);
    let mut wrong_multiplicity = PortDefinition::input(TIME_PORT, "Time", PortDataType::Number);
    wrong_multiplicity.multiplicity = PortMultiplicity::Variadic;
    let mut wrong_exposure = PortDefinition::input(TIME_PORT, "Time", PortDataType::Number);
    wrong_exposure.exposure = PortExposure::Internal;
    let mut wrong_side = PortDefinition::input(TIME_PORT, "Time", PortDataType::Number);
    wrong_side.side = PortSide::Right;

    for (name, port) in [
        ("direction", wrong_direction),
        ("data type", wrong_type),
        ("multiplicity", wrong_multiplicity),
        ("exposure", wrong_exposure),
        ("side", wrong_side),
    ] {
        assert!(
            matches!(
                descriptor_with_ports([port]),
                Err(OperationDescriptorError::InvalidTimePort { .. })
            ),
            "malformed Time {name} must fail closed"
        );
    }
}

#[test]
fn public_constructor_rejects_duplicate_time_before_generic_key_collision() {
    assert_eq!(
        descriptor_with_ports([
            PortDefinition::input(TIME_PORT, "Time", PortDataType::Number),
            PortDefinition::input(TIME_PORT, "Another Time", PortDataType::Number),
        ])
        .expect_err("duplicate Time ports must not be normalized away"),
        OperationDescriptorError::MultipleTimePorts { count: 2 }
    );
}

#[test]
fn time_remains_optional_for_external_non_temporal_operations() {
    let descriptor = descriptor_with_ports([
        output("result", "Result", PortDataType::Number),
        PortDefinition::input("value", "Value", PortDataType::Number),
    ])
    .expect("external operations without temporal dependencies remain valid");

    assert_eq!(
        descriptor
            .declared_ports()
            .iter()
            .map(|port| port.key.clone())
            .collect::<Vec<_>>(),
        [
            property_port_key("enabled"),
            "value".to_string(),
            "result".to_string(),
        ]
    );
}

#[test]
fn bundled_fill_descriptor_keeps_its_persisted_port_contract() {
    let manager = PluginManager::default();
    let descriptor = manager
        .operation_descriptor(STYLE_CATEGORY, "fill", STYLE_APPLY_OPERATION)
        .expect("bundled Fill descriptor should resolve");

    assert_eq!(
        descriptor.declared_ports(),
        &[
            PortDefinition::input(TIME_PORT, "Time", PortDataType::Number),
            PortDefinition::input(&property_port_key("color"), "Color", PortDataType::Color),
            PortDefinition::input(
                &property_port_key("opacity"),
                "Opacity",
                PortDataType::Number,
            ),
            PortDefinition::input(&property_port_key("offset"), "Offset", PortDataType::Number,),
            PortDefinition::input(SHAPE_INPUT_PORT, "Shape", PortDataType::Shape),
            output(IMAGE_OUTPUT_PORT, "Image", PortDataType::Image),
        ]
    );
}
