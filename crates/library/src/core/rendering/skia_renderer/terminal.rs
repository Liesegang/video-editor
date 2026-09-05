//! Project-authorized finalization of the existing Skia root surface.

use ruvie_color_management::GpuTerminalChain;

use super::SkiaRenderer;
use crate::error::LibraryError;
use crate::model::frame::Image;

impl SkiaRenderer {
    pub(super) fn validate_finalization(&self) -> Result<(), LibraryError> {
        if !self.group_surfaces.is_empty() {
            return Err(LibraryError::Render(
                "Cannot finalize with unfinished frame groups".to_string(),
            ));
        }
        if !self.retained_group_surfaces.is_empty() {
            return Err(LibraryError::Render(
                "Cannot finalize with unconsumed retained render layers".to_string(),
            ));
        }
        Ok(())
    }

    pub(super) fn finalize_terminal_image(
        &mut self,
        chain: &GpuTerminalChain,
    ) -> Result<Option<Image>, LibraryError> {
        self.activate_graphics_context()?;
        self.validate_finalization()?;
        if self
            .surface_contract
            .working()
            .is_none_or(|contract| contract.identity() != chain.working_identity())
        {
            return Err(LibraryError::Render(
                "GPU terminal chain does not belong to the active Project working surface"
                    .to_string(),
            ));
        }
        #[cfg(not(feature = "gl"))]
        {
            Ok(None)
        }
        #[cfg(feature = "gl")]
        {
            let Some(compute) = self.terminal_compute.as_mut() else {
                return Ok(None);
            };
            if !compute.supports(chain, self.width, self.height) {
                return Ok(None);
            }
            let Some(texture) = skia_safe::gpu::surfaces::get_backend_texture(
                &mut self.surface,
                skia_safe::surface::BackendHandleAccess::FlushRead,
            )
            .and_then(|texture| texture.gl_texture_info()) else {
                return Ok(None);
            };
            if texture.target != glow::TEXTURE_2D {
                return Ok(None);
            }
            let context = self.gpu_context.as_mut().ok_or_else(|| {
                LibraryError::Render("GPU terminal lost its Ganesh owner".to_string())
            })?;
            context.direct_context.flush_and_submit();
            let result = compute.render(chain, texture.id, self.width, self.height);
            // Raw GL is isolated, but Ganesh must also forget cached bindings
            // regardless of success, so subsequent frames/effects stay valid.
            context.direct_context.reset(None);
            result.map(Some)
        }
    }
}
