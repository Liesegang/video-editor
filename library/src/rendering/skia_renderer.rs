use crate::loader::image::Image;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{CapType, JoinType, PathEffect};
use crate::model::frame::transform::Transform;
use crate::rendering::renderer::Renderer;
use skia_safe::image::CachingHint;
use skia_safe::images::raster_from_data;
use skia_safe::path_effect::PathEffect as SkPathEffect;
use skia_safe::surfaces::raster;
use skia_safe::trim_path_effect::Mode;
use skia_safe::utils::parse_path::from_svg;
use skia_safe::{
    AlphaType, Canvas, Color as SkColor, ColorType, CubicResampler, Data, Font, FontMgr, FontStyle,
    ISize, ImageInfo, Matrix, Paint, PaintStyle, Point, SamplingOptions, Surface,
};
use std::error::Error;

pub struct SkiaRenderer {
    width: u32,
    height: u32,
    surface: Surface,
}

impl SkiaRenderer {
    pub fn new(width: u32, height: u32, background_color: Color) -> Self {
        let info = ImageInfo::new_n32_premul((width as i32, height as i32), None);
        let mut surface = raster(&info, None, None).expect("Cannot create Skia Surface");
        let bg = SkColor::from_argb(
            background_color.a,
            background_color.r,
            background_color.g,
            background_color.b,
        );
        // Clear with background color.
        surface.canvas().clear(bg);
        SkiaRenderer {
            width,
            height,
            surface,
        }
    }
}

fn build_transform_matrix(transform: &Transform) -> Matrix {
    let anchor = Point::new(transform.anchor.x, transform.anchor.y);
    let mut matrix = Matrix::new_identity();
    matrix.pre_translate((
        transform.position.x - anchor.x,
        transform.position.y - anchor.y,
    ));
    matrix.pre_rotate(transform.rotation, anchor);
    matrix.pre_scale((transform.scale.x, transform.scale.y), anchor);
    matrix
}

fn convert_path_effect(path_effect: &PathEffect) -> Result<skia_safe::PathEffect, Box<dyn Error>> {
    match path_effect {
        PathEffect::Dash { intervals, phase } => {
            let intervals: Vec<f32> = intervals.iter().map(|&x| x as f32).collect();
            Ok(SkPathEffect::dash(&intervals, *phase).ok_or("Failed to create PathEffect")?)
        }
        PathEffect::Corner { radius } => {
            Ok(SkPathEffect::corner_path(*radius).ok_or("Failed to create PathEffect")?)
        }
        PathEffect::Discrete {
            seg_length,
            deviation,
            seed,
        } => Ok(SkPathEffect::discrete(*seg_length, *deviation, *seed)
            .ok_or("Failed to create PathEffect")?),
        PathEffect::Trim { start, end } => {
            Ok(SkPathEffect::trim(*start, *end, Mode::Normal)
                .ok_or("Failed to create PathEffect")?)
        }
    }
}

fn apply_path_effects(
    path_effects: &Vec<PathEffect>,
    paint: &mut Paint,
) -> Result<(), Box<dyn Error>> {
    if !path_effects.is_empty() {
        let mut composed_effect: Option<skia_safe::PathEffect> = None;
        for effect in path_effects {
            let sk_path_effect = convert_path_effect(effect)?;

            composed_effect = match composed_effect {
                Some(e) => Some(SkPathEffect::compose(e, sk_path_effect)),
                None => Some(sk_path_effect),
            };
        }
        paint.set_path_effect(composed_effect.ok_or("Failed to compose PathEffects")?);
    }
    Ok(())
}

impl Renderer for SkiaRenderer {
    fn draw_image(
        &mut self,
        video_frame: &Image,
        transform: &Transform,
    ) -> Result<(), Box<dyn Error>> {
        let canvas: &Canvas = self.surface.canvas();

        let info = ImageInfo::new(
            ISize::new(video_frame.width as i32, video_frame.height as i32),
            ColorType::RGBA8888,
            AlphaType::Premul,
            None,
        );
        let sk_data = Data::new_copy(video_frame.data.as_slice());
        let src_image = raster_from_data(&info, sk_data, (video_frame.width * 4) as usize)
            .ok_or("Failed to create Skia image")?;

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

    fn draw_text(
        &mut self,
        text: &str,
        size: f32,
        font_name: &String,
        color: &Color,
        transform: &Transform,
    ) -> Result<(), Box<dyn Error>> {
        let canvas: &Canvas = self.surface.canvas();
        let font_mgr = FontMgr::default();
        let typeface = font_mgr
            .match_family_style(font_name, FontStyle::normal())
            .ok_or("Failed to match typeface")?;
        let mut font = Font::default();
        font.set_typeface(typeface);
        font.set_size(size);

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
        Ok(())
    }

    fn draw_shape_fill(
        &mut self,
        path_data: &str,
        color: &Color,
        path_effects: &Vec<PathEffect>,
        transform: &Transform,
    ) -> Result<(), Box<dyn Error>> {
        let canvas: &Canvas = self.surface.canvas();
        let matrix = build_transform_matrix(transform);
        canvas.save();
        canvas.concat(&matrix);

        let path = from_svg(path_data).ok_or("Failed to parse SVG path data")?;

        let mut fill_paint = Paint::default();
        fill_paint.set_anti_alias(true);
        fill_paint.set_style(PaintStyle::Fill);
        fill_paint.set_color(skia_safe::Color::from_argb(
            color.a, color.r, color.g, color.b,
        ));
        apply_path_effects(path_effects, &mut fill_paint)?;
        canvas.draw_path(&path, &fill_paint);

        canvas.restore();
        Ok(())
    }

    fn draw_shape_stroke(
        &mut self,
        path_data: &str,
        color: &Color,
        path_effects: &Vec<PathEffect>,
        width: f32,
        cap: CapType,
        join: JoinType,
        miter: f32,
        transform: &Transform,
    ) -> Result<(), Box<dyn Error>> {
        let canvas: &Canvas = self.surface.canvas();
        let matrix = build_transform_matrix(transform);
        canvas.save();
        canvas.concat(&matrix);

        let path = from_svg(path_data).ok_or("Failed to parse SVG path data")?;

        let mut stroke_paint = Paint::default();
        stroke_paint.set_anti_alias(true);
        stroke_paint.set_style(PaintStyle::Stroke);
        stroke_paint.set_color(skia_safe::Color::from_argb(
            color.a, color.r, color.g, color.b,
        ));
        stroke_paint.set_stroke_width(width);
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
        stroke_paint.set_stroke_miter(miter);
        apply_path_effects(path_effects, &mut stroke_paint)?;
        canvas.draw_path(&path, &stroke_paint);

        canvas.restore();
        Ok(())
    }

    fn finalize(&mut self) -> Result<Image, Box<dyn Error>> {
        let snapshot = self.surface.image_snapshot();

        let width = self.width;
        let height = self.height;
        let row_bytes = (width * 4) as usize;
        let mut buffer = vec![0u8; (height as usize) * row_bytes];

        let image_info = ImageInfo::new(
            ISize::new(width as i32, height as i32),
            ColorType::RGBA8888,
            AlphaType::Premul,
            None,
        );

        if !snapshot.read_pixels(
            &image_info,
            &mut buffer,
            row_bytes,
            (0, 0),
            CachingHint::Allow,
        ) {
            return Err("Failed to read pixels".into());
        }

        Ok(Image {
            width,
            height,
            data: buffer,
        })
    }
}
