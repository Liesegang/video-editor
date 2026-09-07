use super::*;

#[cfg(all(feature = "gl", target_os = "windows"))]
pub(super) fn particle_vec3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
        z: OrderedFloat(z),
    }
}

#[cfg(all(feature = "gl", target_os = "windows"))]
pub(super) fn particle_scene(target_step: u64) -> PointSceneFrame {
    PointSceneFrame {
        point_program: None,
        render_style: Default::default(),
        invocation: SceneInvocationKey {
            instance_path: InstancePath::root(TimelineId::from_uuid(Uuid::from_u128(1))),
            module_instance_id: ModuleInstanceId::from_uuid(Uuid::from_u128(2)),
            state_slot_id: Uuid::from_u128(3),
            output_id: ModuleOutputId::from_uuid(Uuid::from_u128(4)),
        },
        source_node_id: Uuid::from_u128(5),
        color: Color {
            r: 100,
            g: 190,
            b: 255,
            a: 230,
        },
        logical_width: 256,
        logical_height: 144,
        source: PointSceneSource::Particle {
            target_step,
            parameters: ParticleSceneParameters {
                capacity: 1_024,
                emission_rate: OrderedFloat(120.0),
                lifetime_seconds: OrderedFloat(4.0),
                seed: 42,
                emitter_shape: crate::model::frame::particle::ParticleEmitterShape::Point,
                emitter_position: particle_vec3(0.0, 0.0, 0.0),
                emitter_radius: OrderedFloat(0.0),
                emitter_size: particle_vec3(0.0, 0.0, 0.0),
                emitter_surface_only: false,
                velocity_min: particle_vec3(-40.0, -120.0, -20.0),
                velocity_max: particle_vec3(40.0, -80.0, 20.0),
                forces: vec![
                    crate::model::frame::particle::ParticleForce::Gravity {
                        acceleration: particle_vec3(0.0, 100.0, 0.0),
                    },
                    crate::model::frame::particle::ParticleForce::Drag {
                        coefficient: OrderedFloat(0.1),
                    },
                ],
                collisions: Vec::new(),
                size_min: OrderedFloat(4.0),
                size_max: OrderedFloat(10.0),
            },
        },
    }
}

#[cfg(all(feature = "gl", target_os = "windows"))]
pub(super) fn render_point_test_scene(
    renderer: &mut SkiaRenderer,
    scene: &PointSceneFrame,
) -> Result<Image, String> {
    render_point_test_scene_with_transform(renderer, scene, &Affine2D::IDENTITY)
}

#[cfg(all(feature = "gl", target_os = "windows"))]
pub(super) fn render_point_test_scene_with_transform(
    renderer: &mut SkiaRenderer,
    scene: &PointSceneFrame,
    transform: &Affine2D,
) -> Result<Image, String> {
    let output = renderer
        .rasterize_point_layer(PointRasterRequest {
            scene,
            transform,
            sprites: &[],
        })
        .map_err(|error| error.to_string())?;
    match output {
        RenderOutput::Image(image) => Ok(image),
        other => Err(format!("unexpected Particle output {other:?}")),
    }
}

#[cfg(all(feature = "gl", target_os = "windows"))]
pub(super) fn nontransparent_bounds(image: &Image) -> Option<(u32, u32)> {
    let mut min_x = image.width;
    let mut min_y = image.height;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found = false;
    for (index, pixel) in image.data.chunks_exact(4).enumerate() {
        if pixel[3] == 0 {
            continue;
        }
        let index = index as u32;
        let (x, y) = (index % image.width, index / image.width);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
        found = true;
    }
    found.then_some((max_x - min_x + 1, max_y - min_y + 1))
}

#[cfg(all(feature = "gl", target_os = "windows"))]
#[test]
fn cpu_renderer_fails_closed_for_gpu_particle_scene() {
    let mut renderer = SkiaRenderer::new(256, 144, Color::black(), false, None, None).unwrap();
    let error = render_point_test_scene(&mut renderer, &particle_scene(1)).unwrap_err();
    assert!(error.contains("no active GPU context"));
}

