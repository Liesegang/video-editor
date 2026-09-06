use ordered_float::OrderedFloat;

use super::*;
use crate::model::frame::particle::{ParticleEmitterShape, ParticleForce};
use crate::model::point::{PointAttributeSchema, PointInstruction, PointRenderProgram};
use crate::model::property::Vec3;
use crate::model::property::{GradientValue, PropertyValue};

fn vec3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
        z: OrderedFloat(z),
    }
}

fn parameters() -> ParticleSceneParameters {
    ParticleSceneParameters {
        capacity: 8_192,
        emission_rate: OrderedFloat(120.0),
        lifetime_seconds: OrderedFloat(4.0),
        seed: 7,
        emitter_shape: ParticleEmitterShape::Point,
        emitter_position: vec3(0.0, 0.0, 0.0),
        emitter_radius: OrderedFloat(0.0),
        emitter_size: vec3(0.0, 0.0, 0.0),
        emitter_surface_only: false,
        velocity_min: vec3(-1.0, -2.0, -3.0),
        velocity_max: vec3(1.0, 2.0, 3.0),
        forces: vec![
            ParticleForce::Gravity {
                acceleration: vec3(0.0, 180.0, 0.0),
            },
            ParticleForce::Drag {
                coefficient: OrderedFloat(0.15),
            },
        ],
        size_min: OrderedFloat(6.0),
        size_max: OrderedFloat(18.0),
    }
}

fn scene(target_step: u64) -> PointSceneFrame {
    PointSceneFrame {
        invocation: SceneInvocationKey {
            instance_path: crate::model::authoring::InstancePath::root(
                crate::model::authoring::TimelineId::new(),
            ),
            module_instance_id: crate::model::authoring::ModuleInstanceId::new(),
            state_slot_id: uuid::Uuid::from_u128(1),
            output_id: crate::model::authoring::ModuleOutputId::new(),
        },
        source_node_id: uuid::Uuid::from_u128(2),
        executable_hash: [7; 32],
        logical_width: 1920,
        logical_height: 1080,
        source: PointSceneSource::Particle {
            target_step,
            parameters: parameters(),
        },
        color: crate::model::frame::color::Color {
            r: 20,
            g: 40,
            b: 60,
            a: 200,
        },
        point_program: None,
    }
}

#[test]
fn render_only_color_does_not_invalidate_simulation_history() {
    let first = parameters();
    let mut changed_force = first.clone();
    changed_force.forces[0] = ParticleForce::Gravity {
        acceleration: vec3(0.0, 200.0, 0.0),
    };
    assert_ne!(
        stable_parameter_hash(&first),
        stable_parameter_hash(&changed_force)
    );
    let mut reordered = first.clone();
    reordered.forces.reverse();
    assert_ne!(
        stable_parameter_hash(&first),
        stable_parameter_hash(&reordered)
    );

    let mut changed_emitter_shape = first.clone();
    changed_emitter_shape.emitter_shape = ParticleEmitterShape::Sphere;
    assert_ne!(
        stable_parameter_hash(&first),
        stable_parameter_hash(&changed_emitter_shape),
        "birth-position changes must restart derived simulation state"
    );
}

#[test]
fn replay_and_target_allocations_fail_at_explicit_bounds() {
    assert_eq!(validate_replay(10, 20).unwrap(), 10);
    assert!(validate_replay(0, PARTICLE_MAX_REPLAY_STEPS + 1).is_err());
    let capability = gl_backend::CapabilityProfile {
        label: "test OpenGL".to_string(),
        max_texture_size: 16_384,
    };
    assert!(
        validate_target(
            &capability,
            8_192,
            8_192,
            SceneTextureFormat::LinearRgbaF32,
            64 * 1024 * 1024,
        )
        .is_err()
    );
}

#[test]
fn arbitrary_seek_replays_only_the_live_particle_history() {
    let mut scene = scene(21_600);
    let (target_step, parameters) = match &mut scene.source {
        PointSceneSource::Particle {
            target_step,
            parameters,
        } => (target_step, parameters),
        PointSceneSource::Grid(_) => panic!("test fixture must remain a Particle source"),
    };
    assert_eq!(bounded_replay_origin(parameters, *target_step), 21_120);
    assert_eq!(
        validate_replay(
            bounded_replay_origin(parameters, *target_step),
            *target_step
        )
        .unwrap(),
        480
    );

    parameters.lifetime_seconds = OrderedFloat(120.0);
    assert_eq!(bounded_replay_origin(parameters, *target_step), 7_200);
    assert_eq!(
        validate_replay(
            bounded_replay_origin(parameters, *target_step),
            *target_step
        )
        .unwrap(),
        PARTICLE_MAX_REPLAY_STEPS
    );
}

#[test]
fn renderer_branches_keep_the_emitters_random_stream() {
    let mut second_renderer = scene(240);
    let first_seed = invocation_seed(&second_renderer);
    second_renderer.invocation.state_slot_id = uuid::Uuid::from_u128(3);
    second_renderer.invocation.output_id = crate::model::authoring::ModuleOutputId::new();
    assert_eq!(
        invocation_seed(&second_renderer),
        first_seed,
        "renderer-owned state and output identities must not perturb a shared emitter stream"
    );

    second_renderer.source_node_id = uuid::Uuid::from_u128(4);
    assert_ne!(
        invocation_seed(&second_renderer),
        first_seed,
        "distinct emitters need independent deterministic streams"
    );
}

fn constant_color_program(use_last_stop: bool) -> PointRenderProgram {
    let gradient = GradientValue::default();
    let stop = if use_last_stop {
        gradient.stops().last().unwrap()
    } else {
        gradient.stops().first().unwrap()
    };
    PointRenderProgram {
        schema: PointAttributeSchema::new(Vec::new()).unwrap(),
        instructions: vec![PointInstruction::Constant {
            value: PropertyValue::ColorValue(stop.color().clone()),
        }],
        ramps: Vec::new(),
        color_register: 0,
    }
}

#[test]
fn point_pipeline_keys_are_source_and_shader_shape_specific() {
    let first = constant_color_program(false);
    let recolored = constant_color_program(true);
    let particle_hash = point_fields::source_hash(PointSourceKind::Particle, Some(&first)).unwrap();
    assert_eq!(
        particle_hash,
        point_fields::source_hash(PointSourceKind::Particle, Some(&recolored)).unwrap(),
        "uniform program values must reuse one compiled shader"
    );
    assert_ne!(
        PointPipelineKey {
            source_kind: PointSourceKind::Particle,
            field_source_hash: particle_hash,
        },
        PointPipelineKey {
            source_kind: PointSourceKind::Grid,
            field_source_hash: point_fields::source_hash(PointSourceKind::Grid, Some(&first))
                .unwrap(),
        },
        "one Module executable may contain both Particle and Grid producers"
    );
}

#[test]
fn grid_field_shader_rejects_age_without_a_fake_lifetime() {
    let mut program = constant_color_program(false);
    program.instructions.insert(0, PointInstruction::Age);
    program.color_register = 1;
    let error = point_fields::source_hash(PointSourceKind::Grid, Some(&program)).unwrap_err();
    assert!(error.to_string().contains("require a Particle source"));
}
