use super::*;
use crate::model::property::{GradientValue, PropertyValue, Vec2, Vec3, Vec4};
use ordered_float::OrderedFloat;

fn heat_program() -> PointRenderProgram {
    PointRenderProgram {
        schema: PointAttributeSchema::new(vec![
            PointAttributeDefinition::new(
                PointAttributeId::new(),
                "heat",
                PointAttributeElementType::Number,
                PropertyValue::Number(OrderedFloat(0.0)),
            )
            .unwrap(),
        ])
        .unwrap(),
        instructions: vec![
            PointInstruction::NormalizedAge,
            PointInstruction::StoreAttribute {
                attribute: 0,
                value: 0,
            },
            PointInstruction::LoadAttribute { attribute: 0 },
            PointInstruction::Constant {
                value: PropertyValue::Number(OrderedFloat(2.0)),
            },
            PointInstruction::Binary {
                operation: NumericBinaryOperation::Multiply,
                left: 2,
                right: 3,
            },
            PointInstruction::ColorRamp {
                gradient: 0,
                factor: 4,
            },
        ],
        ramps: vec![GradientValue::default()],
        color_register: 5,
        position_register: None,
        size_register: None,
    }
}

#[test]
fn render_stage_heat_program_is_typed_bounded_and_serializable_as_frame_command() {
    let program = heat_program();
    program.validate().unwrap();
    let decoded: PointRenderProgram =
        serde_json::from_str(&serde_json::to_string(&program).unwrap()).unwrap();
    assert_eq!(decoded, program);
    decoded.validate().unwrap();
}

#[test]
fn point_program_rejects_forward_registers_wrong_types_and_unwritten_attributes() {
    let mut program = heat_program();
    program.instructions[1] = PointInstruction::StoreAttribute {
        attribute: 0,
        value: 5,
    };
    assert!(program.validate().unwrap_err().contains("earlier"));
    program = heat_program();
    program.instructions[4] = PointInstruction::LoadAttribute { attribute: 1 };
    assert!(program.validate().unwrap_err().contains("earlier Store"));
    program = heat_program();
    program.color_register = 4;
    assert!(program.validate().unwrap_err().contains("requires Color"));
    program = heat_program();
    program.instructions[0] = PointInstruction::LoadAttribute { attribute: 0 };
    assert!(program.validate().unwrap_err().contains("earlier Store"));
}

#[test]
fn point_program_rejects_nonfinite_constants_and_out_of_range_resources() {
    let mut program = heat_program();
    program.instructions[3] = PointInstruction::Constant {
        value: PropertyValue::Number(OrderedFloat(f64::INFINITY)),
    };
    assert!(program.validate().is_err());
    program = heat_program();
    program.instructions[0] = PointInstruction::Random { channel: 3 };
    assert!(program.validate().unwrap_err().contains("channel"));
    program = heat_program();
    program.instructions[5] = PointInstruction::ColorRamp {
        gradient: 1,
        factor: 4,
    };
    assert!(program.validate().unwrap_err().contains("missing Gradient"));
    program = heat_program();
    program
        .instructions
        .resize(POINT_MAX_INSTRUCTIONS + 1, PointInstruction::Age);
    assert!(program.validate().unwrap_err().contains("instructions"));
}

const ATTRIBUTE_TYPES: [PointAttributeElementType; 7] = [
    PointAttributeElementType::Number,
    PointAttributeElementType::Integer,
    PointAttributeElementType::Boolean,
    PointAttributeElementType::Vec2,
    PointAttributeElementType::Vec3,
    PointAttributeElementType::Vec4,
    PointAttributeElementType::Color,
];

fn typed_program(kind: PointAttributeElementType, value: PropertyValue) -> PointRenderProgram {
    PointRenderProgram {
        schema: PointAttributeSchema::new(
            ["source", "captured"]
                .map(|name| {
                    PointAttributeDefinition::new(
                        PointAttributeId::new(),
                        name,
                        kind,
                        kind.default_value(),
                    )
                    .unwrap()
                })
                .to_vec(),
        )
        .unwrap(),
        instructions: vec![
            PointInstruction::Constant { value },
            PointInstruction::StoreAttribute {
                attribute: 0,
                value: 0,
            },
            PointInstruction::LoadAttribute { attribute: 0 },
            PointInstruction::StoreAttribute {
                attribute: 1,
                value: 2,
            },
            PointInstruction::LoadAttribute { attribute: 1 },
            PointInstruction::Constant {
                value: PointAttributeElementType::Color.default_value(),
            },
        ],
        ramps: Vec::new(),
        color_register: if kind == PointAttributeElementType::Color {
            4
        } else {
            5
        },
        position_register: None,
        size_register: None,
    }
}

