use crate::error::LibraryError;
use crate::loader::image::Image;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{CapType, DrawStyle, JoinType, PathEffect};
use crate::model::frame::entity::StyleConfig;
use crate::model::frame::transform::Transform;
use crate::rendering::renderer::{RenderOutput, Renderer, TextureInfo};
use crate::rendering::shader_utils::{self, ShaderContext};
use crate::rendering::skia_utils::{
    GpuContext, create_gpu_context, create_image_from_texture, create_surface, image_to_skia,
    surface_to_image,
};
use crate::util::timing::ScopedTimer;
use log::{debug, trace};
use skia_safe::path_effect::PathEffect as SkPathEffect;
use skia_safe::trim_path_effect::Mode;

use skia_safe::{
    AlphaType, Canvas, Color as SkColor, ColorType, CubicResampler, ISize, ImageInfo, Matrix,
    Paint, PaintStyle, Point, SamplingOptions, Surface,
};

pub struct SkiaRenderer {
    width: u32,
    height: u32,
    background_color: Color,
    surface: Surface,
    gpu_context: Option<GpuContext>,
    sharing_handle: Option<usize>,
    sharing_hwnd: Option<isize>,
}

impl SkiaRenderer {
    pub fn render_to_texture(&mut self) -> Result<TextureInfo, LibraryError> {
        let _timer = ScopedTimer::debug_lazy(|| "SkiaRenderer::render_to_texture".to_string());
        if let Some(context) = self.gpu_context.as_mut() {
            context.direct_context.flush_and_submit();

            // Get the backend texture from the surface if possible
            if let Some(texture) = skia_safe::gpu::surfaces::get_backend_texture(
                &mut self.surface,
                skia_safe::surface::BackendHandleAccess::FlushRead,
            ) {
                if let Some(gl_info) = texture.gl_texture_info() {
                    return Ok(TextureInfo {
                        texture_id: gl_info.id,
                        width: self.width,
                        height: self.height,
                    });
                }
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

    fn snapshot_surface(
        surface: &mut Surface,
        gpu_context: &mut Option<GpuContext>,
        width: u32,
        height: u32,
    ) -> Result<RenderOutput, LibraryError> {
        if let Some(ctx) = gpu_context.as_mut() {
            ctx.direct_context.flush_and_submit();
            if let Some(texture) = skia_safe::gpu::surfaces::get_backend_texture(
                surface,
                skia_safe::surface::BackendHandleAccess::FlushRead,
            ) {
                if let Some(gl_info) = texture.gl_texture_info() {
                    return Ok(RenderOutput::Texture(TextureInfo {
                        texture_id: gl_info.id,
                        width,
                        height,
                    }));
                }
            }
        }

        let image = surface_to_image(surface, width, height)?;
        Ok(RenderOutput::Image(image))
    }
}

impl SkiaRenderer {
    pub fn take_context(&mut self) -> Option<GpuContext> {
        self.gpu_context.take()
    }

    pub fn new(
        width: u32,
        height: u32,
        background_color: Color,
        use_gpu: bool,
        existing_context: Option<GpuContext>,
    ) -> Self {
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
        .map_err(|e| LibraryError::Render(format!("Cannot create Skia Surface: {}", e)))
        .expect("Cannot create Skia Surface");

        let mut renderer = SkiaRenderer {
            width,
            height,
            background_color,
            surface,
            gpu_context,
            sharing_handle: None,
            sharing_hwnd: None,
        };
        renderer
            .clear()
            .map_err(|e| LibraryError::Render(format!("Failed to clear render target: {}", e)))
            .expect("Failed to clear render target");
        renderer
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
        create_surface(
            self.width,
            self.height,
            self.gpu_context.as_mut().map(|ctx| &mut ctx.direct_context),
        )
    }

    fn draw_shape_fill_on_canvas(
        &self,
        canvas: &Canvas,
        path: &skia_safe::Path,
        color: &Color,
        path_effects: &Vec<PathEffect>,
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
        color: &Color,
        path_effects: &Vec<PathEffect>,
        width: f64,
        offset: f64,
        cap: CapType,
        join: JoinType,
        miter: f64,
        dash_array: &Vec<f64>,
        dash_offset: f64,
    ) -> Result<(), LibraryError> {
        if width <= 0.0 {
            return Ok(());
        }

        // Prepare base stroke paint
        let mut stroke_paint =
            Self::create_stroke_paint(color, width as f32, &cap, &join, miter as f32);

        // Path Effects (Dash + others)
        let mut effects_to_apply = Vec::new();
        if !dash_array.is_empty() {
            effects_to_apply.push(PathEffect::Dash {
                intervals: dash_array.clone(),
                phase: dash_offset,
            });
        }
        effects_to_apply.extend_from_slice(path_effects);

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
}

fn build_transform_matrix(transform: &Transform) -> Matrix {
    let anchor = Point::new(transform.anchor.x as f32, transform.anchor.y as f32);
    let mut matrix = Matrix::new_identity();
    matrix.pre_translate((
        transform.position.x as f32 - anchor.x,
        transform.position.y as f32 - anchor.y,
    ));
    matrix.pre_rotate(transform.rotation as f32, anchor);
    matrix.pre_scale((transform.scale.x as f32, transform.scale.y as f32), anchor);
    matrix
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

fn apply_path_effects(
    path_effects: &Vec<PathEffect>,
    paint: &mut Paint,
) -> Result<(), LibraryError> {
    if !path_effects.is_empty() {
        let mut composed_effect: Option<skia_safe::PathEffect> = None;
        for effect in path_effects {
            trace!("Applying path effect {:?}", effect);
            match convert_path_effect(effect) {
                Ok(sk_path_effect) => {
                    composed_effect = match composed_effect {
                        Some(e) => Some(SkPathEffect::compose(e, sk_path_effect)),
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
    fn draw_layer(
        &mut self,
        layer: &RenderOutput,
        transform: &Transform,
    ) -> Result<(), LibraryError> {
        let _timer = ScopedTimer::debug_lazy(|| "SkiaRenderer::draw_layer".to_string());
        let canvas: &Canvas = self.surface.canvas();

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

        canvas.save();
        canvas.concat(&matrix);

        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_alpha_f(transform.opacity as f32);

        let cubic_resampler = CubicResampler::mitchell();
        let sampling = SamplingOptions::from(cubic_resampler);
        canvas.draw_image_with_sampling_options(&src_image, (0, 0), sampling, Some(&paint));

        canvas.restore();

        Ok(())
    }

    fn rasterize_sksl_layer(
        &mut self,
        shader_code: &str,
        resolution: (f32, f32),
        time: f32,
        transform: &Transform,
    ) -> Result<RenderOutput, LibraryError> {
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
                paint.set_alpha_f(transform.opacity as f32);

                let matrix = build_transform_matrix(transform);
                canvas.save();
                canvas.concat(&matrix);
                let rect = skia_safe::Rect::from_wh(resolution.0, resolution.1);
                canvas.draw_rect(rect, &paint);
                canvas.restore();
            }
        }

        Self::snapshot_surface(&mut layer, &mut self.gpu_context, self.width, self.height)
    }

    fn rasterize_text_layer(
        &mut self,
        text: &str,
        size: f64,
        font_name: &String,
        styles: &[StyleConfig],
        transform: &Transform,
    ) -> Result<RenderOutput, LibraryError> {
        let _timer = ScopedTimer::debug_lazy(|| {
            format!(
                "SkiaRenderer::rasterize_text_layer len={} size={}",
                text.len(),
                size
            )
        });
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::from_argb(0, 0, 0, 0));

            let matrix = build_transform_matrix(transform);
            canvas.save();
            canvas.concat(&matrix);

            let mut font_collection = skia_safe::textlayout::FontCollection::new();
            font_collection.set_default_font_manager(skia_safe::FontMgr::default(), None);

            for config in styles {
                let style = &config.style;
                let mut text_style = skia_safe::textlayout::TextStyle::new();
                text_style.set_font_families(&[font_name]);
                text_style.set_font_size(size as f32);

                match style {
                    DrawStyle::Fill { color, offset } => {
                        let mut paint = Paint::default();
                        paint.set_color(skia_safe::Color::from_argb(
                            color.a, color.r, color.g, color.b,
                        ));
                        // NOTE: Simple Text Expansion is handled via StrokeAndFill.
                        if *offset > 0.0 {
                            paint.set_style(PaintStyle::StrokeAndFill);
                            paint.set_stroke_width((*offset * 2.0) as f32);
                            paint.set_stroke_join(skia_safe::paint::Join::Round);
                        } else {
                            paint.set_style(PaintStyle::Fill);
                        }
                        paint.set_anti_alias(true);
                        text_style.set_foreground_paint(&paint);
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
                        let effective_width = (width + offset * 2.0).max(0.0);
                        let mut paint = Self::create_stroke_paint(
                            color,
                            effective_width as f32,
                            cap,
                            join,
                            *miter as f32,
                        );

                        if !dash_array.is_empty() {
                            let intervals: Vec<f32> =
                                dash_array.iter().map(|&x| x as f32).collect();
                            if let Some(effect) =
                                SkPathEffect::dash(&intervals, *dash_offset as f32)
                            {
                                paint.set_path_effect(effect);
                            }
                        }

                        text_style.set_foreground_paint(&paint);
                    }
                }

                let mut paragraph_style = skia_safe::textlayout::ParagraphStyle::new();
                paragraph_style.set_text_style(&text_style);

                let mut builder = skia_safe::textlayout::ParagraphBuilder::new(
                    &paragraph_style,
                    font_collection.clone(),
                );

                builder.add_text(text);

                let mut paragraph = builder.build();
                paragraph.layout(f32::MAX); // Layout on a single line (infinite width)
                paragraph.paint(canvas, (0.0, 0.0));
            }

            canvas.restore();
        }
        Self::snapshot_surface(&mut layer, &mut self.gpu_context, self.width, self.height)
    }

    fn rasterize_shape_layer(
        &mut self,
        path_data: &str,
        styles: &[StyleConfig],
        path_effects: &Vec<PathEffect>,
        transform: &Transform,
    ) -> Result<RenderOutput, LibraryError> {
        let _timer = ScopedTimer::debug_lazy(|| "SkiaRenderer::rasterize_shape_layer".to_string());
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::from_argb(0, 0, 0, 0));
            let path = skia_safe::Path::from_svg(path_data).unwrap_or_default();
            let matrix = build_transform_matrix(transform);
            canvas.save();
            canvas.concat(&matrix);
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
                            color,
                            path_effects,
                            *width,
                            *offset,
                            cap.clone(),
                            join.clone(),
                            *miter,
                            dash_array,
                            *dash_offset,
                        )?;
                    }
                }
            }
            canvas.restore();
        }
        Self::snapshot_surface(&mut layer, &mut self.gpu_context, self.width, self.height)
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
                        AlphaType::Premul,
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
                    Ok(Image {
                        width: info.width,
                        height: info.height,
                        data: buffer,
                    })
                } else {
                    Err(LibraryError::Render(
                        "No GPU context to read texture".to_string(),
                    ))
                }
            }
        }
    }

    fn finalize(&mut self) -> Result<RenderOutput, LibraryError> {
        let _timer = ScopedTimer::debug_lazy(|| {
            format!("SkiaRenderer::finalize {}x{}", self.width, self.height)
        });

        if let Some(context) = self.gpu_context.as_mut() {
            context.direct_context.flush_and_submit();
        }

        // If sharing is enabled, attempt to return a Texture.
        if self.sharing_handle.is_some() {
            if let Some(_context) = self.gpu_context.as_mut() {
                if let Some(texture) = skia_safe::gpu::surfaces::get_backend_texture(
                    &mut self.surface,
                    skia_safe::surface::BackendHandleAccess::FlushRead,
                ) {
                    if let Some(gl_info) = texture.gl_texture_info() {
                        return Ok(RenderOutput::Texture(TextureInfo {
                            texture_id: gl_info.id,
                            width: self.width,
                            height: self.height,
                        }));
                    }
                }
            }
        }

        // Fallback to Image readback (slow, copy)
        let image = surface_to_image(&mut self.surface, self.width, self.height)?;
        Ok(RenderOutput::Image(image))
    }

    fn clear(&mut self) -> Result<(), LibraryError> {
        let _timer = ScopedTimer::debug_lazy(|| "SkiaRenderer::clear".to_string());
        let color = self.background_sk_color();
        let canvas: &Canvas = self.surface.canvas();
        canvas.clear(color);
        Ok(())
    }

    fn get_gpu_context(&mut self) -> Option<&mut crate::rendering::skia_utils::GpuContext> {
        self.gpu_context.as_mut()
    }

    fn set_sharing_context(&mut self, handle: usize, hwnd: Option<isize>) {
        if self.sharing_handle != Some(handle) || self.sharing_hwnd != hwnd {
            log::info!(
                "SkiaRenderer: Setting sharing context handle: {}, hwnd: {:?}",
                handle,
                hwnd
            );
            self.sharing_handle = Some(handle);
            self.sharing_hwnd = hwnd;

            // Recreate context with sharing
            let _old_context = self.gpu_context.take(); // Drop old context
            if let Some(mut ctx) = create_gpu_context(Some(handle), hwnd) {
                ctx.resize(self.width, self.height);
                self.gpu_context = Some(ctx);

                // Recreate surface too!
                self.surface = create_surface(
                    self.width,
                    self.height,
                    self.gpu_context.as_mut().map(|ctx| &mut ctx.direct_context),
                )
                .expect("Failed to recreate surface with sharing");

                log::info!("SkiaRenderer: Recreated GPU context with sharing enabled.");
            } else {
                log::warn!(
                    "SkiaRenderer: Failed to recreate GPU context with sharing! Falling back to isolated context (CPU readback). Preview performance may be reduced."
                );
                self.sharing_handle = None; // Reset sharing handle so we fallback to CPU copy
                self.sharing_hwnd = None;
                self.gpu_context = create_gpu_context(None, None);
                self.surface = create_surface(
                    self.width,
                    self.height,
                    self.gpu_context.as_mut().map(|ctx| &mut ctx.direct_context),
                )
                .expect("Failed to recreate surface fallback");
            }
        }
    }
}
