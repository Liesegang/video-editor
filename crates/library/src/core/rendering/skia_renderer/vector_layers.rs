//! Backend-native vector layer construction and composition.
//!
//! Rasterization and direct composition deliberately share these builders so
//! the effect path can still cross the owned [`RenderOutput`] boundary while
//! the common no-effect path keeps the transient Skia surface native.

use super::vector_bounds::VectorLayerBounds;
use super::vector_path_body::PathBody;
use super::vector_surface::{NativeLayer, VectorSurfaceMode};
use super::*;
use skia_safe::{Picture, PictureRecorder};

impl SkiaRenderer {
    pub(super) fn create_sksl_layer_surface(
        &mut self,
        request: SkSLRasterRequest<'_>,
        mode: VectorSurfaceMode,
    ) -> Result<NativeLayer, LibraryError> {
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
        let local_visual = skia_safe::Rect::from_wh(resolution.0, resolution.1);
        let mut layer = self.create_vector_surface(mode, local_visual, *transform)?;
        {
            let canvas: &Canvas = layer.surface.canvas();

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
        mode: VectorSurfaceMode,
    ) -> Result<NativeLayer, LibraryError> {
        let _timer = ScopedTimer::debug(format!(
            "SkiaRenderer::rasterize_text_layer len={} size={} ensemble={}",
            request.text.len(),
            request.size,
            request.ensemble.is_some()
        ));
        let body = super::vector_text_body::TextBody::resolve(
            request.text,
            request.font_name,
            request.size as f32,
            request.ensemble,
            request.current_time as f32,
        )?;
        let decorators = request
            .ensemble
            .filter(|ensemble| ensemble.enabled)
            .map_or(&[][..], |ensemble| ensemble.decorator_configs.as_slice());
        let bounds = VectorLayerBounds::text_body(&body, request.styles, decorators)?;
        let surface_contract = &self.surface_contract;
        let picture = ordered_appearance_picture(
            surface_contract,
            &mut self.blend_runtime,
            bounds,
            request.styles,
            |canvas, style| body.draw_style(surface_contract, canvas, bounds.geometry, style),
        )?;
        let mut layer = self.create_vector_surface(mode, bounds.visual, request.transform)?;
        let canvas: &Canvas = layer.surface.canvas();
        with_restored_canvas(canvas, |canvas| -> Result<(), LibraryError> {
            canvas.concat(&build_transform_matrix(&request.transform));
            legacy_backplate::draw_text_backplates(
                canvas,
                &body.layout.metadata,
                &body.transforms,
                decorators,
                &self.surface_contract,
            )?;
            if let Some(picture) = &picture {
                canvas.draw_picture(picture, None, None);
            }
            Ok(())
        })?;
        Ok(layer)
    }

    pub(super) fn create_shape_layer_surface(
        &mut self,
        request: ShapeRasterRequest<'_>,
        mode: VectorSurfaceMode,
    ) -> Result<NativeLayer, LibraryError> {
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
        let body_bounds = body.bounds();
        let decorators = ensemble
            .filter(|ensemble| ensemble.enabled)
            .map_or(&[][..], |ensemble| ensemble.decorator_configs.as_slice());
        let bounds = VectorLayerBounds::path(body_bounds, styles, path_effects, decorators)?;
        let surface_contract = &self.surface_contract;
        let picture = ordered_appearance_picture(
            surface_contract,
            &mut self.blend_runtime,
            bounds,
            styles,
            |canvas, style| {
                body.draw_style(
                    surface_contract,
                    canvas,
                    path_effects,
                    bounds.geometry,
                    style,
                )
            },
        )?;
        let mut layer = self.create_vector_surface(mode, bounds.visual, transform)?;
        {
            let canvas: &Canvas = layer.surface.canvas();
            canvas.save();
            canvas.concat(&build_transform_matrix(&transform));
            if let Some(ensemble) = ensemble
                && ensemble.enabled
            {
                legacy_backplate::draw_path_backplates(
                    canvas,
                    body_bounds,
                    &ensemble.decorator_configs,
                    &self.surface_contract,
                )?;
            }
            if let Some(picture) = &picture {
                canvas.draw_picture(picture, None, None);
            }
            canvas.restore();
        }
        Ok(layer)
    }

    pub(super) fn draw_native_layer_surface(
        &mut self,
        mut layer: NativeLayer,
        opacity: f64,
        blend_mode: crate::model::BlendMode,
    ) -> Result<(), LibraryError> {
        let image = layer.surface.image_snapshot();
        // The transient Surface owns a possible backend texture. Keep it alive
        // until the active target has retained the snapshot draw.
        self.activate_graphics_context()?;
        let blend_runtime = &mut self.blend_runtime;
        let canvas: &Canvas = if let Some(group) = self.group_surfaces.last_mut() {
            group.surface.canvas()
        } else {
            self.surface.canvas()
        };
        let result = blend_runtime.draw_image(
            canvas,
            &image,
            layer.origin,
            true,
            opacity.clamp(0.0, 1.0) as f32,
            blend_mode,
        );
        drop(image);
        drop(layer);
        result
    }
}

fn ordered_appearance_picture(
    surface_contract: &SkiaSurfaceContract,
    blend_runtime: &mut BlendRuntime,
    bounds: VectorLayerBounds,
    styles: &[crate::model::frame::entity::StyleConfig],
    mut draw_body: impl FnMut(
        &Canvas,
        &crate::model::frame::entity::StyleConfig,
    ) -> Result<(), LibraryError>,
) -> Result<Option<Picture>, LibraryError> {
    let mut current = None;
    for style in styles {
        match style.style.composite_phase() {
            layer_styles::CompositePhase::Body => {
                let previous = current.take();
                current = Some(record_picture(bounds.visual, |canvas| {
                    if let Some(previous) = &previous {
                        canvas.draw_picture(previous, None, None);
                    }
                    draw_body(canvas, style)
                })?);
            }
            layer_styles::CompositePhase::Underlay | layer_styles::CompositePhase::Overlay => {
                let Some(previous) = current.take() else {
                    // Image -> Image effects applied before the first Shape ->
                    // Image stage have a transparent input by definition.
                    continue;
                };
                let mask = layer_styles::LayerMask::record(bounds, |canvas| {
                    canvas.draw_picture(&previous, None, None);
                    Ok(())
                })?;
                current = Some(record_picture(bounds.visual, |canvas| {
                    layer_styles::LayerStyleRenderer::new(surface_contract, blend_runtime, 1.0)
                        .compose(canvas, &style.style, &mask)
                })?);
            }
        }
    }
    Ok(current)
}

fn record_picture(
    bounds: skia_safe::Rect,
    draw: impl FnOnce(&Canvas) -> Result<(), LibraryError>,
) -> Result<Picture, LibraryError> {
    let mut recorder = PictureRecorder::new();
    let canvas = recorder.begin_recording(bounds, false);
    draw(canvas)?;
    recorder
        .finish_recording_as_picture(Some(&bounds))
        .ok_or_else(|| LibraryError::Render("Cannot record ordered Appearance stage".to_string()))
}