#[test]
fn every_attribute_type_uses_one_typed_store_load_contract() {
    for kind in ATTRIBUTE_TYPES {
        let value = kind.default_value();
        assert_eq!(
            PointAttributeElementType::from_property_value(&value).unwrap(),
            kind
        );
        kind.pack_value(&value).unwrap();
        let program = typed_program(kind, value);
        program.validate().unwrap();
        let decoded: PointRenderProgram =
            serde_json::from_str(&serde_json::to_string(&program).unwrap()).unwrap();
        assert_eq!(decoded, program);
        decoded.validate().unwrap();

        for other_kind in ATTRIBUTE_TYPES {
            if other_kind == kind {
                continue;
            }
            let mut wrong_input = program.clone();
            wrong_input.instructions[0] = PointInstruction::Constant {
                value: other_kind.default_value(),
            };
            assert!(wrong_input.validate().unwrap_err().contains("requires"));

            let mut wrong_load = program.clone();
            wrong_load.schema = PointAttributeSchema::new(vec![
                program.schema.attributes()[0].clone(),
                PointAttributeDefinition::new(
                    program.schema.attributes()[1].id(),
                    "captured",
                    other_kind,
                    other_kind.default_value(),
                )
                .unwrap(),
            ])
            .unwrap();
            assert!(wrong_load.validate().unwrap_err().contains("requires"));
        }
    }
}

#[test]
fn typed_program_rejects_duplicate_missing_and_unwritten_stores() {
    for kind in ATTRIBUTE_TYPES {
        let program = typed_program(kind, kind.default_value());
        let mut duplicate = program.clone();
        duplicate.instructions[3] = PointInstruction::StoreAttribute {
            attribute: 0,
            value: 2,
        };
        assert!(
            duplicate
                .validate()
                .unwrap_err()
                .contains("multiple stores")
        );

        let mut missing = program.clone();
        missing.instructions[3] = PointInstruction::StoreAttribute {
            attribute: 2,
            value: 2,
        };
        assert!(
            missing
                .validate()
                .unwrap_err()
                .contains("missing attribute")
        );

        let mut unwritten = program.clone();
        unwritten.instructions[3] = PointInstruction::Constant {
            value: kind.default_value(),
        };
        unwritten.instructions[4] = PointInstruction::Constant {
            value: PointAttributeElementType::Color.default_value(),
        };
        assert!(
            unwritten
                .validate()
                .unwrap_err()
                .contains("without a Store")
        );
    }
}

#[test]
fn integer_attributes_preserve_exact_i32_constants_without_number_coercion() {
    for value in [i32::MIN, -16_777_217, 16_777_217, i32::MAX] {
        let constant = PropertyValue::Integer(i64::from(value));
        let program = typed_program(PointAttributeElementType::Integer, constant.clone());
        program.validate().unwrap();
        assert_eq!(
            PointAttributeElementType::Integer
                .pack_value(&constant)
                .unwrap(),
            PointAttributeGpuDefault::Integer(value)
        );
    }
    for value in [i64::from(i32::MIN) - 1, i64::from(i32::MAX) + 1] {
        let program = typed_program(
            PointAttributeElementType::Integer,
            PropertyValue::Integer(value),
        );
        assert!(program.validate().unwrap_err().contains("signed i32"));
    }
}

#[test]
fn vector_constants_validate_every_component_without_treating_vec4_as_color() {
    for bad in [f64::NAN, f64::INFINITY, f64::MAX, f64::MIN_POSITIVE] {
        let values = [
            PropertyValue::Vec2(Vec2 {
                x: 0.0.into(),
                y: bad.into(),
            }),
            PropertyValue::Vec3(Vec3 {
                x: 0.0.into(),
                y: 0.0.into(),
                z: bad.into(),
            }),
            PropertyValue::Vec4(Vec4 {
                x: 0.0.into(),
                y: 0.0.into(),
                z: 0.0.into(),
                w: bad.into(),
            }),
        ];
        for value in values {
            let kind = PointAttributeElementType::from_property_value(&value).unwrap();
            assert!(typed_program(kind, value).validate().is_err());
        }
    }
    let vector = PropertyValue::Vec4(Vec4 {
        x: (-3.0).into(),
        y: 2.0.into(),
        z: 12.0.into(),
        w: 42.0.into(),
    });
    typed_program(PointAttributeElementType::Vec4, vector)
        .validate()
        .unwrap();
}

