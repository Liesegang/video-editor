//! Backend-native bridge between the stateful OpenGL SceneRuntime and Skia.

#[cfg(feature = "gl")]
use skia_safe::Image as SkImage;

use super::SkiaRenderer;
use crate::error::LibraryError;
#[cfg(feature = "gl")]
use crate::rendering::renderer::Affine2D;
use crate::rendering::renderer::{ParticleRasterRequest, RenderOutput};
#[cfg(feature = "gl")]
use crate::rendering::skia_working_surface;

impl SkiaRenderer {
    pub(super) fn preflight_particle_output(
        &mut self,
        target_sizes: &[(u32, u32)],
    ) -> Result<(), LibraryError> {
        #[cfg(not(feature = "gl"))]
        {
            let _ = target_sizes;
            Err(LibraryError::Render(
                "GPU Particle unavailable: library was built without the OpenGL backend"
                    .to_string(),
            ))
        }
        #[cfg(feature = "gl")]
        {
            if target_sizes.is_empty() {
                return Err(LibraryError::Render(
                    "GPU Particle preflight requires at least one reached render target"
                        .to_string(),
                ));
            }
            if !self.group_surfaces.is_empty() || !self.retained_group_surfaces.is_empty() {
                return Err(LibraryError::Render(
                    "GPU Particle preflight requires an idle renderer with no open groups"
                        .to_string(),
                ));
            }
            if self.surface_contract.working().is_none() {
                return Err(LibraryError::Render(
                    "GPU Particle export preflight requires the Project-linear surface contract"
                        .to_string(),
                ));
            }
            self.activate_graphics_context()?;
            let root_is_gpu_backed = skia_safe::gpu::surfaces::get_backend_texture(
                &mut self.surface,
                skia_safe::surface::BackendHandleAccess::FlushRead,
            )
            .is_some();
            if !root_is_gpu_backed {
                let mut replacement = skia_working_surface::create_surface(
                    self.width,
                    self.height,
                    self.gpu_context
                        .as_mut()
                        .map(|context| &mut context.direct_context),
                    &self.surface_contract,
                    true,
                )?;
                skia_working_surface::clear_authored_color(
                    &mut replacement,
                    &self.surface_contract,
                    &self.background_color,
                )?;
                self.surface = replacement;
            }
            // Every subsequent root/layer/group allocation in this renderer
            // now fails closed instead of silently falling back to raster.
            self.require_gpu_surfaces = true;

            let contract = self.surface_contract.clone();
            let (gpu_context, scene_runtime) = (
                self.gpu_context.as_mut().ok_or_else(|| {
                    LibraryError::Render(
                        "GPU Particle preflight has no active Ganesh context".to_string(),
                    )
                })?,
                self.scene_runtime.as_mut().ok_or_else(|| {
                    LibraryError::Render(
                        "GPU Particle preflight has no SceneRuntime for the active context"
                            .to_string(),
                    )
                })?,
            );
            let format =
                skia_working_surface::scene_texture_format(&gpu_context.direct_context, &contract)?;
            for &(width, height) in target_sizes {
                // Ganesh may still sample SceneRuntime's previous target.
                // Submit those commands before raw GL allocates/replaces it.
                gpu_context.direct_context.flush_and_submit();
                let texture = scene_runtime.preflight_particle(width, height, format);
                gpu_context.direct_context.reset(None);
                let texture = texture?;
                let image = skia_working_surface::scene_texture_to_skia_image(
                    &mut gpu_context.direct_context,
                    texture,
                    &contract,
                )?;
                // Exercise the same dimension-specific Ganesh allocation and
                // ingestion path that nested composition/image-transform
                // targets use during the actual export. A 1x1 probe would not
                // catch a late allocation failure for a reached large target.
                let mut ingestion_probe = skia_working_surface::create_surface(
                    width,
                    height,
                    Some(&mut gpu_context.direct_context),
                    &contract,
                    true,
                )?;
                ingestion_probe
                    .canvas()
                    .clear(skia_safe::Color::TRANSPARENT);
                ingestion_probe.canvas().draw_image(&image, (0, 0), None);
                gpu_context.direct_context.flush_and_submit();
            }
            Ok(())
        }
    }

    pub(super) fn rasterize_particle_output(
        &mut self,
        request: ParticleRasterRequest<'_>,
    ) -> Result<RenderOutput, LibraryError> {
        #[cfg(not(feature = "gl"))]
        {
            let _ = request;
            Err(LibraryError::Render(
                "GPU Particle unavailable: library was built without the OpenGL backend"
                    .to_string(),
            ))
        }
        #[cfg(feature = "gl")]
        {
            let (target_width, target_height) = self.current_target_dimensions();
            let scene_image = self.particle_scene_image(request)?;
            let mut layer = self.create_layer_surface(target_width, target_height)?;
            layer.canvas().clear(skia_safe::Color::TRANSPARENT);
            layer.canvas().draw_image(&scene_image, (0, 0), None);
            self.snapshot_surface(&mut layer, target_width, target_height)
        }
    }

    pub(super) fn draw_particle_output(
        &mut self,
        request: ParticleRasterRequest<'_>,
        opacity: f64,
        blend_mode: crate::model::BlendMode,
    ) -> Result<(), LibraryError> {
        #[cfg(not(feature = "gl"))]
        {
            let _ = (request, opacity, blend_mode);
            Err(LibraryError::Render(
                "GPU Particle unavailable: library was built without the OpenGL backend"
                    .to_string(),
            ))
        }
        #[cfg(feature = "gl")]
        {
            let scene_image = self.particle_scene_image(request)?;
            self.draw_skia_image_affine_with_blend(
                &scene_image,
                &Affine2D::IDENTITY,
                opacity,
                blend_mode,
            )
        }
    }

    #[cfg(feature = "gl")]
    fn particle_scene_image(
        &mut self,
        request: ParticleRasterRequest<'_>,
    ) -> Result<SkImage, LibraryError> {
        self.activate_graphics_context()?;
        let (target_width, target_height) = self.current_target_dimensions();
        let premultiplied_color = skia_working_surface::authored_premultiplied_rgba(
            &self.surface_contract,
            &request.scene.parameters.color,
        )?;
        let scene_texture = {
            let gpu_context = self.gpu_context.as_mut().ok_or_else(|| {
                LibraryError::Render(
                    "GPU Particle unavailable: SkiaRenderer has no active GPU context".to_string(),
                )
            })?;
            let format = skia_working_surface::scene_texture_format(
                &gpu_context.direct_context,
                &self.surface_contract,
            )?;
            let scene_runtime = self.scene_runtime.as_mut().ok_or_else(|| {
                LibraryError::Render(
                    "GPU Particle unavailable: SceneRuntime was not created for the active GPU context"
                        .to_string(),
                )
            })?;
            // A previous Skia draw may sample this same SceneRuntime-owned
            // texture. Submit it before raw GL mutates the target again.
            gpu_context.direct_context.flush_and_submit();
            let result = scene_runtime.render_particle(
                request.scene,
                request.transform,
                target_width,
                target_height,
                format,
                premultiplied_color,
            );
            // Raw GL invalidates Ganesh's cached assumptions regardless of
            // whether SceneRuntime returned success.
            gpu_context.direct_context.reset(None);
            result?
        };
        let gpu_context = self.gpu_context.as_mut().ok_or_else(|| {
            LibraryError::Render(
                "GPU Particle lost its GPU context before Ganesh ingestion".to_string(),
            )
        })?;
        skia_working_surface::scene_texture_to_skia_image(
            &mut gpu_context.direct_context,
            scene_texture,
            &self.surface_contract,
        )
    }
}
