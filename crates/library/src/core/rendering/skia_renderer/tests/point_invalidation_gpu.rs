use super::point_support::test_gradient;
use super::*;
use crate::model::frame::point::PointGridParameters;
use crate::model::point::{
    NumericBinaryOperation, PointAttributeDefinition, PointAttributeElementType, PointAttributeId,
    PointAttributeSchema, PointInstruction, PointRenderProgram,
};
use crate::model::property::{ColorValue, GradientSpread, PropertyValue};
use crate::rendering::scene_runtime::{PointInvocationStats, SceneRuntimeLimits};

fn color_value(color: Color) -> PropertyValue {
    PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&color))
}

fn color_program(color: Color) -> PointRenderProgram {
    PointRenderProgram {
        schema: PointAttributeSchema::new(Vec::new()).unwrap(),
        instructions: vec![PointInstruction::Constant {
            value: color_value(color),
        }],
        ramps: Vec::new(),
        color_register: 0,
        position_register: None,
        size_register: None,
    }
}

fn shaped_color_program(color: Color) -> PointRenderProgram {
    PointRenderProgram {
        schema: PointAttributeSchema::new(Vec::new()).unwrap(),
        instructions: vec![
            PointInstruction::Constant {
                value: PropertyValue::Number(0.25.into()),
            },
            PointInstruction::Constant {
                value: color_value(color),
            },
        ],
        ramps: Vec::new(),
        color_register: 1,
        position_register: None,
        size_register: None,
    }
}

fn stored_color_program(color: Color) -> PointRenderProgram {
    PointRenderProgram {
        schema: PointAttributeSchema::new(vec![
            PointAttributeDefinition::new(
                PointAttributeId::from_uuid(Uuid::from_u128(801)),
                "value",
                PointAttributeElementType::Number,
                PropertyValue::Number(0.0.into()),
            )
            .unwrap(),
        ])
        .unwrap(),
        instructions: vec![
            PointInstruction::Constant {
                value: PropertyValue::Number(0.25.into()),
            },
            PointInstruction::StoreAttribute {
                attribute: 0,
                value: 0,
            },
            PointInstruction::Constant {
                value: color_value(color),
            },
        ],
        ramps: Vec::new(),
        color_register: 2,
        position_register: None,
        size_register: None,
    }
}

fn positioned_color_program(color: Color, offset: f64) -> PointRenderProgram {
    PointRenderProgram {
        schema: PointAttributeSchema::new(Vec::new()).unwrap(),
        instructions: vec![
            PointInstruction::Position,
            PointInstruction::Constant {
                value: PropertyValue::Vec3(particle_vec3(offset, 0.0, 0.0)),
            },
            PointInstruction::Binary {
                operation: NumericBinaryOperation::Add,
                left: 0,
                right: 1,
            },
            PointInstruction::Constant {
                value: color_value(color),
            },
        ],
        ramps: Vec::new(),
        color_register: 3,
        position_register: Some(2),
        size_register: None,
    }
}

fn ramp_program(end: Color) -> PointRenderProgram {
    PointRenderProgram {
        schema: PointAttributeSchema::new(Vec::new()).unwrap(),
        instructions: vec![
            PointInstruction::Constant {
                value: PropertyValue::Number(0.5.into()),
            },
            PointInstruction::ColorRamp {
                gradient: 0,
                factor: 0,
            },
        ],
        ramps: vec![test_gradient(
            GradientSpread::Pad,
            &[(0.0, Color::black()), (1.0, end)],
        )],
        color_register: 1,
        position_register: None,
        size_register: None,
    }
}

fn stats(renderer: &SkiaRenderer, key: &SceneInvocationKey) -> PointInvocationStats {
    renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .invocation_stats(key)
        .expect("Point invocation stats")
}

