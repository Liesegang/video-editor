use super::*;

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
