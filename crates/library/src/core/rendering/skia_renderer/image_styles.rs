//! One typed Image -> Image layer-style compositor shared by graph execution
//! and the backend-native single-style group fast path.

use skia_safe::{Image as SkImage, Rect, Surface};

use super::*;
use crate::model::frame::entity::StyleConfig;
use crate::model::frame::image_bounds::FrameImageBounds;
use crate::rendering::renderer::ImageStyleContext;

impl SkiaRenderer {
    pub(super) fn apply_image_style_output(
        &mut self,
        layer: &RenderOutput,
        style: &StyleConfig,
        context: ImageStyleContext,
    ) -> Result<RenderOutput, LibraryError> {
        self.activate_graphics_context()?;
        let image = self.output_to_skia_image(layer)?;
        let width = u32::try_from(image.width())
            .map_err(|_| LibraryError::Render("Image style width is invalid".to_string()))?;
        let height = u32::try_from(image.height())
            .map_err(|_| LibraryError::Render("Image style height is invalid".to_string()))?;
        let mut surface = self.image_style_surface(&image, width, height, style, context)?;
        self.snapshot_surface(&mut surface, width, height)
    }

    pub(super) fn finish_group_with_image_style(
        &mut self,
        style: &StyleConfig,
        context: ImageStyleContext,
        transform: &Affine2D,
        opacity: f64,
        blend_mode: crate::model::BlendMode,
    ) -> Result<(), LibraryError> {
        self.activate_graphics_context()?;
        let mut group = self.group_surfaces.pop().ok_or_else(|| {
            LibraryError::Render(
                "end_group_with_image_style_and_draw called without a matching begin_group"
                    .to_string(),
            )
        })?;
        let image = group.surface.image_snapshot();
        let mut styled =
            self.image_style_surface(&image, group.width, group.height, style, context)?;
        let styled_image = styled.image_snapshot();
        // Both source surfaces stay alive until the parent Canvas has retained
        // the composed image. No CPU pixels cross this graph edge.
        let result =
            self.draw_skia_image_affine_with_blend(&styled_image, transform, opacity, blend_mode);
        drop(styled_image);
        drop(styled);
        drop(image);
        drop(group);
        result
    }

    fn image_style_surface(
        &mut self,
        source: &SkImage,
        width: u32,
        height: u32,
        style: &StyleConfig,
        context: ImageStyleContext,
    ) -> Result<Surface, LibraryError> {
        if !context.render_scale.is_finite() || context.render_scale <= 0.0 {
            return Err(LibraryError::Render(format!(
                "Image style render scale must be finite and positive, not {}",
                context.render_scale
            )));
        }
        let bounds = checked_layer_bounds(context.bounds, width, height)?;
        let mask = layer_styles::LayerMask::record(
            vector_bounds::VectorLayerBounds {
                geometry: bounds.geometry,
                content: bounds.content,
                visual: bounds.visual,
            },
            |canvas| {
                canvas.draw_image(source, (0.0, 0.0), None);
                Ok(())
            },
        )?;
        let mut surface = self.create_layer_surface(width.max(1), height.max(1))?;
        surface.canvas().clear(skia_safe::Color::TRANSPARENT);
        layer_styles::LayerStyleRenderer::new(
            &self.surface_contract,
            &mut self.blend_runtime,
            context.render_scale,
        )
        .compose(surface.canvas(), &style.style, &mask)?;
        Ok(surface)
    }
}

#[derive(Clone, Copy)]
struct CheckedLayerBounds {
    geometry: Rect,
    content: Rect,
    visual: Rect,
}

fn checked_layer_bounds(
    bounds: FrameImageBounds,
    width: u32,
    height: u32,
) -> Result<CheckedLayerBounds, LibraryError> {
    let geometry = checked_geometry_rect(bounds.geometry)?;
    let content = checked_allocated_rect(bounds.content, "content", width, height)?;
    let visual = checked_allocated_rect(bounds.visual, "visual", width, height)?;
    if !contains(visual, content) {
        return Err(LibraryError::Render(
            "Image style visual bounds must contain the complete input content".to_string(),
        ));
    }
    Ok(CheckedLayerBounds {
        geometry,
        content,
        visual,
    })
}

fn checked_geometry_rect(
    bounds: crate::model::frame::entity::FrameBounds,
) -> Result<Rect, LibraryError> {
    let (x, y, width, height) = bounds.as_tuple();
    let right = x + width;
    let bottom = y + height;
    if [x, y, width, height, right, bottom]
        .into_iter()
        .any(|value| !value.is_finite())
        || width < 0.0
        || height < 0.0
    {
        return Err(LibraryError::Render(format!(
            "Image style geometry bounds ({x}, {y}, {width}, {height}) are invalid"
        )));
    }
    Ok(Rect::from_xywh(x, y, width, height))
}

fn checked_allocated_rect(
    bounds: crate::model::frame::entity::FrameBounds,
    label: &str,
    width: u32,
    height: u32,
) -> Result<Rect, LibraryError> {
    let (x, y, rect_width, rect_height) = bounds.as_tuple();
    let right = x + rect_width;
    let bottom = y + rect_height;
    if [x, y, rect_width, rect_height, right, bottom]
        .into_iter()
        .any(|value| !value.is_finite())
        || x < 0.0
        || y < 0.0
        || rect_width < 0.0
        || rect_height < 0.0
        || right > width as f32
        || bottom > height as f32
    {
        return Err(LibraryError::Render(format!(
            "Image style {label} bounds ({x}, {y}, {rect_width}, {rect_height}) exceed the {width}x{height} padded layer"
        )));
    }
    Ok(Rect::from_xywh(x, y, rect_width, rect_height))
}

fn contains(outer: Rect, inner: Rect) -> bool {
    inner.left >= outer.left
        && inner.top >= outer.top
        && inner.right <= outer.right
        && inner.bottom <= outer.bottom
}
