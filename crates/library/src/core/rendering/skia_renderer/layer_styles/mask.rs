//! One authoritative alpha mask for a vector layer's composed body.

use skia_safe::{
    BlendMode, Canvas, ColorFilter, ImageFilter, Paint, Picture, PictureRecorder, Rect, Shader,
    TileMode, color_filters, image_filters,
};

use crate::error::LibraryError;
use crate::model::frame::color::Color;
use crate::rendering::skia_working_surface::{self, SkiaSurfaceContract};

#[derive(Clone)]
pub(in crate::core::rendering::skia_renderer) struct LayerMask {
    picture: Picture,
    source: ImageFilter,
    bounds: Rect,
    style_bounds: Rect,
}

impl LayerMask {
    pub(in crate::core::rendering::skia_renderer) fn record(
        bounds: Rect,
        style_bounds: Rect,
        draw: impl FnOnce(&Canvas) -> Result<(), LibraryError>,
    ) -> Result<Self, LibraryError> {
        let mut recorder = PictureRecorder::new();
        let canvas = recorder.begin_recording(bounds, false);
        draw(canvas)?;
        let picture = recorder
            .finish_recording_as_picture(Some(&bounds))
            .ok_or_else(|| LibraryError::Render("Cannot record vector layer mask".to_string()))?;
        let source = image_filters::picture(picture.clone(), Some(&bounds)).ok_or_else(|| {
            LibraryError::Render("Cannot create vector layer mask filter".to_string())
        })?;
        Ok(Self {
            picture,
            source,
            bounds,
            style_bounds,
        })
    }

    pub(in crate::core::rendering::skia_renderer) fn draw_content(&self, canvas: &Canvas) {
        canvas.draw_picture(&self.picture, None, None);
    }

    pub(super) const fn style_bounds(&self) -> Rect {
        self.style_bounds
    }

    pub(super) fn source(&self) -> ImageFilter {
        self.source.clone()
    }

    pub(super) fn draw_filter(&self, canvas: &Canvas, filter: ImageFilter) {
        let mut paint = Paint::default();
        paint.set_image_filter(filter);
        canvas.draw_rect(self.bounds, &paint);
    }

    pub(super) fn solid_tint(
        &self,
        contract: &SkiaSurfaceContract,
        input: ImageFilter,
        color: &Color,
        opacity: f32,
    ) -> Result<ImageFilter, LibraryError> {
        let (color, color_space) =
            skia_working_surface::authored_color4f(contract, color, opacity.clamp(0.0, 1.0))?;
        let tint = color_filters::blend_with_color_space(color, color_space, BlendMode::SrcIn)
            .ok_or_else(|| LibraryError::Render("Cannot create layer style tint".to_string()))?;
        color_filter(tint, input)
    }

    pub(super) fn shader_tint(
        &self,
        input: ImageFilter,
        shader: Shader,
    ) -> Result<ImageFilter, LibraryError> {
        let shader = image_filters::shader(shader, None)
            .ok_or_else(|| LibraryError::Render("Cannot create layer style shader".to_string()))?;
        blend(BlendMode::SrcIn, input, shader)
    }

    pub(super) fn expanded_blur(
        &self,
        size: f64,
        spread: f64,
    ) -> Result<ImageFilter, LibraryError> {
        let mut filter = self.source();
        let spread_width = spread_width(size, spread);
        if spread_width > 0.0 {
            filter = image_filters::dilate((spread_width, spread_width), Some(filter), None)
                .ok_or_else(|| {
                    LibraryError::Render("Cannot dilate layer style mask".to_string())
                })?;
        }
        let sigma = blur_sigma(size, spread);
        if sigma > 0.0 {
            filter = image_filters::blur((sigma, sigma), Some(TileMode::Decal), Some(filter), None)
                .ok_or_else(|| LibraryError::Render("Cannot blur layer style mask".to_string()))?;
        }
        Ok(filter)
    }

    pub(super) fn eroded_blur(&self, size: f64, spread: f64) -> Result<ImageFilter, LibraryError> {
        let mut filter = self.source();
        let erode = spread_width(size, spread);
        if erode > 0.0 {
            filter = image_filters::erode((erode, erode), Some(filter), None)
                .ok_or_else(|| LibraryError::Render("Cannot erode layer style mask".to_string()))?;
        }
        let sigma = blur_sigma(size, spread);
        if sigma > 0.0 {
            filter = image_filters::blur((sigma, sigma), Some(TileMode::Decal), Some(filter), None)
                .ok_or_else(|| LibraryError::Render("Cannot blur layer style mask".to_string()))?;
        }
        Ok(filter)
    }

    pub(super) fn offset(
        &self,
        input: ImageFilter,
        offset: (f32, f32),
    ) -> Result<ImageFilter, LibraryError> {
        image_filters::offset(offset, Some(input), None)
            .ok_or_else(|| LibraryError::Render("Cannot offset layer style mask".to_string()))
    }

    pub(super) fn outside(&self, foreground: ImageFilter) -> Result<ImageFilter, LibraryError> {
        blend(BlendMode::SrcOut, self.source(), foreground)
    }

    pub(super) fn subtract(
        &self,
        background: ImageFilter,
        foreground: ImageFilter,
    ) -> Result<ImageFilter, LibraryError> {
        // Arithmetic DstOut is explicitly clamped to premultiplied alpha.
        // Skia's generic blender can otherwise leave a tiny negative alpha
        // epsilon after chained mask subtraction on RGBAF32 surfaces.
        image_filters::arithmetic(
            -1.0,
            0.0,
            1.0,
            0.0,
            true,
            Some(background),
            Some(foreground),
            None,
        )
        .ok_or_else(|| LibraryError::Render("Cannot subtract layer style masks".to_string()))
    }
}

fn color_filter(
    color_filter: ColorFilter,
    input: ImageFilter,
) -> Result<ImageFilter, LibraryError> {
    image_filters::color_filter(color_filter, Some(input), None)
        .ok_or_else(|| LibraryError::Render("Cannot color layer style mask".to_string()))
}

fn blend(
    mode: BlendMode,
    background: ImageFilter,
    foreground: ImageFilter,
) -> Result<ImageFilter, LibraryError> {
    image_filters::blend(mode, Some(background), Some(foreground), None)
        .ok_or_else(|| LibraryError::Render("Cannot combine layer style masks".to_string()))
}

fn spread_width(size: f64, spread: f64) -> f32 {
    (size.max(0.0) * spread.clamp(0.0, 1.0)) as f32
}

fn blur_sigma(size: f64, spread: f64) -> f32 {
    (size.max(0.0) * (1.0 - spread.clamp(0.0, 1.0)) / 3.0) as f32
}
