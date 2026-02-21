use crate::error::LibraryError;
use crate::evaluation::output::ShapeGroup;
use crate::runtime::Image;

use crate::runtime::draw_type::PathEffect;
use crate::runtime::entity::StyleConfig;
use crate::runtime::transform::Transform;

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
        styles: &[StyleConfig],
        ensemble: Option<&crate::evaluation::ensemble::EnsembleData>,
        transform: &Transform,
    ) -> Result<RenderOutput, LibraryError>;

    fn rasterize_grouped_shapes(
        &mut self,
        groups: &[ShapeGroup],
        styles: &[StyleConfig],
        transform: &Transform,
    ) -> Result<RenderOutput, LibraryError>;

    fn rasterize_shape_layer(
        &mut self,
        path_data: &str,
        styles: &[StyleConfig],
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
    fn get_gpu_context(&mut self) -> Option<&mut crate::rendering::skia_utils::GpuContext> {
        None
    }

    fn set_sharing_context(&mut self, _handle: usize, _hwnd: Option<isize>) {}

    /// Apply a transform to an image, producing a new image on an offscreen surface.
    ///
    /// Creates a layer the same size as the current renderer surface, draws `layer`
    /// with the given transform, and returns the composited result.
    fn transform_layer(
        &mut self,
        layer: &RenderOutput,
        transform: &Transform,
    ) -> Result<RenderOutput, LibraryError>;
}
