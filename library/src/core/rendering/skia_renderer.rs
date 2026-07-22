use crate::cache::SharedCacheManager;
use crate::error::LibraryError;
use crate::model::frame::Image;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{CapType, DrawStyle, JoinType, PathEffect};
use crate::model::frame::runtime_shape::evaluate_text_element_transforms;
use crate::rendering::blend::{BlendRuntime, with_restored_canvas};
use crate::rendering::renderer::{
    Affine2D, RenderOutput, Renderer, ShapeRasterRequest, TextRasterRequest, TextureInfo,
};
use crate::rendering::shader_utils::{self, ShaderContext};
use crate::rendering::skia_utils::{
    GpuContext, create_gpu_context, create_image_from_texture, create_surface, image_to_skia,
    surface_to_image,
};
use crate::rendering::text_layout::{build_text_paragraph, layout_runtime_text_shape};
use crate::util::timing::ScopedTimer;
use log::{debug, trace};
use skia_safe::path_effect::PathEffect as SkPathEffect;
use skia_safe::trim_path_effect::Mode;

use skia_safe::{
    AlphaType, Canvas, Color as SkColor, ColorType, CubicResampler, ISize, ImageInfo, Matrix,
    Paint, PaintStyle, Point, SamplingOptions, Surface,
};

mod legacy_backplate;

pub struct SkiaRenderer {
    width: u32,
    height: u32,
    background_color: Color,
    surface: Surface,
    group_surfaces: Vec<GroupSurface>,
    blend_runtime: BlendRuntime,
    gpu_context: Option<GpuContext>,
    sharing_handle: Option<usize>,
    sharing_hwnd: Option<isize>,
}

struct GroupSurface {
    width: u32,
    height: u32,
    surface: Surface,
}

struct StrokeRenderConfig<'a> {
    color: &'a Color,
    width: f64,
    offset: f64,
    cap: &'a CapType,
    join: &'a JoinType,
    miter: f64,
    dash_array: &'a [f64],
    dash_offset: f64,
}

impl SkiaRenderer {
    pub fn render_to_texture(&mut self) -> Result<TextureInfo, LibraryError> {
        let _timer = ScopedTimer::debug("SkiaRenderer::render_to_texture");
        if let Some(context) = self.gpu_context.as_mut() {
            context.direct_context.flush_and_submit();

            // Get the backend texture from the surface if possible
            if let Some(texture) = skia_safe::gpu::surfaces::get_backend_texture(
                &mut self.surface,
                skia_safe::surface::BackendHandleAccess::FlushRead,
            ) && let Some(gl_info) = texture.gl_texture_info()
            {
                return Ok(TextureInfo {
                    texture_id: gl_info.id,
                    width: self.width,
                    height: self.height,
                });
            }
            Err(LibraryError::Render(
                "Failed to get GL texture info".to_string(),
            ))
        } else {
            Err(LibraryError::Render(
                "GPU context not available".to_string(),
            ))
        }
    }

    fn create_stroke_paint(
        color: &Color,
        width: f32,
        cap: &CapType,
        join: &JoinType,
        miter: f32,
    ) -> Paint {
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_color(skia_safe::Color::from_argb(
            color.a, color.r, color.g, color.b,
        ));
        paint.set_style(PaintStyle::Stroke);
        paint.set_stroke_width(width);
        paint.set_stroke_cap(match cap {
            CapType::Round => skia_safe::paint::Cap::Round,
            CapType::Square => skia_safe::paint::Cap::Square,
            CapType::Butt => skia_safe::paint::Cap::Butt,
        });
        paint.set_stroke_join(match join {
            JoinType::Round => skia_safe::paint::Join::Round,
            JoinType::Bevel => skia_safe::paint::Join::Bevel,
            JoinType::Miter => skia_safe::paint::Join::Miter,
        });
        paint.set_stroke_miter(miter);
        paint
    }

