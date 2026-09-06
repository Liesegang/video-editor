use super::*;
use crate::model::frame::particle::ParticleForce;

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_particle_force_stack_changes_motion_and_replays_exactly() {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer = SkiaRenderer::new(256, 144, transparent.clone(), true, None, None).unwrap();
    assert!(
        renderer.gpu_context.is_some(),
        "force QA requires a real GPU context"
    );
    let mut baseline = particle_scene(180);
    baseline.parameters.velocity_min = particle_vec3(-15.0, -25.0, -5.0);
    baseline.parameters.velocity_max = particle_vec3(15.0, 25.0, 5.0);
    baseline.parameters.forces.clear();
    let still = render_particle_test_scene(&mut renderer, &baseline).unwrap();
    assert!(still.data.chunks_exact(4).any(|rgba| rgba[3] > 0));
    let turbulence = |strength, frequency, seed| ParticleForce::Turbulence {
        strength: OrderedFloat(strength),
        frequency: OrderedFloat(frequency),
        octaves: 2,
        evolution: OrderedFloat(0.3),
        seed,
    };
    let mut neutral = baseline.clone();
    neutral.parameters.forces.push(turbulence(0.0, 0.02, 7));
    assert_eq!(
        render_particle_test_scene(&mut renderer, &neutral)
            .unwrap()
            .data,
        still.data,
        "a neutral factory force must not alter existing motion"
    );
    let mut swirling = baseline.clone();
    swirling.parameters.forces = vec![turbulence(80.0, 0.02, 7)];
    let first = render_particle_test_scene(&mut renderer, &swirling).unwrap();
    assert_ne!(
        first.data, still.data,
        "turbulence must visibly affect motion"
    );
    assert_eq!(
        render_particle_test_scene(&mut renderer, &swirling)
            .unwrap()
            .data,
        first.data
    );
    let mut earlier = swirling.clone();
    earlier.target_step = 70;
    render_particle_test_scene(&mut renderer, &earlier).unwrap();
    assert_eq!(
        render_particle_test_scene(&mut renderer, &swirling)
            .unwrap()
            .data,
        first.data,
        "backward seek and fixed-step replay must reproduce the same field"
    );
    for changed_force in [turbulence(80.0, 0.04, 7), turbulence(80.0, 0.02, 99)] {
        let mut changed = swirling.clone();
        changed.parameters.forces = vec![changed_force];
        let changed_image = render_particle_test_scene(&mut renderer, &changed).unwrap();
        assert_ne!(
            changed_image.data, first.data,
            "field edits must invalidate simulation history"
        );
        assert_eq!(
            render_particle_test_scene(&mut renderer, &swirling)
                .unwrap()
                .data,
            first.data
        );
    }
    let mut cold = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    assert_eq!(
        render_particle_test_scene(&mut cold, &swirling)
            .unwrap()
            .data,
        first.data,
        "an independent export-style session must reproduce the same simulation"
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_particle_vortex_point_and_force_order_are_executable() {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    assert!(
        renderer.gpu_context.is_some(),
        "force QA requires a real GPU context"
    );
    let mut baseline = particle_scene(180);
    baseline.parameters.velocity_min = particle_vec3(-15.0, -25.0, -5.0);
    baseline.parameters.velocity_max = particle_vec3(15.0, 25.0, 5.0);
    baseline.parameters.forces.clear();
    let reference = render_particle_test_scene(&mut renderer, &baseline).unwrap();
    for force in [
        ParticleForce::Vortex {
            axis: particle_vec3(0.0, 0.0, 1.0),
            center: particle_vec3(20.0, 0.0, 0.0),
            strength: OrderedFloat(60.0),
        },
        ParticleForce::Point {
            target: particle_vec3(40.0, 10.0, 0.0),
            strength: OrderedFloat(80.0),
            radius: OrderedFloat(300.0),
            falloff: OrderedFloat(1.0),
        },
        ParticleForce::Point {
            target: particle_vec3(40.0, 10.0, 0.0),
            strength: OrderedFloat(-80.0),
            radius: OrderedFloat(300.0),
            falloff: OrderedFloat(1.0),
        },
    ] {
        let mut scene = baseline.clone();
        scene.parameters.forces = vec![force];
        let image = render_particle_test_scene(&mut renderer, &scene).unwrap();
        assert!(image.data.chunks_exact(4).any(|rgba| rgba[3] > 0));
        assert_ne!(image.data, reference.data);
    }
    let mut ordered = baseline.clone();
    ordered.parameters.forces = vec![
        ParticleForce::Gravity {
            acceleration: particle_vec3(35.0, 60.0, 0.0),
        },
        ParticleForce::Drag {
            coefficient: OrderedFloat(3.0),
        },
    ];
    let first = render_particle_test_scene(&mut renderer, &ordered).unwrap();
    ordered.parameters.forces.reverse();
    assert_ne!(
        render_particle_test_scene(&mut renderer, &ordered)
            .unwrap()
            .data,
        first.data,
        "authored force order must survive GPU lowering"
    );
}

/// Exercises the transaction used when Preview adopts a newly shared WGL
/// context. Both failure rollback and successful replacement must leave the
/// renderer's owning context current before the next SceneRuntime operation.
#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_render_target_replacement_restores_and_activates_the_owner_context() {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    if renderer.gpu_context.is_none() {
        eprintln!("skipping unsupported device: renderer has no GPU context");
        return;
    }
    let Some(previous_handle) = get_current_context_handle() else {
        panic!("GPU renderer did not leave its WGL context current");
    };
    let scene = particle_scene(240);
    let first = match render_particle_test_scene(&mut renderer, &scene) {
        Ok(image) => image,
        Err(diagnostic) if diagnostic.contains("GPU Particle unavailable") => {
            eprintln!("skipping unsupported device: {diagnostic}");
            return;
        }
        Err(error) => panic!("GPU Particle render failed before replacement: {error}"),
    };

    let Some(rejected_context) = create_gpu_context(None, None) else {
        eprintln!("skipping device unable to create a second GPU context");
        return;
    };
    let rejected = renderer.replace_render_target(Some(rejected_context), Some(91), None, |_| {
        Err(LibraryError::Render(
            "injected GPU replacement failure".to_string(),
        ))
    });
    assert!(rejected.is_err());
    assert_eq!(get_current_context_handle(), Some(previous_handle));
    let restored = render_particle_test_scene(&mut renderer, &scene)
        .expect("old SceneRuntime must remain usable after replacement rollback");
    assert_eq!(restored.data, first.data);

    let Some(mut incoming_context) = create_gpu_context(None, None) else {
        eprintln!("skipping device unable to create a replacement GPU context");
        return;
    };
    incoming_context.resize(256, 144);
    let Some(incoming_handle) = get_current_context_handle() else {
        panic!("replacement WGL context was not current after construction");
    };
    assert_ne!(incoming_handle, previous_handle);
    let contract = renderer.surface_contract.clone();
    renderer
        .replace_render_target(
            Some(incoming_context),
            Some(incoming_handle),
            None,
            move |direct_context| {
                crate::rendering::skia_working_surface::create_surface(
                    256,
                    144,
                    direct_context,
                    &contract,
                    false,
                )
            },
        )
        .expect("GPU target replacement");
    assert_eq!(get_current_context_handle(), Some(incoming_handle));
    let replaced = render_particle_test_scene(&mut renderer, &scene)
        .expect("new SceneRuntime must use the replacement context");
    assert_eq!(replaced.data, first.data);
}