fn assert_simulation_unchanged(before: &PointInvocationStats, after: &PointInvocationStats) {
    assert_eq!(after.simulation_generation, before.simulation_generation);
    assert_eq!(after.simulated_steps, before.simulated_steps);
    assert_eq!(after.checkpoint_restores, before.checkpoint_restores);
    assert_eq!(after.current_step, before.current_step);
    assert_eq!(after.checkpoint_steps, before.checkpoint_steps);
}

fn assert_stats_unchanged(before: &PointInvocationStats, after: &PointInvocationStats) {
    assert_simulation_unchanged(before, after);
    assert_eq!(after.field_generation, before.field_generation);
    assert_eq!(after.field_bytes, before.field_bytes);
    assert_eq!(after.allocated_bytes, before.allocated_bytes);
}

fn assert_rebuilt_at_480(before: &PointInvocationStats, after: &PointInvocationStats) {
    assert_ne!(after.simulation_generation, before.simulation_generation);
    assert_eq!(after.simulated_steps, 480);
    assert_eq!(after.checkpoint_restores, 0);
    assert_eq!(after.current_step, 480);
    assert_eq!(after.checkpoint_steps, vec![240, 480]);
}

fn render_rebuilt(
    renderer: &mut SkiaRenderer,
    scene: &PointSceneFrame,
    before: &PointInvocationStats,
) -> PointInvocationStats {
    render_point_test_scene(renderer, scene).unwrap();
    let after = stats(renderer, &scene.invocation);
    assert_rebuilt_at_480(before, &after);
    after
}

type PointSourceSnapshot = (u32, Option<f32>, Option<f32>, Option<[f32; 3]>);

fn source_snapshot(renderer: &SkiaRenderer, key: &SceneInvocationKey) -> Vec<PointSourceSnapshot> {
    renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(key)
        .unwrap()
        .into_iter()
        .map(|point| {
            (
                point.serial,
                point.age,
                point.lifetime,
                point.source_position,
            )
        })
        .collect()
}

