use super::point_support::{test_gradient, test_random};
use super::*;
use crate::model::frame::particle::ParticleForce;
use crate::model::point::{
    NumericBinaryOperation, PointAttributeDefinition, PointAttributeElementType,
    PointAttributeGpuDefault, PointAttributeId, PointAttributeSchema, PointInstruction,
    PointRenderProgram,
};
use crate::model::property::{GradientSpread, PropertyValue};

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
    particle_parameters_mut(&mut baseline).velocity_min = particle_vec3(-15.0, -25.0, -5.0);
    particle_parameters_mut(&mut baseline).velocity_max = particle_vec3(15.0, 25.0, 5.0);
    particle_parameters_mut(&mut baseline).forces.clear();
    let still = render_point_test_scene(&mut renderer, &baseline).unwrap();
    assert!(still.data.chunks_exact(4).any(|rgba| rgba[3] > 0));
    let turbulence = |strength, frequency, seed| ParticleForce::Turbulence {
        strength: OrderedFloat(strength),
        frequency: OrderedFloat(frequency),
        octaves: 2,
        evolution: OrderedFloat(0.3),
        seed,
    };
    let mut neutral = baseline.clone();
    particle_parameters_mut(&mut neutral)
        .forces
        .push(turbulence(0.0, 0.02, 7));
    assert_eq!(
        render_point_test_scene(&mut renderer, &neutral)
            .unwrap()
            .data,
        still.data,
        "a neutral factory force must not alter existing motion"
    );
    let mut swirling = baseline.clone();
    particle_parameters_mut(&mut swirling).forces = vec![turbulence(80.0, 0.02, 7)];
    let first = render_point_test_scene(&mut renderer, &swirling).unwrap();
    assert_ne!(
        first.data, still.data,
        "turbulence must visibly affect motion"
    );
    assert_eq!(
        render_point_test_scene(&mut renderer, &swirling)
            .unwrap()
            .data,
        first.data
    );
    let mut earlier = swirling.clone();
    set_particle_step(&mut earlier, 70);
    render_point_test_scene(&mut renderer, &earlier).unwrap();
    assert_eq!(
        render_point_test_scene(&mut renderer, &swirling)
            .unwrap()
            .data,
        first.data,
        "backward seek and fixed-step replay must reproduce the same field"
    );
    for changed_force in [turbulence(80.0, 0.04, 7), turbulence(80.0, 0.02, 99)] {
        let mut changed = swirling.clone();
        particle_parameters_mut(&mut changed).forces = vec![changed_force];
        let changed_image = render_point_test_scene(&mut renderer, &changed).unwrap();
        assert_ne!(
            changed_image.data, first.data,
            "field edits must invalidate simulation history"
        );
        assert_eq!(
            render_point_test_scene(&mut renderer, &swirling)
                .unwrap()
                .data,
            first.data
        );
    }
    let mut cold = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    assert_eq!(
        render_point_test_scene(&mut cold, &swirling).unwrap().data,
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
    particle_parameters_mut(&mut baseline).velocity_min = particle_vec3(-15.0, -25.0, -5.0);
    particle_parameters_mut(&mut baseline).velocity_max = particle_vec3(15.0, 25.0, 5.0);
    particle_parameters_mut(&mut baseline).forces.clear();
    let reference = render_point_test_scene(&mut renderer, &baseline).unwrap();
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
        particle_parameters_mut(&mut scene).forces = vec![force];
        let image = render_point_test_scene(&mut renderer, &scene).unwrap();
        assert!(image.data.chunks_exact(4).any(|rgba| rgba[3] > 0));
        assert_ne!(image.data, reference.data);
    }
    let mut ordered = baseline.clone();
    particle_parameters_mut(&mut ordered).forces = vec![
        ParticleForce::Gravity {
            acceleration: particle_vec3(35.0, 60.0, 0.0),
        },
        ParticleForce::Drag {
            coefficient: OrderedFloat(3.0),
        },
    ];
    let first = render_point_test_scene(&mut renderer, &ordered).unwrap();
    particle_parameters_mut(&mut ordered).forces.reverse();
    assert_ne!(
        render_point_test_scene(&mut renderer, &ordered)
            .unwrap()
            .data,
        first.data,
        "authored force order must survive GPU lowering"
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_point_fields_store_math_random_ramp_and_checkpoint_exactly() {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut renderer = SkiaRenderer::new(256, 144, transparent.clone(), true, None, None).unwrap();
    assert!(
        renderer.gpu_context.is_some(),
        "Point field QA requires a real GPU context"
    );
    let attribute = PointAttributeId::from_uuid(Uuid::from_u128(81));
    let schema = PointAttributeSchema::new(vec![
        PointAttributeDefinition::new(
            attribute,
            "normalized age",
            PointAttributeElementType::Number,
            PropertyValue::Number(OrderedFloat(0.0)),
        )
        .unwrap(),
    ])
    .unwrap();
    let ramp = test_gradient(
        GradientSpread::Reflect,
        &[(0.0, Color::black()), (1.0, Color::white())],
    );
    let program = PointRenderProgram {
        schema,
        instructions: vec![
            PointInstruction::NormalizedAge,
            PointInstruction::StoreAttribute {
                attribute: 0,
                value: 0,
            },
            PointInstruction::LoadAttribute { attribute: 0 },
            PointInstruction::Random { channel: 1 },
            PointInstruction::Constant {
                value: PropertyValue::Number(OrderedFloat(0.25)),
            },
            PointInstruction::Binary {
                operation: NumericBinaryOperation::Multiply,
                left: 3,
                right: 4,
            },
            PointInstruction::Binary {
                operation: NumericBinaryOperation::Add,
                left: 2,
                right: 5,
            },
            PointInstruction::ColorRamp {
                gradient: 0,
                factor: 6,
            },
        ],
        ramps: vec![ramp.clone()],
        color_register: 7,
    };
    let mut scene = particle_scene(180);
    scene.executable_hash = [81; 32];
    scene.point_program = Some(program);
    let first = render_point_test_scene(&mut renderer, &scene).unwrap();
    let first_fields = renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&scene.invocation)
        .unwrap();
    assert!(
        first_fields.len() > 8,
        "fixture must contain multiple live points"
    );
    let mut distinct_colors = std::collections::HashSet::new();
    assert_eq!(
        first_fields
            .iter()
            .map(|point| point.serial)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        first_fields.len(),
        "each live point must retain its own exact birth serial"
    );
    let field_seed = crate::rendering::scene_runtime::invocation_seed(&scene);
    for point in &first_fields {
        let normalized_age = (point.age.unwrap() / point.lifetime.unwrap()).clamp(0.0, 1.0);
        let PointAttributeGpuDefault::Number(attribute) = point.attributes[0] else {
            panic!("normalized age readback must remain Number")
        };
        assert!((attribute - normalized_age).abs() <= 2.0e-6);
        let factor = attribute + test_random(field_seed, point.serial, 1) * 0.25;
        let expected = crate::color_management::sample_gradient_at(&ramp, f64::from(factor))
            .unwrap()
            .rgba();
        for (actual, expected) in point.color.iter().zip(expected) {
            assert!((f64::from(*actual) - expected).abs() <= 2.0e-6);
        }
        distinct_colors.insert(point.color[0].to_bits());
    }
    assert!(
        distinct_colors.len() > 4,
        "per-point program must produce varied colors"
    );
    let mut later = scene.clone();
    set_particle_step(&mut later, 480);
    render_point_test_scene(&mut renderer, &later).unwrap();
    assert_eq!(
        render_point_test_scene(&mut renderer, &scene).unwrap().data,
        first.data
    );
    assert_eq!(
        renderer
            .scene_runtime
            .as_ref()
            .unwrap()
            .read_point_fields(&scene.invocation)
            .unwrap(),
        first_fields,
        "checkpoint restore must preserve serials and reproduce derived columns/colors"
    );
    let mut cold = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    assert_eq!(
        render_point_test_scene(&mut cold, &scene).unwrap().data,
        first.data
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
    let first = match render_point_test_scene(&mut renderer, &scene) {
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
    let restored = render_point_test_scene(&mut renderer, &scene)
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
    let replaced = render_point_test_scene(&mut renderer, &scene)
        .expect("new SceneRuntime must use the replacement context");
    assert_eq!(replaced.data, first.data);
}
