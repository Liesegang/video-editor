use crate::error::LibraryError;
use crate::io::image::Image;
#[cfg(all(feature = "gl", target_os = "windows"))]
use log::{debug, warn};
#[cfg(all(feature = "gl", target_os = "windows"))]
use skia_safe::gpu::direct_contexts;
#[cfg(all(feature = "gl", target_os = "windows"))]
use skia_safe::gpu::gl::Interface;
use skia_safe::gpu::{self, DirectContext, SurfaceOrigin};
use skia_safe::images::raster_from_data;
use skia_safe::surfaces;
use skia_safe::{AlphaType, ColorType, Data, ISize, Image as SkImage, ImageInfo, Surface};

#[cfg(all(feature = "gl", target_os = "windows"))]
use glutin::config::ConfigSurfaceTypes;
#[cfg(all(feature = "gl", target_os = "windows"))]
use glutin::context::ContextAttributesBuilder;
#[cfg(feature = "gl")]
use glutin::prelude::*;
#[cfg(feature = "gl")]
use glutin::surface::WindowSurface;
#[cfg(all(feature = "gl", target_os = "windows"))]
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle,
};

#[cfg(all(feature = "gl", target_os = "windows"))]
#[cfg(feature = "gl")]
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
#[cfg(all(feature = "gl", target_os = "windows"))]
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(all(feature = "gl", target_os = "windows"))]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_OWNDC, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, HWND_MESSAGE, RegisterClassExW,
    WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};

#[cfg(all(feature = "gl", target_os = "windows"))]
use windows_sys::Win32::Graphics::OpenGL::{HGLRC, wglShareLists};

pub struct GpuContext {
    pub(crate) _display: glutin::display::Display,
    pub(crate) _surface: glutin::surface::Surface<WindowSurface>,
    pub context: glutin::context::PossiblyCurrentContext,
    pub direct_context: skia_safe::gpu::DirectContext,
    pub(crate) _hwnd: usize,
}

impl GpuContext {
    pub fn resize(&mut self, width: u32, height: u32) {
        self._surface.resize(
            &self.context,
            std::num::NonZeroU32::new(width.max(1)).unwrap(),
            std::num::NonZeroU32::new(height.max(1)).unwrap(),
        );
    }
}

pub fn create_gpu_context(share_handle: Option<usize>) -> Option<GpuContext> {
    #[cfg(all(feature = "gl", target_os = "windows"))]
    {
        match init_glutin_headless(share_handle) {
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
    #[cfg(all(feature = "gl", not(target_os = "windows")))]
    {
        None
    }
    #[cfg(not(feature = "gl"))]
    {
        None
    }
}

pub fn get_current_context_handle() -> Option<usize> {
    #[cfg(all(feature = "gl", target_os = "windows"))]
    unsafe {
        let handle = windows_sys::Win32::Graphics::OpenGL::wglGetCurrentContext();
        if !handle.is_null() {
            Some(handle as usize)
        } else {
            None
        }
    }
    #[cfg(not(all(feature = "gl", target_os = "windows")))]
    None
}

#[cfg(all(feature = "gl", target_os = "windows"))]
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(all(feature = "gl", target_os = "windows"))]
fn create_dummy_window() -> Result<RawWindowHandle, String> {
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class_name = "VideoEditorDummyClass\0"
            .encode_utf16()
            .collect::<Vec<u16>>();

        let wnd_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_OWNDC,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };

        RegisterClassExW(&wnd_class);

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );

        if hwnd.is_null() {
            return Err("Failed to create dummy window".to_string());
        }

        let mut handle =
            Win32WindowHandle::new(std::num::NonZeroIsize::new(hwnd as isize).unwrap());
        handle.hinstance = std::num::NonZeroIsize::new(hinstance as isize);

        Ok(RawWindowHandle::Win32(handle))
    }
}

