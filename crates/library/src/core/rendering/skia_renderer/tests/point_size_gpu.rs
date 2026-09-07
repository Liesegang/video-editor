use super::*;
use crate::model::frame::point::PointGridParameters;
use crate::model::point::{
    NumericBinaryOperation, PointAttributeDefinition, PointAttributeElementType,
    PointAttributeGpuDefault, PointAttributeId, PointAttributeSchema, PointInstruction,
    PointRenderProgram,
};
use crate::model::property::{ColorValue, PropertyValue};
use crate::rendering::scene_runtime::PointInvocationStats;

fn white() -> PropertyValue {
    PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&Color::white()))
}

fn grid_scene(counts: [u32; 3], center: [f64; 3]) -> PointSceneFrame {
    let mut scene = particle_scene(0);
    scene.invocation.state_slot_id = Uuid::from_u128(901);
    scene.source_node_id = Uuid::from_u128(902);
    scene.source = PointSceneSource::Grid(PointGridParameters {
        counts,
        spacing: particle_vec3(24.0, 24.0, 24.0),
        center: particle_vec3(center[0], center[1], center[2]),
        size: 8.0.into(),
        seed: 19,
    });
    scene.color = Color::white();
    scene
}

fn size_attribute() -> PointAttributeDefinition {
    PointAttributeDefinition::new(
        PointAttributeId::from_uuid(Uuid::from_u128(903)),
        "size",
        PointAttributeElementType::Number,
        PropertyValue::Number(0.0.into()),
    )
    .unwrap()
}

fn varying_grid_size_program() -> PointRenderProgram {
    PointRenderProgram {
        schema: PointAttributeSchema::new(vec![size_attribute()]).unwrap(),
        instructions: vec![
            PointInstruction::Position,
            PointInstruction::Length { value: 0 },
            PointInstruction::StoreAttribute {
                attribute: 0,
                value: 1,
            },
            PointInstruction::LoadAttribute { attribute: 0 },
            PointInstruction::Constant { value: white() },
        ],
        ramps: Vec::new(),
        color_register: 4,
        position_register: None,
        size_register: Some(3),
        sprite_selection_register: None,
    }
}

fn varying_particle_size_program(scale: f64) -> PointRenderProgram {
    PointRenderProgram {
        schema: PointAttributeSchema::new(vec![size_attribute()]).unwrap(),
        instructions: vec![
            PointInstruction::NormalizedAge,
            PointInstruction::Constant {
                value: PropertyValue::Number(scale.into()),
            },
            PointInstruction::Binary {
                operation: NumericBinaryOperation::Multiply,
                left: 0,
                right: 1,
            },
            PointInstruction::StoreAttribute {
                attribute: 0,
                value: 2,
            },
            PointInstruction::LoadAttribute { attribute: 0 },
            PointInstruction::Constant { value: white() },
        ],
        ramps: Vec::new(),
        color_register: 5,
        position_register: None,
        size_register: Some(4),
        sprite_selection_register: None,
    }
}

fn constant_size_program(size: f64) -> PointRenderProgram {
    PointRenderProgram {
        schema: PointAttributeSchema::new(Vec::new()).unwrap(),
        instructions: vec![
            PointInstruction::Constant {
                value: PropertyValue::Number(size.into()),
            },
            PointInstruction::Constant { value: white() },
        ],
        ramps: Vec::new(),
        color_register: 1,
        position_register: None,
        size_register: Some(0),
        sprite_selection_register: None,
    }
}

fn invalid_size_program(
    operation: NumericBinaryOperation,
    left: f64,
    right: f64,
) -> PointRenderProgram {
    PointRenderProgram {
        schema: PointAttributeSchema::new(Vec::new()).unwrap(),
        instructions: vec![
            PointInstruction::Constant {
                value: PropertyValue::Number(left.into()),
            },
            PointInstruction::Constant {
                value: PropertyValue::Number(right.into()),
            },
            PointInstruction::Binary {
                operation,
                left: 0,
                right: 1,
            },
            PointInstruction::Constant { value: white() },
        ],
        ramps: Vec::new(),
        color_register: 3,
        position_register: None,
        size_register: Some(2),
        sprite_selection_register: None,
    }
}

