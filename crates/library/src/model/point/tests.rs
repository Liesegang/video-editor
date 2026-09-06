use ordered_float::OrderedFloat;
use uuid::Uuid;

use super::*;
use crate::model::project::PortDataType;
use crate::model::property::{ColorSpaceRef, ColorValue, PropertyValue, Vec2, Vec3, Vec4};

fn definition(
    id: u128,
    name: &str,
    element_type: PointAttributeElementType,
    default_value: PropertyValue,
) -> PointAttributeDefinition {
    PointAttributeDefinition::new(
        PointAttributeId::from_uuid(Uuid::from_u128(id)),
        name,
        element_type,
        default_value,
    )
    .unwrap()
}

#[test]
fn authored_schema_round_trips_without_runtime_layout() {
    let schema = PointAttributeSchema::new(vec![definition(
        1,
        "heat",
        PointAttributeElementType::Number,
        PropertyValue::Number(OrderedFloat(0.25)),
    )])
    .unwrap();
    let json = serde_json::to_value(&schema).unwrap();
    assert!(json.is_array());
    assert!(!json.to_string().contains("offset_bytes"));
    assert!(!json.to_string().contains("byte_len"));
    assert_eq!(
        serde_json::from_value::<PointAttributeSchema>(json).unwrap(),
        schema
    );
}

#[test]
fn deserialization_rejects_an_invalid_authored_default() {
    let schema = PointAttributeSchema::new(vec![definition(
        1,
        "count",
        PointAttributeElementType::Integer,
        PropertyValue::Integer(2),
    )])
    .unwrap();
    let mut json = serde_json::to_value(schema).unwrap();
    json[0]["default_value"] = serde_json::json!(2.5);
    assert!(serde_json::from_value::<PointAttributeSchema>(json).is_err());
}

#[test]
fn rename_preserves_identity_and_rejects_duplicate_display_names() {
    let first_id = PointAttributeId::from_uuid(Uuid::from_u128(1));
    let mut schema = PointAttributeSchema::new(vec![
        definition(
            1,
            "heat",
            PointAttributeElementType::Number,
            PropertyValue::Number(OrderedFloat(0.0)),
        ),
        definition(
            2,
            "mass",
            PointAttributeElementType::Number,
            PropertyValue::Number(OrderedFloat(1.0)),
        ),
    ])
    .unwrap();
    schema.rename_attribute(first_id, "temperature").unwrap();
    assert_eq!(schema.attributes()[0].id(), first_id);
    assert_eq!(schema.attributes()[0].display_name(), "temperature");
    assert!(schema.rename_attribute(first_id, "mass").is_err());
    assert_eq!(schema.attributes()[0].display_name(), "temperature");
}

#[test]
fn schema_rejects_duplicate_ids_names_and_excess_attributes() {
    let number = || PropertyValue::Number(OrderedFloat(0.0));
    let duplicate_id = PointAttributeSchema::new(vec![
        definition(1, "a", PointAttributeElementType::Number, number()),
        definition(1, "b", PointAttributeElementType::Number, number()),
    ]);
    assert!(duplicate_id.unwrap_err().contains("duplicate attribute ID"));

    let duplicate_name = PointAttributeSchema::new(vec![
        definition(1, "same", PointAttributeElementType::Number, number()),
        definition(2, "same", PointAttributeElementType::Number, number()),
    ]);
    assert!(
        duplicate_name
            .unwrap_err()
            .contains("duplicate display name")
    );

    let too_many = (0..=POINT_MAX_ATTRIBUTE_COUNT)
        .map(|index| {
            definition(
                index as u128 + 1,
                &format!("a{index}"),
                PointAttributeElementType::Number,
                number(),
            )
        })
        .collect();
    assert!(
        PointAttributeSchema::new(too_many)
            .unwrap_err()
            .contains("exceeding the maximum")
    );
}

#[test]
fn typed_defaults_enforce_exact_integer_and_finite_gpu_values() {
    assert!(
        PointAttributeDefinition::new(
            PointAttributeId::new(),
            "wrong",
            PointAttributeElementType::Number,
            PropertyValue::Integer(1),
        )
        .is_err()
    );
    assert!(
        PointAttributeDefinition::new(
            PointAttributeId::new(),
            "underflow",
            PointAttributeElementType::Number,
            PropertyValue::Number(OrderedFloat(f64::MIN_POSITIVE)),
        )
        .unwrap_err()
        .contains("underflow")
    );
    assert!(
        PointAttributeDefinition::new(
            PointAttributeId::new(),
            "large integer",
            PointAttributeElementType::Integer,
            PropertyValue::Integer(i64::from(i32::MAX) + 1),
        )
        .is_err()
    );
    assert!(
        PointAttributeDefinition::new(
            PointAttributeId::new(),
            "large number",
            PointAttributeElementType::Number,
            PropertyValue::Number(OrderedFloat(f64::MAX)),
        )
        .is_err()
    );
    assert!(
        PointAttributeDefinition::new(
            PointAttributeId::new(),
            "bad vector",
            PointAttributeElementType::Vec2,
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(f64::NAN),
                y: OrderedFloat(0.0),
            }),
        )
        .is_err()
    );
}

