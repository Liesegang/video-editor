//! Production rendering for first-class Timeline image transitions.

use super::*;

impl<T: Renderer> RenderService<T> {
    pub(super) fn render_transition(
        &mut self,
        transition: &FrameTransition,
        parent_context: &RenderContext,
        color_authority: &RenderColorAuthority<'_>,
    ) -> Result<(), LibraryError> {
        let from = self.render_transition_source(
            transition,
            &transition.from,
            parent_context,
            color_authority,
        )?;
        let to = match self.render_transition_source(
            transition,
            &transition.to,
            parent_context,
            color_authority,
        ) {
            Ok(to) => to,
            Err(error) => {
                if let Err(cleanup_error) = self.renderer.release_retained_layer(from) {
                    log::error!(
                        "failed to release retained Transition source after an evaluation error: {cleanup_error}"
                    );
                }
                return Err(error);
            }
        };
        match transition.kind {
            FrameTransitionKind::CrossDissolve => self.renderer.draw_cross_dissolve_retained(
                from,
                to,
                transition.progress.as_f32(),
                transition.blend_mode,
            ),
        }
    }

    fn render_transition_source(
        &mut self,
        transition: &FrameTransition,
        source: &FrameTransitionSource,
        context: &RenderContext,
        color_authority: &RenderColorAuthority<'_>,
    ) -> Result<RetainedRenderLayer, LibraryError> {
        self.renderer.begin_group(
            context.target_width,
            context.target_height,
            &transparent_color(),
        )?;
        let children_result = self.render_items(
            std::slice::from_ref(&source.item),
            context,
            // Each source invocation already carries its own evaluated local
            // time on its Clip/group subtree. Transition progress is a
            // separate coordinate and must not become effect time.
            0.0,
            color_authority,
        );
        if let Err(error) = children_result {
            // Restore the renderer's group stack before returning the source
            // diagnostic. The discarded snapshot only occurs on this error
            // path; successful Preview frames stay backend-native.
            if let Err(cleanup_error) = self.renderer.end_group() {
                log::error!(
                    "failed to close Transition source group after an evaluation error: {cleanup_error}"
                );
            }
            return Err(match error {
                error @ LibraryError::TransitionSourceHandleUnavailable(_) => error,
                error @ (LibraryError::VideoFrameOutOfRange { .. }
                | LibraryError::VideoTimestampOutOfRange { .. }) => TransitionSourceHandleError {
                    transition_id: transition.transition_id,
                    item_id: source.item_id,
                    timeline_time: transition.timeline_time.into_inner(),
                    source_time: source.source_time.into_inner(),
                    reason: error.to_string(),
                }
                .into(),
                error => error,
            });
        }
        self.renderer.end_group_retained()
    }
}
