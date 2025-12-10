use crate::error::LibraryError;
use crate::loader::image::Image;
use log::{debug, warn};
use skia_safe::gpu::gl::Interface;
use skia_safe::gpu::{self, DirectContext, SurfaceOrigin, direct_contexts};
use skia_safe::images::raster_from_data;
use skia_safe::surfaces;
use skia_safe::{AlphaType, ColorType, Data, ISize, Image as SkImage, ImageInfo, Surface};

#[cfg(feature = "gl")]
use glutin::PossiblyCurrent;
#[cfg(feature = "gl")]
use glutin::dpi::PhysicalSize;
#[cfg(feature = "gl")]
use glutin::event_loop::EventLoop;
#[cfg(feature = "gl")]
use glutin::{Context, ContextBuilder};

pub struct GpuContext {
    #[cfg(feature = "gl")]
    #[allow(dead_code)]
    event_loop: &'static EventLoop<()>,
    #[cfg(feature = "gl")]
    #[allow(dead_code)]
    gl_context: &'static Context<PossiblyCurrent>,
    pub direct_context: DirectContext,
}

pub fn create_gpu_context() -> Option<GpuContext> {
    #[cfg(feature = "gl")]
    {
        match init_glutin_headless() {
            Ok(ctx) => Some(ctx),
            Err(err) => {
                warn!(
                    "SkiaRenderer: failed to initialize GPU context via glutin: {}",
                    err
                );
                None
            }
        }
    }
    #[cfg(not(feature = "gl"))]
    {
        None
    }
}

#[cfg(feature = "gl")]
fn init_glutin_headless() -> Result<GpuContext, String> {
    let event_loop = Box::leak(Box::new(EventLoop::new()));
    let size = PhysicalSize::new(16, 16);
    let context = ContextBuilder::new()
        .build_headless(event_loop, size)
        .map_err(|e| e.to_string())?;
    let context = Box::leak(Box::new(unsafe {
        context.make_current().map_err(|(_, e)| e.to_string())?
    }));

    let interface =
        Interface::new_native().ok_or_else(|| "Failed to create Skia GL interface".to_string())?;
    let direct_context = direct_contexts::make_gl(interface, None)
        .ok_or_else(|| "Failed to create Skia DirectContext".to_string())?;

    debug!("SkiaRenderer: initialized headless OpenGL context via glutin");

    Ok(GpuContext {
        event_loop,
        gl_context: context,
        direct_context,
    })
}

pub fn create_surface(
    width: u32,
    height: u32,
    context: Option<&mut DirectContext>,
) -> Result<Surface, LibraryError> {
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

pub fn create_raster_surface(width: u32, height: u32) -> Result<Surface, LibraryError> {
    let info = ImageInfo::new_n32_premul((width as i32, height as i32), None);
    surfaces::raster(&info, None, None)
        .ok_or_else(|| LibraryError::Render("Cannot create Skia surface".to_string()))
}

pub fn image_to_skia(image: &Image) -> Result<SkImage, LibraryError> {
    let info = ImageInfo::new(
        ISize::new(image.width as i32, image.height as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let sk_data = Data::new_copy(image.data.as_slice());
    raster_from_data(&info, sk_data, (image.width * 4) as usize)
        .ok_or_else(|| LibraryError::Render("Failed to create Skia image".to_string()))
}

pub fn surface_to_image(
    surface: &mut Surface,
    width: u32,
    height: u32,
) -> Result<Image, LibraryError> {
    let row_bytes = (width * 4) as usize;
    let mut buffer = vec![0u8; (height as usize) * row_bytes];
    let image_info = ImageInfo::new(
        ISize::new(width as i32, height as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    if !surface.read_pixels(&image_info, &mut buffer, row_bytes, (0, 0)) {
        return Err(LibraryError::Render(
            "Failed to read surface pixels".to_string(),
        ));
    }
    Ok(Image {
        width,
        height,
        data: buffer,
    })
}