    /// Build the Skia paint used by every text rendering path. Ensemble text
    /// changes grapheme drawing and per-element transforms; enabling it
    /// must not silently discard the node's authored Fill/Stroke stack.
    fn create_text_paint(style: &DrawStyle, opacity: f32, color_override: Option<&Color>) -> Paint {
        let apply_opacity = |color: &Color| {
            let color = color_override.unwrap_or(color);
            Color {
                r: color.r,
                g: color.g,
                b: color.b,
                a: (f32::from(color.a) * opacity).clamp(0.0, 255.0) as u8,
            }
        };

        match style {
            DrawStyle::Fill { color, offset } => {
                let color = apply_opacity(color);
                let mut paint = Paint::default();
                paint.set_color(SkColor::from_argb(color.a, color.r, color.g, color.b));
                if *offset > 0.0 {
                    paint.set_style(PaintStyle::StrokeAndFill);
                    paint.set_stroke_width((*offset * 2.0) as f32);
                    paint.set_stroke_join(skia_safe::paint::Join::Round);
                } else {
                    paint.set_style(PaintStyle::Fill);
                }
                paint.set_anti_alias(true);
                paint
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
                let color = apply_opacity(color);
                let effective_width = (width + offset * 2.0).max(0.0);
                let mut paint = Self::create_stroke_paint(
                    &color,
                    effective_width as f32,
                    cap,
                    join,
                    *miter as f32,
                );
                if !dash_array.is_empty() {
                    let intervals = dash_array
                        .iter()
                        .map(|value| *value as f32)
                        .collect::<Vec<_>>();
                    if let Some(effect) = SkPathEffect::dash(&intervals, *dash_offset as f32) {
                        paint.set_path_effect(effect);
                    }
                }
                paint
            }
        }
    }

    fn snapshot_surface(
        surface: &mut Surface,
        width: u32,
        height: u32,
    ) -> Result<RenderOutput, LibraryError> {
        // These surfaces are local temporaries. Returning their backend texture
        // ID would leave a dangling RenderOutput as soon as the Surface drops.
        // The root surface remains alive and may still be finalized as a GPU
        // texture for Preview; transient layers cross this boundary as Images.
        let image = surface_to_image(surface, width, height)?;
        Ok(RenderOutput::Image(image))
    }
}

impl SkiaRenderer {
    pub fn new(
        width: u32,
        height: u32,
        background_color: Color,
        use_gpu: bool,
        existing_context: Option<GpuContext>,
        _cache_manager: Option<SharedCacheManager>,
    ) -> Result<Self, LibraryError> {
        let mut gpu_context = if use_gpu {
            if let Some(mut ctx) = existing_context {
                debug!("SkiaRenderer: Reusing existing GPU context");
                ctx.resize(width, height);
                Some(ctx)
            } else if let Some(mut ctx) = create_gpu_context(None, None) {
                debug!("SkiaRenderer: Created new GPU context");
                ctx.resize(width, height);
                Some(ctx)
            } else {
                debug!("SkiaRenderer: GPU context creation failed, falling back to CPU");
                None
            }
        } else {
            None
        };

        if gpu_context.is_some() {
            debug!("SkiaRenderer: GPU context enabled");
        } else {
            debug!("SkiaRenderer: using CPU raster surfaces");
        }

        let surface = create_surface(
            width,
            height,
            gpu_context.as_mut().map(|ctx| &mut ctx.direct_context),
        )
        .map_err(|error| {
            LibraryError::Render(format!(
                "Cannot create Skia surface {width}x{height}: {error}"
            ))
        })?;

        let mut renderer = SkiaRenderer {
            width,
            height,
            background_color,
            surface,
            group_surfaces: Vec::new(),
            blend_runtime: BlendRuntime::new(),
            gpu_context,
            sharing_handle: None,
            sharing_hwnd: None,
        };
        renderer.clear().map_err(|error| {
            LibraryError::Render(format!("Failed to clear render target: {error}"))
        })?;
        Ok(renderer)
    }

    pub fn resize_render_target(
        &mut self,
        width: u32,
        height: u32,
        background_color: Color,
    ) -> Result<(), LibraryError> {
        let mut surface = create_surface(
            width,
            height,
            self.gpu_context
                .as_mut()
                .map(|context| &mut context.direct_context),
        )
        .map_err(|error| {
            LibraryError::Render(format!(
                "Cannot resize Skia surface to {width}x{height}: {error}"
            ))
        })?;
        surface.canvas().clear(SkColor::from_argb(
            background_color.a,
            background_color.r,
            background_color.g,
            background_color.b,
        ));
        if let Some(context) = self.gpu_context.as_mut() {
            context.resize(width, height);
        }

        self.width = width;
        self.height = height;
        self.background_color = background_color;
        self.surface = surface;
        self.group_surfaces.clear();
        Ok(())
    }

