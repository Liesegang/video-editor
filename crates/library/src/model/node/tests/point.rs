use super::super::*;

#[test]
fn set_position_catalog_keeps_position_field_only_and_supports_point_bypass() {
    let descriptor = native_node_descriptor(PointNodeRole::SetPosition.catalog_id()).unwrap();
    assert_eq!(
        descriptor.qa_id(),
        "node_editor.menu.create.point_set_position"
    );
    assert!(descriptor.supports_general_module_creation());
    assert!(
        descriptor
            .property_definition_for_input(POINT_POSITION_INPUT_PORT)
            .is_none()
    );
    for (key, data_type, has_property) in [
        (POINT_SOURCE_PORT, PortDataType::PointSource, false),
        (POINT_POSITION_INPUT_PORT, PortDataType::Vec3, false),
        (POINT_OFFSET_INPUT_PORT, PortDataType::Vec3, true),
        (POINT_SELECTION_INPUT_PORT, PortDataType::Boolean, true),
    ] {
        assert!(descriptor.ports().iter().any(|port| {
            port.direction == PortDirection::Input && port.key == key && port.data_type == data_type
        }));
        assert_eq!(
            descriptor.property_definition_for_input(key).is_some(),
            has_property
        );
    }
    let node = Node::new_catalog_node(descriptor.catalog_id()).unwrap();
    assert!(node.supports_bypass());
    assert_eq!(
        node.bypass_input_for_output(POINT_SOURCE_PORT),
        Some(POINT_SOURCE_PORT)
    );
    assert_eq!(PointNodeRole::SetPosition.attribute_type(), None);
}

#[test]
fn point_catalog_uses_node_identity_and_one_typed_store_contract() {
    use crate::model::point::{PointAttributeElementType, PointAttributeId};

    let info = native_node_descriptor("native.point.info").unwrap();
    assert!(info.supports_general_module_creation());
    assert_eq!(info.factory(), NativeNodeFactory::NativeOperation);
    assert!(info.property_definitions().is_empty());
    assert!(info.ports().iter().any(|port| {
        port.direction == PortDirection::Output
            && port.key == "position"
            && port.data_type == PortDataType::Vec3
    }));

    let stores = [
        (
            PointAttributeElementType::Number,
            PortDataType::Number,
            "native.point.store-number-attribute",
            "node_editor.menu.create.point_store_number_attribute",
        ),
        (
            PointAttributeElementType::Integer,
            PortDataType::Integer,
            "native.point.store-integer-attribute",
            "node_editor.menu.create.point_store_integer_attribute",
        ),
        (
            PointAttributeElementType::Boolean,
            PortDataType::Boolean,
            "native.point.store-boolean-attribute",
            "node_editor.menu.create.point_store_boolean_attribute",
        ),
        (
            PointAttributeElementType::Vec2,
            PortDataType::Vec2,
            "native.point.store-vec2-attribute",
            "node_editor.menu.create.point_store_vec2_attribute",
        ),
        (
            PointAttributeElementType::Vec3,
            PortDataType::Vec3,
            "native.point.store-vec3-attribute",
            "node_editor.menu.create.point_store_vec3_attribute",
        ),
        (
            PointAttributeElementType::Vec4,
            PortDataType::Vec4,
            "native.point.store-vec4-attribute",
            "node_editor.menu.create.point_store_vec4_attribute",
        ),
        (
            PointAttributeElementType::Color,
            PortDataType::Color,
            "native.point.store-color-attribute",
            "node_editor.menu.create.point_store_color_attribute",
        ),
    ];
    for (element_type, port_type, catalog_id, qa_id) in stores {
        let role = PointNodeRole::StoreAttribute(element_type);
        assert_eq!(role.catalog_id(), catalog_id);
        assert_eq!(PointNodeRole::from_catalog_id(catalog_id), Some(role));
        assert_eq!(role.attribute_type(), Some(element_type));
        let descriptor = native_node_descriptor(catalog_id).unwrap();
        assert_eq!(descriptor.qa_id(), qa_id);
        assert!(descriptor.supports_general_module_creation());
        assert_eq!(descriptor.factory(), NativeNodeFactory::NativeOperation);
        let node = Node::new_catalog_node(descriptor.catalog_id()).unwrap();
        assert!(node.supports_bypass());
        assert_eq!(
            node.bypass_input_for_output(POINT_SOURCE_PORT),
            Some(POINT_SOURCE_PORT)
        );
        assert_eq!(
            node.bypass_input_for_output(POINT_ATTRIBUTE_OUTPUT_PORT),
            Some(POINT_ATTRIBUTE_VALUE_PORT)
        );
        let expected_default = element_type.default_value();
        assert_eq!(
            node.properties().get("value").unwrap().get_static_value(),
            Some(&expected_default)
        );
        for direction in [PortDirection::Input, PortDirection::Output] {
            let port = descriptor
                .ports()
                .iter()
                .find(|port| {
                    port.direction == direction
                        && port.key
                            == if direction == PortDirection::Input {
                                POINT_ATTRIBUTE_VALUE_PORT
                            } else {
                                POINT_ATTRIBUTE_OUTPUT_PORT
                            }
                })
                .unwrap();
            assert_eq!(port.data_type, port_type);
        }
    }

    let store = native_node_descriptor(
        PointNodeRole::StoreAttribute(PointAttributeElementType::Number).catalog_id(),
    )
    .unwrap();
    let mut node = Node::new_catalog_node(store.catalog_id()).unwrap();
    let attribute_id = PointAttributeId::from_uuid(node.id);
    node.name = "heat".to_string();
    assert_eq!(PointAttributeId::from_uuid(node.id), attribute_id);
    assert_eq!(node.name, "heat");
    assert_eq!(PointNodeRole::Grid.attribute_type(), None);
    assert_eq!(PointNodeRole::Info.attribute_type(), None);
    assert!(native_node_descriptor("native.particle.set-attribute").is_none());

    let sprite = native_node_descriptor("native.particle.sprite-renderer").unwrap();
    assert!(sprite.ports().iter().any(|port| {
        port.direction == PortDirection::Input
            && port.key == PARTICLE_SYSTEM_PORT
            && port.data_type == PortDataType::PointSource
    }));
}

