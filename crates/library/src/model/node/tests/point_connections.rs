use super::super::*;
use crate::model::authoring::PublishedParameterAutomationCapability;
use crate::model::frame::color::Color;
use crate::model::frame::point::{
    POINT_CONNECTION_MAX_DISTANCE, POINT_CONNECTION_MAX_NEIGHBORS, POINT_LINE_MAX_WIDTH,
};
use crate::model::project::IMAGE_OUTPUT_PORT;

#[test]
fn connect_points_catalog_has_one_transient_topology_output_and_bounded_controls() {
    let role = PointConnectionNodeRole::ConnectPoints;
    assert_eq!(role.catalog_id(), CONNECT_POINTS_CATALOG_ID);
    assert_eq!(
        PointConnectionNodeRole::from_catalog_id(CONNECT_POINTS_CATALOG_ID),
        Some(role)
    );
    assert!(PointConnectionNodeRole::from_catalog_id("native.point.connect-lines").is_none());

    let descriptor = native_node_descriptor(role.catalog_id()).expect("Connect Points descriptor");
    assert_eq!(descriptor.label(), "Connect Points");
    assert_eq!(descriptor.category(), "Points");
    assert_eq!(
        descriptor.qa_id(),
        "node_editor.menu.create.point_connect_points"
    );
    assert_eq!(descriptor.factory(), NativeNodeFactory::NativeOperation);
    assert!(descriptor.supports_general_module_creation());
    assert_eq!(
        descriptor
            .ports()
            .iter()
            .map(|port| (port.direction, port.key.as_str(), port.data_type))
            .collect::<Vec<_>>(),
        [
            (
                PortDirection::Input,
                POINT_SOURCE_PORT,
                PortDataType::PointSource
            ),
            (
                PortDirection::Input,
                POINT_MIN_DISTANCE_INPUT_PORT,
                PortDataType::Number,
            ),
            (
                PortDirection::Input,
                POINT_MAX_DISTANCE_INPUT_PORT,
                PortDataType::Number,
            ),
            (
                PortDirection::Input,
                POINT_MAX_NEIGHBORS_INPUT_PORT,
                PortDataType::Integer,
            ),
            (
                PortDirection::Output,
                POINT_CONNECTIONS_PORT,
                PortDataType::PointConnections,
            ),
        ]
    );
    let node = Node::new_catalog_node(role.catalog_id()).expect("Connect Points node");
    assert!(!node.supports_bypass());
    for (key, expected) in [
        (
            POINT_MIN_DISTANCE_INPUT_PORT,
            PropertyValue::Number(OrderedFloat(0.0)),
        ),
        (
            POINT_MAX_DISTANCE_INPUT_PORT,
            PropertyValue::Number(OrderedFloat(80.0)),
        ),
        (POINT_MAX_NEIGHBORS_INPUT_PORT, PropertyValue::Integer(6)),
    ] {
        assert_eq!(
            node.properties().get(key).unwrap().get_static_value(),
            Some(&expected),
            "{key}"
        );
        assert_eq!(
            descriptor.input_automation_capability(key),
            PublishedParameterAutomationCapability::FrameSampled,
            "{key}"
        );
    }

    let definitions = descriptor.property_definitions();
    let definition = |key| {
        definitions
            .iter()
            .find(|definition| definition.name() == key)
            .unwrap()
    };
    for key in [POINT_MIN_DISTANCE_INPUT_PORT, POINT_MAX_DISTANCE_INPUT_PORT] {
        assert!(matches!(
            definition(key).ui_type(),
            PropertyUiType::Float {
                min: 0.0,
                max,
                min_hard_limit: true,
                max_hard_limit: true,
                ..
            } if *max == f64::from(POINT_CONNECTION_MAX_DISTANCE)
        ));
    }
    assert_eq!(
        definition(POINT_MIN_DISTANCE_INPUT_PORT).label(),
        "Min Distance"
    );
    assert_eq!(
        definition(POINT_MAX_DISTANCE_INPUT_PORT).label(),
        "Max Distance"
    );
    let port_label = |key| {
        descriptor
            .ports()
            .iter()
            .find(|port| port.key == key)
            .map(|port| port.label.as_str())
            .unwrap()
    };
    assert_eq!(port_label(POINT_MIN_DISTANCE_INPUT_PORT), "Min Distance");
    assert_eq!(port_label(POINT_MAX_DISTANCE_INPUT_PORT), "Max Distance");
    assert!(matches!(
        definition(POINT_MAX_NEIGHBORS_INPUT_PORT).ui_type(),
        PropertyUiType::Integer {
            min: 1,
            max,
            min_hard_limit: true,
            max_hard_limit: true,
            ..
        } if *max == i64::from(POINT_CONNECTION_MAX_NEIGHBORS)
    ));
    descriptor
        .validate_native_properties(node.properties())
        .unwrap();
}