fn geometry_program(position: bool, size: bool, value: f64) -> PointRenderProgram {
    let mut instructions = Vec::new();
    let position_register = position.then(|| {
        instructions.push(PointInstruction::Position);
        instructions.push(PointInstruction::Constant {
            value: PropertyValue::Vec3(particle_vec3(value, 0.0, 0.0)),
        });
        instructions.push(PointInstruction::Binary {
            operation: NumericBinaryOperation::Add,
            left: 0,
            right: 1,
        });
        2
    });
    let size_register = size.then(|| {
        let source = instructions.len() as u16;
        instructions.push(PointInstruction::Size);
        let scale = instructions.len() as u16;
        instructions.push(PointInstruction::Constant {
            value: PropertyValue::Number(value.abs().into()),
        });
        let result = instructions.len() as u16;
        instructions.push(PointInstruction::Binary {
            operation: NumericBinaryOperation::Multiply,
            left: source,
            right: scale,
        });
        result
    });
    let color_register = instructions.len() as u16;
    instructions.push(PointInstruction::Constant { value: white() });
    PointRenderProgram {
        schema: PointAttributeSchema::new(Vec::new()).unwrap(),
        instructions,
        ramps: Vec::new(),
        color_register,
        position_register,
        size_register,
        sprite_selection_register: None,
    }
}

fn stats(renderer: &SkiaRenderer, scene: &PointSceneFrame) -> PointInvocationStats {
    renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .invocation_stats(&scene.invocation)
        .unwrap()
}

fn assert_simulation_same(before: &PointInvocationStats, after: &PointInvocationStats) {
    assert_eq!(after.simulation_generation, before.simulation_generation);
    assert_eq!(after.simulated_steps, before.simulated_steps);
    assert_eq!(after.checkpoint_restores, before.checkpoint_restores);
    assert_eq!(after.current_step, before.current_step);
    assert_eq!(after.checkpoint_steps, before.checkpoint_steps);
}

