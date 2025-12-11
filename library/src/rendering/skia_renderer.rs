use crate::error::LibraryError;
use crate::loader::image::Image;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{CapType, DrawStyle, JoinType, PathEffect};
use crate::model::frame::transform::Transform;
use crate::rendering::renderer::Renderer;
use crate::rendering::skia_utils::{
    GpuContext, create_gpu_context, create_surface, image_to_skia, surface_to_image,
};
use crate::util::timing::ScopedTimer;
use log::{debug, trace};
use skia_safe::path_effect::PathEffect as SkPathEffect;
use skia_safe::trim_path_effect::Mode;
use skia_safe::utils::parse_path::from_svg;
use skia_safe::{
    Canvas, Color as SkColor, CubicResampler, Font, FontMgr, FontStyle, Matrix, Paint, PaintStyle,
    Point, SamplingOptions, Surface,
};

pub struct SkiaRenderer {
    width: u32,
    height: u32,
    background_color: Color,
    surface: Surface,
    gpu_context: Option<GpuContext>,
}

impl SkiaRenderer {
    pub fn new(width: u32, height: u32, background_color: Color) -> Self {
        // let mut gpu_context = create_gpu_context();
        let mut gpu_context: Option<GpuContext> = None; // Force CPU for now
        
        if gpu_context.is_some() {
            debug!("SkiaRenderer: GPU context enabled");
        } else {
            debug!("SkiaRenderer: GPU context unavailable, using CPU raster surfaces");
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

impl Renderer for SkiaRenderer {
    fn draw_image(
        &mut self,
        video_frame: &Image,
        transform: &Transform,
    ) -> Result<(), LibraryError> {
        let _timer = ScopedTimer::debug(format!(
            "SkiaRenderer::draw_image {}x{}",
            video_frame.width, video_frame.height
        ));
        let canvas: &Canvas = self.surface.canvas();

        let src_image = image_to_skia(video_frame)?;

        let matrix = build_transform_matrix(transform);

        canvas.save();
        canvas.concat(&matrix);

        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        let cubic_resampler = CubicResampler::mitchell();
        let sampling = SamplingOptions::from(cubic_resampler);
        canvas.draw_image_with_sampling_options(&src_image, (0, 0), sampling, Some(&paint));

        canvas.restore();

        Ok(())
    }

    fn rasterize_text_layer(
        &mut self,
        text: &str,
        size: f64,
        font_name: &String,
        color: &Color,
        transform: &Transform,
    ) -> Result<Image, LibraryError> {
        let _timer = ScopedTimer::debug(format!(
            "SkiaRenderer::rasterize_text_layer len={} size={}",
            text.len(),
            size
        ));
        let mut layer = self.create_layer_surface()?;
        {
            let canvas: &Canvas = layer.canvas();
            canvas.clear(skia_safe::Color::from_argb(0, 0, 0, 0));
            let font_mgr = FontMgr::default();
            let typeface = font_mgr
                .match_family_style(font_name, FontStyle::normal())
                .ok_or(LibraryError::Render("Failed to match typeface".to_string()))?;
            let mut font = Font::default();
            font.set_typeface(typeface);
            font.set_size(size as f32);

            let mut paint = Paint::default();
            paint.set_anti_alias(true);
            paint.set_color(skia_safe::Color::from_argb(
                color.a, color.r, color.g, color.b,
            ));

            let (_scaled, metrics) = font.metrics();
            let y_offset = -metrics.ascent;

            let matrix = build_transform_matrix(transform);
            canvas.save();
            canvas.concat(&matrix);
            canvas.draw_str(text, (0.0, y_offset), &font, &paint);
            canvas.restore();
        }
        surface_to_image(&mut layer, self.width, self.height)
    }

    fn rasterize_shape_layer(
        &mut self,
        path_data: &str,
        styles: &[DrawStyle],
        path_effects: &Vec<PathEffect>,
        transform: &Transform,
    ) -> Result<Image, LibraryError> {
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
        surface_to_image(&mut layer, self.width, self.height)
    }

    fn finalize(&mut self) -> Result<Image, LibraryError> {
        let _timer = ScopedTimer::debug(format!(
            "SkiaRenderer::finalize {}x{}",
            self.width, self.height
        ));
        if let Some(context) = self.gpu_context.as_mut() {
            context.direct_context.flush_and_submit();
        }
        let width = self.width;
        let height = self.height;
        surface_to_image(&mut self.surface, width, height)
    }

    fn clear(&mut self) -> Result<(), LibraryError> {
        let _timer = ScopedTimer::debug("SkiaRenderer::clear");
        let color = self.background_sk_color();
        let canvas: &Canvas = self.surface.canvas();
        canvas.clear(color);
        Ok(())
    }
}
