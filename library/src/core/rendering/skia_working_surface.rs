//! Explicit Skia adapter for the scene-linear working-image contract.
//!
//! Production's legacy N32 renderer is intentionally not switched here. This
//! module is the tested bridge used while owner-bearing GPU frames replace raw
//! texture IDs. It never presents a working image as encoded RGBA8.

use bytemuck::{cast_slice, cast_slice_mut};
use ruvie_color_management::SceneLinearImage;
use skia_safe::{
    AlphaType, ColorSpace, ColorType, Data, ISize, Image, ImageInfo, Paint, Surface, images,
    surfaces,
};

use crate::error::LibraryError;

const F32_COMPONENT_BYTES: usize = std::mem::size_of::<f32>();
const RGBA_COMPONENTS: usize = 4;

pub(crate) fn composite_source_over(
    background: &SceneLinearImage,
    sources: &[SceneLinearImage],
) -> Result<SceneLinearImage, LibraryError> {
    for source in sources {
        if (source.width(), source.height()) != (background.width(), background.height()) {
            return Err(LibraryError::Render(format!(
                "Cannot composite scene-linear source {}x{} over {}x{}",
                source.width(),
                source.height(),
                background.width(),
                background.height()
            )));
        }
    }

    let color_type = if std::iter::once(background)
        .chain(sources)
        .flat_map(|image| image.pixels().iter().flatten())
        .all(|component| component.abs() <= 65_504.0)
    {
        ColorType::RGBAF16
    } else {
        ColorType::RGBAF32
    };
    let mut surface = create_raster_surface(background.width(), background.height(), color_type)?;
    let background = scene_linear_to_skia_image(background)?;
    surface
        .canvas()
        .draw_image(&background, (0, 0), Some(&source_replacement_paint()));
    for source in sources {
        let source = scene_linear_to_skia_image(source)?;
        surface
            .canvas()
            .draw_image(&source, (0, 0), Some(&Paint::default()));
    }
    surface_to_scene_linear(
        &mut surface,
        background.width() as u32,
        background.height() as u32,
    )
}

fn create_raster_surface(
    width: u32,
    height: u32,
    color_type: ColorType,
) -> Result<Surface, LibraryError> {
    let info = working_image_info(width, height, color_type)?;
    surfaces::raster(&info, None, None).ok_or_else(|| {
        LibraryError::Render(format!(
            "Cannot create scene-linear {color_type:?} surface {width}x{height}"
        ))
    })
}

fn scene_linear_to_skia_image(image: &SceneLinearImage) -> Result<Image, LibraryError> {
    let info = working_image_info(image.width(), image.height(), ColorType::RGBAF32)?;
    let row_bytes = checked_row_bytes(image.width(), F32_COMPONENT_BYTES)?;
    let data = Data::new_copy(cast_slice(image.pixels()));
    images::raster_from_data(&info, data, row_bytes).ok_or_else(|| {
        LibraryError::Render("Cannot wrap scene-linear RGBA32F image for Skia".to_string())
    })
}

fn surface_to_scene_linear(
    surface: &mut Surface,
    width: u32,
    height: u32,
) -> Result<SceneLinearImage, LibraryError> {
    let info = working_image_info(width, height, ColorType::RGBAF32)?;
    let pixel_count = checked_pixel_count(width, height)?;
    let row_bytes = checked_row_bytes(width, F32_COMPONENT_BYTES)?;
    let mut pixels = vec![[0.0_f32; RGBA_COMPONENTS]; pixel_count];
    if !surface.read_pixels(&info, cast_slice_mut(&mut pixels), row_bytes, (0, 0)) {
        return Err(LibraryError::Render(
            "Cannot read scene-linear RGBA32F pixels from Skia".to_string(),
        ));
    }
    SceneLinearImage::from_premultiplied_rgba_f32(width, height, pixels)
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
        ColorSpace::new_srgb_linear(),
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

fn source_replacement_paint() -> Paint {
    let mut paint = Paint::default();
    paint.set_blend_mode(skia_safe::BlendMode::Src);
    paint
}

#[cfg(test)]
mod tests {
    use super::composite_source_over;
    use ruvie_color_management::{
        BackendBuild, BuiltinColorTransform, ColorTransformBackend, ColorTransformRequest,
        CpuColorProcessor, LINEAR_SRGB_SPACE_ID, ManagedSceneLinearImage, SRGB_SPACE_ID,
        SceneLinearImage, WorkingColorIdentity,
    };

    fn processor(source: &str, target: &str) -> Box<dyn CpuColorProcessor> {
        BuiltinColorTransform
            .create_cpu_processor(&ColorTransformRequest::explicit(source, target))
            .unwrap()
    }

    fn identity() -> WorkingColorIdentity {
        WorkingColorIdentity::scene_linear_f32(
            "test-config",
            "builtin.extended-srgb",
            BackendBuild::Real,
            "test-fingerprint",
            LINEAR_SRGB_SPACE_ID,
        )
    }

    #[test]
    fn rgba16f_skia_composite_matches_cpu_linear_oracle_and_terminal_encoding() {
        let to_working = processor(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        let to_display = processor(LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID);
        let mut cpu = ManagedSceneLinearImage::new(
            identity(),
            SceneLinearImage::from_straight_rgba8(
                2,
                1,
                &[0, 0, 0, 255, 32, 64, 128, 255],
                to_working.as_ref(),
            )
            .unwrap(),
        );
        let source = SceneLinearImage::from_straight_rgba8(
            2,
            1,
            &[255, 255, 255, 128, 240, 80, 20, 96],
            to_working.as_ref(),
        )
        .unwrap();
        cpu.composite_source_over(&ManagedSceneLinearImage::new(identity(), source.clone()))
            .unwrap();

        let skia = composite_source_over(
            &SceneLinearImage::from_straight_rgba8(
                2,
                1,
                &[0, 0, 0, 255, 32, 64, 128, 255],
                to_working.as_ref(),
            )
            .unwrap(),
            &[source],
        )
        .unwrap();

        for (actual, expected) in skia
            .pixels()
            .iter()
            .flatten()
            .zip(cpu.pixels().pixels().iter().flatten())
        {
            assert!((actual - expected).abs() <= 0.001);
        }
        let encoded = skia.to_straight_rgba8(to_display.as_ref()).unwrap();
        assert_eq!(&encoded[0..4], &[188, 188, 188, 255]);
        for (actual, expected) in encoded
            .iter()
            .zip(cpu.pixels().to_straight_rgba8(to_display.as_ref()).unwrap())
        {
            assert!(actual.abs_diff(expected) <= 1);
        }
    }

    #[test]
    fn rgba16f_surface_preserves_extended_range_and_canonical_transparency() {
        let background = SceneLinearImage::from_premultiplied_rgba_f32(
            2,
            1,
            vec![[-0.25, 2.0, 0.5, 1.0], [0.8, 0.7, 0.6, 0.0]],
        )
        .unwrap();
        let output = composite_source_over(&background, &[]).unwrap();

        for (actual, expected) in output.pixels()[0].iter().zip(background.pixels()[0]) {
            assert!((actual - expected).abs() <= 0.001);
        }
        assert_eq!(output.pixels()[1], [0.0; 4]);
    }

    #[test]
    fn values_outside_f16_range_promote_surface_to_f32() {
        let background = SceneLinearImage::from_premultiplied_rgba_f32(
            1,
            1,
            vec![[70_000.0, -70_000.0, 0.25, 1.0]],
        )
        .unwrap();
        let output = composite_source_over(&background, &[]).unwrap();
        assert_eq!(output.pixels()[0], background.pixels()[0]);
    }
}
