//! Image isolation, local raster domains, and group composition.

use super::*;

impl<T: Renderer> RenderService<T> {
    pub(super) fn render_group(
        &mut self,
        group: &FrameGroup,
        parent_context: &RenderContext,
        color_authority: &RenderColorAuthority<'_>,
    ) -> Result<(), LibraryError> {
        if group.kind == FrameGroupKind::Composition {
            return self.render_composition_group(group, parent_context, color_authority);
        }
        if matches!(
            group.kind,
            FrameGroupKind::ImageTransform | FrameGroupKind::ImageStyle
        ) {
            return self.render_local_image_group(group, parent_context, color_authority);
        }
        let child_context = parent_context.with_transform(&group.transform);
        if !group_requires_isolation(group) {
            return self.render_items(
                &group.items,
                &child_context,
                group.effect_time.into_inner(),
                color_authority,
            );
        }

        self.renderer.begin_group(
            parent_context.target_width,
            parent_context.target_height,
            &transparent_color(),
        )?;
        let children_result = self.render_items(
            &group.items,
            &child_context,
            group.effect_time.into_inner(),
            color_authority,
        );
        if let Err(error) = children_result {
            return Err(self.close_failed_group(error, "isolated group"));
        }
        let image_context = ImageStyleContext {
            render_scale: parent_context.render_scale,
            bounds: self
                .frame_image_bounds
                .items_bounds(&group.items, group.effect_time.into_inner())
                .and_then(|bounds| bounds.transformed(child_context.logical_to_target))
                .unwrap_or_else(|| parent_context.image_style_context().bounds),
        };
        self.finish_image_group(group, image_context, &Affine2D::IDENTITY, color_authority)
    }

    /// Rasterize the upstream Image subtree before its affine transform,
    /// preserving graph order and treating descendants as one image.
    fn render_local_image_group(
        &mut self,
        group: &FrameGroup,
        parent_context: &RenderContext,
        color_authority: &RenderColorAuthority<'_>,
    ) -> Result<(), LibraryError> {
        let scale = parent_context.render_scale;
        let input_bounds = self
            .frame_image_bounds
            .items_bounds(&group.items, group.effect_time.into_inner())
            .unwrap_or_else(|| {
                FrameImageBounds::from_rect(crate::model::frame::entity::FrameBounds::new(
                    0.0,
                    0.0,
                    group.width as f32,
                    group.height as f32,
                ))
            });
        let output_bounds =
            image_style_bounds(input_bounds, &group.effects, 1.0).ok_or_else(|| {
                LibraryError::Render("Image group has invalid visual bounds".to_string())
            })?;
        let (x, y, logical_width, logical_height) = output_bounds.visual.as_tuple();
        let left = (f64::from(x) * scale).floor() - FRAME_IMAGE_RASTER_GUARD;
        let top = (f64::from(y) * scale).floor() - FRAME_IMAGE_RASTER_GUARD;
        let width = image_dimension(
            (f64::from(x + logical_width) * scale).ceil() + FRAME_IMAGE_RASTER_GUARD - left,
        )?;
        let height = image_dimension(
            (f64::from(y + logical_height) * scale).ceil() + FRAME_IMAGE_RASTER_GUARD - top,
        )?;
        let logical_to_surface =
            Affine2D::translate(-left, -top).compose(Affine2D::scale(scale, scale));
        let image_context = ImageStyleContext {
            render_scale: scale,
            bounds: input_bounds
                .transformed(logical_to_surface)
                .ok_or_else(|| {
                    LibraryError::Render("Image group has an invalid raster domain".to_string())
                })?,
        };
        let child_context = RenderContext {
            logical_to_target: logical_to_surface,
            render_scale: scale,
            target_width: width,
            target_height: height,
        };

        self.renderer
            .begin_group(width, height, &transparent_color())?;
        let children_result = self.render_items(
            &group.items,
            &child_context,
            group.effect_time.into_inner(),
            color_authority,
        );
        if let Err(error) = children_result {
            return Err(self.close_failed_group(error, "local Image group"));
        }

        let pixel_to_local =
            Affine2D::scale(1.0 / scale, 1.0 / scale).compose(Affine2D::translate(left, top));
        let transform = parent_context
            .logical_to_target
            .compose(Affine2D::from(&group.transform))
            .compose(pixel_to_local);
        self.finish_image_group(group, image_context, &transform, color_authority)
    }

    fn render_composition_group(
        &mut self,
        group: &FrameGroup,
        parent_context: &RenderContext,
        color_authority: &RenderColorAuthority<'_>,
    ) -> Result<(), LibraryError> {
        let width = scaled_dimension(group.width as f64, parent_context.render_scale);
        let height = scaled_dimension(group.height as f64, parent_context.render_scale);
        let child_context = RenderContext::composition(parent_context.render_scale, width, height);

        self.renderer
            .begin_group(width, height, &group.background_color)?;
        let children_result = self.render_items(
            &group.items,
            &child_context,
            group.effect_time.into_inner(),
            color_authority,
        );
        if let Err(error) = children_result {
            return Err(self.close_failed_group(error, "Composition group"));
        }

        let pixel_to_local = Affine2D::scale(
            1.0 / parent_context.render_scale,
            1.0 / parent_context.render_scale,
        );
        let transform = parent_context
            .logical_to_target
            .compose(Affine2D::from(&group.transform))
            .compose(pixel_to_local);
        self.finish_image_group(
            group,
            child_context.image_style_context(),
            &transform,
            color_authority,
        )
    }

    /// Complete an isolated image with one authoritative effect dispatch.
    /// Empty and single-style groups keep backend-native storage.
    fn finish_image_group(
        &mut self,
        group: &FrameGroup,
        image_context: ImageStyleContext,
        transform: &Affine2D,
        color_authority: &RenderColorAuthority<'_>,
    ) -> Result<(), LibraryError> {
        if group.effects.is_empty() {
            return self.renderer.end_group_and_draw(
                transform,
                group.transform.opacity,
                group.blend_mode,
            );
        }
        if let [crate::model::frame::effect::ImageEffect::LayerStyle(style)] =
            group.effects.as_slice()
        {
            return self.renderer.end_group_with_image_style_and_draw(
                style,
                ImageStyleContext {
                    bounds: image_style_bounds(
                        image_context.bounds,
                        &group.effects,
                        image_context.render_scale,
                    )
                    .ok_or_else(|| {
                        LibraryError::Render("Image style has invalid bounds".to_string())
                    })?,
                    ..image_context
                },
                transform,
                group.transform.opacity,
                group.blend_mode,
            );
        }
        let output = self.renderer.end_group()?;
        let output = self.apply_effects(
            output,
            &group.effects,
            group.effect_time.into_inner(),
            image_context,
            color_authority,
        )?;
        self.renderer.draw_layer_affine_with_blend(
            &output,
            transform,
            group.transform.opacity,
            group.blend_mode,
        )
    }

    fn close_failed_group(&mut self, render_error: LibraryError, label: &str) -> LibraryError {
        if let Err(cleanup_error) = self.renderer.end_group() {
            log::error!("failed to close {label} after child render error: {cleanup_error}");
        }
        render_error
    }
}

fn image_dimension(value: f64) -> Result<u32, LibraryError> {
    if !value.is_finite() || value <= 0.0 || value > f64::from(u32::MAX) {
        return Err(LibraryError::Render(format!(
            "Invalid Image raster dimension {value}"
        )));
    }
    Ok(value as u32)
}
