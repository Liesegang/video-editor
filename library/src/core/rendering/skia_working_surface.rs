//! Skia storage adapter for legacy and Project-linear surface contracts.
//!
//! Project surfaces, transient layers, and isolated groups all use RGBAF32
//! premultiplied storage. The exact Project color identity travels beside the
//! pixels; Skia's linear color-space tag prevents encoded-light resampling and
//! blending but is not used as a replacement for Project/OCIO authority.

use bytemuck::{cast_slice, cast_slice_mut};
use ruvie_color_management::{LinearWorkingImage, ManagedLinearWorkingImage};
use skia_safe::{
    AlphaType, Color4f, ColorSpace, ColorType, Data, ISize, Image, ImageInfo, Paint, Surface, gpu,
    images, surfaces,
};

use crate::error::LibraryError;
use crate::model::frame::color::Color;
use crate::rendering::renderer::{RenderOutput, WorkingSurfaceContract};
#[cfg(feature = "gl")]
use crate::rendering::scene_runtime::{SceneTexture, SceneTextureFormat};
use crate::rendering::skia_utils;

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
) -> Result<Surface, LibraryError> {
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
    let info = working_image_info(width, height, ColorType::RGBAF32)?;
    if let Some(context) = context
        && let Some(surface) = gpu::surfaces::render_target(
            context,
            gpu::Budgeted::Yes,
            &info,
            None,
            gpu::SurfaceOrigin::TopLeft,
            None,
            false,
            false,
        )
    {
        return Ok(surface);
    }
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
) -> Result<Image, LibraryError> {
    if image.identity() != contract.identity() {
        return Err(LibraryError::Render(format!(
            "cannot draw working image {:?} into Project surface {:?}",
            image.identity(),
            contract.identity()
        )));
    }
    linear_working_to_skia_image(image.pixels())
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

/// Borrow SceneRuntime's GL texture into Ganesh under the exact surface
/// storage/color contract. The runtime retains ownership of the GL name.
#[cfg(feature = "gl")]
pub(super) fn scene_texture_to_skia_image(
    context: &mut skia_safe::gpu::DirectContext,
    texture: SceneTexture,
    contract: &SkiaSurfaceContract,
) -> Result<Image, LibraryError> {
    let (expected_format, gl_format, color_type, color_space) = match contract {
        SkiaSurfaceContract::UnmanagedSrgba8 => (
            SceneTextureFormat::Srgba8,
            glow::RGBA8,
            ColorType::RGBA8888,
            None,
        ),
        SkiaSurfaceContract::ProjectLinear(_) => (
            SceneTextureFormat::LinearRgbaF32,
            glow::RGBA32F,
            ColorType::RGBAF32,
            Some(working_color_space()),
        ),
    };
    if texture.format != expected_format {
        return Err(LibraryError::Render(format!(
            "GPU Particle texture {:?} is incompatible with the active Skia surface contract",
            texture.format
        )));
    }
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
