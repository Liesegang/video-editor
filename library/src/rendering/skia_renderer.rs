use crate::error::LibraryError;
use crate::loader::image::Image;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{CapType, DrawStyle, JoinType, PathEffect};
use crate::model::frame::transform::Transform;
use crate::rendering::renderer::{RenderOutput, Renderer, TextureInfo};
use crate::rendering::skia_utils::{
    GpuContext, create_gpu_context, create_image_from_texture, create_surface, image_to_skia,
    surface_to_image,
};
use crate::util::timing::ScopedTimer;
use log::{debug, trace};
use skia_safe::path_effect::PathEffect as SkPathEffect;
use skia_safe::trim_path_effect::Mode;
use skia_safe::utils::parse_path::from_svg;
use skia_safe::{
    AlphaType, Canvas, Color as SkColor, ColorType, CubicResampler, Font, FontMgr, FontStyle,
    ISize, ImageInfo, Matrix, Paint, PaintStyle, Point, SamplingOptions, Surface,
};

pub struct SkiaRenderer {
    width: u32,
    height: u32,
    background_color: Color,
    surface: Surface,
    gpu_context: Option<GpuContext>,
}

impl SkiaRenderer {
    pub fn render_to_texture(&mut self) -> Result<TextureInfo, LibraryError> {
        let _timer = ScopedTimer::debug("SkiaRenderer::render_to_texture");
        if let Some(context) = self.gpu_context.as_mut() {
            context.direct_context.flush_and_submit();

            // Get the backend texture from the surface if possible
            // use skia_safe::gpu::surfaces::get_backend_texture if available
            // Try 'backend_texture' method if 'get_backend_texture' is gone
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
            } else if let Some(mut ctx) = create_gpu_context() {
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
    ) -> Result<(), LibraryError> {
        let mut fill_paint = Paint::default();
        fill_paint.set_anti_alias(true);
        fill_paint.set_style(PaintStyle::Fill);
        fill_paint.set_color(skia_safe::Color::from_argb(
            color.a, color.r, color.g, color.b,
        ));
        apply_path_effects(path_effects, &mut fill_paint)?;
        canvas.draw_path(path, &fill_paint);
        Ok(())
    }

    fn draw_shape_stroke_on_canvas(
        &self,
        canvas: &Canvas,
        path: &skia_safe::Path,
        color: &Color,
        path_effects: &Vec<PathEffect>,
        width: f64,
        cap: CapType,
        join: JoinType,
        miter: f64,
    ) -> Result<(), LibraryError> {
        let mut stroke_paint = Paint::default();
        stroke_paint.set_anti_alias(true);
        stroke_paint.set_style(PaintStyle::Stroke);
        stroke_paint.set_color(skia_safe::Color::from_argb(
            color.a, color.r, color.g, color.b,
        ));
        stroke_paint.set_stroke_width(width as f32);
        stroke_paint.set_stroke_cap(match cap {
            CapType::Round => skia_safe::paint::Cap::Round,
            CapType::Square => skia_safe::paint::Cap::Square,
            CapType::Butt => skia_safe::paint::Cap::Butt,
        });
        stroke_paint.set_stroke_join(match join {
            JoinType::Round => skia_safe::paint::Join::Round,
            JoinType::Bevel => skia_safe::paint::Join::Bevel,
            JoinType::Miter => skia_safe::paint::Join::Miter,
        });
        stroke_paint.set_stroke_miter(miter as f32);
        apply_path_effects(path_effects, &mut stroke_paint)?;
        canvas.draw_path(path, &stroke_paint);
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
            let sk_path_effect = convert_path_effect(effect)?;

            composed_effect = match composed_effect {
                Some(e) => Some(SkPathEffect::compose(e, sk_path_effect)),
                None => Some(sk_path_effect),
            };
        }
        paint.set_path_effect(composed_effect.ok_or(LibraryError::Render(
            "Failed to compose PathEffects".to_string(),
        ))?);
    }
    Ok(())
}

