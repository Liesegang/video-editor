//! Backend-native vector layer construction and composition.
//!
//! Rasterization and direct composition deliberately share these builders so
//! the effect path can still cross the owned [`RenderOutput`] boundary while
//! the common no-effect path keeps the transient Skia surface native.

use super::*;

impl SkiaRenderer {
    pub(super) fn create_sksl_layer_surface(
        &mut self,
        request: SkSLRasterRequest<'_>,
    ) -> Result<Surface, LibraryError> {
        match request.color_domain {
            SkSLColorDomain::ProjectWorkingLinear if self.surface_contract.working().is_none() => {
                return Err(LibraryError::Render(
                    "Project-working-linear SkSL cannot render into an unmanaged sRGBA8 surface"
                        .to_string(),
                ));
            }
            SkSLColorDomain::ProjectWorkingLinear => {}
        }
        let shader_code = request.shader_code;
        let resolution = request.resolution;
        let time = request.time;
        let transform = request.transform;
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::TRANSPARENT);

            let preprocessed_code = shader_utils::preprocess_shader(shader_code);
            let effect = skia_safe::RuntimeEffect::make_for_shader(&preprocessed_code, None)
                .map_err(|error| {
                    log::error!(
                        "SkSL Compilation Error: {}\nCode:\n{}",
                        error,
                        preprocessed_code
                    );
                    LibraryError::Render(format!("SkSL compilation failed: {error}"))
                })?;
            let uniform_size = effect.uniform_size();
            let mut data: Vec<u8> = vec![0; uniform_size];

            let shader_context = ShaderContext {
                resolution,
                time,
                time_delta: 1.0 / 60.0,
                frame: (time * 60.0).floor(),
                mouse: (0.0, 0.0, 0.0, 0.0),
                date: (2024.0, 1.0, 1.0, 0.0),
            };
            shader_utils::bind_standard_uniforms(&effect, &mut data, &shader_context);

            let uniforms = skia_safe::Data::new_copy(&data);
            let straight_shader =
                effect
                    .make_shader(uniforms, &[], None)
                    .ok_or(LibraryError::Render(
                        "Failed to create SkSL shader".to_string(),
                    ))?;
            // ProjectWorkingLinear is a straight-alpha ABI. Skia assumes every
            // shader result is premultiplied, so adapt it exactly once.
            let shader = self.premultiply_straight_sksl_shader(straight_shader)?;