/// This exercises a real OpenGL compute/SSBO/FBO path. Keep it opt-in so CI
/// without a GPU and interactive sessions sharing the user's GPU stay safe.
#[cfg(all(feature = "gl", target_os = "windows"))]
#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn gpu_particle_seek_and_independent_renderer_are_deterministic() {
    let transparent = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let mut preview = SkiaRenderer::new(256, 144, transparent.clone(), true, None, None).unwrap();
    let at_checkpoint = particle_scene(240);
    let first = match render_point_test_scene(&mut preview, &at_checkpoint) {
        Ok(image) => image,
        Err(diagnostic) if diagnostic.contains("GPU Particle unavailable") => {
            eprintln!("skipping unsupported device: {diagnostic}");
            return;
        }
        Err(error) => panic!("GPU Particle render failed: {error}"),
    };
    assert!(first.data.iter().any(|component| *component != 0));
    render_point_test_scene(&mut preview, &particle_scene(480)).expect("forward simulation");
    let replayed =
        render_point_test_scene(&mut preview, &at_checkpoint).expect("checkpoint restore");
    assert_eq!(first.data, replayed.data);

    let mut shaped_emitter = particle_scene(64);
    particle_parameters_mut(&mut shaped_emitter).capacity = 64;
    particle_parameters_mut(&mut shaped_emitter).emission_rate = OrderedFloat(120.0);
    particle_parameters_mut(&mut shaped_emitter).velocity_min = particle_vec3(0.0, 0.0, 0.0);
    particle_parameters_mut(&mut shaped_emitter).velocity_max = particle_vec3(0.0, 0.0, 0.0);
    particle_parameters_mut(&mut shaped_emitter).forces.clear();
    particle_parameters_mut(&mut shaped_emitter).size_min = OrderedFloat(4.0);
    particle_parameters_mut(&mut shaped_emitter).size_max = OrderedFloat(4.0);
    shaped_emitter.color = Color::white();
    let point = render_point_test_scene(&mut preview, &shaped_emitter)
        .expect("point-emitter Particle render");
    particle_parameters_mut(&mut shaped_emitter).emitter_shape =
        crate::model::frame::particle::ParticleEmitterShape::Box;
    particle_parameters_mut(&mut shaped_emitter).emitter_size = particle_vec3(120.0, 60.0, 0.0);
    particle_parameters_mut(&mut shaped_emitter).emitter_surface_only = true;
    let box_surface = render_point_test_scene(&mut preview, &shaped_emitter)
        .expect("box-surface Particle render");
    let (point_width, point_height) = nontransparent_bounds(&point).expect("point-emitter pixels");
    let (box_width, box_height) = nontransparent_bounds(&box_surface).expect("box-emitter pixels");
    assert!(
        box_width > point_width * 4 && box_height > point_height * 4,
        "Box birth positions must spread sprites beyond Point bounds: point={point_width}x{point_height}, box={box_width}x{box_height}"
    );

    particle_parameters_mut(&mut shaped_emitter).emitter_shape =
        crate::model::frame::particle::ParticleEmitterShape::Sphere;
    particle_parameters_mut(&mut shaped_emitter).emitter_radius = OrderedFloat(50.0);
    let sphere_surface = render_point_test_scene(&mut preview, &shaped_emitter)
        .expect("sphere-surface Particle render");
    let (sphere_width, sphere_height) =
        nontransparent_bounds(&sphere_surface).expect("sphere-emitter pixels");
    assert!(
        sphere_width > point_width * 4 && sphere_height > point_height * 4,
        "Sphere birth positions must spread sprites beyond Point bounds: point={point_width}x{point_height}, sphere={sphere_width}x{sphere_height}"
    );

    let mut export = SkiaRenderer::new(256, 144, transparent, true, None, None).unwrap();
    let preview_handle = preview
        .gpu_context
        .as_ref()
        .and_then(|context| {
            context.ensure_current().ok()?;
            get_current_context_handle()
        })
        .expect("preview renderer must own a WGL context");
    let export_handle = export
        .gpu_context
        .as_ref()
        .and_then(|context| {
            context.ensure_current().ok()?;
            get_current_context_handle()
        })
        .expect("export renderer must own a WGL context");
    assert_ne!(preview_handle, export_handle);

    // Export construction made a second context current. Preview must reclaim
    // its own context at the Renderer boundary without the caller knowing
    // about GL ownership, then export must be able to do the same in reverse.
    let preview_after_export_creation = render_point_test_scene(&mut preview, &at_checkpoint)
        .expect("preview must reactivate its context after export construction");
    assert_eq!(get_current_context_handle(), Some(preview_handle));
    assert_eq!(first.data, preview_after_export_creation.data);
    let independent =
        render_point_test_scene(&mut export, &at_checkpoint).expect("independent export session");
    assert_eq!(get_current_context_handle(), Some(export_handle));
    assert_eq!(first.data, independent.data);

    let singular = render_point_test_scene_with_transform(
        &mut preview,
        &at_checkpoint,
        &Affine2D::scale(0.0, 0.0),
    )
    .expect("singular Particle transform");
    assert!(
        singular.data.iter().all(|component| *component == 0),
        "a zero-area Particle transform must produce exact transparent pixels"
    );

    let mut translucent_overlap = particle_scene(4);
    particle_parameters_mut(&mut translucent_overlap).capacity = 4;
    particle_parameters_mut(&mut translucent_overlap).emission_rate = OrderedFloat(120.0);
    particle_parameters_mut(&mut translucent_overlap).velocity_min =
        particle_vec3(0.0, 0.0, -120.0);
    particle_parameters_mut(&mut translucent_overlap).velocity_max =
        particle_vec3(0.0, 0.0, -120.0);
    particle_parameters_mut(&mut translucent_overlap)
        .forces
        .clear();
    particle_parameters_mut(&mut translucent_overlap).size_min = OrderedFloat(32.0);
    particle_parameters_mut(&mut translucent_overlap).size_max = OrderedFloat(32.0);
    translucent_overlap.color = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 64,
    };
    let soft_particles = render_point_test_scene(&mut preview, &translucent_overlap)
        .expect("overlapping translucent Particles");
    let center_alpha = soft_particles.data[((72 * 256 + 128) * 4 + 3) as usize];
    assert!(
        center_alpha > 150,
        "all four translucent sprites must composite at center; alpha was {center_alpha}"
    );

    let mut stretched_scene = particle_scene(1);
    particle_parameters_mut(&mut stretched_scene).capacity = 1;
    particle_parameters_mut(&mut stretched_scene).emission_rate = OrderedFloat(120.0);
    particle_parameters_mut(&mut stretched_scene).velocity_min = particle_vec3(0.0, 0.0, 0.0);
    particle_parameters_mut(&mut stretched_scene).velocity_max = particle_vec3(0.0, 0.0, 0.0);
    particle_parameters_mut(&mut stretched_scene).forces.clear();
    particle_parameters_mut(&mut stretched_scene).size_min = OrderedFloat(32.0);
    particle_parameters_mut(&mut stretched_scene).size_max = OrderedFloat(32.0);
    stretched_scene.color = Color::white();
    let centered_non_uniform = Affine2D::translate(128.0, 72.0)
        .compose(Affine2D::scale(4.0, 0.25))
        .compose(Affine2D::translate(-128.0, -72.0));
    let stretched = render_point_test_scene_with_transform(
        &mut preview,
        &stretched_scene,
        &centered_non_uniform,
    )
    .expect("non-uniform Particle transform");
    let (stretched_width, stretched_height) =
        nontransparent_bounds(&stretched).expect("stretched sprite pixels");
    assert!(
        stretched_width > 80 && stretched_height < 20 && stretched_width > stretched_height * 8,
        "Particle quad must follow the full affine; bounds were {stretched_width}x{stretched_height}"
    );

    // Perspective is authored in logical Composition space. Preview quality
    // scaling must only change raster resolution, never the apparent logical
    // size of a particle with non-zero Z.
    let mut perspective_scene = particle_scene(180);
    particle_parameters_mut(&mut perspective_scene).capacity = 1;
    particle_parameters_mut(&mut perspective_scene).emission_rate = OrderedFloat(1.0);
    particle_parameters_mut(&mut perspective_scene).velocity_min = particle_vec3(0.0, 0.0, 144.0);
    particle_parameters_mut(&mut perspective_scene).velocity_max = particle_vec3(0.0, 0.0, 144.0);
    particle_parameters_mut(&mut perspective_scene)
        .forces
        .clear();
    particle_parameters_mut(&mut perspective_scene).size_min = OrderedFloat(48.0);
    particle_parameters_mut(&mut perspective_scene).size_max = OrderedFloat(48.0);
    perspective_scene.color = Color::white();
    let full_resolution = render_point_test_scene(&mut preview, &perspective_scene)
        .expect("full-resolution perspective Particle");
    let mut half_resolution_renderer = SkiaRenderer::new(
        128,
        72,
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        },
        true,
        None,
        None,
    )
    .unwrap();
    let half_resolution = render_point_test_scene_with_transform(
        &mut half_resolution_renderer,
        &perspective_scene,
        &Affine2D::scale(0.5, 0.5),
    )
    .expect("half-resolution perspective Particle");
    let (full_width, full_height) =
        nontransparent_bounds(&full_resolution).expect("full-resolution perspective pixels");
    let (half_width, half_height) =
        nontransparent_bounds(&half_resolution).expect("half-resolution perspective pixels");
    assert!(
        full_width.abs_diff(half_width * 2) <= 4 && full_height.abs_diff(half_height * 2) <= 4,
        "logical-space perspective must be resolution invariant; full={full_width}x{full_height}, half={half_width}x{half_height}"
    );

    // Three minutes is beyond the per-request replay budget and the retained
    // checkpoint window. A cold/direct seek reconstructs only the live
    // lifetime suffix and must equal ordinary sequential playback exactly.
    render_point_test_scene(&mut preview, &particle_scene(7_200)).expect("first minute");
    render_point_test_scene(&mut preview, &particle_scene(14_400)).expect("second minute");
    let sequential_far = render_point_test_scene(&mut preview, &particle_scene(21_600))
        .expect("sequential third minute");
    let direct_far =
        render_point_test_scene(&mut export, &particle_scene(21_600)).expect("bounded direct seek");
    assert_eq!(sequential_far.data, direct_far.data);
    let distant_rewind =
        render_point_test_scene(&mut preview, &at_checkpoint).expect("distant rewind");
    assert_eq!(first.data, distant_rewind.data);

    // Ganesh may leave the borrowed SceneRuntime texture bound after a draw.
    // Replacing that target on resize must never restore its deleted GL name.
    preview
        .resize_render_target(
            320,
            180,
            Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
        )
        .expect("resize Particle renderer");
    let resized = render_point_test_scene(&mut preview, &at_checkpoint)
        .expect("Particle render after target growth");
    assert_eq!((resized.width, resized.height), (320, 180));
    preview
        .resize_render_target(
            256,
            144,
            Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
        )
        .expect("restore Particle renderer size");
    let after_resize = render_point_test_scene(&mut preview, &at_checkpoint)
        .expect("Particle render after target restoration");
    assert_eq!(after_resize.data, first.data);
}

pub(super) fn particle_parameters_mut(scene: &mut PointSceneFrame) -> &mut ParticleSceneParameters {
    let PointSceneSource::Particle { parameters, .. } = &mut scene.source else {
        panic!("expected Particle fixture")
    };
    parameters
}

pub(super) fn set_particle_step(scene: &mut PointSceneFrame, step: u64) {
    let PointSceneSource::Particle { target_step, .. } = &mut scene.source else {
        panic!("expected Particle fixture")
    };
    *target_step = step;
}
