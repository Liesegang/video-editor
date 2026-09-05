//! Backend-native vector layer construction and composition.
//!
//! Rasterization and direct composition deliberately share these builders so
//! the effect path can still cross the owned [`RenderOutput`] boundary while
//! the common no-effect path keeps the transient Skia surface native.

use super::vector_bounds::VectorLayerBounds;
use super::vector_path_body::PathBody;
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
        let has_content = has_content_styles(styles);
        let mask = if layer_styles::has_mask_styles(styles) {
            Some(layer_styles::LayerMask::record(
                VectorLayerBounds::text(text, font_name, size as f32, styles),
                |canvas| {
                    if has_content {
                        draw_text_body(
                            &self.surface_contract,
                            canvas,
                            text,
                            font_name,
                            size as f32,
                            styles,
                        )
                    } else {
                        draw_text_silhouette(
                            &self.surface_contract,
                            canvas,
                            text,
                            font_name,
                            size as f32,
                        )
                    }
                },
            )?)
        } else {
            None
        };
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::TRANSPARENT);
            canvas.save();
            canvas.concat(&build_transform_matrix(&transform));
            if let Some(mask) = &mask {
                self.draw_mask_style_phase(
                    canvas,
                    styles,
                    layer_styles::CompositePhase::Underlay,
                    mask,
                )?;
                if has_content {
                    mask.draw_content(canvas);
                }
                self.draw_mask_style_phase(
                    canvas,
                    styles,
                    layer_styles::CompositePhase::Overlay,
                    mask,
                )?;
            } else {
                draw_text_body(
                    &self.surface_contract,
                    canvas,
                    text,
                    font_name,
                    size as f32,
                    styles,
                )?;
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
                | EffectorConfig::Randomize { target, .. }
                | EffectorConfig::Tracking { target, .. } => target,
            };
            if *target == EffectorTarget::Parts {
                return Err(LibraryError::Render(
                    "Ensemble EffectorTarget::Parts is not supported".to_string(),
                ));
            }
        }

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
        let has_content = has_content_styles(styles);
        let mask = if layer_styles::has_mask_styles(styles) {
            Some(layer_styles::LayerMask::record(
                VectorLayerBounds::ensemble(&runtime_text, &character_transforms, &font, styles),
                |canvas| {
                    if has_content {
                        draw_ensemble_text_body(
                            &self.surface_contract,
                            canvas,
                            &runtime_text,
                            &character_transforms,
                            &font,
                            styles,
                        )
                    } else {
                        draw_ensemble_text_silhouette(
                            &self.surface_contract,
                            canvas,
                            &runtime_text,
                            &character_transforms,
                            &font,
                        )
                    }
                },
            )?)
        } else {
            None
        };

        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::TRANSPARENT);
            canvas.save();
            canvas.concat(&build_transform_matrix(&transform));

            legacy_backplate::draw_text_backplates(
                canvas,
                &runtime_text,
                &character_transforms,
                &ensemble_data.decorator_configs,
                &self.surface_contract,
            )?;
            if let Some(mask) = &mask {
                self.draw_mask_style_phase(
                    canvas,
                    styles,
                    layer_styles::CompositePhase::Underlay,
                    mask,
                )?;
                if has_content {
                    mask.draw_content(canvas);
                }
                self.draw_mask_style_phase(
                    canvas,
                    styles,
                    layer_styles::CompositePhase::Overlay,
                    mask,
                )?;
            } else {
                draw_ensemble_text_body(
                    &self.surface_contract,
                    canvas,
                    &runtime_text,
                    &character_transforms,
                    &font,
                    styles,
                )?;
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
            parts,
            styles,
            path_effects,
            ensemble,
            transform,
        } = request;
        let body = PathBody::resolve(canonical_path, path_data, parts)?;
        let has_content = has_content_styles(styles);
        let mask = if layer_styles::has_mask_styles(styles) {
            Some(layer_styles::LayerMask::record(
                VectorLayerBounds::path(body.bounds(), styles, path_effects),
                |canvas| {
                    if has_content {
                        body.draw_body(&self.surface_contract, canvas, path_effects, styles)
                    } else {
                        body.draw_silhouette(&self.surface_contract, canvas, path_effects)
                    }
                },
            )?)
        } else {
            None
        };
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::TRANSPARENT);
            canvas.save();
            canvas.concat(&build_transform_matrix(&transform));
            if let Some(ensemble) = ensemble
                && ensemble.enabled
            {
                legacy_backplate::draw_path_backplates(
                    canvas,
                    body.aggregate_path(),
                    &ensemble.decorator_configs,
                    &self.surface_contract,
                )?;
            }
            if let Some(mask) = &mask {
                self.draw_mask_style_phase(
                    canvas,
                    styles,
                    layer_styles::CompositePhase::Underlay,
                    mask,
                )?;
                if has_content {
                    mask.draw_content(canvas);
                }
                self.draw_mask_style_phase(
                    canvas,
                    styles,
                    layer_styles::CompositePhase::Overlay,
                    mask,
                )?;
            } else {
                body.draw_body(&self.surface_contract, canvas, path_effects, styles)?;
            }
            canvas.restore();
        }
        Ok(layer)
    }

    fn draw_mask_style_phase(
        &mut self,
        canvas: &Canvas,
        styles: &[crate::model::frame::entity::StyleConfig],
        phase: layer_styles::CompositePhase,
        mask: &layer_styles::LayerMask,
    ) -> Result<(), LibraryError> {
        layer_styles::visit_phase(styles, phase, |config| {
            layer_styles::LayerStyleRenderer::new(&self.surface_contract, &mut self.blend_runtime)
                .draw(canvas, &config.style, mask)
        })
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

fn draw_text_body(
    contract: &SkiaSurfaceContract,
    canvas: &Canvas,
    text: &str,
    font_name: &str,
    size: f32,
    styles: &[crate::model::frame::entity::StyleConfig],
) -> Result<(), LibraryError> {
    for config in styles {
        if config.style.composite_phase() != layer_styles::CompositePhase::Body {
            continue;
        }
        let paint = PaintFactory::new(contract).text_paint(&config.style, 1.0, None)?;
        let paragraph = build_text_paragraph(text, font_name, size, Some(&paint));
        paragraph.paint(canvas, (0.0, 0.0));
    }
    Ok(())
}

fn draw_text_silhouette(
    contract: &SkiaSurfaceContract,
    canvas: &Canvas,
    text: &str,
    font_name: &str,
    size: f32,
) -> Result<(), LibraryError> {
    let mut paint = Paint::default();
    skia_working_surface::set_paint_authored_color(&mut paint, contract, &Color::white(), 1.0)?;
    paint.set_anti_alias(true);
    let paragraph = build_text_paragraph(text, font_name, size, Some(&paint));
    paragraph.paint(canvas, (0.0, 0.0));
    Ok(())
}

fn draw_ensemble_text_body(
    contract: &SkiaSurfaceContract,
    canvas: &Canvas,
    runtime_text: &crate::model::frame::runtime_shape::RuntimeTextShape,
    transforms: &[crate::core::ensemble::TransformData],
    font: &skia_safe::Font,
    styles: &[crate::model::frame::entity::StyleConfig],
) -> Result<(), LibraryError> {
    for config in styles {
        if config.style.composite_phase() != layer_styles::CompositePhase::Body {
            continue;
        }
        for (character, transform) in runtime_text.elements.iter().zip(transforms) {
            let center = Point::new(
                character.bounds.left + character.advance / 2.0,
                (character.bounds.top + character.bounds.bottom) / 2.0,
            );
            canvas.save();
            canvas.translate((center.x, center.y));
            canvas.translate(transform.translate);
            canvas.rotate(transform.rotate, None);
            canvas.scale(transform.scale);
            canvas.translate((-center.x, -center.y));
            let paint = PaintFactory::new(contract).text_paint(
                &config.style,
                transform.opacity,
                transform.color_override.as_ref(),
            )?;
            // TODO: draw SkParagraph shaping runs with source mapping.
            canvas.draw_str(
                &character.source,
                (character.bounds.left, character.baseline),
                font,
                &paint,
            );
            canvas.restore();
        }
    }
    Ok(())
}

fn draw_ensemble_text_silhouette(
    contract: &SkiaSurfaceContract,
    canvas: &Canvas,
    runtime_text: &crate::model::frame::runtime_shape::RuntimeTextShape,
    transforms: &[crate::core::ensemble::TransformData],
    font: &skia_safe::Font,
) -> Result<(), LibraryError> {
    for (character, transform) in runtime_text.elements.iter().zip(transforms) {
        let center = Point::new(
            character.bounds.left + character.advance / 2.0,
            (character.bounds.top + character.bounds.bottom) / 2.0,
        );
        canvas.save();
        canvas.translate((center.x, center.y));
        canvas.translate(transform.translate);
        canvas.rotate(transform.rotate, None);
        canvas.scale(transform.scale);
        canvas.translate((-center.x, -center.y));
        let mut paint = Paint::default();
        skia_working_surface::set_paint_authored_color(
            &mut paint,
            contract,
            &Color::white(),
            transform.opacity,
        )?;
        paint.set_anti_alias(true);
        canvas.draw_str(
            &character.source,
            (character.bounds.left, character.baseline),
            font,
            &paint,
        );
        canvas.restore();
    }
    Ok(())
}

fn has_content_styles(styles: &[crate::model::frame::entity::StyleConfig]) -> bool {
    styles
        .iter()
        .any(|config| config.style.composite_phase() == layer_styles::CompositePhase::Body)
}
