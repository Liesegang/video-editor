use super::*;

#[test]
fn construction_returns_an_error_for_invalid_dimensions() {
    let result = SkiaRenderer::new(0, 0, Color::black(), false, None, None);
    assert!(matches!(result, Err(LibraryError::Render(_))));
}

#[test]
fn failed_render_target_replacement_preserves_the_current_surface() {
    let mut renderer = SkiaRenderer::new(2, 2, Color::black(), false, None, None).unwrap();
    let result = renderer.replace_render_target(None, Some(99), Some(77), |_| {
        Err(LibraryError::Render(
            "injected surface creation failure".to_string(),
        ))
    });

    assert!(matches!(result, Err(LibraryError::Render(_))));
    assert_eq!(renderer.sharing_handle, None);
    assert_eq!(renderer.sharing_hwnd, None);
    renderer.clear().unwrap();
    let RenderOutput::Image(image) = renderer.finalize().unwrap() else {
        panic!("CPU renderer must retain its image surface");
    };
    assert_eq!((image.width, image.height), (2, 2));
}
