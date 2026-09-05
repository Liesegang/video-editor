//! Ownership-safe replacement and destruction of renderer GL contexts.

use skia_safe::Surface;

#[cfg(feature = "gl")]
use super::SceneRuntime;
use super::SkiaRenderer;
use crate::error::LibraryError;
use crate::rendering::skia_utils::GpuContext;
use crate::rendering::skia_working_surface;

impl SkiaRenderer {
    /// Activate the graphics context which owns this renderer's Ganesh and
    /// raw-GL resources. CPU renderers intentionally treat this as a no-op.
    ///
    /// A thread may alternate between Preview, export, and shared-context
    /// renderers. `PossiblyCurrentContext` describes the context's capability,
    /// not which renderer currently owns the thread, so every GPU entry point
    /// must cross this boundary before touching a Surface, DirectContext, or
    /// SceneRuntime.
    pub(super) fn activate_graphics_context(&self) -> Result<(), LibraryError> {
        if let Some(context) = self.gpu_context.as_ref() {
            context.ensure_current()?;
        }
        Ok(())
    }

    pub(super) fn replace_render_target(
        &mut self,
        mut incoming_context: Option<GpuContext>,
        sharing_handle: Option<usize>,
        sharing_hwnd: Option<isize>,
        create: impl FnOnce(Option<&mut skia_safe::gpu::DirectContext>) -> Result<Surface, LibraryError>,
    ) -> Result<(), LibraryError> {
        // Do not depend on context construction having happened immediately
        // before replacement: another renderer may have claimed this thread
        // in between. Stage the new root surface under its actual owner.
        if let Some(context) = incoming_context.as_ref()
            && let Err(error) = context.ensure_current()
        {
            return Err(self.discard_incoming_and_restore(incoming_context, error));
        }
        let mut incoming_surface = match create(
            incoming_context
                .as_mut()
                .map(|context| &mut context.direct_context),
        ) {
            Ok(surface) => surface,
            Err(error) => {
                return Err(self.discard_incoming_and_restore(incoming_context, error));
            }
        };
        if let Err(error) = skia_working_surface::clear_authored_color(
            &mut incoming_surface,
            &self.surface_contract,
            &self.background_color,
        ) {
            // Surface destruction must precede destruction of its context.
            drop(incoming_surface);
            return Err(self.discard_incoming_and_restore(incoming_context, error));
        }

        if let Some(previous_context) = self.gpu_context.as_ref()
            && let Err(error) = previous_context.ensure_current()
        {
            // The staged surface still belongs to the incoming context, which
            // remains current when activating the previous context failed.
            drop(incoming_surface);
            return Err(self.discard_incoming_and_restore(incoming_context, error));
        }

        // Every old-context resource must die while that context and its
        // Ganesh owner are still live and current. SceneRuntime goes first
        // because its Drop issues raw glDelete calls.
        #[cfg(feature = "gl")]
        {
            self.scene_runtime = None;
        }
        self.group_surfaces.clear();
        self.retained_group_surfaces.clear();
        let previous_surface = std::mem::replace(&mut self.surface, incoming_surface);
        drop(previous_surface);
        let previous_context = self.gpu_context.take();
        drop(previous_context);

        // Dropping the previous GpuContext explicitly unbound it. Install and
        // reactivate the incoming owner before constructing SceneRuntime or
        // allowing any Skia command to touch the staged surface.
        self.gpu_context = incoming_context;
        self.sharing_handle = sharing_handle;
        self.sharing_hwnd = sharing_hwnd;
        if let Some(incoming_context) = self.gpu_context.as_ref() {
            incoming_context.ensure_current()?;
        }
        #[cfg(feature = "gl")]
        {
            self.scene_runtime = self
                .gpu_context
                .as_ref()
                .map(|context| SceneRuntime::new(context.create_glow_context()));
        }
        Ok(())
    }

    fn discard_incoming_and_restore(
        &self,
        incoming_context: Option<GpuContext>,
        replacement_error: LibraryError,
    ) -> LibraryError {
        // GpuContext Drop releases and unbinds a current incoming context.
        drop(incoming_context);
        if let Some(previous_context) = self.gpu_context.as_ref()
            && let Err(restore_error) = previous_context.ensure_current()
        {
            return LibraryError::Render(format!(
                "{replacement_error}; additionally failed to restore the previous OpenGL context: {restore_error}"
            ));
        }
        replacement_error
    }
}

impl Drop for SkiaRenderer {
    fn drop(&mut self) {
        // Fields are destroyed in declaration order after this method. Make
        // the renderer's context current before Surface/SceneRuntime drops;
        // GpuContext drops last and performs Ganesh release plus GL unbind.
        if let Some(context) = self.gpu_context.as_ref()
            && let Err(error) = context.ensure_current()
        {
            log::warn!("SkiaRenderer: failed to activate its context for destruction: {error}");
        }
    }
}
