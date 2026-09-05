use ordered_float::OrderedFloat;

use super::*;
use crate::model::frame::color::Color;
use crate::model::frame::particle::ParticleEmitterShape;

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
        gravity: vec3(0.0, 180.0, 0.0),
        drag: OrderedFloat(0.15),
        size_min: OrderedFloat(6.0),
        size_max: OrderedFloat(18.0),
        color: Color {
            r: 20,
            g: 40,
            b: 60,
            a: 200,
        },
    }
}

fn scene(target_step: u64) -> ParticleSceneFrame {
    ParticleSceneFrame {
        invocation: SceneInvocationKey {
            instance_path: crate::model::authoring::InstancePath::root(
                crate::model::authoring::TimelineId::new(),
            ),
            module_instance_id: crate::model::authoring::ModuleInstanceId::new(),
            state_slot_id: uuid::Uuid::from_u128(1),
            output_id: crate::model::authoring::ModuleOutputId::new(),
        },
        random_stream_id: uuid::Uuid::from_u128(2),
        executable_hash: [7; 32],
        target_step,
        logical_width: 1920,
        logical_height: 1080,
        parameters: parameters(),
    }
}

#[test]
fn render_only_color_does_not_invalidate_simulation_history() {
    let first = parameters();
    let mut recolored = first.clone();
    recolored.color = Color::white();
    assert_eq!(
        stable_parameter_hash(&first),
        stable_parameter_hash(&recolored)
    );

    let mut changed_force = first.clone();
    changed_force.gravity = vec3(0.0, 200.0, 0.0);
    assert_ne!(
        stable_parameter_hash(&first),
        stable_parameter_hash(&changed_force)
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
    assert_eq!(bounded_replay_origin(&scene), 21_120);
    assert_eq!(
        validate_replay(bounded_replay_origin(&scene), scene.target_step).unwrap(),
        480
    );

    scene.parameters.lifetime_seconds = OrderedFloat(120.0);
    assert_eq!(bounded_replay_origin(&scene), 7_200);
    assert_eq!(
        validate_replay(bounded_replay_origin(&scene), scene.target_step).unwrap(),
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

    second_renderer.random_stream_id = uuid::Uuid::from_u128(4);
    assert_ne!(
        invocation_seed(&second_renderer),
        first_seed,
        "distinct emitters need independent deterministic streams"
    );
}
