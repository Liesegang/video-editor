use crate::loader::image::Image;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{DrawStyle, PathEffect};
use crate::model::frame::effect::ImageEffect;
use crate::model::frame::transform::Transform;
use std::error::Error;

pub trait Renderer {
  fn draw_image(
    &mut self,
    image: &Image,
    transform: &Transform,
    effects: &[ImageEffect],
  ) -> Result<(), Box<dyn Error>>;

  fn rasterize_text_layer(
    &mut self,
    text: &str,
    size: f64,
    font_name: &String,
    color: &Color,
    transform: &Transform,
  ) -> Result<Image, Box<dyn Error>>;

  fn rasterize_shape_layer(
    &mut self,
    path_data: &str,
    styles: &[DrawStyle],
    path_effects: &Vec<PathEffect>,
    transform: &Transform,
  ) -> Result<Image, Box<dyn Error>>;

  fn finalize(&mut self) -> Result<Image, Box<dyn Error>>;
  fn clear(&mut self) -> Result<(), Box<dyn Error>>;
}
