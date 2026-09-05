//! Skia storage adapter for legacy and Project-linear surface contracts.
//!
//! Project surfaces, transient layers, and isolated groups use premultiplied
//! floating-point storage. Raster surfaces retain RGBAF32; Ganesh selects
//! RGBAF32 or RGBAF16 according to the real device render-target support and
//! converts the terminal readback to the authoritative RGBAF32 working image.
//! The exact Project color identity travels beside the pixels; Skia's linear
//! color-space tag prevents encoded-light resampling and blending but is not
//! used as a replacement for Project/OCIO authority.

use bytemuck::{cast_slice, cast_slice_mut};
use ruvie_color_management::{LinearWorkingImage, ManagedLinearWorkingImage};
use skia_safe::{
    AlphaType, Color4f, ColorSpace, ColorType, Data, ISize, Image, ImageInfo, Paint, Surface, gpu,
    images, surfaces,
};

use crate::error::LibraryError;
use crate::model::frame::color::Color;
#[cfg(feature = "gl")]
use crate::rendering::gl_resources::SavedGlState;
use crate::rendering::renderer::{RenderOutput, WorkingSurfaceContract};
#[cfg(feature = "gl")]
use crate::rendering::scene_runtime::{SceneTexture, SceneTextureFormat};
use crate::rendering::skia_utils::{self, GpuContext};

const F32_COMPONENT_BYTES: usize = std::mem::size_of::<f32>();
const RGBA_COMPONENTS: usize = 4;
const MAX_WORKING_SURFACE_BYTES: usize = 512 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(super) enum SkiaSurfaceContract {
    UnmanagedSrgba8,
    ProjectLinear(Box<WorkingSurfaceContract>),
}

impl SkiaSurfaceContract {
    pub(super) fn working(&self) -> Option<&WorkingSurfaceContract> {
        match self {
            Self::UnmanagedSrgba8 => None,
            Self::ProjectLinear(contract) => Some(contract),
        }
    }

    pub(super) fn same_storage_contract(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::UnmanagedSrgba8, Self::UnmanagedSrgba8) => true,
            (Self::ProjectLinear(left), Self::ProjectLinear(right)) => {
                left.identity() == right.identity()
            }
            _ => false,
        }
    }
}

pub(super) fn create_surface(
    width: u32,
    height: u32,
    context: Option<&mut skia_safe::gpu::DirectContext>,
    contract: &SkiaSurfaceContract,
    require_gpu_backing: bool,
) -> Result<Surface, LibraryError> {
    if require_gpu_backing {
        let context = context.ok_or_else(|| {
            LibraryError::Render(
                "GPU Particle requires a GPU-backed Skia surface, but no Ganesh context is active"
                    .to_string(),
            )
        })?;
        return match contract {
            SkiaSurfaceContract::UnmanagedSrgba8 => {
                skia_utils::create_texture_surface(width, height, context).map_err(|error| {
                    LibraryError::Render(format!(
                        "GPU Particle cannot create an encoded GPU-backed Skia surface {width}x{height}: {error}"
                    ))
                })
            }
            SkiaSurfaceContract::ProjectLinear(_) => {
                create_gpu_working_surface(width, height, context)
            }
        };
    }
    match contract {
        SkiaSurfaceContract::UnmanagedSrgba8 => skia_utils::create_surface(width, height, context),
        SkiaSurfaceContract::ProjectLinear(_) => create_working_surface(width, height, context),
    }
}

pub(crate) fn create_working_surface(
    width: u32,
    height: u32,
    context: Option<&mut skia_safe::gpu::DirectContext>,
) -> Result<Surface, LibraryError> {
    validate_working_surface_payload(width, height)?;
    if let Some(context) = context
        && let Some(surface) = try_create_gpu_working_surface(width, height, context)?
    {
        return Ok(surface);
    }
    let info = working_image_info(width, height, ColorType::RGBAF32)?;
    surfaces::raster(&info, None, None).ok_or_else(|| {
        LibraryError::Render(format!(
            "cannot create Project linear RGBAF32 surface {width}x{height}"
        ))
    })
}

pub(super) fn snapshot_surface(
    surface: &mut Surface,
    width: u32,
    height: u32,
    contract: &SkiaSurfaceContract,
) -> Result<RenderOutput, LibraryError> {
    match contract {
        SkiaSurfaceContract::UnmanagedSrgba8 => {
            skia_utils::surface_to_image(surface, width, height).map(RenderOutput::Image)
        }
        SkiaSurfaceContract::ProjectLinear(contract) => {
            surface_to_managed_working(surface, width, height, contract).map(RenderOutput::Working)
        }
    }
}

