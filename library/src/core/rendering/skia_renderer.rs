use crate::core::evaluation::output::ShapeGroup;
use crate::error::LibraryError;
use crate::model::frame::Image;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{DrawStyle, PathEffect};
use crate::model::frame::entity::StyleConfig;
use crate::model::frame::transform::Transform;
use crate::rendering::renderer::{RenderOutput, Renderer, TextureInfo};
use crate::rendering::shader_utils::{self, ShaderContext};
use crate::rendering::skia_utils::{
    GpuContext, create_gpu_context, create_image_from_texture, create_surface, image_to_skia,
    surface_to_image,
};
use crate::util::timing::ScopedTimer;
use log::debug;
use skia_safe::path_effect::PathEffect as SkPathEffect;

use skia_safe::{
    AlphaType, Canvas, Color as SkColor, ColorType, CubicResampler, ISize, ImageInfo, Paint,
    PaintStyle, SamplingOptions, Surface,
};

use super::{paint_utils, shape_renderer, text_renderer};

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
    pub(crate) fn render_to_texture(&mut self) -> Result<TextureInfo, LibraryError> {
        let _timer = ScopedTimer::debug("SkiaRenderer::render_to_texture");
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
            Err(LibraryError::render(
                "Failed to get GL texture info".to_string(),
            ))
        } else {
            Err(LibraryError::render(
                "GPU context not available".to_string(),
            ))
        }
    }

    pub(crate) fn take_context(&mut self) -> Option<GpuContext> {
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
        .map_err(|e| LibraryError::render(format!("Cannot create Skia Surface: {}", e)))
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
            .map_err(|e| LibraryError::render(format!("Failed to clear render target: {}", e)))
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

    /// Render text with ensemble effectors and decorators.
    fn rasterize_ensemble_text(
        &mut self,
        text: &str,
        size: f64,
        font_name: &String,
        styles: &[StyleConfig],
        ensemble_data: &crate::core::ensemble::EnsembleData,
        transform: &Transform,
        current_time: f32,
    ) -> Result<RenderOutput, LibraryError> {
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::from_argb(0, 0, 0, 0));

            text_renderer::render_ensemble_text(
                canvas,
                text,
                size,
                font_name,
                styles,
                ensemble_data,
                transform,
                current_time,
            );
        }
        paint_utils::snapshot_surface(&mut layer, &mut self.gpu_context, self.width, self.height)
    }
}

