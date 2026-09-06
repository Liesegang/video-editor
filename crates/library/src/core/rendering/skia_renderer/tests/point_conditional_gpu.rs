use super::point_attribute_gpu::{definition, one_point_grid, working_color};
use super::*;
use crate::model::ComparisonOperation;
use crate::model::conditional::{evaluate_comparison, evaluate_selection};
use crate::model::point::{
    PointAttributeElementType, PointAttributeGpuDefault, PointAttributeSchema, PointInstruction,
    PointRenderProgram,
};
use crate::model::project::PortDataType;
use crate::model::property::{ColorSpaceRef, ColorValue, PropertyValue, Vec2, Vec4};

fn transparent() -> Color {
    Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    }
}

fn store(instructions: &mut Vec<PointInstruction>, attribute: u16, value_register: u16) {
    instructions.push(PointInstruction::StoreAttribute {
        attribute,
        value: value_register,
    });
}

fn comparison_program(
    left: f64,
    right: f64,
) -> (PointRenderProgram, Vec<PointAttributeGpuDefault>) {
    let cases = ComparisonOperation::ALL;
    let schema = PointAttributeSchema::new(
        (0..=cases.len())
            .map(|index| definition(401 + index as u128, PointAttributeElementType::Boolean))
            .collect(),
    )
    .unwrap();
    let mut instructions = Vec::new();
    let mut expected = Vec::new();
    for (attribute, operation) in cases.into_iter().enumerate() {
        let left = PropertyValue::Number(left.into());
        let right = PropertyValue::Number(right.into());
        let left_register = instructions.len() as u16;
        instructions.push(PointInstruction::Constant {
            value: left.clone(),
        });
        let right_register = instructions.len() as u16;
        instructions.push(PointInstruction::Constant {
            value: right.clone(),
        });
        let result_register = instructions.len() as u16;
        instructions.push(PointInstruction::Compare {
            operation,
            left: left_register,
            right: right_register,
        });
        store(&mut instructions, attribute as u16, result_register);
        let value = evaluate_comparison(operation, &left, &right).unwrap();
        expected.push(
            PointAttributeElementType::Boolean
                .pack_value(&value)
                .unwrap(),
        );
    }
    let loaded = instructions.len() as u16;
    instructions.push(PointInstruction::LoadAttribute { attribute: 0 });
    store(&mut instructions, cases.len() as u16, loaded);
    expected.push(expected[0]);
    let color_register = instructions.len() as u16;
    instructions.push(PointInstruction::Constant {
        value: working_color(),
    });
    (
        PointRenderProgram {
            schema,
            instructions,
            ramps: Vec::new(),
            color_register,
        },
        expected,
    )
}

fn selection_cases() -> Vec<(PortDataType, PropertyValue, PropertyValue)> {
    vec![
        (
            PortDataType::Number,
            PropertyValue::Number(2.25.into()),
            PropertyValue::Number((-3.5).into()),
        ),
        (
            PortDataType::Integer,
            PropertyValue::Integer(i64::from(i32::MAX)),
            PropertyValue::Integer(i64::from(i32::MIN)),
        ),
        (
            PortDataType::Vec2,
            PropertyValue::Vec2(Vec2 {
                x: 1.0.into(),
                y: 2.0.into(),
            }),
            PropertyValue::Vec2(Vec2 {
                x: (-1.0).into(),
                y: (-2.0).into(),
            }),
        ),
        (
            PortDataType::Vec3,
            PropertyValue::Vec3(particle_vec3(3.0, 4.0, 5.0)),
            PropertyValue::Vec3(particle_vec3(-3.0, -4.0, -5.0)),
        ),
        (
            PortDataType::Vec4,
            PropertyValue::Vec4(Vec4 {
                x: 6.0.into(),
                y: 7.0.into(),
                z: 8.0.into(),
                w: 9.0.into(),
            }),
            PropertyValue::Vec4(Vec4 {
                x: (-6.0).into(),
                y: (-7.0).into(),
                z: (-8.0).into(),
                w: (-9.0).into(),
            }),
        ),
        (
            PortDataType::Color,
            working_color(),
            PropertyValue::ColorValue(
                ColorValue::new(ColorSpaceRef::linear_srgb(), [0.8, 0.2, 0.4, 0.6]).unwrap(),
            ),
        ),
        (
            PortDataType::Boolean,
            PropertyValue::Boolean(true),
            PropertyValue::Boolean(false),
        ),
    ]
}

