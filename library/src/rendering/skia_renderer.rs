use crate::loader::image::Image;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{CapType, DrawStyle, JoinType, PathEffect};
use crate::model::frame::effect::ImageEffect;
use crate::model::frame::transform::Transform;
use crate::rendering::renderer::Renderer;
use crate::util::timing::ScopedTimer;
use log::trace;
use skia_safe::image::CachingHint;
use skia_safe::image_filters;
use skia_safe::images::raster_from_data;
use skia_safe::path_effect::PathEffect as SkPathEffect;
use skia_safe::surfaces;
use skia_safe::trim_path_effect::Mode;
use skia_safe::utils::parse_path::from_svg;
use skia_safe::{
  AlphaType, Canvas, Color as SkColor, ColorType, CubicResampler, Data, Font, FontMgr, FontStyle,
  ISize, ImageFilter, ImageInfo, Matrix, Paint, PaintStyle, Point, SamplingOptions, Surface,
  TileMode,
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
    let mut surface = surfaces::raster(&info, None, None).expect("Cannot create Skia Surface");
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

  fn new_layer_surface(&self) -> Result<Surface, Box<dyn Error>> {
    let info = ImageInfo::new_n32_premul((self.width as i32, self.height as i32), None);
    surfaces::raster(&info, None, None).ok_or_else(|| "Cannot create layer surface".into())
  }

  fn surface_to_image(&self, surface: &mut Surface) -> Result<Image, Box<dyn Error>> {
    let snapshot = surface.image_snapshot();
    let row_bytes = (self.width * 4) as usize;
    let mut buffer = vec![0u8; (self.height as usize) * row_bytes];
    let image_info = ImageInfo::new(
      ISize::new(self.width as i32, self.height as i32),
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
      return Err("Failed to read layer pixels".into());
    }
    Ok(Image {
      width: self.width,
      height: self.height,
      data: buffer,
    })
  }

  fn draw_shape_fill_on_canvas(
    &self,
    canvas: &Canvas,
    path: &skia_safe::Path,
    color: &Color,
    path_effects: &Vec<PathEffect>,
  ) -> Result<(), Box<dyn Error>> {
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
  ) -> Result<(), Box<dyn Error>> {
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

fn convert_path_effect(path_effect: &PathEffect) -> Result<skia_safe::PathEffect, Box<dyn Error>> {
  match path_effect {
    PathEffect::Dash { intervals, phase } => {
      let intervals: Vec<f32> = intervals.iter().map(|&x| x as f32).collect();
      Ok(SkPathEffect::dash(&intervals, *phase as f32).ok_or("Failed to create PathEffect")?)
    }
    PathEffect::Corner { radius } => {
      Ok(SkPathEffect::corner_path(*radius as f32).ok_or("Failed to create PathEffect")?)
    }
    PathEffect::Discrete {
      seg_length,
      deviation,
      seed,
    } => Ok(
      SkPathEffect::discrete(*seg_length as f32, *deviation as f32, *seed as u32)
        .ok_or("Failed to create PathEffect")?,
    ),
    PathEffect::Trim { start, end } => Ok(
      SkPathEffect::trim(*start as f32, *end as f32, Mode::Normal)
        .ok_or("Failed to create PathEffect")?,
    ),
  }
}

fn apply_path_effects(
  path_effects: &Vec<PathEffect>,
  paint: &mut Paint,
) -> Result<(), Box<dyn Error>> {
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
    paint.set_path_effect(composed_effect.ok_or("Failed to compose PathEffects")?);
  }
  Ok(())
}

fn apply_image_effects(effects: &[ImageEffect], paint: &mut Paint) -> Result<(), Box<dyn Error>> {
  if effects.is_empty() {
    return Ok(());
  }

  let mut current_filter: Option<ImageFilter> = None;
  for effect in effects {
    match effect {
      ImageEffect::Blur { radius } => {
        let sigma = (*radius).max(0.0);
        if sigma <= 0.0 {
          continue;
        }
        let filter = image_filters::blur(
          (sigma, sigma),
          None::<TileMode>,
          current_filter.take(),
          None,
        )
        .ok_or("Failed to create blur filter")?;
        current_filter = Some(filter);
      }
    }
  }

  if let Some(filter) = current_filter {
    paint.set_image_filter(filter);
  }

  Ok(())
}

impl Renderer for SkiaRenderer {
  fn draw_image(
    &mut self,
    video_frame: &Image,
    transform: &Transform,
    effects: &[ImageEffect],
  ) -> Result<(), Box<dyn Error>> {
    let _timer = ScopedTimer::debug(format!(
      "SkiaRenderer::draw_image {}x{}",
      video_frame.width, video_frame.height
    ));
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
    apply_image_effects(effects, &mut paint)?;
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
  ) -> Result<Image, Box<dyn Error>> {
    let _timer = ScopedTimer::debug(format!(
      "SkiaRenderer::rasterize_text_layer len={} size={}",
      text.len(),
      size
    ));
    let mut layer = self.new_layer_surface()?;
    {
      let canvas: &Canvas = layer.canvas();
      canvas.clear(skia_safe::Color::from_argb(0, 0, 0, 0));
      let font_mgr = FontMgr::default();
      let typeface = font_mgr
        .match_family_style(font_name, FontStyle::normal())
        .ok_or("Failed to match typeface")?;
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
    self.surface_to_image(&mut layer)
  }

  fn rasterize_shape_layer(
    &mut self,
    path_data: &str,
    styles: &[DrawStyle],
    path_effects: &Vec<PathEffect>,
    transform: &Transform,
  ) -> Result<Image, Box<dyn Error>> {
    let _timer = ScopedTimer::debug("SkiaRenderer::rasterize_shape_layer");
    let mut layer = self.new_layer_surface()?;
    {
      let canvas: &Canvas = layer.canvas();
      canvas.clear(skia_safe::Color::from_argb(0, 0, 0, 0));
      let path = from_svg(path_data).ok_or("Failed to parse SVG path data")?;
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
    self.surface_to_image(&mut layer)
  }

  fn finalize(&mut self) -> Result<Image, Box<dyn Error>> {
    let _timer = ScopedTimer::debug(format!(
      "SkiaRenderer::finalize {}x{}",
      self.width, self.height
    ));
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

  fn clear(&mut self) -> Result<(), Box<dyn Error>> {
    let _timer = ScopedTimer::debug("SkiaRenderer::clear");
    let canvas: &Canvas = self.surface.canvas();
    canvas.clear(skia_safe::Color::from_argb(0, 0, 0, 0));
    Ok(())
  }
}
