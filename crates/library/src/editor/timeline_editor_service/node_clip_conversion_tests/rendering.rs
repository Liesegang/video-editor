use super::*;

pub(in crate::editor::timeline_editor_service) fn rendered_pixels(
    project: &AuthoringProject,
    plugins: Arc<PluginManager>,
    frame_number: u64,
) -> Vec<u8> {
    rendered_pixels_at_scale(project, plugins, frame_number, 1.0)
}

pub(in crate::editor::timeline_editor_service) fn rendered_pixels_at_scale(
    project: &AuthoringProject,
    plugins: Arc<PluginManager>,
    frame_number: u64,
    render_scale: f64,
) -> Vec<u8> {
    let timeline = &project.timelines[&project.root_timeline_id];
    let plan = RenderPlanCompiler::compile(project).expect("RenderPlan");
    let frame = evaluate_render_plan_frame(
        project,
        &plan,
        plugins.as_ref(),
        frame_number,
        render_scale,
        None,
    )
    .expect("evaluated frame");
    assert!(contains_visible_content(&frame.items));
    let cache = Arc::new(CacheManager::new());
    let scaled = |value: u64| {
        u32::try_from((value as f64 * render_scale).round() as i64)
            .expect("positive scaled render dimension")
            .max(1)
    };
    let renderer = SkiaRenderer::new(
        scaled(timeline.width),
        scaled(timeline.height),
        timeline.background_color.clone(),
        false,
        None,
        Some(Arc::clone(&cache)),
    )
    .expect("CPU renderer");
    let mut render_service = RenderService::new(renderer, plugins, cache);
    let RenderOutput::Image(image) = render_service
        .render_authoring_frame(project, &frame, RenderDestination::Preview)
        .expect("authoring frame")
    else {
        panic!("Preview must be an Image");
    };
    image.data
}

fn contains_visible_content(items: &[FrameItem]) -> bool {
    items.iter().any(|item| match item {
        FrameItem::Object(_) => true,
        FrameItem::Group(group) => contains_visible_content(&group.items),
        FrameItem::Transition(transition) => {
            contains_visible_content(std::slice::from_ref(&transition.from.item))
                || contains_visible_content(std::slice::from_ref(&transition.to.item))
        }
    })
}
