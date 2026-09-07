use super::particle_collision_support::*;
use super::*;
use crate::model::frame::particle::{ParticleCollider, ParticleCollisionMode};

fn sphere(
    center: [f64; 3],
    radius: f32,
    particle_radius: f32,
    mode: ParticleCollisionMode,
    bounce: f32,
    friction: f32,
) -> ParticleCollider {
    ParticleCollider::Sphere {
        center: particle_vec3(center[0], center[1], center[2]),
        radius: radius.into(),
        particle_radius: particle_radius.into(),
        mode,
        bounce: bounce.into(),
        friction: friction.into(),
    }
}

fn plane(point: [f64; 3], normal: [f64; 3], bounce: f32) -> ParticleCollider {
    ParticleCollider::Plane {
        plane_point: particle_vec3(point[0], point[1], point[2]),
        plane_normal: particle_vec3(normal[0], normal[1], normal[2]),
        radius: 0.0.into(),
        bounce: bounce.into(),
        friction: 0.0.into(),
    }
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_sphere_no_hit_preserves_particle_pixels_and_state_exactly() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let baseline = deterministic_scene(120, [0.0, 20.0, 0.0], [15.0, -20.0, 3.0]);
    let baseline_pixels = render_point_test_scene(&mut renderer, &baseline).unwrap();
    let baseline_fields = fields(&renderer, &baseline);
    assert!(!baseline_fields.is_empty());

    let mut no_hit = baseline.clone();
    particle_parameters_mut(&mut no_hit).collisions = vec![sphere(
        [100_000.0, 100_000.0, 0.0],
        10.0,
        2.0,
        ParticleCollisionMode::Solid,
        0.8,
        0.6,
    )];
    assert_eq!(
        render_point_test_scene(&mut renderer, &no_hit)
            .unwrap()
            .data,
        baseline_pixels.data
    );
    assert_eq!(fields(&renderer, &no_hit), baseline_fields);
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_swept_solid_sphere_catches_pass_through_and_applies_particle_radius() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let base = deterministic_scene(2, [-2.0, 0.0, 0.0], [480.0, 0.0, 0.0]);
    let mut endpoint_only = base;
    render_point_test_scene(&mut renderer, &endpoint_only).unwrap();
    assert_vec3_near(
        serial(&fields(&renderer, &endpoint_only), 0)
            .source_position
            .unwrap(),
        [2.0, 0.0, 0.0],
        2.0e-6,
        "unconstrained endpoint",
    );

    for (particle_radius, expected_position) in [(0.0, -2.5), (0.5, -3.25)] {
        particle_parameters_mut(&mut endpoint_only).collisions = vec![sphere(
            [0.0; 3],
            1.0,
            particle_radius,
            ParticleCollisionMode::Solid,
            0.5,
            0.0,
        )];
        render_point_test_scene(&mut renderer, &endpoint_only).unwrap();
        let point = serial(&fields(&renderer, &endpoint_only), 0).clone();
        assert_vec3_near(
            point.source_position.unwrap(),
            [expected_position, 0.0, 0.0],
            4.0e-5,
            "swept Solid position",
        );
        assert_vec3_near(
            point.source_velocity.unwrap(),
            [-240.0, 0.0, 0.0],
            3.0e-5,
            "swept Solid velocity",
        );
        assert_eq!(point.serial, 0);
        assert!((point.age.unwrap() - STEP_SECONDS).abs() <= f32::EPSILON);
    }

    let mut small = deterministic_scene(2, [-0.002, 0.0, 0.0], [0.48, 0.0, 0.0]);
    particle_parameters_mut(&mut small).collisions = vec![sphere(
        [0.0; 3],
        0.001,
        0.0,
        ParticleCollisionMode::Solid,
        0.5,
        0.0,
    )];
    render_point_test_scene(&mut renderer, &small).unwrap();
    let small_point = serial(&fields(&renderer, &small), 0).clone();
    assert_vec3_near(
        small_point.source_position.unwrap(),
        [-0.0025, 0.0, 0.0],
        2.0e-7,
        "small Sphere swept position",
    );
    assert_vec3_near(
        small_point.source_velocity.unwrap(),
        [-0.24, 0.0, 0.0],
        2.0e-7,
        "small Sphere response",
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_sphere_closest_approach_handles_large_distance_and_finite_extreme_velocity() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    for (start, velocity, expected_position, position_relative_tolerance) in [
        (-4_096.0, 1_000_000.0, -4_239.333_5, 2.0e-6),
        (-2.0, 1.0e20, -8.333_334e17, 3.0e-6),
    ] {
        let mut scene = deterministic_scene(2, [start, 0.0, 0.0], [velocity, 0.0, 0.0]);
        particle_parameters_mut(&mut scene).collisions = vec![sphere(
            [0.0; 3],
            1.0,
            0.0,
            ParticleCollisionMode::Solid,
            1.0,
            0.0,
        )];
        render_point_test_scene(&mut renderer, &scene).unwrap();
        let point = serial(&fields(&renderer, &scene), 0).clone();
        let position = point.source_position.unwrap();
        let reflected = point.source_velocity.unwrap();
        assert!(position.into_iter().all(f32::is_finite));
        assert!(reflected.into_iter().all(f32::is_finite));
        assert!(
            (position[0] - expected_position).abs()
                <= expected_position.abs() * position_relative_tolerance,
            "scaled Sphere TOI expected x={expected_position}, got {}",
            position[0]
        );
        assert!(
            (reflected[0] + velocity as f32).abs() <= (velocity as f32).abs() * 2.0e-6,
            "head-on Sphere reflection expected {}, got {}",
            -(velocity as f32),
            reflected[0]
        );
        assert_eq!(reflected[1..], [0.0, 0.0]);
        assert_eq!(point.serial, 0);
        assert!((point.age.unwrap() - STEP_SECONDS).abs() <= f32::EPSILON);
    }
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_container_sphere_uses_inner_radius_and_reflects_outward_motion() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let mut scene = deterministic_scene(2, [0.0; 3], [480.0, 0.0, 0.0]);
    particle_parameters_mut(&mut scene).collisions = vec![sphere(
        [0.0; 3],
        2.0,
        0.5,
        ParticleCollisionMode::Container,
        0.25,
        0.0,
    )];
    render_point_test_scene(&mut renderer, &scene).unwrap();
    let point = serial(&fields(&renderer, &scene), 0).clone();
    assert_vec3_near(
        point.source_position.unwrap(),
        [0.875, 0.0, 0.0],
        3.0e-5,
        "Container remainder",
    );
    assert_vec3_near(
        point.source_velocity.unwrap(),
        [-120.0, 0.0, 0.0],
        3.0e-5,
        "Container reflection",
    );

    let mut tangent = deterministic_scene(2, [0.0, 1.0, 0.0], [480.0, 0.0, 0.0]);
    particle_parameters_mut(&mut tangent).collisions = vec![sphere(
        [0.0; 3],
        1.0,
        0.0,
        ParticleCollisionMode::Container,
        0.5,
        0.25,
    )];
    render_point_test_scene(&mut renderer, &tangent).unwrap();
    let tangent_point = serial(&fields(&renderer, &tangent), 0).clone();
    assert_vec3_near(
        tangent_point.source_position.unwrap(),
        [0.0, 1.0, 0.0],
        2.0e-6,
        "bounded Container tangent",
    );
    assert_vec3_near(
        tangent_point.source_velocity.unwrap(),
        [480.0, 0.0, 0.0],
        2.0e-6,
        "finite Container tangent velocity",
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_sphere_oblique_tangent_miss_and_center_projection_are_deterministic() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let collision = sphere([0.0; 3], 1.0, 0.0, ParticleCollisionMode::Solid, 0.5, 0.25);
    let mut oblique = deterministic_scene(2, [-1.0, 0.0, 0.0], [120.0, 120.0, 0.0]);
    particle_parameters_mut(&mut oblique).collisions = vec![collision.clone()];
    render_point_test_scene(&mut renderer, &oblique).unwrap();
    let oblique_point = serial(&fields(&renderer, &oblique), 0).clone();
    assert_vec3_near(
        oblique_point.source_velocity.unwrap(),
        [-60.0, 90.0, 0.0],
        3.0e-5,
        "oblique response",
    );
    assert_vec3_near(
        oblique_point.source_position.unwrap(),
        [-1.5, 0.75, 0.0],
        3.0e-5,
        "oblique remainder",
    );

    for (position, tangent) in [([-2.0, 1.0, 0.0], true), ([-2.0, 1.1, 0.0], false)] {
        let baseline = deterministic_scene(2, position, [480.0, 0.0, 0.0]);
        render_point_test_scene(&mut renderer, &baseline).unwrap();
        let expected = serial(&fields(&renderer, &baseline), 0).clone();
        let mut constrained = baseline.clone();
        particle_parameters_mut(&mut constrained).collisions = vec![collision.clone()];
        render_point_test_scene(&mut renderer, &constrained).unwrap();
        let actual = serial(&fields(&renderer, &constrained), 0).clone();
        if tangent {
            assert_vec3_near(
                actual.source_position.unwrap(),
                expected.source_position.unwrap(),
                2.0e-5,
                "finite tangent position",
            );
            assert_eq!(actual.source_velocity, expected.source_velocity);
            assert_eq!(actual.serial, expected.serial);
            assert_eq!(actual.age, expected.age);
        } else {
            assert_eq!(actual, expected, "a Sphere miss must remain exact");
        }
    }

    for (velocity, expected_velocity) in [
        ([-120.0, 0.0, 0.0], [60.0, 0.0, 0.0]),
        ([120.0, 0.0, 0.0], [120.0, 0.0, 0.0]),
    ] {
        let mut centered = deterministic_scene(1, [0.0; 3], velocity);
        particle_parameters_mut(&mut centered).collisions = vec![collision.clone()];
        render_point_test_scene(&mut renderer, &centered).unwrap();
        let point = serial(&fields(&renderer, &centered), 0).clone();
        assert_vec3_near(
            point.source_position.unwrap(),
            [1.0, 0.0, 0.0],
            2.0e-6,
            "center projection +X",
        );
        assert_vec3_near(
            point.source_velocity.unwrap(),
            expected_velocity,
            2.0e-6,
            "center response",
        );
        assert_eq!(point.age, Some(0.0));
    }

    let mut incompatible = deterministic_scene(1, [0.0; 3], [0.0; 3]);
    particle_parameters_mut(&mut incompatible).collisions = vec![
        sphere([0.0; 3], 2.0, 0.0, ParticleCollisionMode::Solid, 0.0, 0.0),
        sphere(
            [0.0; 3],
            1.0,
            0.0,
            ParticleCollisionMode::Container,
            0.0,
            0.0,
        ),
    ];
    let image = render_point_test_scene(&mut renderer, &incompatible).unwrap();
    assert!(image.data.chunks_exact(4).all(|pixel| pixel[3] == 0));
    assert!(fields(&renderer, &incompatible).is_empty());
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_mixed_plane_sphere_selects_earliest_contact_independent_of_authored_order() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let first_plane = plane([-1.5, 0.0, 0.0], [-1.0, 0.0, 0.0], 0.5);
    let first_sphere = sphere([0.0; 3], 1.0, 0.0, ParticleCollisionMode::Solid, 0.2, 0.0);
    let mut snapshots = Vec::new();
    for collisions in [
        vec![first_plane.clone(), first_sphere.clone()],
        vec![first_sphere, first_plane],
    ] {
        let mut scene = deterministic_scene(2, [-2.0, 0.0, 0.0], [480.0, 0.0, 0.0]);
        particle_parameters_mut(&mut scene).collisions = collisions;
        render_point_test_scene(&mut renderer, &scene).unwrap();
        snapshots.push(serial(&fields(&renderer, &scene), 0).clone());
    }
    assert_eq!(snapshots[0], snapshots[1]);
    assert_vec3_near(
        snapshots[0].source_position.unwrap(),
        [-3.25, 0.0, 0.0],
        4.0e-5,
        "earliest Plane position",
    );
    assert_vec3_near(
        snapshots[0].source_velocity.unwrap(),
        [-240.0, 0.0, 0.0],
        3.0e-5,
        "earliest Plane velocity",
    );

    let later_plane = plane([1.5, 0.0, 0.0], [-1.0, 0.0, 0.0], 0.5);
    let earlier_sphere = sphere([0.0; 3], 1.0, 0.0, ParticleCollisionMode::Solid, 0.2, 0.0);
    let mut sphere_first = Vec::new();
    for collisions in [
        vec![later_plane.clone(), earlier_sphere.clone()],
        vec![earlier_sphere, later_plane],
    ] {
        let mut scene = deterministic_scene(2, [-2.0, 0.0, 0.0], [480.0, 0.0, 0.0]);
        particle_parameters_mut(&mut scene).collisions = collisions;
        render_point_test_scene(&mut renderer, &scene).unwrap();
        sphere_first.push(serial(&fields(&renderer, &scene), 0).clone());
    }
    assert_eq!(sphere_first[0], sphere_first[1]);
    assert_vec3_near(
        sphere_first[0].source_position.unwrap(),
        [-1.6, 0.0, 0.0],
        4.0e-5,
        "earliest Sphere position",
    );
    assert_vec3_near(
        sphere_first[0].source_velocity.unwrap(),
        [-96.0, 0.0, 0.0],
        3.0e-5,
        "earliest Sphere velocity",
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_sphere_collision_resets_simulation_but_rewind_and_cold_replay_match() {
    let mut renderer = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    let mut scene = deterministic_scene(480, [-30.0, 0.0, 0.0], [80.0, 0.0, 0.0]);
    particle_parameters_mut(&mut scene).capacity = 512;
    particle_parameters_mut(&mut scene).lifetime_seconds = 5.0.into();
    particle_parameters_mut(&mut scene).collisions = vec![sphere(
        [0.0; 3],
        20.0,
        1.0,
        ParticleCollisionMode::Solid,
        0.8,
        0.15,
    )];
    let original = render_point_test_scene(&mut renderer, &scene).unwrap();
    let original_fields = fields(&renderer, &scene);
    let original_stats = stats(&renderer, &scene);
    assert_eq!(original_stats.checkpoint_steps, vec![240, 480]);

    let mut earlier = scene.clone();
    set_particle_step(&mut earlier, 300);
    render_point_test_scene(&mut renderer, &earlier).unwrap();
    let rewind = stats(&renderer, &scene);
    assert_eq!(
        rewind.simulation_generation,
        original_stats.simulation_generation
    );
    assert_eq!(
        rewind.checkpoint_restores,
        original_stats.checkpoint_restores + 1
    );
    assert_eq!(rewind.checkpoint_steps, vec![240, 480]);
    assert_eq!(
        render_point_test_scene(&mut renderer, &scene).unwrap().data,
        original.data
    );
    assert_eq!(fields(&renderer, &scene), original_fields);

    let before_field = stats(&renderer, &scene);
    scene.point_program = Some(color_program(Color {
        r: 240,
        g: 90,
        b: 30,
        a: 255,
    }));
    assert_ne!(
        render_point_test_scene(&mut renderer, &scene).unwrap().data,
        original.data
    );
    let after_field = stats(&renderer, &scene);
    assert_eq!(
        after_field.simulation_generation,
        before_field.simulation_generation
    );
    assert_eq!(after_field.simulated_steps, before_field.simulated_steps);

    let ParticleCollider::Sphere { bounce, .. } =
        &mut particle_parameters_mut(&mut scene).collisions[0]
    else {
        panic!("Sphere fixture changed collider kind")
    };
    *bounce = 0.35.into();
    render_point_test_scene(&mut renderer, &scene).unwrap();
    let after_collision = stats(&renderer, &scene);
    assert_ne!(
        after_collision.simulation_generation,
        after_field.simulation_generation
    );
    assert_eq!(after_collision.current_step, 480);
    assert_eq!(after_collision.simulated_steps, 480);

    let final_pixels = render_point_test_scene(&mut renderer, &scene).unwrap();
    let final_fields = fields(&renderer, &scene);
    let mut cold = SkiaRenderer::new(256, 144, transparent(), true, None, None).unwrap();
    assert_eq!(
        render_point_test_scene(&mut cold, &scene).unwrap().data,
        final_pixels.data
    );
    assert_eq!(fields(&cold, &scene), final_fields);
}