fn transparent() -> Color {
    Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    }
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_grid_size_uses_position_length_stored_attribute_and_cold_parity() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let mut scene = grid_scene([5, 3, 1], [0.0, 0.0, 0.0]);
    scene.point_program = Some(varying_grid_size_program());
    let image = render_point_test_scene(&mut renderer, &scene).unwrap();
    assert!(image.data.chunks_exact(4).any(|pixel| pixel[3] > 0));
    let fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&scene.invocation)
        .unwrap();
    assert_eq!(fields.len(), 15);
    for (slot, point) in fields.iter().enumerate() {
        let position = [
            (slot % 5) as f32 * 24.0 - 48.0,
            (slot / 5) as f32 * 24.0 - 24.0,
            0.0,
        ];
        let expected = position[0].hypot(position[1]);
        assert_eq!(point.position, Some(position));
        assert_eq!(point.source_size, None);
        let PointAttributeGpuDefault::Number(stored) = point.attributes[0] else {
            panic!("stored size must remain Number")
        };
        let size = point.size.unwrap();
        // Length uses a scaled f32 norm on the GPU; CPU hypot rounds the
        // complete norm. These exact finite fixture inputs differ by at most
        // one output ULP. Attribute storage and final geometry must be exact.
        assert_eq!(size.to_bits(), stored.to_bits());
        assert!(
            size.to_bits().abs_diff(expected.to_bits()) <= 1,
            "slot={slot} source_position={position:?} expected={expected:?} size={size:?} stored_attribute={stored:?} delta={:?}",
            (size - expected).abs()
        );
    }
    assert_eq!(fields[7].size, Some(0.0));
    let mut cold = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    assert_eq!(
        render_point_test_scene(&mut cold, &scene).unwrap().data,
        image.data
    );
    assert_eq!(
        cold.scene_runtime
            .as_ref()
            .unwrap()
            .read_point_fields(&scene.invocation)
            .unwrap(),
        fields
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_particle_age_drives_size_without_mutating_birth_size_or_simulation() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let mut scene = particle_scene(480);
    scene.point_program = Some(varying_particle_size_program(20.0));
    let pixels = render_point_test_scene(&mut renderer, &scene).unwrap();
    assert!(pixels.data.chunks_exact(4).any(|pixel| pixel[3] > 0));
    let first_stats = stats(&renderer, &scene);
    let first = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&scene.invocation)
        .unwrap();
    assert!(!first.is_empty());
    let mut distinct = std::collections::HashSet::new();
    for point in &first {
        let normalized = (point.age.unwrap() / point.lifetime.unwrap()).clamp(0.0, 1.0);
        let expected = normalized * 20.0;
        assert!((point.size.unwrap() - expected).abs() <= 2e-6);
        assert!(point.source_size.is_some());
        assert_eq!(point.position, point.source_position);
        distinct.insert(point.size.unwrap().to_bits());
    }
    assert!(distinct.len() > 4);

    scene.point_program = Some(varying_particle_size_program(28.0));
    let changed = render_point_test_scene(&mut renderer, &scene).unwrap();
    assert_ne!(changed.data, pixels.data);
    let changed_stats = stats(&renderer, &scene);
    assert_simulation_same(&first_stats, &changed_stats);
    assert_eq!(changed_stats.field_generation, first_stats.field_generation);
    let changed_fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&scene.invocation)
        .unwrap();
    for (before, after) in first.iter().zip(&changed_fields) {
        assert_eq!(after.source_size, before.source_size);
        assert_eq!(after.source_position, before.source_position);
    }
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_zero_size_is_valid_but_negative_and_nonfinite_are_atomic_invalid() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let mut scene = grid_scene([1, 1, 1], [3.0, 4.0, 0.0]);
    scene.point_program = Some(constant_size_program(0.0));
    let zero = render_point_test_scene(&mut renderer, &scene).unwrap();
    assert!(zero.data.chunks_exact(4).all(|pixel| pixel[3] == 0));
    let zero_field = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&scene.invocation)
        .unwrap()
        .remove(0);
    assert_eq!(zero_field.position, Some([3.0, 4.0, 0.0]));
    assert_eq!(zero_field.size, Some(0.0));
    assert_eq!(zero_field.color, [1.0; 4]);

    for program in [
        constant_size_program(-1.0),
        invalid_size_program(NumericBinaryOperation::Divide, 1.0, 0.0),
        invalid_size_program(NumericBinaryOperation::Multiply, f64::from(f32::MAX), 2.0),
    ] {
        scene.point_program = Some(program);
        let image = render_point_test_scene(&mut renderer, &scene).unwrap();
        assert!(image.data.chunks_exact(4).all(|pixel| pixel[3] == 0));
        let field = renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .read_point_fields(&scene.invocation)
            .unwrap()
            .remove(0);
        assert_eq!(field.position, Some([0.0; 3]));
        assert_eq!(field.size, Some(0.0));
        assert_eq!(field.color, [0.0; 4]);
    }
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_size_and_position_share_one_geometry_buffer_without_simulation_reset() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let mut scene = particle_scene(480);
    scene.point_program = Some(geometry_program(false, false, 0.0));
    render_point_test_scene(&mut renderer, &scene).unwrap();
    let color_only = stats(&renderer, &scene);
    let capacity = u64::from(particle_parameters_mut(&mut scene).capacity);

    scene.point_program = Some(geometry_program(false, true, 12.0));
    render_point_test_scene(&mut renderer, &scene).unwrap();
    let size_only = stats(&renderer, &scene);
    assert_simulation_same(&color_only, &size_only);
    assert_ne!(size_only.field_generation, color_only.field_generation);
    assert_eq!(
        size_only.field_bytes - color_only.field_bytes,
        capacity * 16
    );

    scene.point_program = Some(geometry_program(true, true, 12.0));
    render_point_test_scene(&mut renderer, &scene).unwrap();
    let both = stats(&renderer, &scene);
    assert_simulation_same(&color_only, &both);
    assert_eq!(both.field_generation, size_only.field_generation);
    assert_eq!(both.field_bytes, size_only.field_bytes);

    scene.point_program = Some(geometry_program(true, false, 12.0));
    render_point_test_scene(&mut renderer, &scene).unwrap();
    let position_only = stats(&renderer, &scene);
    assert_simulation_same(&color_only, &position_only);
    assert_eq!(position_only.field_generation, size_only.field_generation);
    assert_eq!(position_only.field_bytes, size_only.field_bytes);

    let pipeline_count = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .compiled_point_pipeline_count();
    scene.point_program = Some(geometry_program(false, true, 18.0));
    let final_pixels = render_point_test_scene(&mut renderer, &scene).unwrap();
    let final_stats = stats(&renderer, &scene);
    assert_simulation_same(&color_only, &final_stats);
    assert_eq!(final_stats.field_generation, size_only.field_generation);
    assert_eq!(final_stats.field_bytes, size_only.field_bytes);
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .compiled_point_pipeline_count(),
        pipeline_count,
        "returning to the same size-only shader shape must hit the cache"
    );
    let final_fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&scene.invocation)
        .unwrap();
    for point in &final_fields {
        let source_size = point.source_size.expect("Particle birth size");
        assert!((point.size.unwrap() - source_size * 18.0).abs() <= 2e-5);
        assert_eq!(point.position, point.source_position);
    }

    let mut cold = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    assert_eq!(
        render_point_test_scene(&mut cold, &scene).unwrap().data,
        final_pixels.data
    );
    assert_eq!(
        cold.scene_runtime
            .as_ref()
            .unwrap()
            .read_point_fields(&scene.invocation)
            .unwrap(),
        final_fields
    );
}
