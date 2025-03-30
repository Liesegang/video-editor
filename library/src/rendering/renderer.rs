use crate::loader::image::Image;
use std::error::Error;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{CapType, JoinType, PathEffect};
use crate::model::frame::transform::Transform;

pub trait Renderer {
    fn draw_image(
        &mut self,
        video_frame: &Image,
        transform: &Transform,
    ) -> Result<(), Box<dyn Error>>;
    fn draw_text(
        &mut self,
        text: &str,
        size: f64,
        font_name: &String,
        color: &Color,
        transform: &Transform,
    ) -> Result<(), Box<dyn Error>>;
    fn draw_shape_fill(
        &mut self,
        path_data: &str,
        color: &Color,
        path_effects: &Vec<PathEffect>,
        transform: &Transform,
    ) -> Result<(), Box<dyn Error>>;
    fn draw_shape_stroke(
        &mut self,
        path_data: &str,
        color: &Color,
        path_effects: &Vec<PathEffect>,
        width: f64,
        cap: CapType,
        join: JoinType,
        miter: f64,
        transform: &Transform,
    ) -> Result<(), Box<dyn Error>>;
    fn finalize(&mut self) -> Result<Image, Box<dyn Error>>;
    fn clear(&mut self) -> Result<(), Box<dyn Error>>;
}