impl Renderer for SkiaRenderer {
    fn draw_layer(
        &mut self,
        layer: &RenderOutput,
        transform: &Transform,
    ) -> Result<(), LibraryError> {
        let _timer = ScopedTimer::debug("SkiaRenderer::draw_layer");
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
                    return Err(LibraryError::render(
                        "Cannot render texture without GPU context".to_string(),
                    ));
                }
            }
        };

        let matrix = paint_utils::build_transform_matrix(transform);

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
                        .ok_or(LibraryError::render(
                            "Failed to create SkSL shader".to_string(),
                        ))?;

                let mut paint = Paint::default();
                paint.set_shader(shader);
                paint.set_alpha_f(transform.opacity as f32);

                let matrix = paint_utils::build_transform_matrix(transform);
                canvas.save();
                canvas.concat(&matrix);
                let rect = skia_safe::Rect::from_wh(resolution.0, resolution.1);
                canvas.draw_rect(rect, &paint);
                canvas.restore();
            }
        }

        paint_utils::snapshot_surface(&mut layer, &mut self.gpu_context, self.width, self.height)
    }

    fn rasterize_text_layer(
        &mut self,
        text: &str,
        size: f64,
        font_name: &String,
        styles: &[StyleConfig],
        ensemble: Option<&crate::core::ensemble::EnsembleData>,
        transform: &Transform,
    ) -> Result<RenderOutput, LibraryError> {
        let _timer = ScopedTimer::debug(format!(
            "SkiaRenderer::rasterize_text_layer len={} size={} ensemble={}",
            text.len(),
            size,
            ensemble.is_some()
        ));

        // If ensemble is enabled, use ensemble rendering
        if let Some(ensemble_data) = ensemble {
            if ensemble_data.enabled {
                // TODO: Get actual time from composition/frame tracking
                // For now, use 0.0 which will show initial state
                let current_time = 0.0f32;

                return self.rasterize_ensemble_text(
                    text,
                    size,
                    font_name,
                    styles,
                    ensemble_data,
                    transform,
                    current_time,
                );
            }
        }

        // Standard text rendering (existing code)
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::from_argb(0, 0, 0, 0));

            let matrix = paint_utils::build_transform_matrix(transform);
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
                        let mut paint = paint_utils::create_stroke_paint(
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
        paint_utils::snapshot_surface(&mut layer, &mut self.gpu_context, self.width, self.height)
    }

    fn rasterize_grouped_shapes(
        &mut self,
        groups: &[ShapeGroup],
        styles: &[StyleConfig],
        transform: &Transform,
    ) -> Result<RenderOutput, LibraryError> {
        let _timer = ScopedTimer::debug("SkiaRenderer::rasterize_grouped_shapes");
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::TRANSPARENT);

            let matrix = paint_utils::build_transform_matrix(transform);
            canvas.save();
            canvas.concat(&matrix);

            for group in groups {
                canvas.save();

                // Apply per-group transform around the group's center
                let cx = group.base_position.0 + group.bounds.2 / 2.0;
                let cy = group.base_position.1 + group.bounds.3 / 2.0;

                canvas.translate((cx, cy));
                canvas.translate((group.transform.translate.0, group.transform.translate.1));
                canvas.rotate(group.transform.rotate, None);
                canvas.scale((group.transform.scale.0, group.transform.scale.1));
                canvas.translate((-cx, -cy));

                // 1. Draw behind-decorations
                for deco in &group.decorations {
                    if deco.behind {
                        if let Some(deco_path) = skia_safe::Path::from_svg(&deco.path) {
                            let mut paint = Paint::default();
                            paint.set_color(skia_safe::Color::from_argb(
                                deco.color.a,
                                deco.color.r,
                                deco.color.g,
                                deco.color.b,
                            ));
                            paint.set_anti_alias(true);
                            canvas.draw_path(&deco_path, &paint);
                        }
                    }
                }

                // 2. Draw main glyph path with styles
                if !group.path.is_empty() {
                    if let Some(glyph_path) = skia_safe::Path::from_svg(&group.path) {
                        for config in styles {
                            match &config.style {
                                DrawStyle::Fill { color, offset } => {
                                    let final_alpha = (color.a as f32 * group.transform.opacity)
                                        .clamp(0.0, 255.0)
                                        as u8;
                                    let mut paint = Paint::default();
                                    paint.set_color(skia_safe::Color::from_argb(
                                        final_alpha,
                                        color.r,
                                        color.g,
                                        color.b,
                                    ));
                                    paint.set_anti_alias(true);
                                    if *offset > 0.0 {
                                        paint.set_style(PaintStyle::StrokeAndFill);
                                        paint.set_stroke_width((*offset * 2.0) as f32);
                                        paint.set_stroke_join(skia_safe::paint::Join::Round);
                                    } else {
                                        paint.set_style(PaintStyle::Fill);
                                    }
                                    canvas.draw_path(&glyph_path, &paint);
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
                                    let final_alpha = (color.a as f32 * group.transform.opacity)
                                        .clamp(0.0, 255.0)
                                        as u8;
                                    let mut paint = paint_utils::create_stroke_paint(
                                        &Color {
                                            r: color.r,
                                            g: color.g,
                                            b: color.b,
                                            a: final_alpha,
                                        },
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
                                    canvas.draw_path(&glyph_path, &paint);
                                }
                            }
                        }
                    }
                }

                // 3. Draw front-decorations
                for deco in &group.decorations {
                    if !deco.behind {
                        if let Some(deco_path) = skia_safe::Path::from_svg(&deco.path) {
                            let mut paint = Paint::default();
                            paint.set_color(skia_safe::Color::from_argb(
                                deco.color.a,
                                deco.color.r,
                                deco.color.g,
                                deco.color.b,
                            ));
                            paint.set_anti_alias(true);
                            canvas.draw_path(&deco_path, &paint);
                        }
                    }
                }

                canvas.restore();
            }

            canvas.restore();
        }
        paint_utils::snapshot_surface(&mut layer, &mut self.gpu_context, self.width, self.height)
    }

    fn rasterize_shape_layer(
        &mut self,
        path_data: &str,
        styles: &[StyleConfig],
        path_effects: &Vec<PathEffect>,
        transform: &Transform,
    ) -> Result<RenderOutput, LibraryError> {
        let _timer = ScopedTimer::debug("SkiaRenderer::rasterize_shape_layer");
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::from_argb(0, 0, 0, 0));
            let path = skia_safe::Path::from_svg(path_data).unwrap_or_default();
            let matrix = paint_utils::build_transform_matrix(transform);
            canvas.save();
            canvas.concat(&matrix);
            for config in styles {
                let style = &config.style;
                match style {
                    DrawStyle::Fill { color, offset } => {
                        shape_renderer::draw_shape_fill_on_canvas(
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
                        shape_renderer::draw_shape_stroke_on_canvas(
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
        paint_utils::snapshot_surface(&mut layer, &mut self.gpu_context, self.width, self.height)
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
                        return Err(LibraryError::render(
                            "Failed to read texture pixels".to_string(),
                        ));
                    }
                    Ok(Image {
                        width: info.width,
                        height: info.height,
                        data: buffer,
                    })
                } else {
                    Err(LibraryError::render(
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
        let _timer = ScopedTimer::debug("SkiaRenderer::clear");
        let color = self.background_sk_color();
        let canvas: &Canvas = self.surface.canvas();
        canvas.clear(color);
        Ok(())
    }

    fn get_gpu_context(&mut self) -> Option<&mut crate::rendering::skia_utils::GpuContext> {
        self.gpu_context.as_mut()
    }

    fn transform_layer(
        &mut self,
        layer: &RenderOutput,
        transform: &Transform,
    ) -> Result<RenderOutput, LibraryError> {
        let mut offscreen = self.create_layer_surface()?;
        {
            let canvas = offscreen.canvas();
            canvas.clear(skia_safe::Color::TRANSPARENT);

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
                        return Err(LibraryError::render(
                            "Cannot render texture without GPU context",
                        ));
                    }
                }
            };

            let matrix = paint_utils::build_transform_matrix(transform);
            canvas.save();
            canvas.concat(&matrix);

            let mut paint = Paint::default();
            paint.set_anti_alias(true);
            paint.set_alpha_f(transform.opacity as f32);

            let cubic_resampler = CubicResampler::mitchell();
            let sampling = SamplingOptions::from(cubic_resampler);
            canvas.draw_image_with_sampling_options(&src_image, (0, 0), sampling, Some(&paint));
            canvas.restore();
        }
        paint_utils::snapshot_surface(
            &mut offscreen,
            &mut self.gpu_context,
            self.width,
            self.height,
        )
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