#[test]
fn boolean_schema_and_port_mapping_use_one_exact_column_contract() {
    let schema = PointAttributeSchema::new(vec![definition(
        1,
        "mask",
        PointAttributeElementType::Boolean,
        PropertyValue::Boolean(true),
    )])
    .unwrap();
    let layout = PointColumnLayout::derive(&schema, 3).unwrap();
    assert_eq!(layout.attributes[0].stride_bytes, 4);
    assert_eq!(
        layout.attributes[0].default_value,
        PointAttributeGpuDefault::Boolean(true)
    );
    assert_eq!(
        PointAttributeElementType::from_port_data_type(PortDataType::Boolean),
        Ok(PointAttributeElementType::Boolean)
    );
    assert!(PointAttributeElementType::from_port_data_type(PortDataType::Numeric).is_err());
    assert!(
        PointAttributeDefinition::new(
            PointAttributeId::new(),
            "mask",
            PointAttributeElementType::Boolean,
            PropertyValue::Integer(1),
        )
        .is_err()
    );
}

#[test]
fn authored_color_space_survives_without_backend_but_layout_fails_closed() {
    let color = ColorValue::new(
        ColorSpaceRef::new("future-wide-gamut").unwrap(),
        [0.2, 0.4, 0.6, 1.0],
    )
    .unwrap();
    let schema = PointAttributeSchema::new(vec![definition(
        1,
        "color",
        PointAttributeElementType::Color,
        PropertyValue::ColorValue(color),
    )])
    .unwrap();
    let json = serde_json::to_value(&schema).unwrap();
    let restored = serde_json::from_value::<PointAttributeSchema>(json).unwrap();
    assert_eq!(restored, schema);
    assert!(PointColumnLayout::derive(&restored, 1).is_err());
}

#[test]
fn layout_is_deterministic_aligned_and_includes_exact_serial_column() {
    let linear = ColorValue::new(ColorSpaceRef::linear_srgb(), [2.0, -0.25, 0.5, 0.75]).unwrap();
    let schema = PointAttributeSchema::new(vec![
        definition(
            1,
            "weight",
            PointAttributeElementType::Number,
            PropertyValue::Number(OrderedFloat(0.5)),
        ),
        definition(
            2,
            "position offset",
            PointAttributeElementType::Vec3,
            PropertyValue::Vec3(Vec3 {
                x: OrderedFloat(1.0),
                y: OrderedFloat(2.0),
                z: OrderedFloat(3.0),
            }),
        ),
        definition(
            3,
            "color",
            PointAttributeElementType::Color,
            PropertyValue::ColorValue(linear),
        ),
    ])
    .unwrap();
    let first = PointColumnLayout::derive(&schema, 3).unwrap();
    let second = PointColumnLayout::derive(&schema, 3).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.serial_offset_bytes, 0);
    assert_eq!(first.serial_stride_bytes, 4);
    assert_eq!(first.attributes[0].offset_bytes, 12);
    assert_eq!(first.attributes[0].stride_bytes, 4);
    assert_eq!(first.attributes[1].offset_bytes, 32);
    assert_eq!(first.attributes[1].stride_bytes, 16);
    assert_eq!(first.attributes[2].offset_bytes, 80);
    assert_eq!(first.attributes[2].stride_bytes, 16);
    assert_eq!(first.byte_len, 128);
}

#[test]
fn color_default_uses_existing_working_linear_boundary() {
    let encoded = ColorValue::new(ColorSpaceRef::srgb(), [0.5, 0.25, 0.75, 0.4]).unwrap();
    let attribute = definition(
        1,
        "color",
        PointAttributeElementType::Color,
        PropertyValue::ColorValue(encoded.clone()),
    );
    let schema = PointAttributeSchema::new(vec![attribute]).unwrap();
    let layout = PointColumnLayout::derive(&schema, 1).unwrap();
    let expected = crate::color_management::to_working_linear_srgb(&encoded)
        .unwrap()
        .rgba()
        .map(|component| component as f32);
    assert_eq!(
        layout.attributes[0].default_value,
        PointAttributeGpuDefault::Color(expected)
    );
}

#[test]
fn layout_rejects_capacity_bounds_and_checked_total_byte_limit() {
    let empty = PointAttributeSchema::new(Vec::new()).unwrap();
    assert!(PointColumnLayout::derive(&empty, 0).is_err());
    assert!(PointColumnLayout::derive(&empty, POINT_MAX_CAPACITY + 1).is_err());

    let attributes = (0..POINT_MAX_ATTRIBUTE_COUNT)
        .map(|index| {
            definition(
                index as u128 + 1,
                &format!("v{index}"),
                PointAttributeElementType::Vec4,
                PropertyValue::Vec4(Vec4 {
                    x: OrderedFloat(0.0),
                    y: OrderedFloat(0.0),
                    z: OrderedFloat(0.0),
                    w: OrderedFloat(0.0),
                }),
            )
        })
        .collect();
    let schema = PointAttributeSchema::new(attributes).unwrap();
    let error = PointColumnLayout::derive(&schema, POINT_MAX_CAPACITY).unwrap_err();
    assert!(error.contains("byte limit"));
}

#[test]
fn point_identity_keeps_producer_and_exact_u32_serial_separate() {
    let producer = PointProducerId::from_uuid(Uuid::from_u128(7));
    let point = PointId::new(producer, u32::MAX);
    assert_eq!(point.producer(), producer);
    assert_eq!(point.serial(), u32::MAX);
    assert_ne!(PointId::new(producer, 0), point);
}
