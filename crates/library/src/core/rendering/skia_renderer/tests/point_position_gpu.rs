use super::*;
use crate::model::frame::point::PointGridParameters;
use crate::model::point::{
    NumericBinaryOperation, PointAttributeSchema, PointInstruction, PointRenderProgram,
};
use crate::model::property::{ColorValue, PropertyValue};

fn grid_scene() -> PointSceneFrame {
    PointSceneFrame {
        invocation: SceneInvocationKey {
            instance_path: InstancePath::root(TimelineId::from_uuid(Uuid::from_u128(701))),
            module_instance_id: ModuleInstanceId::from_uuid(Uuid::from_u128(702)),
            state_slot_id: Uuid::from_u128(703),
            output_id: ModuleOutputId::from_uuid(Uuid::from_u128(704)),
        },
        source_node_id: Uuid::from_u128(705),
        logical_width: 256,
        logical_height: 144,
        source: PointSceneSource::Grid(PointGridParameters {
            counts: [5, 3, 1],
            spacing: particle_vec3(24.0, 24.0, 24.0),
            center: particle_vec3(0.0, 0.0, 0.0),
            size: 8.0.into(),
            seed: 17,
        }),
        color: Color::white(),
        point_program: None,
    }
}

fn white() -> PropertyValue {
    PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&Color::white()))
}

fn position_program(offset: [f64; 3]) -> PointRenderProgram {
    PointRenderProgram {
        schema: PointAttributeSchema::new(Vec::new()).unwrap(),
        instructions: vec![
            PointInstruction::Position,
            PointInstruction::Constant {
                value: PropertyValue::Vec3(particle_vec3(offset[0], offset[1], offset[2])),
            },
            PointInstruction::Binary {
                operation: NumericBinaryOperation::Add,
                left: 0,
                right: 1,
            },
            PointInstruction::Constant { value: white() },
        ],
        ramps: Vec::new(),
        color_register: 3,
        position_register: Some(2),
    }
}

fn pixel(image: &Image, x: usize, y: usize) -> &[u8] {
    let offset = (y * image.width as usize + x) * 4;
    &image.data[offset..offset + 4]
}

fn assert_position(actual: Option<[f32; 3]>, expected: [f32; 3]) {
    let actual = actual.expect("Set Position must publish derived geometry");
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() <= 1e-6, "{actual} != {expected}");
    }
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_grid_position_output_preserves_zero_and_reuses_warm_shape_for_offsets() {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer = SkiaRenderer::new(256, 144, transparent.clone(), true, None, None).unwrap();
    let mut scene = grid_scene();
    let baseline = render_point_test_scene(&mut renderer, &scene).unwrap();

    scene.point_program = Some(position_program([0.0, 0.0, 0.0]));
    let neutral = render_point_test_scene(&mut renderer, &scene).unwrap();
    assert_eq!(
        neutral.data, baseline.data,
        "zero offset must be pixel-neutral"
    );
    let neutral_fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&scene.invocation)
        .unwrap();
    assert_eq!(neutral_fields.len(), 15);
    for (slot, point) in neutral_fields.iter().enumerate() {
        let x = (slot % 5) as f32 * 24.0 - 48.0;
        let y = (slot / 5) as f32 * 24.0 - 24.0;
        assert_position(point.position, [x, y, 0.0]);
        assert!(point.source_position.is_none());
    }
    let warm_pipelines = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .compiled_point_pipeline_count();

    scene.point_program = Some(position_program([12.0, -8.0, 0.0]));
    let moved = render_point_test_scene(&mut renderer, &scene).unwrap();
    assert_ne!(moved.data, baseline.data);
    assert_eq!(pixel(&moved, 140, 64), &[255; 4]);
    assert_eq!(pixel(&moved, 128, 72), &[0; 4]);
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .compiled_point_pipeline_count(),
        warm_pipelines,
        "changing only the sampled offset must reuse the shader shape"
    );
    for (neutral, moved) in neutral_fields.iter().zip(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .read_point_fields(&scene.invocation)
            .unwrap(),
    ) {
        let source = neutral.position.unwrap();
        assert_position(
            moved.position,
            [source[0] + 12.0, source[1] - 8.0, source[2]],
        );
    }
    let mut cold = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    assert_eq!(
        render_point_test_scene(&mut cold, &scene).unwrap().data,
        moved.data,
        "an export-style cold renderer must consume the same derived positions"
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_particle_position_is_render_only_and_replays_from_unchanged_simulation() {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    let mut scene = particle_scene(180);
    scene.point_program = Some(position_program([30.0, -4.0, 0.0]));
    let first_pixels = render_point_test_scene(&mut renderer, &scene).unwrap();
    let first = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&scene.invocation)
        .unwrap();
    assert!(!first.is_empty());
    for point in &first {
        let source = point.source_position.expect("Particle source state");
        assert_position(
            point.position,
            [source[0] + 30.0, source[1] - 4.0, source[2]],
        );
    }
    let warm_pipelines = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .compiled_point_pipeline_count();

    scene.point_program = Some(position_program([-20.0, 7.0, 0.0]));
    let changed_pixels = render_point_test_scene(&mut renderer, &scene).unwrap();
    let changed = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&scene.invocation)
        .unwrap();
    assert_ne!(changed_pixels.data, first_pixels.data);
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .compiled_point_pipeline_count(),
        warm_pipelines
    );
    for (before, after) in first.iter().zip(&changed) {
        assert_eq!(before.serial, after.serial);
        assert_eq!(before.age, after.age);
        assert_eq!(before.lifetime, after.lifetime);
        assert_eq!(before.source_position, after.source_position);
        let source = after.source_position.unwrap();
        assert_position(
            after.position,
            [source[0] - 20.0, source[1] + 7.0, source[2]],
        );
    }

    let mut earlier = scene.clone();
    set_particle_step(&mut earlier, 60);
    render_point_test_scene(&mut renderer, &earlier).unwrap();
    assert_eq!(
        render_point_test_scene(&mut renderer, &scene).unwrap().data,
        changed_pixels.data,
        "backward seek must replay identical simulation and derived geometry"
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_invalid_position_is_zeroed_and_makes_the_point_transparent() {
    let mut scene = grid_scene();
    scene.point_program = Some(PointRenderProgram {
        schema: PointAttributeSchema::new(Vec::new()).unwrap(),
        instructions: vec![
            PointInstruction::Position,
            PointInstruction::Constant {
                value: PropertyValue::Number(0.0.into()),
            },
            PointInstruction::Binary {
                operation: NumericBinaryOperation::Divide,
                left: 0,
                right: 1,
            },
            PointInstruction::Constant { value: white() },
        ],
        ramps: Vec::new(),
        color_register: 3,
        position_register: Some(2),
    });
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    let pixels = render_point_test_scene(&mut renderer, &scene).unwrap();
    assert!(pixels.data.chunks_exact(4).all(|pixel| pixel[3] == 0));
    for point in renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&scene.invocation)
        .unwrap()
    {
        assert_eq!(point.position, Some([0.0; 3]));
        assert_eq!(point.color, [0.0; 4]);
    }
}