fn preprocess_shader(code: &str) -> String {
    // 1. Inject Standard Uniforms
    let standard_uniforms = r#"
uniform float3 iResolution;
uniform float iTime;
uniform float iTimeDelta;
uniform float iFrame;
uniform float4 iMouse;
uniform float4 iDate;
"#;
    // 2. Preprocess using shaderc
    let compiler = shaderc::Compiler::new().unwrap();
    let options = shaderc::CompileOptions::new().unwrap();

    // Shaderc requires a version directive. SkSL usually doesn't have it.
    // We inject one for preprocessing, then strip directives from the output.
    let version_directive = "#version 310 es\n";
    let full_source = if code.trim().starts_with("#version") {
        format!("{}\n{}", standard_uniforms, code)
    } else {
        format!("{}{}\n{}", version_directive, standard_uniforms, code)
    };

    // Try to preprocess
    match compiler.preprocess(&full_source, "shader.glsl", "main", Some(&options)) {
        Ok(artifact) => {
            let output = artifact.as_text();
            // Clean up output: remove #version, #line, #extension which might confuse SkSL
            output
                .lines()
                .filter(|l| {
                    let t = l.trim();
                    !t.starts_with("#version")
                        && !t.starts_with("#extension")
                        && !t.starts_with("#line")
                        && !t.starts_with("#pragma")
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        Err(e) => {
            // Fallback: log error and return original code (with injected uniforms)
            // This allows non-macro code to likely work even if shaderc fails (e.g. valid SkSL but invalid GLSL?)
            // But SkSL is mostly GLSL.
            // We prepend standard uniforms manually in fallback.
            format!(
                "// Preprocessing failed: {}\n{}\n{}",
                e, standard_uniforms, code
            )
        }
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
        // Apply opacity from transform
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

            let preprocessed_code = preprocess_shader(shader_code);
            let result = skia_safe::RuntimeEffect::make_for_shader(&preprocessed_code, None);

            // Handle shader compilation errors
            if let Err(error) = result {
                log::error!(
                    "SkSL Compilation Error: {}\nCode:\n{}",
                    error,
                    preprocessed_code
                );
                // Fallback: Red background to indicate error
                canvas.clear(skia_safe::Color::RED);
            } else if let Ok(effect) = result {
                // Dynamic Uniform Binding
                let uniform_size = effect.uniform_size();
                let mut data: Vec<u8> = vec![0; uniform_size];

                // Inspect uniforms expected by the shader
                for uniform in effect.uniforms() {
                    let offset = uniform.offset();
                    let name = uniform.name();

                    // Helper helper to write f32 to data at offset
                    let mut write_f32 = |offset: usize, val: f32| {
                        if offset + 4 <= data.len() {
                            let bytes = val.to_le_bytes();
                            data[offset..offset + 4].copy_from_slice(&bytes);
                        }
                    };

                    match name {
                        "iResolution" => {
                            // float3
                            write_f32(offset, resolution.0);
                            write_f32(offset + 4, resolution.1);
                            write_f32(offset + 8, 1.0);
                        }
                        "iTime" => {
                            write_f32(offset, time);
                        }
                        "iTimeDelta" => {
                            write_f32(offset, 1.0 / 60.0); // Approx
                        }
                        "iFrame" => {
                            write_f32(offset, (time * 60.0).floor());
                        }
                        "iMouse" => {
                            // float4
                            write_f32(offset, 0.0);
                            write_f32(offset + 4, 0.0);
                            write_f32(offset + 8, 0.0);
                            write_f32(offset + 12, 0.0);
                        }
                        "iDate" => {
                            // float4, year, month, day, seconds
                            write_f32(offset, 2024.0);
                            write_f32(offset + 4, 1.0);
                            write_f32(offset + 8, 1.0);
                            write_f32(offset + 12, 0.0);
                        }
                        "iChannelTime" => {
                            // float[4] ?
                            // If it's an array, we might need to be careful.
                            // For now assume 0.0s
                            write_f32(offset, time);
                            write_f32(offset + 4, time);
                            write_f32(offset + 8, time);
                            write_f32(offset + 12, time);
                        }
                        _ => {
                            // trace!("Unknown uniform: {}", name);
                        }
                    }
                }

                let uniforms = skia_safe::Data::new_copy(&data);

                let shader =
                    effect
                        .make_shader(uniforms, &[], None)
                        .ok_or(LibraryError::Render(
                            "Failed to create SkSL shader".to_string(),
                        ))?;

                let mut paint = Paint::default();
                paint.set_shader(shader);
                // Opacity is 0.0-1.0 (already normalized by EntityConverter)
                paint.set_alpha_f(transform.opacity as f32);

                let matrix = build_transform_matrix(transform);
                canvas.save();
                canvas.concat(&matrix);
                // We will fill the configured resolution rect (0,0, width, height)
                let rect = skia_safe::Rect::from_wh(resolution.0, resolution.1);
                canvas.draw_rect(rect, &paint);
                canvas.restore();
            }
        }

        if let Some(ctx) = self.gpu_context.as_mut() {
            ctx.direct_context.flush_and_submit();
            if let Some(texture) = skia_safe::gpu::surfaces::get_backend_texture(
                &mut layer,
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

        let image = surface_to_image(&mut layer, self.width, self.height)?;
        Ok(RenderOutput::Image(image))
    }

    fn rasterize_text_layer(
        &mut self,
        text: &str,
        size: f64,
        font_name: &String,
        color: &Color,
        transform: &Transform,
    ) -> Result<RenderOutput, LibraryError> {
        let _timer = ScopedTimer::debug(format!(
            "SkiaRenderer::rasterize_text_layer len={} size={}",
            text.len(),
            size
        ));
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::from_argb(0, 0, 0, 0));
            // FontMgr::default() removed

            // let mut paint = Paint::default(); // Unused with Paragraph

            let matrix = build_transform_matrix(transform);
            canvas.save();
            canvas.concat(&matrix);

            let mut font_collection = skia_safe::textlayout::FontCollection::new();
            font_collection.set_default_font_manager(skia_safe::FontMgr::default(), None);

            let mut text_style = skia_safe::textlayout::TextStyle::new();
            text_style.set_font_families(&[font_name]);
            text_style.set_font_size(size as f32);
            text_style.set_color(skia_safe::Color::from_argb(
                color.a, color.r, color.g, color.b,
            ));

            let mut paragraph_style = skia_safe::textlayout::ParagraphStyle::new();
            paragraph_style.set_text_style(&text_style);

            let mut builder =
                skia_safe::textlayout::ParagraphBuilder::new(&paragraph_style, font_collection);

            builder.add_text(text);

            let mut paragraph = builder.build();
            paragraph.layout(f32::MAX); // Layout on a single line (infinite width)
            paragraph.paint(canvas, (0.0, 0.0));

            canvas.restore();
        }
        if let Some(ctx) = self.gpu_context.as_mut() {
            ctx.direct_context.flush_and_submit();
            if let Some(texture) = skia_safe::gpu::surfaces::get_backend_texture(
                &mut layer,
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

        let image = surface_to_image(&mut layer, self.width, self.height)?;
        Ok(RenderOutput::Image(image))
    }

    fn rasterize_shape_layer(
        &mut self,
        path_data: &str,
        styles: &[DrawStyle],
        path_effects: &Vec<PathEffect>,
        transform: &Transform,
    ) -> Result<RenderOutput, LibraryError> {
        let _timer = ScopedTimer::debug("SkiaRenderer::rasterize_shape_layer");
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::from_argb(0, 0, 0, 0));
            let path = from_svg(path_data).ok_or(LibraryError::Render(
                "Failed to parse SVG path data".to_string(),
            ))?;
            let matrix = build_transform_matrix(transform);
            canvas.save();
            canvas.concat(&matrix);
            for style in styles {
                match style {
                    DrawStyle::Fill { color } => {
                        self.draw_shape_fill_on_canvas(canvas, &path, color, path_effects)?;
                    }
                    DrawStyle::Stroke {
                        color,
                        width,
                        cap,
                        join,
                        miter,
                    } => {
                        self.draw_shape_stroke_on_canvas(
                            canvas,
                            &path,
                            color,
                            path_effects,
                            *width,
                            cap.clone(),
                            join.clone(),
                            *miter,
                        )?;
                    }
                }
            }
            canvas.restore();
        }
        let image = surface_to_image(&mut layer, self.width, self.height)?;
        Ok(RenderOutput::Image(image))
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
        let _timer = ScopedTimer::debug(format!(
            "SkiaRenderer::finalize {}x{}",
            self.width, self.height
        ));

        if let Some(context) = self.gpu_context.as_mut() {
            context.direct_context.flush_and_submit();
        }

        let width = self.width;
        let height = self.height;
        let image = surface_to_image(&mut self.surface, width, height)?;
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
}