    fn background_sk_color(&self) -> SkColor {
        SkColor::from_argb(
            self.background_color.a,
            self.background_color.r,
            self.background_color.g,
            self.background_color.b,
        )
    }

    fn create_layer_surface(&mut self) -> Result<Surface, LibraryError> {
        let (width, height) = self.current_target_dimensions();
        create_surface(
            width,
            height,
            self.gpu_context.as_mut().map(|ctx| &mut ctx.direct_context),
        )
    }

    fn current_target_dimensions(&self) -> (u32, u32) {
        self.group_surfaces
            .last()
            .map(|group| (group.width, group.height))
            .unwrap_or((self.width, self.height))
    }

    fn replace_render_target(
        &mut self,
        mut gpu_context: Option<GpuContext>,
        sharing_handle: Option<usize>,
        sharing_hwnd: Option<isize>,
        create: impl FnOnce(Option<&mut skia_safe::gpu::DirectContext>) -> Result<Surface, LibraryError>,
    ) -> Result<(), LibraryError> {
        let surface = create(
            gpu_context
                .as_mut()
                .map(|context| &mut context.direct_context),
        )?;
        self.surface = surface;
        self.gpu_context = gpu_context;
        self.sharing_handle = sharing_handle;
        self.sharing_hwnd = sharing_hwnd;
        Ok(())
    }

