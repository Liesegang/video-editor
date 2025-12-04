use crate::loader::image::Image;
use skia_safe::gpu::{self, DirectContext, SurfaceOrigin};
use skia_safe::image::CachingHint;
use skia_safe::images::raster_from_data;
use skia_safe::surfaces;
use skia_safe::{AlphaType, ColorType, Data, ISize, Image as SkImage, ImageInfo, Surface};
use std::error::Error;

#[cfg(feature = "gl")]
fn create_gl_context() -> Option<DirectContext> {
    use skia_safe::gpu::gl;
    unsafe { gl::Interface::new_native() }
        .and_then(|interface| gpu::DirectContext::new_gl(interface, None))
}

#[cfg(not(feature = "gl"))]
fn create_gl_context() -> Option<DirectContext> {
    None
}

pub fn create_gpu_context() -> Option<DirectContext> {
    create_gl_context()
}

pub fn create_surface(
    width: u32,
    height: u32,
    context: Option<&mut DirectContext>,
) -> Result<Surface, Box<dyn Error>> {
    if let Some(ctx) = context {
        if let Some(surface) = gpu::surfaces::render_target(
            ctx,
            gpu::Budgeted::Yes,
            &ImageInfo::new_n32_premul((width as i32, height as i32), None),
            None,
            SurfaceOrigin::TopLeft,
            None,
            false,
            false,
        ) {
            return Ok(surface);
        }
    }
    create_raster_surface(width, height)
}

pub fn create_raster_surface(width: u32, height: u32) -> Result<Surface, Box<dyn Error>> {
    let info = ImageInfo::new_n32_premul((width as i32, height as i32), None);
    surfaces::raster(&info, None, None).ok_or_else(|| "Cannot create Skia surface".into())
}

pub fn image_to_skia(image: &Image) -> Result<SkImage, Box<dyn Error>> {
    let info = ImageInfo::new(
        ISize::new(image.width as i32, image.height as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let sk_data = Data::new_copy(image.data.as_slice());
    raster_from_data(&info, sk_data, (image.width * 4) as usize)
        .ok_or_else(|| "Failed to create Skia image".into())
}

pub fn surface_to_image(
    surface: &mut Surface,
    width: u32,
    height: u32,
) -> Result<Image, Box<dyn Error>> {
    let snapshot = surface.image_snapshot();
    let row_bytes = (width * 4) as usize;
    let mut buffer = vec![0u8; (height as usize) * row_bytes];
    let image_info = ImageInfo::new(
        ISize::new(width as i32, height as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    if !snapshot.read_pixels(
        &image_info,
        &mut buffer,
        row_bytes,
        (0, 0),
        CachingHint::Allow,
    ) {
        return Err("Failed to read surface pixels".into());
    }
    Ok(Image {
        width,
        height,
        data: buffer,
    })
}