#[test]
fn point_grid_catalog_exposes_bounded_lattice_properties_and_point_output() {
    use crate::model::frame::point::{POINT_GRID_MAX_AXIS, POINT_GRID_MAX_SIZE};
    use crate::model::property::Vec3;

    let descriptor = native_node_descriptor("native.point.grid").unwrap();
    assert!(descriptor.supports_general_module_creation());
    assert_eq!(descriptor.factory(), NativeNodeFactory::NativeOperation);
    assert_eq!(descriptor.qa_id(), "node_editor.menu.create.point_grid");
    let input_keys = descriptor
        .ports()
        .iter()
        .filter(|port| port.direction == PortDirection::Input)
        .map(|port| port.key.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        input_keys,
        [
            "count_x", "count_y", "count_z", "spacing", "center", "size", "seed"
        ]
    );
    let outputs = descriptor
        .ports()
        .iter()
        .filter(|port| port.direction == PortDirection::Output)
        .collect::<Vec<_>>();
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].key, POINT_SOURCE_PORT);
    assert_eq!(outputs[0].data_type, PortDataType::PointSource);
    let input = |key| {
        descriptor
            .ports()
            .iter()
            .find(|port| port.direction == PortDirection::Input && port.key == key)
            .unwrap()
    };
    for key in ["count_x", "count_y", "count_z", "seed"] {
        assert_eq!(input(key).data_type, PortDataType::Integer);
    }
    for key in ["spacing", "center"] {
        assert_eq!(input(key).data_type, PortDataType::Vec3);
    }
    assert_eq!(input("size").data_type, PortDataType::Number);

    let node = Node::new_catalog_node(descriptor.catalog_id()).unwrap();
    let value = |key| {
        node.properties()
            .get(key)
            .unwrap()
            .get_static_value()
            .unwrap()
    };
    assert_eq!(value("count_x"), &PropertyValue::Integer(8));
    assert_eq!(value("count_y"), &PropertyValue::Integer(8));
    assert_eq!(value("count_z"), &PropertyValue::Integer(1));
    assert_eq!(
        value("spacing"),
        &PropertyValue::Vec3(Vec3 {
            x: OrderedFloat(24.0),
            y: OrderedFloat(24.0),
            z: OrderedFloat(24.0),
        })
    );
    assert_eq!(
        value("center"),
        &PropertyValue::Vec3(Vec3 {
            x: OrderedFloat(0.0),
            y: OrderedFloat(0.0),
            z: OrderedFloat(0.0),
        })
    );
    assert_eq!(value("size"), &PropertyValue::Number(OrderedFloat(8.0)));
    assert_eq!(value("seed"), &PropertyValue::Integer(1));
    descriptor
        .validate_native_properties(node.properties())
        .unwrap();
    let definitions = descriptor.property_definitions();
    let definition = |key| {
        definitions
            .iter()
            .find(|definition| definition.name() == key)
            .unwrap()
    };
    assert!(matches!(
        definition("count_x").ui_type(),
        PropertyUiType::Integer { min: 1, max, min_hard_limit: true, max_hard_limit: true, .. }
            if *max == i64::from(POINT_GRID_MAX_AXIS)
    ));
    assert!(matches!(
        definition("size").ui_type(),
        PropertyUiType::Float { min, max, min_hard_limit: true, max_hard_limit: true, .. }
            if *min > 0.0 && *max == f64::from(POINT_GRID_MAX_SIZE)
    ));

    let mut negative_spacing = node.properties().clone();
    negative_spacing.set(
        "spacing".to_string(),
        Property::constant(PropertyValue::Vec3(Vec3 {
            x: OrderedFloat(-24.0),
            y: OrderedFloat(0.0),
            z: OrderedFloat(24.0),
        })),
    );
    descriptor
        .validate_native_properties(&negative_spacing)
        .unwrap();

    let mut invalid_count = node.properties().clone();
    invalid_count.set(
        "count_x".to_string(),
        Property::constant(PropertyValue::Integer(0)),
    );
    assert!(
        descriptor
            .validate_native_properties(&invalid_count)
            .unwrap_err()
            .contains("cannot be less than 1")
    );

    let mut invalid_spacing = node.properties().clone();
    invalid_spacing.set(
        "spacing".to_string(),
        Property::constant(PropertyValue::Vec3(Vec3 {
            x: OrderedFloat(f64::NAN),
            y: OrderedFloat(-24.0),
            z: OrderedFloat(24.0),
        })),
    );
    assert!(
        descriptor
            .validate_native_properties(&invalid_spacing)
            .unwrap_err()
            .contains("must be finite")
    );

    let mut invalid_size = node.properties().clone();
    invalid_size.set(
        "size".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(0.0))),
    );
    assert!(
        descriptor
            .validate_native_properties(&invalid_size)
            .unwrap_err()
            .contains("cannot be less than")
    );
}