pub(super) fn managed_working_to_skia_image(
    image: &ManagedLinearWorkingImage,
    contract: &WorkingSurfaceContract,
    gpu_context: Option<&mut GpuContext>,
) -> Result<Image, LibraryError> {
    if image.identity() != contract.identity() {
        return Err(LibraryError::Render(format!(
            "cannot draw working image {:?} into Project surface {:?}",
            image.identity(),
            contract.identity()
        )));
    }
    #[cfg(feature = "gl")]
    if let Some(gpu_context) = gpu_context {
        return upload_linear_working_to_gpu_image(image.pixels(), gpu_context);
    }
    #[cfg(not(feature = "gl"))]
    let _ = gpu_context;
    linear_working_to_skia_image(image.pixels())
}

/// Upload Project-linear pixels without asking Skia to convert premultiplied
/// RGBAF32 into the device texture format. Skia's raster-to-texture conversion
/// clamps premultiplied channels to `[0, alpha]`, which destroys legitimate
/// scene-linear negative and super-white values before compositing. The
/// renderer's existing GL owner uploads the f32 payload without a gamut
/// conversion (the driver may store it as f16), then transfers texture
/// ownership to Ganesh for ordinary sampling and blending.
#[cfg(feature = "gl")]
fn upload_linear_working_to_gpu_image(
    image: &LinearWorkingImage,
    gpu_context: &mut GpuContext,
) -> Result<Image, LibraryError> {
    use glow::HasContext;

    validate_working_surface_payload(image.width(), image.height())?;
    let upload = working_upload_format(&gpu_context.direct_context)?;

    // Finish Ganesh commands before raw GL changes state on the same context.
    gpu_context.direct_context.flush_and_submit();
    let gl = gpu_context.create_glow_context();
    let saved = SavedGlState::capture(&gl);
    let uploaded = (|| {
        // SAFETY: this queries the current renderer-owned context.
        let max_texture_size =
            u32::try_from(unsafe { gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE) }).map_err(
                |_| LibraryError::Render("OpenGL reported an invalid texture limit".to_string()),
            )?;
        if image.width() > max_texture_size || image.height() > max_texture_size {
            return Err(LibraryError::Render(format!(
                "Project-linear upload {}x{} exceeds OpenGL texture limit {max_texture_size}",
                image.width(),
                image.height()
            )));
        }
        // SAFETY: the renderer activated and exclusively borrows this GL
        // owner. `image` remains alive for the synchronous TexImage call.
        let texture = unsafe { gl.create_texture() }.map_err(|error| {
            LibraryError::Render(format!(
                "Cannot create Project-linear upload texture: {error}"
            ))
        })?;
        // SAFETY: `texture` is a fresh name owned by this scope; the f32 byte
        // slice has the exact RGBA/width/height layout validated above. A PBO
        // is explicitly unbound so the slice is interpreted as client data.
        unsafe {
            gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::NEAREST as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::NEAREST as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                upload.gl_internal_format() as i32,
                image.width() as i32,
                image.height() as i32,
                0,
                glow::RGBA,
                glow::FLOAT,
                glow::PixelUnpackData::Slice(Some(cast_slice(image.pixels()))),
            );
        }
        // SAFETY: the upload above is complete on this current context. On
        // failure this scope still owns the fresh texture and deletes it.
        let upload_error = unsafe { gl.get_error() };
        if upload_error != glow::NO_ERROR {
            // SAFETY: adoption has not happened, so this error path still
            // uniquely owns the fresh texture on the current context.
            unsafe { gl.delete_texture(texture) };
            return Err(LibraryError::Render(format!(
                "Project-linear floating upload failed with OpenGL error {upload_error:#x}"
            )));
        }

        let texture_info = gpu::gl::TextureInfo {
            target: glow::TEXTURE_2D,
            id: texture.0.get(),
            format: upload.gl_internal_format(),
            protected: gpu::Protected::No,
        };
        // SAFETY: the descriptor matches the immutable storage allocated
        // above and is used immediately by this context's Ganesh owner.
        let backend = unsafe {
            gpu::backend_textures::make_gl(
                (image.width() as i32, image.height() as i32),
                gpu::Mipmapped::No,
                texture_info,
                "Project-linear floating upload",
            )
        };
        match gpu::images::adopt_texture_from(
            &mut gpu_context.direct_context,
            &backend,
            gpu::SurfaceOrigin::TopLeft,
            upload.color_type(),
            AlphaType::Premul,
            working_color_space(),
        ) {
            Some(image) => Ok(image),
            None => {
                // Ownership transfers only when Ganesh returns an Image.
                // SAFETY: this error path still uniquely owns `texture`.
                unsafe { gl.delete_texture(texture) };
                Err(LibraryError::Render(
                    "Ganesh could not adopt the Project-linear floating upload texture".to_string(),
                ))
            }
        }
    })();
    saved.restore(&gl);
    gpu_context.direct_context.reset(None);
    uploaded
}