            let mut paint = Paint::default();
            paint.set_shader(shader);
            let matrix = build_transform_matrix(transform);
            canvas.save();
            canvas.concat(&matrix);
            canvas.draw_rect(skia_safe::Rect::from_wh(resolution.0, resolution.1), &paint);
            canvas.restore();
        }
        Ok(layer)
    }

    pub(super) fn create_text_layer_surface(
        &mut self,
        request: TextRasterRequest<'_>,
    ) -> Result<Surface, LibraryError> {
        let _timer = ScopedTimer::debug(format!(
            "SkiaRenderer::rasterize_text_layer len={} size={} ensemble={}",
            request.text.len(),
            request.size,
            request.ensemble.is_some()
        ));
        if let Some(ensemble_data) = request.ensemble
            && ensemble_data.enabled
        {
            return self.create_ensemble_text_layer_surface(request, ensemble_data);
        }

        let TextRasterRequest {
            text,
            size,
            font_name,
            styles,
            transform,
            ..
        } = request;
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::TRANSPARENT);
            canvas.save();
            canvas.concat(&build_transform_matrix(&transform));
            for config in styles {
                let paint = PaintFactory::new(&self.surface_contract).text_paint(
                    &config.style,
                    1.0,
                    None,
                )?;
                let paragraph = build_text_paragraph(text, font_name, size as f32, Some(&paint));
                paragraph.paint(canvas, (0.0, 0.0));
            }
            canvas.restore();
        }
        Ok(layer)
    }

    fn create_ensemble_text_layer_surface(
        &mut self,
        request: TextRasterRequest<'_>,
        ensemble_data: &crate::core::ensemble::EnsembleData,
    ) -> Result<Surface, LibraryError> {
        use crate::core::ensemble::target::EffectorTarget;
        use crate::core::ensemble::types::EffectorConfig;

        let TextRasterRequest {
            text,
            size,
            font_name,
            styles,
            transform,
            current_time,
            ..
        } = request;
        let current_time = current_time as f32;
        log::debug!(
            "Ensemble rendering: {} effectors, {} decorators",
            ensemble_data.effector_configs.len(),
            ensemble_data.decorator_configs.len()
        );
        for config in &ensemble_data.effector_configs {
            let target = match config {
                EffectorConfig::Transform { target, .. }
                | EffectorConfig::StepDelay { target, .. }
                | EffectorConfig::Opacity { target, .. }
                | EffectorConfig::Randomize { target, .. } => target,
            };
            if *target == EffectorTarget::Parts {
                return Err(LibraryError::Render(
                    "Ensemble EffectorTarget::Parts is not supported".to_string(),
                ));
            }
        }

        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::TRANSPARENT);
            canvas.save();
            canvas.concat(&build_transform_matrix(&transform));

            let font_mgr = skia_safe::FontMgr::default();
            let typeface = font_mgr
                .match_family_style(font_name, skia_safe::FontStyle::default())
                .or_else(|| font_mgr.legacy_make_typeface(None, skia_safe::FontStyle::default()))
                .ok_or_else(|| {
                    LibraryError::Render(format!(
                        "No usable font was found for Ensemble text family {font_name:?}"
                    ))
                })?;
            let font = skia_safe::Font::from_typeface(typeface, size as f32);
            let runtime_text = layout_runtime_text_shape(text, font_name, size as f32);
            let character_transforms =
                evaluate_text_element_transforms(&runtime_text, ensemble_data, current_time)?;

            legacy_backplate::draw_text_backplates(
                canvas,
                &runtime_text,
                &character_transforms,
                &ensemble_data.decorator_configs,
                &self.surface_contract,
            )?;
            for (character, character_transform) in
                runtime_text.elements.iter().zip(&character_transforms)
            {
                let center = Point::new(
                    character.bounds.left + character.advance / 2.0,
                    (character.bounds.top + character.bounds.bottom) / 2.0,
                );
                canvas.save();
                canvas.translate((center.x, center.y));
                canvas.translate(character_transform.translate);
                canvas.rotate(character_transform.rotate, None);
                canvas.scale(character_transform.scale);
                canvas.translate((-center.x, -center.y));
                for config in styles {
                    let paint = PaintFactory::new(&self.surface_contract).text_paint(
                        &config.style,
                        character_transform.opacity,
                        character_transform.color_override.as_ref(),
                    )?;
                    // TODO: draw SkParagraph shaping runs with source mapping.
                    canvas.draw_str(
                        &character.source,
                        (character.bounds.left, character.baseline),
                        &font,
                        &paint,
                    );
                }
                canvas.restore();
            }
            canvas.restore();
        }
        Ok(layer)
    }

    pub(super) fn create_shape_layer_surface(
        &mut self,
        request: ShapeRasterRequest<'_>,
    ) -> Result<Surface, LibraryError> {
        let _timer = ScopedTimer::debug("SkiaRenderer::rasterize_shape_layer");
        let ShapeRasterRequest {
            path_data,
            canonical_path,
            styles,
            path_effects,
            ensemble,
            transform,
        } = request;
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::TRANSPARENT);
            let path =
                super::super::path_geometry::resolve_renderer_path(canonical_path, path_data)?;
            canvas.save();
            canvas.concat(&build_transform_matrix(&transform));
            if let Some(ensemble) = ensemble
                && ensemble.enabled
            {
                legacy_backplate::draw_path_backplates(
                    canvas,
                    &path,
                    &ensemble.decorator_configs,
                    &self.surface_contract,
                )?;
            }
            for config in styles {
                match &config.style {
                    DrawStyle::Fill { color, offset } => {
                        PaintFactory::new(&self.surface_contract).draw_shape_fill(
                            canvas,
                            &path,
                            color,
                            path_effects,
                            *offset,
                        )?;
                    }
                    DrawStyle::Stroke {
                        color,
                        width,
                        offset,
                        cap,
                        join,
                        miter,
                        dash_array,
                        dash_offset,
                    } => {
                        PaintFactory::new(&self.surface_contract).draw_shape_stroke(
                            canvas,
                            &path,
                            path_effects,
                            StrokeRenderConfig {
                                color,
                                width: *width,
                                offset: *offset,
                                cap,
                                join,
                                miter: *miter,
                                dash_array,
                                dash_offset: *dash_offset,
                            },
                        )?;
                    }
                }
            }
            canvas.restore();
        }
        Ok(layer)
    }

    pub(super) fn draw_native_layer_surface(
        &mut self,
        mut layer: Surface,
        opacity: f64,
        blend_mode: crate::model::BlendMode,
    ) -> Result<(), LibraryError> {
        let image = layer.image_snapshot();
        // The transient Surface owns a possible backend texture. Keep it alive
        // until the active target has retained the snapshot draw.
        let result = self.draw_skia_image_affine_with_blend(
            &image,
            &Affine2D::IDENTITY,
            opacity,
            blend_mode,
        );
        drop(image);
        drop(layer);
        result
    }
}
