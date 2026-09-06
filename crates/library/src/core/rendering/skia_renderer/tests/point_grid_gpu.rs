use super::point_support::{test_gradient, test_random};
use super::*;
use crate::model::frame::point::PointGridParameters;
use crate::model::point::{
    NumericBinaryOperation, PointAttributeDefinition, PointAttributeElementType,
    PointAttributeGpuDefault, PointAttributeId, PointAttributeSchema, PointInstruction,
    PointRenderProgram,
};
use crate::model::property::{GradientSpread, PropertyValue};

fn grid_scene() -> PointSceneFrame {
    PointSceneFrame {
        invocation: SceneInvocationKey {
            instance_path: InstancePath::root(TimelineId::from_uuid(Uuid::from_u128(1))),
            module_instance_id: ModuleInstanceId::from_uuid(Uuid::from_u128(2)),
            state_slot_id: Uuid::from_u128(3),
            output_id: ModuleOutputId::from_uuid(Uuid::from_u128(4)),
        },
        source_node_id: Uuid::from_u128(7),
        executable_hash: [90; 32],
        logical_width: 256,
        logical_height: 144,
        source: PointSceneSource::Grid(PointGridParameters {
            counts: [5, 3, 1],
            spacing: particle_vec3(24.0, 24.0, 24.0),
            center: particle_vec3(0.0, 0.0, 0.0),
            size: 8.0.into(),
            seed: 123,
        }),
        color: Color::white(),
        point_program: None,
    }
}

fn pixel(image: &Image, x: usize, y: usize) -> &[u8] {
    let start = (y * image.width as usize + x) * 4;
    &image.data[start..start + 4]
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_grid_uses_shared_sprite_fields_and_retains_stable_point_attributes() {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer = SkiaRenderer::new(256, 144, transparent.clone(), true, None, None).unwrap();
    assert!(
        renderer.gpu_context.is_some(),
        "Grid QA requires a real GPU"
    );
    let mut scene = grid_scene();
    let white = render_point_test_scene(&mut renderer, &scene).unwrap();
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .point_invocation_has_particle_state(&scene.invocation),
        Some(false)
    );
    let uniform_pipelines = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .compiled_point_pipeline_count();
    for y in 0..3 {
        for x in 0..5 {
            assert_eq!(pixel(&white, 80 + x * 24, 48 + y * 24), &[255; 4]);
        }
    }
    assert_eq!(pixel(&white, 92, 48), &[0; 4]);
    assert_eq!(
        render_point_test_scene(&mut renderer, &scene).unwrap().data,
        white.data
    );
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .compiled_point_pipeline_count(),
        uniform_pipelines
    );

    let ramp = test_gradient(
        GradientSpread::Pad,
        &[(0.0, Color::black()), (1.0, Color::white())],
    );
    scene.executable_hash = [91; 32];
    scene.point_program = Some(PointRenderProgram {
        schema: PointAttributeSchema::new(vec![
            PointAttributeDefinition::new(
                PointAttributeId::from_uuid(Uuid::from_u128(82)),
                "heat",
                PointAttributeElementType::Number,
                PropertyValue::Number(0.0.into()),
            )
            .unwrap(),
        ])
        .unwrap(),
        instructions: vec![
            PointInstruction::Random { channel: 0 },
            PointInstruction::StoreAttribute {
                attribute: 0,
                value: 0,
            },
            PointInstruction::LoadAttribute { attribute: 0 },
            PointInstruction::Constant {
                value: PropertyValue::Number(0.75.into()),
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
        ramps: vec![ramp.clone()],
        color_register: 5,
    });
    let colored = render_point_test_scene(&mut renderer, &scene).unwrap();
    let field_pipelines = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .compiled_point_pipeline_count();
    assert_ne!(colored.data, white.data);
    let fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&scene.invocation)
        .unwrap();
    assert_eq!(fields.len(), 15);
    let seed = crate::rendering::scene_runtime::invocation_seed(&scene);
    for (index, point) in fields.iter().enumerate() {
        assert_eq!((point.age, point.lifetime), (None, None));
        let PointSceneSource::Grid(grid) = &scene.source else {
            panic!("expected Grid fixture")
        };
        let coordinate = [(index % 5) as u32, (index / 5) as u32, 0];
        assert_eq!(point.serial, grid.point_serial(coordinate).unwrap());
        let expected = test_random(seed, point.serial, 0);
        let PointAttributeGpuDefault::Number(attribute) = point.attributes[0] else {
            panic!("Grid heat readback must remain Number")
        };
        assert!((attribute - expected).abs() <= 2e-6);
        let expected_color =
            crate::color_management::sample_gradient_at(&ramp, f64::from(expected * 0.75))
                .unwrap()
                .rgba();
        for (actual, expected) in point.color.iter().zip(expected_color) {
            assert!((f64::from(*actual) - expected).abs() <= 2e-6);
        }
    }
    // Allocation changes alter dense slots, not surviving lattice IDs or values.
    let PointSceneSource::Grid(grid) = &mut scene.source else {
        panic!("expected Grid fixture")
    };
    grid.counts[0] = 7;
    render_point_test_scene(&mut renderer, &scene).unwrap();
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .compiled_point_pipeline_count(),
        field_pipelines
    );
    let expanded = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&scene.invocation)
        .unwrap();
    assert_eq!(expanded.len(), 21);
    for before in &fields {
        let after = expanded
            .iter()
            .find(|point| point.serial == before.serial)
            .unwrap();
        assert_eq!(before.attributes, after.attributes);
        assert_eq!(before.color, after.color);
    }
    let PointSceneSource::Grid(grid) = &mut scene.source else {
        panic!("expected Grid fixture")
    };
    grid.counts[0] = 5;
    assert_eq!(
        render_point_test_scene(&mut renderer, &scene).unwrap().data,
        colored.data
    );
    let mut export = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    assert_eq!(
        render_point_test_scene(&mut export, &scene).unwrap().data,
        colored.data
    );

    // Switching producer kind at the same invocation cannot reuse stale state.
    let mut particle = particle_scene(240);
    // Different Sprite endpoints in one authored Module share its fingerprint.
    // GPU pipelines must distinguish the producer and field program shape.
    particle.executable_hash = scene.executable_hash;
    particle.invocation = scene.invocation.clone();
    let particle_pixels = render_point_test_scene(&mut renderer, &particle).unwrap();
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .point_invocation_has_particle_state(&scene.invocation),
        Some(true)
    );
    assert_ne!(particle_pixels.data, colored.data);
    assert_eq!(
        render_point_test_scene(&mut renderer, &scene).unwrap().data,
        colored.data
    );
    let PointSceneSource::Grid(grid) = &mut scene.source else {
        panic!("expected Grid fixture")
    };
    grid.counts[2] = 2;
    let volume = render_point_test_scene(&mut renderer, &scene).unwrap();
    assert_ne!(volume.data, colored.data);
    let volume_fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&scene.invocation)
        .unwrap();
    assert_eq!(volume_fields.len(), 30);
    let PointSceneSource::Grid(grid) = &scene.source else {
        panic!("expected Grid fixture")
    };
    for (index, point) in volume_fields.iter().enumerate() {
        let coordinate = [
            (index % 5) as u32,
            ((index / 5) % 3) as u32,
            (index / 15) as u32,
        ];
        assert_eq!(point.serial, grid.point_serial(coordinate).unwrap());
        let PointAttributeGpuDefault::Number(attribute) = point.attributes[0] else {
            panic!("Grid heat readback must remain Number")
        };
        assert!((attribute - test_random(seed, point.serial, 0)).abs() <= 2e-6);
    }
}