    fn draw_shape_fill_on_canvas(
        &self,
        canvas: &Canvas,
        path: &skia_safe::Path,
        color: &Color,
        path_effects: &[PathEffect],
        offset: f64,
    ) -> Result<(), LibraryError> {
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_color(skia_safe::Color::from_argb(
            color.a, color.r, color.g, color.b,
        ));
        apply_path_effects(path_effects, &mut paint)?;

        if offset >= 0.0 {
            // Positive offset: Stroke and Fill to expand
            if offset > 0.0 {
                paint.set_style(PaintStyle::StrokeAndFill);
                paint.set_stroke_width((offset * 2.0) as f32);
                paint.set_stroke_join(skia_safe::paint::Join::Round);
            } else {
                paint.set_style(PaintStyle::Fill);
            }
            canvas.draw_path(path, &paint);
        } else {
            // Negative offset: Draw Fill, then Erase edges
            // 1. Draw original Fill
            paint.set_style(PaintStyle::Fill);
            canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default());
            canvas.draw_path(path, &paint);

            // 2. Erase (DstOut) the border stroke
            let mut erase_paint = Paint::default();
            erase_paint.set_anti_alias(true);
            erase_paint.set_style(PaintStyle::Stroke);
            erase_paint.set_stroke_width((-offset * 2.0) as f32);
            erase_paint.set_stroke_join(skia_safe::paint::Join::Round);
            erase_paint.set_blend_mode(skia_safe::BlendMode::DstOut);

            apply_path_effects(path_effects, &mut erase_paint)?;

            canvas.draw_path(path, &erase_paint);
            canvas.restore();
        }
        Ok(())
    }

    fn draw_shape_stroke_on_canvas(
        &self,
        canvas: &Canvas,
        path: &skia_safe::Path,
        path_effects: &[PathEffect],
        config: StrokeRenderConfig<'_>,
    ) -> Result<(), LibraryError> {
        let StrokeRenderConfig {
            color,
            width,
            offset,
            cap,
            join,
            miter,
            dash_array,
            dash_offset,
        } = config;
        if width <= 0.0 {
            return Ok(());
        }

        // Prepare base stroke paint
        let mut stroke_paint =
            Self::create_stroke_paint(color, width as f32, cap, join, miter as f32);

        // Stroke dash runs after upstream Shape operations.
        let mut effects_to_apply = path_effects.to_vec();
        if !dash_array.is_empty() {
            effects_to_apply.push(PathEffect::Dash {
                intervals: dash_array.to_vec(),
                phase: dash_offset,
            });
        }

        if offset == 0.0 {
            // Standard Stroke
            stroke_paint.set_style(PaintStyle::Stroke);
            stroke_paint.set_stroke_width(width as f32);
            apply_path_effects(&effects_to_apply, &mut stroke_paint)?;
            canvas.draw_path(path, &stroke_paint);
            return Ok(());
        }

        // Offset Stroke Logic
        let outer_r = offset.abs() + width / 2.0;
        let inner_r = offset.abs() - width / 2.0;

        canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default()); // Isolate blending

        // Setup Clipping
        if offset > 0.0 {
            canvas.clip_path(path, skia_safe::ClipOp::Difference, true);
        } else {
            canvas.clip_path(path, skia_safe::ClipOp::Intersect, true);
        }

        // Apply path effects to paint before drawing
        apply_path_effects(&effects_to_apply, &mut stroke_paint)?;

        // Draw Outer (Base)
        stroke_paint.set_style(PaintStyle::Stroke);
        stroke_paint.set_stroke_width((outer_r * 2.0) as f32);
        canvas.draw_path(path, &stroke_paint);

        // Erase Inner (Hole)
        if inner_r > 0.0 {
            let mut erase_paint = stroke_paint.clone();
            erase_paint.set_blend_mode(skia_safe::BlendMode::DstOut);
            erase_paint.set_stroke_width((inner_r * 2.0) as f32);
            canvas.draw_path(path, &erase_paint);
        }

        canvas.restore();
        Ok(())
    }

    /// Render text with ensemble effectors and decorators.
    fn rasterize_ensemble_text(
        &mut self,
        request: TextRasterRequest<'_>,
        ensemble_data: &crate::core::ensemble::EnsembleData,
    ) -> Result<RenderOutput, LibraryError> {
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
        let (target_width, target_height) = self.current_target_dimensions();
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::from_argb(0, 0, 0, 0));

            let matrix = build_transform_matrix(&transform);
            canvas.save();
            canvas.concat(&matrix);

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
            let elements = &runtime_text.elements;

            let character_transforms =
                evaluate_text_element_transforms(&runtime_text, ensemble_data, current_time)?;

            legacy_backplate::draw_text_backplates(
                canvas,
                &runtime_text,
                &character_transforms,
                &ensemble_data.decorator_configs,
            )?;

            for (character, character_transform) in elements.iter().zip(&character_transforms) {
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
                    let paint = Self::create_text_paint(
                        &config.style,
                        character_transform.opacity,
                        character_transform.color_override.as_ref(),
                    );
                    // TODO: draw SkParagraph shaping runs with source mapping.
                    // Per-grapheme draw_str cannot preserve cross-element
                    // ligatures or contextual forms in complex scripts.
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
        Self::snapshot_surface(&mut layer, target_width, target_height)
    }
}

fn build_transform_matrix(transform: &Affine2D) -> Matrix {
    Matrix::new_all(
        transform.scale_x as f32,
        transform.skew_x as f32,
        transform.translate_x as f32,
        transform.skew_y as f32,
        transform.scale_y as f32,
        transform.translate_y as f32,
        0.0,
        0.0,
        1.0,
    )
}

fn convert_path_effect(path_effect: &PathEffect) -> Result<skia_safe::PathEffect, LibraryError> {
    match path_effect {
        PathEffect::Dash { intervals, phase } => {
            let intervals: Vec<f32> = intervals.iter().map(|&x| x as f32).collect();
            Ok(
                SkPathEffect::dash(&intervals, *phase as f32).ok_or(LibraryError::Render(
                    "Failed to create PathEffect".to_string(),
                ))?,
            )
        }
        PathEffect::Corner { radius } => Ok(SkPathEffect::corner_path(*radius as f32).ok_or(
            LibraryError::Render("Failed to create PathEffect".to_string()),
        )?),
        PathEffect::Discrete {
            seg_length,
            deviation,
            seed,
        } => Ok(
            SkPathEffect::discrete(*seg_length as f32, *deviation as f32, *seed as u32).ok_or(
                LibraryError::Render("Failed to create PathEffect".to_string()),
            )?,
        ),
        PathEffect::Trim { start, end } => {
            Ok(
                SkPathEffect::trim(*start as f32, *end as f32, Mode::Normal).ok_or(
                    LibraryError::Render("Failed to create PathEffect".to_string()),
                )?,
            )
        }
    }
}

