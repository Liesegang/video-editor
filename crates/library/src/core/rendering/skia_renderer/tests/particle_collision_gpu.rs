use super::particle_collision_support::*;
use super::*;
use crate::model::frame::particle::ParticleCollider;

fn plane(
    point: [f64; 3],
    normal: [f64; 3],
    radius: f32,
    bounce: f32,
    friction: f32,
) -> ParticleCollider {
    ParticleCollider::Plane {
        plane_point: particle_vec3(point[0], point[1], point[2]),
        plane_normal: particle_vec3(normal[0], normal[1], normal[2]),
        radius: radius.into(),
        bounce: bounce.into(),
        friction: friction.into(),
    }
}

fn normalized(value: [f32; 3]) -> [f32; 3] {
    let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
    value.map(|component| component / length)
}

/// Independent one-contact response oracle; position sweep/iteration remain
/// exercised solely by the production GPU implementation.
fn plane_response(velocity: [f32; 3], normal: [f32; 3], bounce: f32, friction: f32) -> [f32; 3] {
    let normal = normalized(normal);
    let normal_speed = velocity
        .into_iter()
        .zip(normal)
        .map(|(velocity, normal)| velocity * normal)
        .sum::<f32>();
    if normal_speed >= 0.0 {
        return velocity;
    }
    std::array::from_fn(|axis| {
        let normal_velocity = normal[axis] * normal_speed;
        let tangent_velocity = velocity[axis] - normal_velocity;
        tangent_velocity * (1.0 - friction) - normal_velocity * bounce
    })
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_empty_and_no_hit_plane_preserve_the_existing_particle_path_exactly() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let baseline = deterministic_scene(120, [0.0, 40.0, 0.0], [15.0, -20.0, 3.0]);
    let baseline_pixels = render_point_test_scene(&mut renderer, &baseline).unwrap();
    let baseline_fields = fields(&renderer, &baseline);
    assert!(!baseline_fields.is_empty());

    let mut no_hit = baseline.clone();
    particle_parameters_mut(&mut no_hit).collisions = vec![plane(
        [0.0, -100_000.0, 0.0],
        [0.0, 1.0, 0.0],
        2.0,
        0.8,
        0.6,
    )];
    assert_eq!(
        render_point_test_scene(&mut renderer, &no_hit)
            .unwrap()
            .data,
        baseline_pixels.data,
        "a non-contacting Plane and the disabled/empty collision path must be bit-exact"
    );
    assert_eq!(fields(&renderer, &no_hit), baseline_fields);
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_swept_plane_handles_high_speed_bounce_friction_and_radius() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let mut scene = deterministic_scene(2, [0.0, 1.0, 0.0], [60.0, -240.0, 0.0]);
    particle_parameters_mut(&mut scene).collisions =
        vec![plane([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.5, 0.5, 0.25)];
    render_point_test_scene(&mut renderer, &scene).unwrap();
    let point = serial(&fields(&renderer, &scene), 0).clone();

    // Contact occurs at 1/480s: x=.125, y=.5. The reflected velocity is
    // [60*(1-.25), 240*.5], then advances through the remaining 1/160s.
    assert_vec3_near(
        point.source_position.unwrap(),
        [0.406_25, 1.25, 0.0],
        3.0e-5,
        "swept position",
    );
    assert_vec3_near(
        point.source_velocity.unwrap(),
        [45.0, 120.0, 0.0],
        3.0e-5,
        "collision velocity",
    );
    assert!((point.age.unwrap() - STEP_SECONDS).abs() <= f32::EPSILON);
    assert_eq!(point.serial, 0);

    let mut endpoint_only = scene.clone();
    particle_parameters_mut(&mut endpoint_only)
        .collisions
        .clear();
    render_point_test_scene(&mut renderer, &endpoint_only).unwrap();
    let missed = serial(&fields(&renderer, &endpoint_only), 0).clone();
    assert_vec3_near(
        missed.source_position.unwrap(),
        [0.5, -1.0, 0.0],
        2.0e-6,
        "unconstrained position",
    );
    assert_eq!(missed.serial, point.serial);
    assert_eq!(missed.age, point.age);
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_plane_normal_scale_and_tiny_nonzero_normal_preserve_orientation() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let base = deterministic_scene(2, [0.0, 1.0, 0.0], [30.0, -180.0, 0.0]);
    let mut snapshots = Vec::new();
    for normal in [
        [0.0, 1.0, 0.0],
        [0.0, 1_000_000.0, 0.0],
        [0.0, 1.0e-200, 0.0],
    ] {
        let mut scene = base.clone();
        particle_parameters_mut(&mut scene).collisions =
            vec![plane([0.0, 0.0, 0.0], normal, 0.25, 0.75, 0.1)];
        render_point_test_scene(&mut renderer, &scene).unwrap();
        snapshots.push(serial(&fields(&renderer, &scene), 0).clone());
    }
    assert_eq!(snapshots[1], snapshots[0]);
    assert_eq!(snapshots[2], snapshots[0]);
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_spawn_penetration_projects_and_only_reflects_inward_velocity() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let collision = plane([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.5, 0.5, 0.25);
    for (velocity, expected_velocity) in [
        ([60.0, -120.0, 0.0], [45.0, 60.0, 0.0]),
        ([60.0, 120.0, 0.0], [60.0, 120.0, 0.0]),
    ] {
        let mut scene = deterministic_scene(1, [0.0, -4.0, 0.0], velocity.map(f64::from));
        particle_parameters_mut(&mut scene).collisions = vec![collision.clone()];
        render_point_test_scene(&mut renderer, &scene).unwrap();
        let point = serial(&fields(&renderer, &scene), 0).clone();
        assert_vec3_near(
            point.source_position.unwrap(),
            [0.0, 0.5, 0.0],
            2.0e-6,
            "spawn projection",
        );
        assert_vec3_near(
            point.source_velocity.unwrap(),
            expected_velocity,
            2.0e-6,
            "spawn response",
        );
        assert_eq!(point.age, Some(0.0));
    }
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_collision_order_breaks_ties_and_incompatible_planes_kill_boundedly() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let first = plane([0.0; 3], [1.0, 0.0, 0.0], 0.0, 0.5, 0.25);
    let second = plane([0.0; 3], [1.0, 1.0, 0.0], 0.0, 0.25, 0.5);
    let initial_velocity = [-120.0, -240.0, 0.0];
    let mut observed = Vec::new();
    for collisions in [vec![first.clone(), second.clone()], vec![second, first]] {
        // Both signed distances and therefore both times of impact are exact
        // zero. The non-orthogonal responses do not commute, so this proves
        // that an exact tie follows authored order rather than float noise.
        let mut scene = deterministic_scene(2, [0.0, 0.0, 0.0], initial_velocity.map(f64::from));
        particle_parameters_mut(&mut scene).collisions = collisions.clone();
        render_point_test_scene(&mut renderer, &scene).unwrap();
        let velocity = serial(&fields(&renderer, &scene), 0)
            .source_velocity
            .unwrap();
        let expected = collisions
            .iter()
            .fold(initial_velocity, |velocity, collision| {
                let ParticleCollider::Plane {
                    plane_normal,
                    bounce,
                    friction,
                    ..
                } = collision
                else {
                    panic!("Plane tie fixture changed collider kind")
                };
                plane_response(
                    velocity,
                    [
                        plane_normal.x.into_inner() as f32,
                        plane_normal.y.into_inner() as f32,
                        plane_normal.z.into_inner() as f32,
                    ],
                    bounce.into_inner(),
                    friction.into_inner(),
                )
            });
        assert_vec3_near(velocity, expected, 5.0e-5, "tie response");
        observed.push(velocity);
    }
    assert_ne!(observed[0], observed[1], "tie order must be authored order");

    let x_plane = plane([0.0; 3], [1.0, 0.0, 0.0], 0.0, 1.0, 0.0);
    let y_plane = plane([0.0; 3], [0.0, 1.0, 0.0], 0.0, 1.0, 0.0);
    let mut earliest_snapshots = Vec::new();
    for collisions in [
        vec![x_plane.clone(), y_plane.clone()],
        vec![y_plane, x_plane],
    ] {
        // X hits at .0025s and Y at .006s. Reversing authored order must not
        // change a non-tie: the sweep always chooses the earliest contact.
        let mut scene = deterministic_scene(2, [0.3, 0.6, 0.0], [-120.0, -100.0, 0.0]);
        particle_parameters_mut(&mut scene).collisions = collisions;
        render_point_test_scene(&mut renderer, &scene).unwrap();
        earliest_snapshots.push(serial(&fields(&renderer, &scene), 0).clone());
    }
    assert_eq!(earliest_snapshots[0], earliest_snapshots[1]);

    let mut impossible = deterministic_scene(1, [0.0, 0.0, 0.0], [0.0; 3]);
    particle_parameters_mut(&mut impossible).collisions = vec![
        plane([0.0, 1.0, 0.0], [0.0, 1.0, 0.0], 0.0, 0.0, 0.0),
        plane([0.0, -1.0, 0.0], [0.0, -1.0, 0.0], 0.0, 0.0, 0.0),
    ];
    let image = render_point_test_scene(&mut renderer, &impossible).unwrap();
    assert!(image.data.chunks_exact(4).all(|pixel| pixel[3] == 0));
    assert!(fields(&renderer, &impossible).is_empty());
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_collision_changes_reset_history_while_render_fields_rewind_and_cold_replay() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let mut scene = deterministic_scene(480, [0.0, 50.0, 0.0], [18.0, -80.0, 0.0]);
    particle_parameters_mut(&mut scene).capacity = 512;
    particle_parameters_mut(&mut scene).collisions =
        vec![plane([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0, 0.8, 0.15)];
    let original = render_point_test_scene(&mut renderer, &scene).unwrap();
    let original_fields = fields(&renderer, &scene);
    let original_stats = stats(&renderer, &scene);
    assert_eq!(original_stats.checkpoint_steps, vec![240, 480]);

    let mut earlier = scene.clone();
    set_particle_step(&mut earlier, 300);
    render_point_test_scene(&mut renderer, &earlier).unwrap();
    let rewind_stats = stats(&renderer, &scene);
    assert_eq!(
        rewind_stats.simulation_generation,
        original_stats.simulation_generation
    );
    assert_eq!(
        rewind_stats.checkpoint_restores,
        original_stats.checkpoint_restores + 1
    );
    assert_eq!(rewind_stats.current_step, 300);
    assert_eq!(rewind_stats.checkpoint_steps, vec![240, 480]);
    assert_eq!(
        render_point_test_scene(&mut renderer, &scene).unwrap().data,
        original.data
    );
    assert_eq!(fields(&renderer, &scene), original_fields);

    let before_field_edit = stats(&renderer, &scene);
    scene.point_program = Some(color_program(Color {
        r: 255,
        g: 80,
        b: 30,
        a: 255,
    }));
    assert_ne!(
        render_point_test_scene(&mut renderer, &scene).unwrap().data,
        original.data
    );
    let field_edit = stats(&renderer, &scene);
    assert_eq!(
        field_edit.simulation_generation,
        before_field_edit.simulation_generation
    );
    assert_eq!(
        field_edit.simulated_steps,
        before_field_edit.simulated_steps
    );
    assert_eq!(field_edit.current_step, before_field_edit.current_step);

    let ParticleCollider::Plane { bounce, .. } =
        &mut particle_parameters_mut(&mut scene).collisions[0]
    else {
        panic!("Plane reset fixture changed collider kind")
    };
    *bounce = 0.35.into();
    render_point_test_scene(&mut renderer, &scene).unwrap();
    let collision_edit = stats(&renderer, &scene);
    assert_ne!(
        collision_edit.simulation_generation,
        field_edit.simulation_generation
    );
    assert_eq!(collision_edit.current_step, 480);
    assert_eq!(collision_edit.simulated_steps, 480);

    let final_pixels = render_point_test_scene(&mut renderer, &scene).unwrap();
    let final_fields = fields(&renderer, &scene);
    let mut cold = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    assert_eq!(
        render_point_test_scene(&mut cold, &scene).unwrap().data,
        final_pixels.data
    );
    assert_eq!(fields(&cold, &scene), final_fields);
}