#[cfg(feature = "gl")]
enum WorkingUploadFormat {
    RgbaF32,
    RgbaF16,
}

#[cfg(feature = "gl")]
impl WorkingUploadFormat {
    fn color_type(&self) -> ColorType {
        match self {
            Self::RgbaF32 => ColorType::RGBAF32,
            Self::RgbaF16 => ColorType::RGBAF16,
        }
    }

    fn gl_internal_format(&self) -> u32 {
        match self {
            Self::RgbaF32 => glow::RGBA32F,
            Self::RgbaF16 => glow::RGBA16F,
        }
    }
}

#[cfg(feature = "gl")]
fn working_upload_format(
    context: &gpu::DirectContext,
) -> Result<WorkingUploadFormat, LibraryError> {
    for (color_type, gl_format) in [
        (ColorType::RGBAF32, glow::RGBA32F),
        (ColorType::RGBAF16, glow::RGBA16F),
    ] {
        if context
            .default_backend_format(color_type, gpu::Renderable::No)
            .as_gl_format_enum()
            != gl_format
        {
            continue;
        }
        if color_type == ColorType::RGBAF32 {
            return Ok(WorkingUploadFormat::RgbaF32);
        }
        return Ok(WorkingUploadFormat::RgbaF16);
    }
    Err(LibraryError::Render(
        "OpenGL cannot sample a floating texture required to preserve Project-linear extended values"
            .to_string(),
    ))
}

pub(super) fn surface_to_managed_working(
    surface: &mut Surface,
    width: u32,
    height: u32,
    contract: &WorkingSurfaceContract,
) -> Result<ManagedLinearWorkingImage, LibraryError> {
    let pixels = surface_to_linear_working(surface, width, height)?;
    // SAFETY: The surface was created as premultiplied RGBAF32 for this exact
    // contract. Every draw path verifies or converts color-bearing inputs into
    // the same working identity and applies no terminal transform.
    Ok(unsafe {
        ManagedLinearWorkingImage::from_working_pixels_unchecked(
            contract.identity().clone(),
            pixels,
        )
    })
}

pub(super) fn authored_color4f(
    contract: &SkiaSurfaceContract,
    color: &Color,
    opacity: f32,
) -> Result<(Color4f, Option<ColorSpace>), LibraryError> {
    match contract {
        SkiaSurfaceContract::UnmanagedSrgba8 => Ok((
            Color4f::new(
                f32::from(color.r) / 255.0,
                f32::from(color.g) / 255.0,
                f32::from(color.b) / 255.0,
                f32::from(color.a) / 255.0 * opacity,
            ),
            None,
        )),
        SkiaSurfaceContract::ProjectLinear(contract) => {
            let rgba = contract.authoring_color_to_working(color, opacity)?;
            Ok((
                Color4f::new(rgba[0], rgba[1], rgba[2], rgba[3]),
                Some(working_color_space()),
            ))
        }
    }
}

#[cfg(feature = "gl")]
pub(super) fn authored_premultiplied_rgba(
    contract: &SkiaSurfaceContract,
    color: &Color,
) -> Result<[f32; 4], LibraryError> {
    let (color, _) = authored_color4f(contract, color, 1.0)?;
    Ok([
        color.r * color.a,
        color.g * color.a,
        color.b * color.a,
        color.a,
    ])
}

/// Select the highest-precision floating GL storage Ganesh can borrow for a
/// Project-linear Particle intermediate.
#[cfg(feature = "gl")]
pub(super) fn scene_texture_format(
    context: &skia_safe::gpu::DirectContext,
    contract: &SkiaSurfaceContract,
) -> Result<SceneTextureFormat, LibraryError> {
    if contract.working().is_none() {
        return Ok(SceneTextureFormat::Srgba8);
    }
    for (color_type, gl_format, scene_format) in [
        (
            ColorType::RGBAF32,
            glow::RGBA32F,
            SceneTextureFormat::LinearRgbaF32,
        ),
        (
            ColorType::RGBAF16,
            glow::RGBA16F,
            SceneTextureFormat::LinearRgbaF16,
        ),
    ] {
        let backend_format =
            context.default_backend_format(color_type, skia_safe::gpu::Renderable::No);
        if backend_format.as_gl_format_enum() == gl_format {
            return Ok(scene_format);
        }
    }
    Err(LibraryError::Render(
        "GPU Particle unavailable: OpenGL/Ganesh cannot borrow a floating-point Project-linear scene texture"
            .to_string(),
    ))
}

