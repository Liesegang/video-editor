use super::point_support::test_gradient;
use super::*;
use crate::model::frame::point::PointGridParameters;
use crate::model::point::{
    PointAttributeDefinition, PointAttributeElementType, PointAttributeGpuDefault,
    PointAttributeId, PointAttributeSchema, PointInstruction, PointRenderProgram,
};
use crate::model::property::{
    ColorSpaceRef, ColorValue, GradientSpread, PropertyValue, Vec2, Vec4,
};

const INTEGER_CASES: [i32; 6] = [1, -1, i32::MAX, i32::MIN, 16_777_217, -16_777_217];

pub(super) fn definition(index: u128, kind: PointAttributeElementType) -> PointAttributeDefinition {
    PointAttributeDefinition::new(
        PointAttributeId::from_uuid(Uuid::from_u128(index)),
        format!("attribute-{index}"),
        kind,
        kind.default_value(),
    )
    .unwrap()
}

fn store_constant(
    instructions: &mut Vec<PointInstruction>,
    expected: &mut Vec<PointAttributeGpuDefault>,
    attribute: u16,
    value: PropertyValue,
) {
    let value_register = instructions.len() as u16;
    let kind = PointAttributeElementType::from_property_value(&value).unwrap();
    expected.push(kind.pack_value(&value).unwrap());
    instructions.push(PointInstruction::Constant { value });
    instructions.push(PointInstruction::StoreAttribute {
        attribute,
        value: value_register,
    });
}

fn typed_program() -> (PointRenderProgram, Vec<PointAttributeGpuDefault>) {
    let kinds = [
        PointAttributeElementType::Number,
        PointAttributeElementType::Integer,
        PointAttributeElementType::Integer,
        PointAttributeElementType::Integer,
        PointAttributeElementType::Integer,
        PointAttributeElementType::Integer,
        PointAttributeElementType::Integer,
        PointAttributeElementType::Vec2,
        PointAttributeElementType::Vec3,
        PointAttributeElementType::Vec3,
        PointAttributeElementType::Vec3,
        PointAttributeElementType::Vec4,
        PointAttributeElementType::Color,
        PointAttributeElementType::Color,
    ];
    let schema = PointAttributeSchema::new(
        kinds
            .into_iter()
            .enumerate()
            .map(|(index, kind)| definition(100 + index as u128, kind))
            .collect(),
    )
    .unwrap();
    let number = PropertyValue::Number(0.375.into());
    let vec2 = PropertyValue::Vec2(Vec2 {
        x: (-2.25).into(),
        y: 7.5.into(),
    });
    let vec3 = PropertyValue::Vec3(particle_vec3(11.0, -13.5, 17.25));
    let vec4 = PropertyValue::Vec4(Vec4 {
        x: (-1.0).into(),
        y: 0.25.into(),
        z: 2.5.into(),
        w: 9.0.into(),
    });
    let color = PropertyValue::ColorValue(
        ColorValue::new(ColorSpaceRef::linear_srgb(), [0.125, 0.5, 1.25, 0.75]).unwrap(),
    );
    let mut instructions = Vec::new();
    let mut expected = Vec::new();
    store_constant(&mut instructions, &mut expected, 0, number);
    for (offset, value) in INTEGER_CASES.into_iter().enumerate() {
        store_constant(
            &mut instructions,
            &mut expected,
            1 + offset as u16,
            PropertyValue::Integer(i64::from(value)),
        );
    }
    store_constant(&mut instructions, &mut expected, 7, vec2);
    store_constant(&mut instructions, &mut expected, 8, vec3);

    let position = instructions.len() as u16;
    instructions.push(PointInstruction::Position);
    instructions.push(PointInstruction::StoreAttribute {
        attribute: 9,
        value: position,
    });
    let loaded_position = instructions.len() as u16;
    instructions.push(PointInstruction::LoadAttribute { attribute: 9 });
    instructions.push(PointInstruction::StoreAttribute {
        attribute: 10,
        value: loaded_position,
    });
    store_constant(&mut instructions, &mut expected, 11, vec4);
    store_constant(&mut instructions, &mut expected, 12, color);

    let random = instructions.len() as u16;
    instructions.push(PointInstruction::Random { channel: 0 });
    let varied_color = instructions.len() as u16;
    instructions.push(PointInstruction::ColorRamp {
        gradient: 0,
        factor: random,
    });
    let color_register = instructions.len() as u16;
    instructions.push(PointInstruction::StoreAttribute {
        attribute: 13,
        value: varied_color,
    });
    (
        PointRenderProgram {
            schema,
            instructions,
            ramps: vec![test_gradient(
                GradientSpread::Pad,
                &[(0.0, Color::black()), (1.0, Color::white())],
            )],
            color_register,
            position_register: None,
            size_register: None,
            sprite_selection_register: None,
        },
        expected,
    )
}

