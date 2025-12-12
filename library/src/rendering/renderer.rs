use crate::error::LibraryError;
use crate::loader::image::Image;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{DrawStyle, PathEffect};
use crate::model::frame::transform::Transform;

#[derive(Clone)]
pub enum RenderOutput {
    Image(Image),
    Texture(u32), // Texture ID
}

pub trait Renderer {
    fn draw_image(&mut self, image: &Image, transform: &Transform) -> Result<(), LibraryError>;

    fn rasterize_text_layer(
        &mut self,
        text: &str,
        size: f64,
        font_name: &String,
        color: &Color,
        transform: &Transform,
    ) -> Result<Image, LibraryError>;

    fn rasterize_shape_layer(
        &mut self,
        path_data: &str,
        styles: &[DrawStyle],
        path_effects: &Vec<PathEffect>,
        transform: &Transform,
    ) -> Result<Image, LibraryError>;

    fn finalize(&mut self) -> Result<RenderOutput, LibraryError>;
    fn clear(&mut self) -> Result<(), LibraryError>;
}