fn create_gpu_working_surface(
    width: u32,
    height: u32,
    context: &mut skia_safe::gpu::DirectContext,
) -> Result<Surface, LibraryError> {
    validate_working_surface_payload(width, height)?;
    try_create_gpu_working_surface(width, height, context)?.ok_or_else(|| {
        LibraryError::Render(format!(
            "GPU Particle cannot create a GPU-backed Project-linear floating-point Skia surface {width}x{height}; raster fallback is forbidden"
        ))
    })
}

fn try_create_gpu_working_surface(
    width: u32,
    height: u32,
    context: &mut skia_safe::gpu::DirectContext,
) -> Result<Option<Surface>, LibraryError> {
    for color_type in [ColorType::RGBAF32, ColorType::RGBAF16] {
        let info = working_image_info(width, height, color_type)?;
        if let Some(surface) = gpu::surfaces::render_target(
            context,
            gpu::Budgeted::Yes,
            &info,
            None,
            gpu::SurfaceOrigin::TopLeft,
            None,
            false,
            false,
        ) {
            return Ok(Some(surface));
        }
    }
    Ok(None)
}

/// Borrow SceneRuntime's live GL texture into Ganesh. SceneRuntime retains
/// ownership of the GL name.
#[cfg(feature = "gl")]
pub(super) fn scene_texture_to_skia_image(
    context: &mut skia_safe::gpu::DirectContext,
    texture: SceneTexture,
    contract: &SkiaSurfaceContract,
) -> Result<Image, LibraryError> {
    let (gl_format, color_type, color_space) = match (contract, texture.format) {
        (SkiaSurfaceContract::UnmanagedSrgba8, SceneTextureFormat::Srgba8) => {
            (glow::RGBA8, ColorType::RGBA8888, None)
        }
        (SkiaSurfaceContract::ProjectLinear(_), SceneTextureFormat::LinearRgbaF16) => (
            glow::RGBA16F,
            ColorType::RGBAF16,
            Some(working_color_space()),
        ),
        (SkiaSurfaceContract::ProjectLinear(_), SceneTextureFormat::LinearRgbaF32) => (
            glow::RGBA32F,
            ColorType::RGBAF32,
            Some(working_color_space()),
        ),
        _ => {
            return Err(LibraryError::Render(format!(
                "GPU Particle texture {:?} is incompatible with the active Skia surface contract",
                texture.format
            )));
        }
    };
    let texture_info = skia_safe::gpu::gl::TextureInfo {
        target: glow::TEXTURE_2D,
        id: texture.texture_id,
        format: gl_format,
        protected: skia_safe::gpu::Protected::No,
    };
    // SAFETY: SceneRuntime created this live GL texture on `context`'s exact
    // current glutin context. The descriptor matches its immutable storage;
    // Skia borrows the texture and never owns/deletes it.
    let backend = unsafe {
        skia_safe::gpu::backend_textures::make_gl(
            (texture.width as i32, texture.height as i32),
            skia_safe::gpu::Mipmapped::No,
            texture_info,
            "GPU Particle Scene",
        )
    };
    Image::from_texture(
        context,
        &backend,
        skia_safe::gpu::SurfaceOrigin::BottomLeft,
        color_type,
        AlphaType::Premul,
        color_space,
    )
    .ok_or_else(|| {
        LibraryError::Render("Ganesh could not borrow the GPU Particle scene texture".to_string())
    })
}

pub(super) fn set_paint_authored_color(
    paint: &mut Paint,
    contract: &SkiaSurfaceContract,
    color: &Color,
    opacity: f32,
) -> Result<(), LibraryError> {
    let (color, color_space) = authored_color4f(contract, color, opacity)?;
    paint.set_color4f(color, color_space.as_ref());
    Ok(())
}

pub(super) fn clear_authored_color(
    surface: &mut Surface,
    contract: &SkiaSurfaceContract,
    color: &Color,
) -> Result<(), LibraryError> {
    let (color, color_space) = authored_color4f(contract, color, 1.0)?;
    if let Some(color_space) = color_space {
        // `Canvas::clear(Color4f)` has no source-color-space parameter and
        // treats its RGB as encoded sRGB before converting to the linear
        // destination. Use an explicit Src paint so an authored color already
        // transformed into Project working space is not linearized twice.
        let mut paint = Paint::default();
        paint.set_color4f(color, Some(&color_space));
        paint.set_blend_mode(skia_safe::BlendMode::Src);
        surface.canvas().draw_paint(&paint);
    } else {
        surface.canvas().clear(color);
    }
    Ok(())
}