fn assert_typed_fields(
    points: &[crate::rendering::scene_runtime::PointFieldReadback],
    expected: &[PointAttributeGpuDefault],
) {
    assert!(!points.is_empty());
    let mut colors = std::collections::HashSet::new();
    for point in points {
        assert_eq!(&point.attributes[..9], &expected[..9]);
        assert_eq!(point.attributes[9], point.attributes[10]);
        assert_eq!(&point.attributes[11..13], &expected[9..]);
        assert_eq!(
            point.attributes[13],
            PointAttributeGpuDefault::Color(point.color)
        );
        colors.insert(point.color.map(f32::to_bits));
    }
    assert!(
        colors.len() > 1,
        "Random→Color Ramp must vary Sprite colors"
    );
}

pub(super) fn working_color() -> PropertyValue {
    PropertyValue::ColorValue(
        ColorValue::new(ColorSpaceRef::linear_srgb(), [0.125, 0.5, 1.25, 0.75]).unwrap(),
    )
}

fn copy_program(kind: PointAttributeElementType, value: PropertyValue) -> PointRenderProgram {
    let mut instructions = vec![
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
    ];
    let color_register = if kind == PointAttributeElementType::Color {
        3
    } else {
        let register = instructions.len() as u16;
        instructions.push(PointInstruction::Constant {
            value: working_color(),
        });
        register
    };
    PointRenderProgram {
        schema: PointAttributeSchema::new(vec![definition(201, kind), definition(202, kind)])
            .unwrap(),
        instructions,
        ramps: Vec::new(),
        color_register,
        position_register: None,
        size_register: None,
        sprite_selection_register: None,
    }
}