#[test]
fn line_renderer_catalog_consumes_connections_and_exposes_frame_sampled_style() {
    let role = PointConnectionNodeRole::LineRenderer;
    assert_eq!(role.catalog_id(), POINT_LINE_RENDERER_CATALOG_ID);
    assert_eq!(
        PointConnectionNodeRole::from_catalog_id(POINT_LINE_RENDERER_CATALOG_ID),
        Some(role)
    );
    let descriptor = native_node_descriptor(role.catalog_id()).expect("Line Renderer descriptor");
    assert_eq!(descriptor.label(), "Line Renderer");
    assert_eq!(descriptor.category(), "Points");
    assert_eq!(
        descriptor.qa_id(),
        "node_editor.menu.create.point_line_renderer"
    );
    assert_eq!(descriptor.factory(), NativeNodeFactory::NativeOperation);
    assert!(descriptor.supports_general_module_creation());
    assert_eq!(
        descriptor
            .ports()
            .iter()
            .map(|port| (port.direction, port.key.as_str(), port.data_type))
            .collect::<Vec<_>>(),
        [
            (
                PortDirection::Input,
                POINT_CONNECTIONS_PORT,
                PortDataType::PointConnections,
            ),
            (
                PortDirection::Input,
                POINT_COLOR_INPUT_PORT,
                PortDataType::Color
            ),
            (
                PortDirection::Input,
                POINT_LINE_WIDTH_INPUT_PORT,
                PortDataType::Number,
            ),
            (
                PortDirection::Input,
                POINT_LINE_FADE_INPUT_PORT,
                PortDataType::Number,
            ),
            (
                PortDirection::Output,
                IMAGE_OUTPUT_PORT,
                PortDataType::Image
            ),
        ]
    );
    let node = Node::new_catalog_node(role.catalog_id()).expect("Line Renderer node");
    assert!(!node.supports_bypass());
    for (key, expected) in [
        (POINT_COLOR_INPUT_PORT, PropertyValue::Color(Color::white())),
        (
            POINT_LINE_WIDTH_INPUT_PORT,
            PropertyValue::Number(OrderedFloat(2.0)),
        ),
        (
            POINT_LINE_FADE_INPUT_PORT,
            PropertyValue::Number(OrderedFloat(0.0)),
        ),
    ] {
        assert_eq!(
            node.properties().get(key).unwrap().get_static_value(),
            Some(&expected),
            "{key}"
        );
        assert_eq!(
            descriptor.input_automation_capability(key),
            PublishedParameterAutomationCapability::FrameSampled,
            "{key}"
        );
    }

    let definitions = descriptor.property_definitions();
    let definition = |key| {
        definitions
            .iter()
            .find(|definition| definition.name() == key)
            .unwrap()
    };
    assert!(matches!(
        definition(POINT_COLOR_INPUT_PORT).ui_type(),
        PropertyUiType::Color
    ));
    assert!(matches!(
        definition(POINT_LINE_WIDTH_INPUT_PORT).ui_type(),
        PropertyUiType::Float {
            min: 0.0,
            max,
            step: 0.1,
            min_hard_limit: true,
            max_hard_limit: true,
            ..
        } if *max == f64::from(POINT_LINE_MAX_WIDTH)
    ));
    assert!(matches!(
        definition(POINT_LINE_FADE_INPUT_PORT).ui_type(),
        PropertyUiType::Float {
            min: 0.0,
            max: 1.0,
            step: 0.01,
            min_hard_limit: true,
            max_hard_limit: true,
            ..
        }
    ));
    descriptor
        .validate_native_properties(node.properties())
        .unwrap();
}