#[test]
fn point_position_is_vec3_and_remains_typed_through_capture() {
    let mut program = typed_program(
        PointAttributeElementType::Vec3,
        PointAttributeElementType::Vec3.default_value(),
    );
    program.instructions[0] = PointInstruction::Position;
    program.validate().unwrap();
    program.color_register = 4;
    assert!(
        program
            .validate()
            .unwrap_err()
            .contains("requires Color, got Vec3")
    );
}

#[test]
fn point_constants_reject_untyped_strings_maps_and_encoded_colors() {
    for value in [
        PropertyValue::String("custom".into()),
        PropertyValue::Map(Default::default()),
        PropertyValue::Gradient(GradientValue::default()),
        PropertyValue::Color(crate::model::frame::color::Color::white()),
    ] {
        assert!(PointAttributeElementType::from_property_value(&value).is_err());
        assert!(
            typed_program(PointAttributeElementType::Number, value)
                .validate()
                .is_err()
        );
    }
}

#[test]
fn compare_returns_boolean_and_select_requires_exact_eager_branch_types() {
    let program = value_program(
        vec![
            PointInstruction::NormalizedAge,
            PointInstruction::Constant {
                value: PropertyValue::Number(0.5.into()),
            },
            PointInstruction::Compare {
                operation: crate::model::ComparisonOperation::Greater,
                left: 0,
                right: 1,
            },
            PointInstruction::Constant {
                value: PropertyValue::Vec3(Vec3 {
                    x: 1.0.into(),
                    y: 2.0.into(),
                    z: 3.0.into(),
                }),
            },
            PointInstruction::Constant {
                value: PropertyValue::Vec3(Vec3 {
                    x: 4.0.into(),
                    y: 5.0.into(),
                    z: 6.0.into(),
                }),
            },
            PointInstruction::Select {
                element_type: PointAttributeElementType::Vec3,
                condition: 2,
                when_true: 3,
                when_false: 4,
            },
            PointInstruction::Constant {
                value: PointAttributeElementType::Color.default_value(),
            },
        ],
        6,
    );
    assert_eq!(
        program.register_types().unwrap(),
        vec![
            PointAttributeElementType::Number,
            PointAttributeElementType::Number,
            PointAttributeElementType::Boolean,
            PointAttributeElementType::Vec3,
            PointAttributeElementType::Vec3,
            PointAttributeElementType::Vec3,
            PointAttributeElementType::Color,
        ]
    );

    let mut wrong_condition = program.clone();
    wrong_condition.instructions[5] = PointInstruction::Select {
        element_type: PointAttributeElementType::Vec3,
        condition: 0,
        when_true: 3,
        when_false: 4,
    };
    assert!(wrong_condition.validate().unwrap_err().contains("Boolean"));

    let mut wrong_branch = program;
    wrong_branch.instructions[4] = PointInstruction::Constant {
        value: PropertyValue::Vec2(Vec2 {
            x: 4.0.into(),
            y: 5.0.into(),
        }),
    };
    assert!(
        wrong_branch
            .validate()
            .unwrap_err()
            .contains("requires Vec3")
    );

    let wrong_declared_type = value_program(
        vec![
            PointInstruction::Constant {
                value: PropertyValue::Boolean(true),
            },
            PointInstruction::Constant {
                value: PropertyValue::Vec3(Vec3 {
                    x: 1.0.into(),
                    y: 2.0.into(),
                    z: 3.0.into(),
                }),
            },
            PointInstruction::Constant {
                value: PropertyValue::Vec3(Vec3 {
                    x: 4.0.into(),
                    y: 5.0.into(),
                    z: 6.0.into(),
                }),
            },
            PointInstruction::Select {
                element_type: PointAttributeElementType::Number,
                condition: 0,
                when_true: 1,
                when_false: 2,
            },
            PointInstruction::Constant {
                value: PointAttributeElementType::Color.default_value(),
            },
        ],
        4,
    );
    assert!(
        wrong_declared_type
            .validate()
            .unwrap_err()
            .contains("requires Number")
    );
}

