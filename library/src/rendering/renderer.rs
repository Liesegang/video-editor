use crate::error::LibraryError;
use crate::loader::image::Image;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{DrawStyle, PathEffect};
use crate::model::frame::transform::Transform;

#[derive(Clone, Debug)]
pub enum RenderOutput {
    Image(Image),
    Texture(TextureInfo),
}

#[derive(Clone, Debug)]
pub struct TextureInfo {
    pub texture_id: u32,
    pub width: u32,
    pub height: u32,
}

pub trait Renderer {
    fn draw_layer(
        &mut self,
        layer: &RenderOutput,
        transform: &Transform,
    ) -> Result<(), LibraryError>;

    fn rasterize_text_layer(
        &mut self,
        text: &str,
        size: f64,
        font_name: &String,
        color: &Color,
        transform: &Transform,
    ) -> Result<RenderOutput, LibraryError>;

    fn rasterize_shape_layer(
        &mut self,
        path_data: &str,
        styles: &[DrawStyle],
        path_effects: &Vec<PathEffect>,
        transform: &Transform,
    ) -> Result<RenderOutput, LibraryError>;

    fn rasterize_sksl_layer(
        &mut self,
        shader_code: &str,
        resolution: (f32, f32),
        time: f32,
        transform: &Transform,
    ) -> Result<RenderOutput, LibraryError>;

    fn read_surface(&mut self, output: &RenderOutput) -> Result<Image, LibraryError>;

    fn finalize(&mut self) -> Result<RenderOutput, LibraryError>;
    fn clear(&mut self) -> Result<(), LibraryError>;
}