#[cfg(all(feature = "gl", target_os = "windows"))]
fn init_glutin_headless(share_handle: Option<usize>) -> Result<GpuContext, String> {
    // 1. Create Dummy Window
    let raw_window_handle = create_dummy_window()?;
    let hwnd = match raw_window_handle {
        RawWindowHandle::Win32(h) => h.hwnd.get() as usize,
        _ => return Err("Invalid window handle type".to_string()),
    };

    // 2. Create Display
    // We can use the window handle to create display?
    // Or just empty display handle. Glutin works with empty.
    let raw_display_handle = RawDisplayHandle::Windows(WindowsDisplayHandle::new());
    let display = unsafe {
        glutin::display::Display::new(
            raw_display_handle,
            glutin::display::DisplayApiPreference::Wgl(None),
        )
    }
    .map_err(|e| format!("Display creation failed: {}", e))?;

    // 3. Find Config (WINDOW)
    let template = glutin::config::ConfigTemplateBuilder::new()
        .with_surface_type(ConfigSurfaceTypes::WINDOW)
        .build();

    let config = unsafe { display.find_configs(template) }
        .map_err(|e| format!("Failed to find configs: {}", e))?
        .reduce(|accum, config| {
            if config.num_samples() == 0 && accum.num_samples() > 0 {
                return config;
            }
            if config.num_samples() > 0 && accum.num_samples() == 0 {
                return accum;
            }
            if config.alpha_size() > accum.alpha_size() {
                config
            } else {
                accum
            }
        })
        .ok_or("No matching GL config found")?;

    debug!(
        "init_glutin_headless: Selected config. Alpha: {}, Samples: {}",
        config.alpha_size(),
        config.num_samples()
    );

    // 4. Create Context
    // Pass window handle
    let context_attributes = ContextAttributesBuilder::new()
        .with_context_api(glutin::context::ContextApi::OpenGl(None))
        .build(Some(raw_window_handle));

    let not_current_context = unsafe { display.create_context(&config, &context_attributes) }
        .map_err(|e| format!("Failed to create GL context: {}", e))?;

    // 5. Create Window Surface
    let attrs = glutin::surface::SurfaceAttributesBuilder::<WindowSurface>::new().build(
        raw_window_handle,
        std::num::NonZeroU32::new(1920).unwrap(), // Initial Size
        std::num::NonZeroU32::new(1080).unwrap(),
    );

    let surface = unsafe { display.create_window_surface(&config, &attrs) }
        .map_err(|e| format!("Failed to create window surface: {}", e))?;

    // 6. Make Current
    let context = not_current_context
        .make_current(&surface)
        .map_err(|e| format!("Make current failed: {}", e))?;

    // 7. Share Lists (Context Sharing)
    if let Some(share_hglrc) = share_handle {
        // We need the raw HGLRC of our new context.
        // Glutin's PossiblyCurrentContext doesn't easily expose raw handle in safe API.
        // But we are on Windows/WGL.
        // We can use wglGetCurrentContext right now because we just made it current!
        unsafe {
            let my_hglrc = windows_sys::Win32::Graphics::OpenGL::wglGetCurrentContext();
            if my_hglrc.is_null() {
                warn!("SkiaRenderer: wglGetCurrentContext returned null after make_current");
            } else {
                let success = wglShareLists(share_hglrc as HGLRC, my_hglrc);
                if success == 0 {
                    warn!("SkiaRenderer: wglShareLists failed! Sharing might not work.");
                } else {
                    debug!("SkiaRenderer: wglShareLists success! Contexts shared.");
                }
            }
        }
    }

    let interface = Interface::new_native().ok_or("Failed to create native interface")?;
    let direct_context =
        direct_contexts::make_gl(interface, None).ok_or("Failed to create DirectContext")?;

    Ok(GpuContext {
        _display: display,
        _surface: surface,
        context,
        direct_context,
        _hwnd: hwnd,
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

pub fn create_texture_surface(
    width: u32,
    height: u32,
    context: &mut DirectContext,
) -> Result<Surface, LibraryError> {
    let info = ImageInfo::new_n32_premul((width as i32, height as i32), None);
    gpu::surfaces::render_target(
        context,
        gpu::Budgeted::Yes,
        &info,
        None,
        SurfaceOrigin::TopLeft,
        None,
        false,
        false,
    )
    .ok_or_else(|| LibraryError::Render("Cannot create buffer Skia surface".to_string()))
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
pub fn create_image_from_texture(
    context: &mut DirectContext,
    texture_id: u32,
    width: u32,
    height: u32,
) -> Result<SkImage, LibraryError> {
    #[cfg(feature = "gl")]
    {
        let texture_info = skia_safe::gpu::gl::TextureInfo {
            target: 0x0DE1, // GL_TEXTURE_2D
            id: texture_id,
            format: 0x8058, // GL_RGBA8
            protected: skia_safe::gpu::Protected::No,
        };
        let backend_texture = unsafe {
            skia_safe::gpu::backend_textures::make_gl(
                (width as i32, height as i32),
                skia_safe::gpu::Mipmapped::No,
                texture_info,
                "Texture",
            )
        };

        SkImage::from_texture(
            context,
            &backend_texture,
            SurfaceOrigin::TopLeft, // Standard for us?
            ColorType::RGBA8888,
            AlphaType::Premul,
            None,
        )
        .ok_or(LibraryError::Render(
            "Failed to create image from texture".to_string(),
        ))
    }
    #[cfg(not(feature = "gl"))]
    {
        Err(LibraryError::Render("GL feature not enabled".to_string()))
    }
}

pub fn get_available_fonts() -> Vec<String> {
    let font_mgr = skia_safe::FontMgr::default();
    let count = font_mgr.count_families();
    let mut families = Vec::with_capacity(count);

    for i in 0..count {
        let name = font_mgr.family_name(i);
        families.push(name);
    }

    families.sort();
    families.dedup();
    families
}
