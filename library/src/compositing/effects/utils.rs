use crate::error::LibraryError;
use crate::graphics::renderer::{RenderOutput, TextureInfo};
use crate::graphics::skia_utils::{GpuContext, image_to_skia, surface_to_image};
use skia_safe::{ImageFilter, Paint};

pub fn apply_skia_filter<F>(
    input: &RenderOutput,
    gpu_context: Option<&mut GpuContext>,
    filter_factory: F,
) -> Result<RenderOutput, LibraryError>
where
    F: Fn(&skia_safe::Image, u32, u32) -> Result<ImageFilter, LibraryError>,
{
    let perform_filter = |image: &skia_safe::Image,
                          width: u32,
                          height: u32,
                          context: Option<&mut skia_safe::gpu::DirectContext>|
     -> Result<RenderOutput, LibraryError> {
        let mut surface = crate::graphics::skia_utils::create_surface(width, height, context)?;
        let canvas = surface.canvas();
        canvas.clear(skia_safe::Color::TRANSPARENT);

        let mut paint = Paint::default();
        let filter = filter_factory(image, width, height)?;
        paint.set_image_filter(filter);
        canvas.draw_image(image, (0, 0), Some(&paint));

        // If we have a context, try to return a texture
        let ctx_opt = surface.recording_context();
        if let Some(mut ctx) = ctx_opt {
            if let Some(mut dctx) = ctx.as_direct_context() {
                dctx.flush_and_submit();
            }

            if let Some(texture) = skia_safe::gpu::surfaces::get_backend_texture(
                &mut surface,
                skia_safe::surface::BackendHandleAccess::FlushRead,
            ) {
                if let Some(gl_info) = texture.gl_texture_info() {
                    return Ok(RenderOutput::Texture(TextureInfo {
                        texture_id: gl_info.id,
                        width,
                        height,
                    }));
                }
            }
        }
        // Fallback to Image
        let image = surface_to_image(&mut surface, width, height)?;
        Ok(RenderOutput::Image(image))
    };

    match input {
        RenderOutput::Texture(info) => {
            if let Some(ctx) = gpu_context {
                let image = crate::graphics::skia_utils::create_image_from_texture(
                    &mut ctx.direct_context,
                    info.texture_id,
                    info.width,
                    info.height,
                )?;
                perform_filter(
                    &image,
                    info.width,
                    info.height,
                    Some(&mut ctx.direct_context),
                )
            } else {
                Err(LibraryError::Render(
                    "Texture input without GPU context".to_string(),
                ))
            }
        }
        RenderOutput::Image(img) => {
            let sk_image = image_to_skia(img)?;
            if let Some(ctx) = gpu_context {
                perform_filter(
                    &sk_image,
                    img.width,
                    img.height,
                    Some(&mut ctx.direct_context),
                )
            } else {
                perform_filter(&sk_image, img.width, img.height, None)
            }
        }
    }
}