pub(super) fn working_color_space() -> ColorSpace {
    ColorSpace::new_srgb_linear()
}

pub(crate) fn linear_working_to_skia_image(
    image: &LinearWorkingImage,
) -> Result<Image, LibraryError> {
    validate_working_surface_payload(image.width(), image.height())?;
    let info = working_image_info(image.width(), image.height(), ColorType::RGBAF32)?;
    let row_bytes = checked_row_bytes(image.width(), F32_COMPONENT_BYTES)?;
    // Skia may retain the Data past a draw (notably for deferred GPU upload),
    // so a no-copy borrowed slice cannot be made sound with the current API.
    // Keep this owner-bearing copy until Skia exposes a release-proc/Arc
    // boundary. The per-image payload cap bounds this extra allocation.
    let data = Data::new_copy(cast_slice(image.pixels()));
    images::raster_from_data(&info, data, row_bytes).ok_or_else(|| {
        LibraryError::Render("Cannot wrap linear working RGBA32F image for Skia".to_string())
    })
}

pub(crate) fn surface_to_linear_working(
    surface: &mut Surface,
    width: u32,
    height: u32,
) -> Result<LinearWorkingImage, LibraryError> {
    validate_working_surface_payload(width, height)?;
    let info = working_image_info(width, height, ColorType::RGBAF32)?;
    let pixel_count = checked_pixel_count(width, height)?;
    let row_bytes = checked_row_bytes(width, F32_COMPONENT_BYTES)?;
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(pixel_count).map_err(|_| {
        LibraryError::Render(format!(
            "cannot allocate Project linear readback for {width}x{height} RGBAF32 pixels"
        ))
    })?;
    pixels.resize(pixel_count, [0.0_f32; RGBA_COMPONENTS]);
    if !surface.read_pixels(&info, cast_slice_mut(&mut pixels), row_bytes, (0, 0)) {
        return Err(LibraryError::Render(
            "Cannot read linear working RGBA32F pixels from Skia".to_string(),
        ));
    }
    LinearWorkingImage::from_premultiplied_rgba_f32(width, height, pixels)
        .map_err(|error| LibraryError::Render(error.to_string()))
}

fn working_image_info(
    width: u32,
    height: u32,
    color_type: ColorType,
) -> Result<ImageInfo, LibraryError> {
    let width = i32::try_from(width)
        .map_err(|_| LibraryError::Render("Working image width exceeds Skia limits".to_string()))?;
    let height = i32::try_from(height).map_err(|_| {
        LibraryError::Render("Working image height exceeds Skia limits".to_string())
    })?;
    if width <= 0 || height <= 0 {
        return Err(LibraryError::Render(
            "Working image dimensions must be positive".to_string(),
        ));
    }
    Ok(ImageInfo::new(
        ISize::new(width, height),
        color_type,
        AlphaType::Premul,
        working_color_space(),
    ))
}

fn checked_pixel_count(width: u32, height: u32) -> Result<usize, LibraryError> {
    let count = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| LibraryError::Render("Working image dimensions overflow".to_string()))?;
    Ok(count)
}

fn checked_row_bytes(width: u32, component_bytes: usize) -> Result<usize, LibraryError> {
    usize::try_from(width)
        .ok()
        .and_then(|value| value.checked_mul(RGBA_COMPONENTS))
        .and_then(|value| value.checked_mul(component_bytes))
        .ok_or_else(|| LibraryError::Render("Working image row size overflows".to_string()))
}

fn validate_working_surface_payload(width: u32, height: u32) -> Result<(), LibraryError> {
    let row_bytes = checked_row_bytes(width, F32_COMPONENT_BYTES)?;
    let height = usize::try_from(height)
        .map_err(|_| LibraryError::Render("working surface height exceeds usize".to_string()))?;
    let bytes = row_bytes.checked_mul(height).ok_or_else(|| {
        LibraryError::Render("Project linear surface byte size overflows".to_string())
    })?;
    if bytes > MAX_WORKING_SURFACE_BYTES {
        return Err(LibraryError::Render(format!(
            "Project linear surface {width}x{} requires {bytes} bytes; per-surface limit is {MAX_WORKING_SURFACE_BYTES}",
            height
        )));
    }
    Ok(())
}