fn apply_path_effects(path_effects: &[PathEffect], paint: &mut Paint) -> Result<(), LibraryError> {
    if !path_effects.is_empty() {
        let mut composed_effect: Option<skia_safe::PathEffect> = None;
        for effect in path_effects {
            trace!("Applying path effect {:?}", effect);
            match convert_path_effect(effect) {
                Ok(sk_path_effect) => {
                    composed_effect = match composed_effect {
                        // compose(outer, inner) evaluates graph upstream first.
                        Some(upstream) => Some(SkPathEffect::compose(sk_path_effect, upstream)),
                        None => Some(sk_path_effect),
                    };
                }
                Err(e) => {
                    log::warn!("Failed to apply path effect {:?}: {}", effect, e);
                }
            }
        }
        if let Some(composed) = composed_effect {
            paint.set_path_effect(composed);
        }
    }
    Ok(())
}

impl Renderer for SkiaRenderer {
    fn draw_layer_affine_with_blend(
        &mut self,
        layer: &RenderOutput,
        transform: &Affine2D,
        opacity: f64,
        blend_mode: crate::model::BlendMode,
    ) -> Result<(), LibraryError> {
        let _timer = ScopedTimer::debug("SkiaRenderer::draw_layer");

        let src_image = match layer {
            RenderOutput::Image(img) => image_to_skia(img)?,
            RenderOutput::Texture(info) => {
                if let Some(ctx) = self.gpu_context.as_mut() {
                    create_image_from_texture(
                        &mut ctx.direct_context,
                        info.texture_id,
                        info.width,
                        info.height,
                    )?
                } else {
                    return Err(LibraryError::Render(
                        "Cannot render texture without GPU context".to_string(),
                    ));
                }
            }
        };

        let matrix = build_transform_matrix(transform);
        let identity = *transform == Affine2D::IDENTITY;
        let sampling = if identity {
            SamplingOptions::default()
        } else {
            SamplingOptions::from(CubicResampler::mitchell())
        };
        let blend_runtime = &mut self.blend_runtime;
        let canvas: &Canvas = if let Some(group) = self.group_surfaces.last_mut() {
            group.surface.canvas()
        } else {
            self.surface.canvas()
        };

        with_restored_canvas(canvas, |canvas| {
            canvas.concat(&matrix);
            blend_runtime.draw_image(
                canvas,
                &src_image,
                sampling,
                identity,
                opacity.clamp(0.0, 1.0) as f32,
                blend_mode,
            )
        })?;

        Ok(())
    }

    fn begin_group(
        &mut self,
        width: u32,
        height: u32,
        background_color: &Color,
    ) -> Result<(), LibraryError> {
        let width = width.max(1);
        let height = height.max(1);
        let mut surface = create_surface(
            width,
            height,
            self.gpu_context
                .as_mut()
                .map(|context| &mut context.direct_context),
        )?;
        surface.canvas().clear(SkColor::from_argb(
            background_color.a,
            background_color.r,
            background_color.g,
            background_color.b,
        ));
        self.group_surfaces.push(GroupSurface {
            width,
            height,
            surface,
        });
        Ok(())
    }

    fn end_group(&mut self) -> Result<RenderOutput, LibraryError> {
        let mut group = self.group_surfaces.pop().ok_or_else(|| {
            LibraryError::Render("end_group called without a matching begin_group".to_string())
        })?;

        // A texture ID is owned by its Surface. Read the isolated target before
        // dropping it so nested groups cannot leave dangling GPU texture IDs.
        let image = surface_to_image(&mut group.surface, group.width, group.height)?;
        Ok(RenderOutput::Image(image))
    }

    fn rasterize_sksl_layer(
        &mut self,
        shader_code: &str,
        resolution: (f32, f32),
        time: f32,
        transform: &Affine2D,
    ) -> Result<RenderOutput, LibraryError> {
        let (target_width, target_height) = self.current_target_dimensions();
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::TRANSPARENT);

            let preprocessed_code = shader_utils::preprocess_shader(shader_code);
            let result = skia_safe::RuntimeEffect::make_for_shader(&preprocessed_code, None);