pub(super) fn one_point_grid(program: PointRenderProgram) -> PointSceneFrame {
    let mut scene = particle_scene(0);
    scene.source = PointSceneSource::Grid(PointGridParameters {
        counts: [1, 1, 1],
        spacing: particle_vec3(1.0, 1.0, 1.0),
        center: particle_vec3(0.0, 0.0, 0.0),
        size: 8.0.into(),
        seed: 101,
    });
    scene.point_program = Some(program);
    scene
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_point_attributes_preserve_all_types_integer_bits_and_position_on_grid_and_particle() {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let (program, expected) = typed_program();
    program.validate().unwrap();
    let mut renderer = SkiaRenderer::new(256, 144, transparent.clone(), true, None, None).unwrap();
    assert!(
        renderer.gpu_context.is_some(),
        "Point field QA requires a real GPU"
    );

    let mut grid = particle_scene(0);
    grid.source = PointSceneSource::Grid(PointGridParameters {
        counts: [3, 2, 1],
        spacing: particle_vec3(12.0, 20.0, 1.0),
        center: particle_vec3(3.0, -4.0, 0.0),
        size: 8.0.into(),
        seed: 97,
    });
    grid.point_program = Some(program.clone());
    let grid_pixels = render_point_test_scene(&mut renderer, &grid).unwrap();
    let grid_fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&grid.invocation)
        .unwrap();
    assert_typed_fields(&grid_fields, &expected);
    for (slot, point) in grid_fields.iter().enumerate() {
        let x = slot % 3;
        let y = slot / 3;
        assert_eq!(
            point.attributes[9],
            PointAttributeGpuDefault::Vec3([-9.0 + x as f32 * 12.0, -14.0 + y as f32 * 20.0, 0.0,])
        );
    }
    renderer
        .resize_render_target(320, 180, transparent.clone())
        .unwrap();
    render_point_test_scene(&mut renderer, &grid).unwrap();
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .read_point_fields(&grid.invocation)
            .unwrap(),
        grid_fields,
        "render-target resize must not alter typed Point columns"
    );
    renderer
        .resize_render_target(256, 144, transparent.clone())
        .unwrap();
    assert_eq!(
        render_point_test_scene(&mut renderer, &grid).unwrap().data,
        grid_pixels.data
    );

    let mut particle = particle_scene(180);
    particle.point_program = Some(program);
    let particle_pixels = render_point_test_scene(&mut renderer, &particle).unwrap();
    let particle_fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&particle.invocation)
        .unwrap();
    assert_typed_fields(&particle_fields, &expected);
    let mut cold = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    assert_eq!(
        render_point_test_scene(&mut cold, &particle).unwrap().data,
        particle_pixels.data,
        "cold Particle execution must preserve typed field rendering"
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_point_attribute_loads_roundtrip_every_type_and_invalid_integer_stores_zero() {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    let mut cases = vec![
        PropertyValue::Number((-73.125).into()),
        PropertyValue::Vec2(Vec2 {
            x: (-2.0).into(),
            y: 3.25.into(),
        }),
        PropertyValue::Vec3(particle_vec3(-4.5, 5.75, 6.0)),
        PropertyValue::Vec4(Vec4 {
            x: (-7.0).into(),
            y: 8.0.into(),
            z: 9.5.into(),
            w: (-10.25).into(),
        }),
        working_color(),
    ];
    cases.extend(
        INTEGER_CASES
            .into_iter()
            .map(|value| PropertyValue::Integer(i64::from(value))),
    );
    for value in cases {
        let kind = PointAttributeElementType::from_property_value(&value).unwrap();
        let expected = kind.pack_value(&value).unwrap();
        let scene = one_point_grid(copy_program(kind, value));
        render_point_test_scene(&mut renderer, &scene).unwrap();
        let fields = renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .read_point_fields(&scene.invocation)
            .unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].attributes, vec![expected, expected]);
    }

    let scene = one_point_grid(PointRenderProgram {
        schema: PointAttributeSchema::new(vec![
            definition(203, PointAttributeElementType::Integer),
            definition(204, PointAttributeElementType::Integer),
        ])
        .unwrap(),
        instructions: vec![
            PointInstruction::Constant {
                value: PropertyValue::Number(1.0.into()),
            },
            PointInstruction::Constant {
                value: PropertyValue::Number(0.0.into()),
            },
            PointInstruction::Binary {
                operation: crate::model::point::NumericBinaryOperation::Divide,
                left: 0,
                right: 1,
            },
            PointInstruction::Constant {
                value: PropertyValue::Integer(i64::from(i32::MAX)),
            },
            PointInstruction::StoreAttribute {
                attribute: 0,
                value: 3,
            },
            PointInstruction::LoadAttribute { attribute: 0 },
            PointInstruction::StoreAttribute {
                attribute: 1,
                value: 5,
            },
            PointInstruction::Constant {
                value: working_color(),
            },
        ],
        ramps: Vec::new(),
        color_register: 7,
        position_register: None,
        size_register: None,
        sprite_selection_register: None,
    });
    let pixels = render_point_test_scene(&mut renderer, &scene).unwrap();
    let fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&scene.invocation)
        .unwrap();
    assert_eq!(
        fields[0].attributes,
        vec![PointAttributeGpuDefault::Integer(0); 2]
    );
    assert_eq!(fields[0].color, [0.0; 4]);
    assert!(pixels.data.chunks_exact(4).all(|pixel| pixel[3] == 0));
}