fn selection_program(
    invert_conditions: bool,
) -> (PointRenderProgram, Vec<PointAttributeGpuDefault>) {
    let cases = selection_cases();
    let kinds = cases
        .iter()
        .map(|(_, when_true, _)| PointAttributeElementType::from_property_value(when_true).unwrap())
        .collect::<Vec<_>>();
    let mut definitions = kinds
        .iter()
        .enumerate()
        .map(|(index, kind)| definition(501 + index as u128, *kind))
        .collect::<Vec<_>>();
    definitions.push(definition(508, PointAttributeElementType::Boolean));
    let schema = PointAttributeSchema::new(definitions).unwrap();
    let mut instructions = Vec::new();
    let mut expected = Vec::new();
    for (attribute, (data_type, when_true, when_false)) in cases.into_iter().enumerate() {
        let condition = PropertyValue::Boolean((attribute % 2 == 0) ^ invert_conditions);
        let condition_register = instructions.len() as u16;
        instructions.push(PointInstruction::Constant {
            value: condition.clone(),
        });
        let true_register = instructions.len() as u16;
        instructions.push(PointInstruction::Constant {
            value: when_true.clone(),
        });
        let false_register = instructions.len() as u16;
        instructions.push(PointInstruction::Constant {
            value: when_false.clone(),
        });
        let selected_register = instructions.len() as u16;
        instructions.push(PointInstruction::Select {
            condition: condition_register,
            when_true: true_register,
            when_false: false_register,
            element_type: kinds[attribute],
        });
        store(&mut instructions, attribute as u16, selected_register);
        let selected = evaluate_selection(data_type, &condition, &when_true, &when_false).unwrap();
        expected.push(kinds[attribute].pack_value(&selected).unwrap());
    }
    let loaded_boolean = instructions.len() as u16;
    instructions.push(PointInstruction::LoadAttribute { attribute: 6 });
    store(&mut instructions, 7, loaded_boolean);
    expected.push(expected[6]);
    let color_register = instructions.len() as u16;
    instructions.push(PointInstruction::Constant {
        value: working_color(),
    });
    (
        PointRenderProgram {
            schema,
            instructions,
            ramps: Vec::new(),
            color_register,
        },
        expected,
    )
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_point_comparisons_match_shared_boundaries_and_boolean_bits_roundtrip() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let mut warmed_pipeline_count = None;
    for (left, right) in [(-3.0, 2.0), (2.0, 2.0), (3.0, -2.0), (-0.0, 0.0)] {
        let (program, expected) = comparison_program(left, right);
        let scene = one_point_grid(program);
        render_point_test_scene(&mut renderer, &scene).unwrap();
        let fields = renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .read_point_fields(&scene.invocation)
            .unwrap();
        assert_eq!(fields[0].attributes, expected);
        for (raw, expected) in fields[0].attribute_words.iter().zip(&expected) {
            let PointAttributeGpuDefault::Boolean(expected) = expected else {
                panic!("comparison fixture contains only Boolean attributes")
            };
            assert_eq!(*raw, [u32::from(*expected), 0, 0, 0]);
        }
        let pipeline_count = renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .compiled_point_pipeline_count();
        if let Some(warmed_pipeline_count) = warmed_pipeline_count {
            assert_eq!(
                pipeline_count, warmed_pipeline_count,
                "comparison constants must reuse the warmed shader"
            );
        } else {
            warmed_pipeline_count = Some(pipeline_count);
        }
    }
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_point_select_covers_every_type_cache_warmth_and_particle_grid_sources() {
    let (program, expected) = selection_program(false);
    let grid = one_point_grid(program);
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    render_point_test_scene(&mut renderer, &grid).unwrap();
    let fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&grid.invocation)
        .unwrap();
    assert_eq!(fields[0].attributes, expected);
    let pipeline_count = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .compiled_point_pipeline_count();

    let (changed_program, changed_expected) = selection_program(true);
    let changed_grid = one_point_grid(changed_program.clone());
    render_point_test_scene(&mut renderer, &changed_grid).unwrap();
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .compiled_point_pipeline_count(),
        pipeline_count,
        "condition constants must reuse the typed Select shader"
    );
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .read_point_fields(&changed_grid.invocation)
            .unwrap()[0]
            .attributes,
        changed_expected
    );

    let mut particle = particle_scene(180);
    particle.executable_hash = changed_grid.executable_hash;
    particle.point_program = Some(changed_program);
    render_point_test_scene(&mut renderer, &particle).unwrap();
    let particle_fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&particle.invocation)
        .unwrap();
    assert!(
        !particle_fields.is_empty(),
        "Particle Select proof requires at least one live point"
    );
    for point in particle_fields {
        assert_eq!(point.attributes, changed_expected);
    }
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_point_select_is_eager_and_does_not_hide_an_invalid_unselected_branch() {
    let scene = one_point_grid(PointRenderProgram {
        schema: PointAttributeSchema::new(vec![definition(601, PointAttributeElementType::Vec3)])
            .unwrap(),
        instructions: vec![
            PointInstruction::Constant {
                value: PropertyValue::Boolean(true),
            },
            PointInstruction::Constant {
                value: PropertyValue::Vec3(particle_vec3(1.0, 2.0, 3.0)),
            },
            PointInstruction::Constant {
                value: PropertyValue::Vec3(particle_vec3(4.0, 5.0, 6.0)),
            },
            PointInstruction::Constant {
                value: PropertyValue::Vec3(particle_vec3(1.0, 0.0, 2.0)),
            },
            PointInstruction::Binary {
                operation: crate::model::point::NumericBinaryOperation::Divide,
                left: 2,
                right: 3,
            },
            PointInstruction::Select {
                condition: 0,
                when_true: 1,
                when_false: 4,
                element_type: PointAttributeElementType::Vec3,
            },
            PointInstruction::StoreAttribute {
                attribute: 0,
                value: 5,
            },
            PointInstruction::Constant {
                value: working_color(),
            },
        ],
        ramps: Vec::new(),
        color_register: 7,
    });
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let pixels = render_point_test_scene(&mut renderer, &scene).unwrap();
    let fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&scene.invocation)
        .unwrap();
    assert_eq!(
        fields[0].attributes[0],
        PointAttributeGpuDefault::Vec3([0.0; 3])
    );
    assert_eq!(fields[0].color, [0.0; 4]);
    assert!(pixels.data.chunks_exact(4).all(|pixel| pixel[3] == 0));
}