fn value_program(instructions: Vec<PointInstruction>, color_register: u16) -> PointRenderProgram {
    PointRenderProgram {
        schema: PointAttributeSchema::new(Vec::new()).unwrap(),
        instructions,
        ramps: Vec::new(),
        color_register,
        position_register: None,
        size_register: None,
    }
}

#[test]
fn point_program_position_register_is_optional_and_requires_vec3() {
    let mut program = value_program(
        vec![
            PointInstruction::Position,
            PointInstruction::Constant {
                value: PointAttributeElementType::Color.default_value(),
            },
        ],
        1,
    );
    program.position_register = Some(0);
    program.validate().unwrap();

    program.position_register = Some(1);
    assert!(program.validate().unwrap_err().contains("requires Vec3"));
}

#[test]
fn point_program_size_register_is_optional_and_requires_number() {
    let mut program = value_program(
        vec![
            PointInstruction::Size,
            PointInstruction::Constant {
                value: PointAttributeElementType::Color.default_value(),
            },
        ],
        1,
    );
    assert!(!program.has_geometry_output());
    program.size_register = Some(0);
    assert!(program.has_geometry_output());
    program.validate().unwrap();
    let decoded: PointRenderProgram =
        serde_json::from_str(&serde_json::to_string(&program).unwrap()).unwrap();
    assert_eq!(decoded, program);

    program.size_register = Some(1);
    assert!(program.validate().unwrap_err().contains("requires Number"));

    for size in [0.0, -1.0] {
        let mut derived = value_program(
            vec![
                PointInstruction::Constant {
                    value: PropertyValue::Number(size.into()),
                },
                PointInstruction::Constant {
                    value: PointAttributeElementType::Color.default_value(),
                },
            ],
            1,
        );
        derived.size_register = Some(0);
        derived.validate().unwrap();
    }
}

#[test]
fn point_binary_uses_shared_vector_shape_and_scalar_broadcast_rules() {
    let program = value_program(
        vec![
            PointInstruction::Constant {
                value: PropertyValue::Vec3(Vec3 {
                    x: 1.0.into(),
                    y: 2.0.into(),
                    z: 3.0.into(),
                }),
            },
            PointInstruction::Constant {
                value: PropertyValue::Number(2.0.into()),
            },
            PointInstruction::Binary {
                operation: NumericBinaryOperation::Multiply,
                left: 0,
                right: 1,
            },
            PointInstruction::Constant {
                value: PointAttributeElementType::Color.default_value(),
            },
        ],
        3,
    );
    assert_eq!(
        program.register_types().unwrap(),
        vec![
            PointAttributeElementType::Vec3,
            PointAttributeElementType::Number,
            PointAttributeElementType::Vec3,
            PointAttributeElementType::Color,
        ]
    );

    let mut mismatched = program.clone();
    mismatched.instructions[1] = PointInstruction::Constant {
        value: PropertyValue::Vec2(Vec2 {
            x: 1.0.into(),
            y: 2.0.into(),
        }),
    };
    assert!(mismatched.validate().unwrap_err().contains("incompatible"));

    let mut integer = program;
    integer.instructions[1] = PointInstruction::Constant {
        value: PropertyValue::Integer(2),
    };
    assert!(
        integer
            .validate()
            .unwrap_err()
            .contains("does not accept Integer")
    );
}

#[test]
fn point_length_accepts_numeric_shapes_and_returns_number() {
    let program = value_program(
        vec![
            PointInstruction::Position,
            PointInstruction::Length { value: 0 },
            PointInstruction::Constant {
                value: PointAttributeElementType::Color.default_value(),
            },
        ],
        2,
    );
    assert_eq!(
        program.register_types().unwrap(),
        vec![
            PointAttributeElementType::Vec3,
            PointAttributeElementType::Number,
            PointAttributeElementType::Color,
        ]
    );
    let decoded: PointRenderProgram =
        serde_json::from_str(&serde_json::to_string(&program).unwrap()).unwrap();
    assert_eq!(decoded, program);

    let mut wrong = program;
    wrong.instructions[0] = PointInstruction::Constant {
        value: PointAttributeElementType::Color.default_value(),
    };
    assert!(
        wrong
            .validate()
            .unwrap_err()
            .contains("does not accept Color")
    );
}
