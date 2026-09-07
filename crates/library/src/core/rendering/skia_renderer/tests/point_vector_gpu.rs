use super::point_attribute_gpu::{one_point_grid, working_color};
use super::*;
use crate::model::numeric::{evaluate_numeric_binary, evaluate_numeric_length};
use crate::model::point::{
    NumericBinaryOperation, PointAttributeDefinition, PointAttributeElementType,
    PointAttributeGpuDefault, PointAttributeId, PointAttributeSchema, PointInstruction,
    PointRenderProgram,
};
use crate::model::property::{PropertyValue, Vec2, Vec4};

fn vector_value(dimension: usize, values: [f32; 4]) -> PropertyValue {
    match dimension {
        1 => PropertyValue::Number(f64::from(values[0]).into()),
        2 => PropertyValue::Vec2(Vec2 {
            x: f64::from(values[0]).into(),
            y: f64::from(values[1]).into(),
        }),
        3 => PropertyValue::Vec3(particle_vec3(
            f64::from(values[0]),
            f64::from(values[1]),
            f64::from(values[2]),
        )),
        4 => PropertyValue::Vec4(Vec4 {
            x: f64::from(values[0]).into(),
            y: f64::from(values[1]).into(),
            z: f64::from(values[2]).into(),
            w: f64::from(values[3]).into(),
        }),
        _ => panic!("test vector dimension must be in 1..=4"),
    }
}

fn result_program(
    result_kind: PointAttributeElementType,
    instructions: Vec<PointInstruction>,
    value_register: u16,
) -> PointRenderProgram {
    let mut instructions = instructions;
    let store_register = instructions.len() as u16;
    instructions.push(PointInstruction::StoreAttribute {
        attribute: 0,
        value: value_register,
    });
    let color_register = instructions.len() as u16;
    instructions.push(PointInstruction::Constant {
        value: working_color(),
    });
    debug_assert_eq!(store_register + 1, color_register);
    PointRenderProgram {
        schema: PointAttributeSchema::new(vec![
            PointAttributeDefinition::new(
                PointAttributeId::from_uuid(Uuid::from_u128(301)),
                "result",
                result_kind,
                result_kind.default_value(),
            )
            .unwrap(),
        ])
        .unwrap(),
        instructions,
        ramps: Vec::new(),
        color_register,
        position_register: None,
        size_register: None,
    }
}

fn binary_program(
    operation: NumericBinaryOperation,
    left: PropertyValue,
    right: PropertyValue,
    result_kind: PointAttributeElementType,
) -> PointRenderProgram {
    result_program(
        result_kind,
        vec![
            PointInstruction::Constant { value: left },
            PointInstruction::Constant { value: right },
            PointInstruction::Binary {
                operation,
                left: 0,
                right: 1,
            },
        ],
        2,
    )
}

fn components(value: PointAttributeGpuDefault) -> Vec<f32> {
    match value {
        PointAttributeGpuDefault::Number(value) => vec![value],
        PointAttributeGpuDefault::Vec2(value) => value.to_vec(),
        PointAttributeGpuDefault::Vec3(value) => value.to_vec(),
        PointAttributeGpuDefault::Vec4(value) => value.to_vec(),
        PointAttributeGpuDefault::Integer(_)
        | PointAttributeGpuDefault::Boolean(_)
        | PointAttributeGpuDefault::Color(_) => {
            panic!("numeric GPU test received a non-numeric field")
        }
    }
}

fn canonical_numeric_components(value: &PropertyValue) -> Vec<f32> {
    let kind = PointAttributeElementType::from_property_value(value).unwrap();
    components(kind.pack_value(value).unwrap())
}

