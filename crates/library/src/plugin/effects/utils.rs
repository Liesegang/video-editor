use crate::error::LibraryError;
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::{GpuContext, image_to_skia, surface_to_image};
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
        let mut surface = crate::rendering::skia_utils::create_surface(width, height, context)?;
        let canvas = surface.canvas();
        canvas.clear(skia_safe::Color::TRANSPARENT);

        let mut paint = Paint::default();
        let filter = filter_factory(image, width, height)?;
        paint.set_image_filter(filter);
        canvas.draw_image(image, (0, 0), Some(&paint));

        // A backend texture borrowed from this local Surface becomes invalid
        // as soon as the function returns and drops the Surface. Keep this
        // boundary owned until RenderOutput gains an owner-bearing GPU frame.
        // Returning a copied Image is slower, but cannot expose a dangling GL
        // texture ID to the next effect or Preview.
        let image = surface_to_image(&mut surface, width, height)?;
        Ok(RenderOutput::Image(image))
    };

    match input {
        RenderOutput::Working(working) => {
            let image = crate::rendering::skia_working_surface::linear_working_to_skia_image(
                working.pixels(),
            )?;
            let width = working.pixels().width();
            let height = working.pixels().height();
            let mut surface = crate::rendering::skia_working_surface::create_working_surface(
                width,
                height,
                gpu_context.map(|context| &mut context.direct_context),
            )?;
            surface
                .canvas()
                .clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));
            let mut paint = Paint::default();
            paint.set_image_filter(filter_factory(&image, width, height)?);
            surface.canvas().draw_image(&image, (0, 0), Some(&paint));
            let pixels = crate::rendering::skia_working_surface::surface_to_linear_working(
                &mut surface,
                width,
                height,
            )?;
            // SAFETY: This spatial Skia filter path uses a linear RGBAF32
            // surface, introduces no authored color and applies no color
            // transform. The PluginManager admits only effects which declare
            // this exact preserving contract and verifies the returned token.
            let output = unsafe {
                ruvie_color_management::ManagedLinearWorkingImage::from_working_pixels_unchecked(
                    working.identity().clone(),
                    pixels,
                )
            };
            Ok(RenderOutput::Working(output))
        }
        RenderOutput::Texture(info) => {
            if let Some(ctx) = gpu_context {
                let image = crate::rendering::skia_utils::create_image_from_texture(
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