fn opaque(r: u8, g: u8, b: u8) -> Color {
    Color { r, g, b, a: 255 }
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn render_field_changes_retain_warm_particle_simulation_and_checkpoints() {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer = SkiaRenderer::new(256, 144, transparent.clone(), true, None, None).unwrap();
    let mut scene = particle_scene(480);
    scene.color = Color::white();
    let uniform_pixels = render_point_test_scene(&mut renderer, &scene).unwrap();
    let at_480 = stats(&renderer, &scene.invocation);
    let mut at_300 = scene.clone();
    set_particle_step(&mut at_300, 300);
    render_point_test_scene(&mut renderer, &at_300).unwrap();
    let rewound = stats(&renderer, &scene.invocation);
    assert_eq!(rewound.simulation_generation, at_480.simulation_generation);
    assert_eq!(rewound.checkpoint_restores, at_480.checkpoint_restores + 1);
    assert_eq!(rewound.simulated_steps, at_480.simulated_steps + 60);
    assert_eq!(rewound.current_step, 300);
    assert_eq!(rewound.checkpoint_steps, vec![240, 480]);
    render_point_test_scene(&mut renderer, &scene).unwrap();
    let uniform_stats = stats(&renderer, &scene.invocation);
    assert_eq!(uniform_stats.simulated_steps, rewound.simulated_steps + 180);
    assert_eq!(
        uniform_stats.checkpoint_restores,
        rewound.checkpoint_restores
    );
    assert_eq!(uniform_stats.checkpoint_steps, vec![240, 480]);
    assert_eq!(uniform_stats.field_bytes, 0);

    scene.point_program = Some(color_program(Color::white()));
    let field_pixels = render_point_test_scene(&mut renderer, &scene).unwrap();
    assert_eq!(field_pixels.data, uniform_pixels.data);
    let base = stats(&renderer, &scene.invocation);
    assert_simulation_unchanged(&uniform_stats, &base);
    assert!(base.field_generation > uniform_stats.field_generation);
    assert_eq!(
        base.allocated_bytes,
        base.field_bytes + (uniform_stats.allocated_bytes - uniform_stats.field_bytes),
        "adding fields must retain exactly the existing simulation/checkpoints"
    );
    let source = source_snapshot(&renderer, &scene.invocation);

    scene.point_program = Some(color_program(opaque(255, 0, 0)));
    let recolored = render_point_test_scene(&mut renderer, &scene).unwrap();
    assert_ne!(recolored.data, field_pixels.data);
    let recolored_stats = stats(&renderer, &scene.invocation);
    assert_simulation_unchanged(&base, &recolored_stats);
    assert_eq!(recolored_stats.field_generation, base.field_generation);
    assert_eq!(source_snapshot(&renderer, &scene.invocation), source);

    scene.point_program = Some(shaped_color_program(opaque(255, 0, 0)));
    render_point_test_scene(&mut renderer, &scene).unwrap();
    let reshaped = stats(&renderer, &scene.invocation);
    assert_simulation_unchanged(&base, &reshaped);
    assert_eq!(reshaped.field_generation, base.field_generation);
    assert_eq!(source_snapshot(&renderer, &scene.invocation), source);

    scene.point_program = Some(ramp_program(opaque(255, 0, 0)));
    let ramp_red = render_point_test_scene(&mut renderer, &scene).unwrap();
    let ramp_red_stats = stats(&renderer, &scene.invocation);
    assert_simulation_unchanged(&base, &ramp_red_stats);
    assert_eq!(ramp_red_stats.field_generation, base.field_generation);
    let ramp_pipeline_count = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .compiled_point_pipeline_count();
    scene.point_program = Some(ramp_program(opaque(0, 0, 255)));
    let ramp_blue = render_point_test_scene(&mut renderer, &scene).unwrap();
    let ramp_blue_stats = stats(&renderer, &scene.invocation);
    assert_ne!(ramp_blue.data, ramp_red.data);
    assert_simulation_unchanged(&base, &ramp_blue_stats);
    assert_eq!(ramp_blue_stats.field_generation, base.field_generation);
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .compiled_point_pipeline_count(),
        ramp_pipeline_count,
        "Gradient stop edits are program data, not shader shape"
    );

    scene.point_program = Some(stored_color_program(opaque(0, 255, 0)));
    render_point_test_scene(&mut renderer, &scene).unwrap();
    let schema_changed = stats(&renderer, &scene.invocation);
    assert_simulation_unchanged(&base, &schema_changed);
    assert_ne!(schema_changed.field_generation, reshaped.field_generation);
    assert_eq!(
        schema_changed.field_bytes - base.field_bytes,
        u64::from(particle_parameters_mut(&mut scene).capacity) * 4
    );
    assert_eq!(source_snapshot(&renderer, &scene.invocation), source);

    scene.point_program = Some(positioned_color_program(opaque(0, 255, 0), 16.0));
    let positioned_pixels = render_point_test_scene(&mut renderer, &scene).unwrap();
    let positioned = stats(&renderer, &scene.invocation);
    assert_simulation_unchanged(&base, &positioned);
    assert_ne!(positioned.field_generation, schema_changed.field_generation);
    assert_eq!(
        positioned.field_bytes - base.field_bytes,
        u64::from(particle_parameters_mut(&mut scene).capacity) * 16
    );
    assert_eq!(source_snapshot(&renderer, &scene.invocation), source);

    let positioned_source = source_snapshot(&renderer, &scene.invocation);
    let mut export = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    assert_eq!(
        render_point_test_scene(&mut export, &scene).unwrap().data,
        positioned_pixels.data
    );
    assert_eq!(
        source_snapshot(&export, &scene.invocation),
        positioned_source
    );

    scene.point_program = None;
    render_point_test_scene(&mut renderer, &scene).unwrap();
    let removed = stats(&renderer, &scene.invocation);
    assert_simulation_unchanged(&base, &removed);
    assert_eq!(removed.field_bytes, 0);
    assert_ne!(removed.field_generation, positioned.field_generation);
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn particle_source_identity_parameters_seed_capacity_and_kind_reset_state() {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    let mut scene = particle_scene(480);
    scene.point_program = Some(color_program(Color::white()));
    render_point_test_scene(&mut renderer, &scene).unwrap();
    let mut previous = stats(&renderer, &scene.invocation);

    particle_parameters_mut(&mut scene).emission_rate = 121.0.into();
    previous = render_rebuilt(&mut renderer, &scene, &previous);

    particle_parameters_mut(&mut scene).emitter_shape =
        crate::model::frame::particle::ParticleEmitterShape::Box;
    previous = render_rebuilt(&mut renderer, &scene, &previous);

    particle_parameters_mut(&mut scene).emitter_position = particle_vec3(3.0, -2.0, 1.0);
    previous = render_rebuilt(&mut renderer, &scene, &previous);

    particle_parameters_mut(&mut scene).forces.reverse();
    previous = render_rebuilt(&mut renderer, &scene, &previous);

    let crate::model::frame::particle::ParticleForce::Drag { coefficient } =
        &mut particle_parameters_mut(&mut scene).forces[0]
    else {
        panic!("reversed fixture begins with Drag")
    };
    *coefficient = 0.2.into();
    previous = render_rebuilt(&mut renderer, &scene, &previous);

    particle_parameters_mut(&mut scene).velocity_min = particle_vec3(-39.0, -120.0, -20.0);
    previous = render_rebuilt(&mut renderer, &scene, &previous);

    particle_parameters_mut(&mut scene).size_min = 5.0.into();
    previous = render_rebuilt(&mut renderer, &scene, &previous);

    particle_parameters_mut(&mut scene).lifetime_seconds = 3.9.into();
    previous = render_rebuilt(&mut renderer, &scene, &previous);

    particle_parameters_mut(&mut scene).seed += 1;
    previous = render_rebuilt(&mut renderer, &scene, &previous);

    scene.source_node_id = Uuid::from_u128(899);
    previous = render_rebuilt(&mut renderer, &scene, &previous);

    particle_parameters_mut(&mut scene).capacity = 512;
    let changed_capacity = render_rebuilt(&mut renderer, &scene, &previous);
    assert!(changed_capacity.allocated_bytes < previous.allocated_bytes);

    scene.source = PointSceneSource::Grid(PointGridParameters {
        counts: [8, 8, 1],
        spacing: particle_vec3(12.0, 12.0, 0.0),
        center: particle_vec3(0.0, 0.0, 0.0),
        size: 4.0.into(),
        seed: 9,
    });
    render_point_test_scene(&mut renderer, &scene).unwrap();
    let grid = stats(&renderer, &scene.invocation);
    assert_eq!(grid.simulation_generation, 0);
    assert_eq!(grid.simulated_steps, 0);
    assert_eq!(grid.current_step, 0);
    assert!(grid.checkpoint_steps.is_empty());
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn field_budget_and_validation_failure_preserve_the_warm_invocation() {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    let mut scene = particle_scene(480);
    scene.point_program = Some(color_program(Color::white()));
    let baseline_pixels = render_point_test_scene(&mut renderer, &scene).unwrap();
    let baseline = stats(&renderer, &scene.invocation);
    let source = source_snapshot(&renderer, &scene.invocation);
    let capacity = u64::from(particle_parameters_mut(&mut scene).capacity);
    let base_particle_bytes = (baseline.allocated_bytes - baseline.field_bytes)
        / (1 + baseline.checkpoint_steps.len() as u64);
    assert_eq!(
        baseline.allocated_bytes,
        baseline.field_bytes + base_particle_bytes * (1 + baseline.checkpoint_steps.len() as u64)
    );

    let replacement_field_bytes = baseline.field_bytes + capacity * 16;
    renderer
        .scene_runtime
        .as_mut()
        .unwrap()
        .set_limits_for_test(SceneRuntimeLimits {
            max_state_bytes: baseline.allocated_bytes + replacement_field_bytes - 1,
            ..SceneRuntimeLimits::default()
        });
    scene.point_program = Some(positioned_color_program(Color::white(), 12.0));
    let budget_error = render_point_test_scene(&mut renderer, &scene).unwrap_err();
    assert!(budget_error.contains("state budget"), "{budget_error}");
    let after_budget = stats(&renderer, &scene.invocation);
    assert_stats_unchanged(&baseline, &after_budget);
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .resident_state_bytes(),
        baseline.allocated_bytes
    );

    renderer
        .scene_runtime
        .as_mut()
        .unwrap()
        .set_limits_for_test(SceneRuntimeLimits::default());
    scene.point_program = Some(color_program(Color::white()));
    assert_eq!(
        render_point_test_scene(&mut renderer, &scene).unwrap().data,
        baseline_pixels.data
    );
    assert_stats_unchanged(&baseline, &stats(&renderer, &scene.invocation));
    assert_eq!(source_snapshot(&renderer, &scene.invocation), source);

    let mut invalid = color_program(Color::white());
    invalid.color_register = u16::MAX;
    scene.point_program = Some(invalid);
    assert!(render_point_test_scene(&mut renderer, &scene).is_err());
    assert_stats_unchanged(&baseline, &stats(&renderer, &scene.invocation));
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .resident_state_bytes(),
        baseline.allocated_bytes
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn checkpoint_admission_counts_active_render_field_bytes() {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    renderer
        .scene_runtime
        .as_mut()
        .unwrap()
        .set_limits_for_test(SceneRuntimeLimits {
            // capacity=64: Particle=4096, fields=2304, each checkpoint=4096.
            // One checkpoint totals 10496; a second totals 14592. Omitting
            // fields from admission would incorrectly accept two at 12288.
            max_state_bytes: 13_000,
            ..SceneRuntimeLimits::default()
        });
    let mut scene = particle_scene(480);
    particle_parameters_mut(&mut scene).capacity = 64;
    scene.point_program = Some(positioned_color_program(Color::white(), 0.0));
    render_point_test_scene(&mut renderer, &scene).unwrap();
    let stats = stats(&renderer, &scene.invocation);
    assert_eq!(stats.simulated_steps, 480);
    assert_eq!(stats.current_step, 480);
    assert_eq!(stats.checkpoint_steps, vec![240]);
    assert_eq!(stats.field_bytes, 2_304);
    assert_eq!(stats.allocated_bytes, 10_496);
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .resident_state_bytes(),
        10_496
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn field_evaluation_failure_discards_only_fields_and_reuses_warm_simulation() {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    let mut scene = particle_scene(480);
    scene.point_program = Some(color_program(Color::white()));
    let baseline_pixels = render_point_test_scene(&mut renderer, &scene).unwrap();
    let baseline = stats(&renderer, &scene.invocation);
    let source = source_snapshot(&renderer, &scene.invocation);

    renderer
        .scene_runtime
        .as_mut()
        .unwrap()
        .fail_next_field_evaluation_for_test();
    scene.point_program = Some(color_program(opaque(255, 0, 0)));
    let error = render_point_test_scene(&mut renderer, &scene).unwrap_err();
    assert!(
        error.contains("injected Point field evaluation failure"),
        "{error}"
    );
    let failed = stats(&renderer, &scene.invocation);
    assert_simulation_unchanged(&baseline, &failed);
    assert_eq!(failed.field_generation, 0);
    assert_eq!(failed.field_bytes, 0);
    assert_eq!(
        failed.allocated_bytes,
        baseline.allocated_bytes - baseline.field_bytes
    );

    let recovered_pixels = render_point_test_scene(&mut renderer, &scene).unwrap();
    assert_ne!(recovered_pixels.data, baseline_pixels.data);
    let recovered = stats(&renderer, &scene.invocation);
    assert_simulation_unchanged(&baseline, &recovered);
    assert_ne!(recovered.field_generation, 0);
    assert_eq!(recovered.field_bytes, baseline.field_bytes);
    assert_eq!(source_snapshot(&renderer, &scene.invocation), source);
}