fn assert_near(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        let tolerance = (expected.abs() * 2.0e-5).max(f32::from_bits(8));
        assert!(
            (actual - expected).abs() <= tolerance,
            "actual {actual}, expected {expected}, tolerance {tolerance}"
        );
    }
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_point_vector_arithmetic_and_length_match_shared_numeric_rules() {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    let operations = [
        NumericBinaryOperation::Add,
        NumericBinaryOperation::Subtract,
        NumericBinaryOperation::Multiply,
        NumericBinaryOperation::Divide,
        NumericBinaryOperation::Fmod,
    ];
    let left = [-5.5, 8.25, -9.75, 12.5];
    let right = [2.0, -3.0, 4.0, -5.0];
    for dimension in 1..=4 {
        for operation in operations {
            let directions = [(dimension, dimension), (dimension, 1), (1, dimension)];
            let directions = if dimension == 1 {
                &directions[..1]
            } else {
                &directions[..]
            };
            for &(left_dimension, right_dimension) in directions {
                let left_value = if left_dimension == 1 {
                    vector_value(1, [left[0], 0.0, 0.0, 0.0])
                } else {
                    vector_value(dimension, left)
                };
                let right_value = if right_dimension == 1 {
                    vector_value(1, [right[0], 0.0, 0.0, 0.0])
                } else {
                    vector_value(dimension, right)
                };
                let expected_value =
                    evaluate_numeric_binary(operation, &left_value, &right_value).unwrap();
                let result_kind =
                    PointAttributeElementType::from_property_value(&expected_value).unwrap();
                let expected = canonical_numeric_components(&expected_value);
                let scene = one_point_grid(binary_program(
                    operation,
                    left_value,
                    right_value,
                    result_kind,
                ));
                render_point_test_scene(&mut renderer, &scene).unwrap();
                let actual = renderer
                    .scene_runtime
                    .as_ref()
                    .unwrap()
                    .read_point_fields(&scene.invocation)
                    .unwrap();
                assert_near(&components(actual[0].attributes[0]), &expected);
            }
        }
    }

    for (dimension, input) in [
        (1, [0.0, 0.0, 0.0, 0.0]),
        (1, [f32::MIN_POSITIVE, 0.0, 0.0, 0.0]),
        (
            3,
            [
                f32::MIN_POSITIVE,
                -2.0 * f32::MIN_POSITIVE,
                f32::MIN_POSITIVE,
                0.0,
            ],
        ),
        (2, [1.0e30, -1.0e30, 0.0, 0.0]),
        (3, [3.0, 4.0, 12.0, 0.0]),
        (4, [1.0, -2.0, 2.0, -4.0]),
    ] {
        let scene = one_point_grid(result_program(
            PointAttributeElementType::Number,
            vec![
                PointInstruction::Constant {
                    value: vector_value(dimension, input),
                },
                PointInstruction::Length { value: 0 },
            ],
            1,
        ));
        render_point_test_scene(&mut renderer, &scene).unwrap();
        let fields = renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .read_point_fields(&scene.invocation)
            .unwrap();
        let expected = evaluate_numeric_length(&vector_value(dimension, input)).unwrap();
        let actual = components(fields[0].attributes[0]);
        let expected = canonical_numeric_components(&expected);
        if expected[0] != 0.0 {
            assert_ne!(actual[0], 0.0, "finite nonzero Length must not underflow");
        }
        assert_near(&actual, &expected);
    }

    let invalid = one_point_grid(binary_program(
        NumericBinaryOperation::Divide,
        vector_value(3, [3.0, 4.0, 5.0, 0.0]),
        vector_value(3, [1.0, 0.0, 2.0, 0.0]),
        PointAttributeElementType::Vec3,
    ));
    let invalid_pixels = render_point_test_scene(&mut renderer, &invalid).unwrap();
    let invalid_fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&invalid.invocation)
        .unwrap();
    assert_eq!(
        invalid_fields[0].attributes[0],
        PointAttributeGpuDefault::Vec3([0.0; 3])
    );
    assert_eq!(invalid_fields[0].color, [0.0; 4]);
    assert!(
        invalid_pixels
            .data
            .chunks_exact(4)
            .all(|pixel| pixel[3] == 0)
    );

    // Each authored operand is finite and f32-representable, but one multiply
    // lane overflows in the GPU domain. The whole point must fail atomically.
    let non_finite = one_point_grid(binary_program(
        NumericBinaryOperation::Multiply,
        vector_value(3, [f32::MAX, 2.0, 3.0, 0.0]),
        vector_value(3, [2.0, 4.0, 5.0, 0.0]),
        PointAttributeElementType::Vec3,
    ));
    let non_finite_pixels = render_point_test_scene(&mut renderer, &non_finite).unwrap();
    let non_finite_fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&non_finite.invocation)
        .unwrap();
    assert_eq!(
        non_finite_fields[0].attributes[0],
        PointAttributeGpuDefault::Vec3([0.0; 3])
    );
    assert_eq!(non_finite_fields[0].color, [0.0; 4]);
    assert!(
        non_finite_pixels
            .data
            .chunks_exact(4)
            .all(|pixel| pixel[3] == 0)
    );

    let warm_count = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .compiled_point_pipeline_count();
    let warm = one_point_grid(binary_program(
        NumericBinaryOperation::Add,
        vector_value(3, [20.0, 30.0, 40.0, 0.0]),
        vector_value(1, [2.0, 0.0, 0.0, 0.0]),
        PointAttributeElementType::Vec3,
    ));
    render_point_test_scene(&mut renderer, &warm).unwrap();
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .compiled_point_pipeline_count(),
        warm_count,
        "constant-only changes must reuse the vector field shader"
    );

    let warm_left = vector_value(1, [2.0, 0.0, 0.0, 0.0]);
    let warm_right = vector_value(3, [20.0, 30.0, 40.0, 0.0]);
    let warm_source_shape_expected =
        evaluate_numeric_binary(NumericBinaryOperation::Subtract, &warm_left, &warm_right).unwrap();
    let warm_source_shape = one_point_grid(binary_program(
        NumericBinaryOperation::Subtract,
        warm_left,
        warm_right,
        PointAttributeElementType::Vec3,
    ));
    render_point_test_scene(&mut renderer, &warm_source_shape).unwrap();
    let warm_source_shape_fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&warm_source_shape.invocation)
        .unwrap();
    assert_near(
        &components(warm_source_shape_fields[0].attributes[0]),
        &canonical_numeric_components(&warm_source_shape_expected),
    );

    let mut particle = particle_scene(180);
    particle.point_program = warm.point_program;
    render_point_test_scene(&mut renderer, &particle).unwrap();
    let particle_fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&particle.invocation)
        .unwrap();
    for point in particle_fields {
        assert_eq!(
            point.attributes[0],
            PointAttributeGpuDefault::Vec3([22.0, 32.0, 42.0])
        );
    }
}