            if let Err(error) = result {
                log::error!(
                    "SkSL Compilation Error: {}\nCode:\n{}",
                    error,
                    preprocessed_code
                );
                canvas.clear(skia_safe::Color::RED);
            } else if let Ok(effect) = result {
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

                let shader =
                    effect
                        .make_shader(uniforms, &[], None)
                        .ok_or(LibraryError::Render(
                            "Failed to create SkSL shader".to_string(),
                        ))?;

                let mut paint = Paint::default();
                paint.set_shader(shader);
                let matrix = build_transform_matrix(transform);
                canvas.save();
                canvas.concat(&matrix);
                let rect = skia_safe::Rect::from_wh(resolution.0, resolution.1);
                canvas.draw_rect(rect, &paint);
                canvas.restore();
            }
        }

        Self::snapshot_surface(&mut layer, target_width, target_height)
    }

    fn rasterize_text_layer(
        &mut self,
        request: TextRasterRequest<'_>,
    ) -> Result<RenderOutput, LibraryError> {
        let _timer = ScopedTimer::debug(format!(
            "SkiaRenderer::rasterize_text_layer len={} size={} ensemble={}",
            request.text.len(),
            request.size,
            request.ensemble.is_some()
        ));

        // If ensemble is enabled, use ensemble rendering
        if let Some(ensemble_data) = request.ensemble
            && ensemble_data.enabled
        {
            return self.rasterize_ensemble_text(request, ensemble_data);
        }

        let TextRasterRequest {
            text,
            size,
            font_name,
            styles,
            transform,
            ..
        } = request;

        // Standard text rendering (existing code)
        let (target_width, target_height) = self.current_target_dimensions();
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::from_argb(0, 0, 0, 0));

            let matrix = build_transform_matrix(&transform);
            canvas.save();
            canvas.concat(&matrix);

            for config in styles {
                let style = &config.style;
                let paint = Self::create_text_paint(style, 1.0, None);
                let paragraph = build_text_paragraph(text, font_name, size as f32, Some(&paint));
                paragraph.paint(canvas, (0.0, 0.0));
            }

            canvas.restore();
        }
        Self::snapshot_surface(&mut layer, target_width, target_height)
    }

    fn rasterize_shape_layer(
        &mut self,
        request: ShapeRasterRequest<'_>,
    ) -> Result<RenderOutput, LibraryError> {
        let _timer = ScopedTimer::debug("SkiaRenderer::rasterize_shape_layer");
        let ShapeRasterRequest {
            path_data,
            canonical_path,
            styles,
            path_effects,
            ensemble,
            transform,
        } = request;
        let (target_width, target_height) = self.current_target_dimensions();
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::from_argb(0, 0, 0, 0));
            let path = super::path_geometry::resolve_renderer_path(canonical_path, path_data)?;
            let matrix = build_transform_matrix(&transform);
            canvas.save();
            canvas.concat(&matrix);
            if let Some(ensemble) = ensemble
                && ensemble.enabled
            {
                legacy_backplate::draw_path_backplates(canvas, &path, &ensemble.decorator_configs)?;
            }
            for config in styles {
                let style = &config.style;
                match style {
                    DrawStyle::Fill { color, offset } => {
                        self.draw_shape_fill_on_canvas(
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
                        self.draw_shape_stroke_on_canvas(
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
        Self::snapshot_surface(&mut layer, target_width, target_height)
    }

    fn read_surface(&mut self, output: &RenderOutput) -> Result<Image, LibraryError> {
        match output {
            RenderOutput::Image(img) => Ok(img.clone()),
            RenderOutput::Texture(info) => {
                if let Some(ctx) = self.gpu_context.as_mut() {
                    let image = create_image_from_texture(
                        &mut ctx.direct_context,
                        info.texture_id,
                        info.width,
                        info.height,
                    )?;
                    // Read pixels
                    let row_bytes = (info.width * 4) as usize;
                    let mut buffer = vec![0u8; (info.height as usize) * row_bytes];
                    let image_info = ImageInfo::new(
                        ISize::new(info.width as i32, info.height as i32),
                        ColorType::RGBA8888,
                        AlphaType::Unpremul,
                        None,
                    );
                    if !image.read_pixels(
                        &image_info,
                        &mut buffer,
                        row_bytes,
                        (0, 0),
                        skia_safe::image::CachingHint::Disallow,
                    ) {
                        return Err(LibraryError::Render(
                            "Failed to read texture pixels".to_string(),
                        ));
                    }
                    Ok(Image::new(info.width, info.height, buffer))
                } else {
                    Err(LibraryError::Render(
                        "No GPU context to read texture".to_string(),
                    ))
                }
            }
        }
    }

    fn finalize(&mut self) -> Result<RenderOutput, LibraryError> {
        let _timer = ScopedTimer::debug(format!(
            "SkiaRenderer::finalize {}x{}",
            self.width, self.height
        ));

        if !self.group_surfaces.is_empty() {
            return Err(LibraryError::Render(
                "Cannot finalize with unfinished frame groups".to_string(),
            ));
        }

        if let Some(context) = self.gpu_context.as_mut() {
            context.direct_context.flush_and_submit();
        }

        // If sharing is enabled, attempt to return a Texture.
        if self.sharing_handle.is_some()
            && self.gpu_context.is_some()
            && let Some(texture) = skia_safe::gpu::surfaces::get_backend_texture(
                &mut self.surface,
                skia_safe::surface::BackendHandleAccess::FlushRead,
            )
            && let Some(gl_info) = texture.gl_texture_info()
        {
            return Ok(RenderOutput::Texture(TextureInfo {
                texture_id: gl_info.id,
                width: self.width,
                height: self.height,
            }));
        }

        // Fallback to Image readback (slow, copy)
        let image = surface_to_image(&mut self.surface, self.width, self.height)?;
        Ok(RenderOutput::Image(image))
    }

    fn clear(&mut self) -> Result<(), LibraryError> {
        let _timer = ScopedTimer::debug("SkiaRenderer::clear");
        self.group_surfaces.clear();
        let color = self.background_sk_color();
        let canvas: &Canvas = self.surface.canvas();
        canvas.clear(color);
        Ok(())
    }

    fn get_gpu_context(&mut self) -> Option<&mut crate::rendering::skia_utils::GpuContext> {
        self.gpu_context.as_mut()
    }

    fn set_sharing_context(
        &mut self,
        handle: usize,
        hwnd: Option<isize>,
    ) -> Result<(), LibraryError> {
        if self.sharing_handle == Some(handle) {
            return Ok(());
        }

        log::info!(
            "SkiaRenderer: Setting sharing context handle: {}, hwnd: {:?}",
            handle,
            hwnd
        );
        let mut context = create_gpu_context(Some(handle), hwnd).ok_or_else(|| {
            LibraryError::Render(format!(
                "Cannot create shared GPU context for handle {handle}"
            ))
        })?;
        context.resize(self.width, self.height);
        let (width, height) = (self.width, self.height);
        self.replace_render_target(Some(context), Some(handle), hwnd, |direct_context| {
            create_surface(width, height, direct_context).map_err(|error| {
                LibraryError::Render(format!(
                    "Cannot create shared Skia surface {width}x{height}: {error}"
                ))
            })
        })?;
        log::info!("SkiaRenderer: Recreated GPU context with sharing enabled.");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{RenderOutput, Renderer, SkiaRenderer};
    use crate::error::LibraryError;
    use crate::model::frame::color::Color;

    #[test]
    fn construction_returns_an_error_for_invalid_dimensions() {
        let result = SkiaRenderer::new(0, 0, Color::black(), false, None, None);
        assert!(matches!(result, Err(LibraryError::Render(_))));
    }

    #[test]
    fn failed_render_target_replacement_preserves_the_current_surface() {
        let mut renderer = SkiaRenderer::new(2, 2, Color::black(), false, None, None).unwrap();
        let result = renderer.replace_render_target(None, Some(99), Some(77), |_| {
            Err(LibraryError::Render(
                "injected surface creation failure".to_string(),
            ))
        });

        assert!(matches!(result, Err(LibraryError::Render(_))));
        assert_eq!(renderer.sharing_handle, None);
        assert_eq!(renderer.sharing_hwnd, None);
        renderer.clear().unwrap();
        let RenderOutput::Image(image) = renderer.finalize().unwrap() else {
            panic!("CPU renderer must retain its image surface");
        };
        assert_eq!((image.width, image.height), (2, 2));
    }
}
