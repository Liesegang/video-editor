use crate::error::LibraryError;
use crate::model::frame::Image;
use crate::model::frame::color::Color;

use crate::model::BlendMode;
use crate::model::frame::draw_type::PathEffect;
use crate::model::frame::entity::StyleConfig;
use crate::model::frame::transform::Transform;

/// A render-time 2D affine mapping.
///
/// Project transforms remain the user-editable position/scale/rotation/anchor
/// model. Rendering composes those transforms across non-raster Node, Clip,
/// and Track containers into this matrix so vector generators reach the final
/// target without being enlarged from an intermediate bitmap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine2D {
    pub scale_x: f64,
    pub skew_x: f64,
    pub translate_x: f64,
    pub skew_y: f64,
    pub scale_y: f64,
    pub translate_y: f64,
}

impl Affine2D {
    pub const IDENTITY: Self = Self {
        scale_x: 1.0,
        skew_x: 0.0,
        translate_x: 0.0,
        skew_y: 0.0,
        scale_y: 1.0,
        translate_y: 0.0,
    };

    pub fn scale(x: f64, y: f64) -> Self {
        Self {
            scale_x: x,
            scale_y: y,
            ..Self::IDENTITY
        }
    }

    pub fn translate(x: f64, y: f64) -> Self {
        Self {
            translate_x: x,
            translate_y: y,
            ..Self::IDENTITY
        }
    }

    /// Compose mappings so `child` is applied first and `self` second.
    pub fn compose(self, child: Self) -> Self {
        Self {
            scale_x: self.scale_x * child.scale_x + self.skew_x * child.skew_y,
            skew_x: self.scale_x * child.skew_x + self.skew_x * child.scale_y,
            translate_x: self.scale_x * child.translate_x
                + self.skew_x * child.translate_y
                + self.translate_x,
            skew_y: self.skew_y * child.scale_x + self.scale_y * child.skew_y,
            scale_y: self.skew_y * child.skew_x + self.scale_y * child.scale_y,
            translate_y: self.skew_y * child.translate_x
                + self.scale_y * child.translate_y
                + self.translate_y,
        }
    }

    pub fn map_point(self, x: f64, y: f64) -> (f64, f64) {
        (
            self.scale_x * x + self.skew_x * y + self.translate_x,
            self.skew_y * x + self.scale_y * y + self.translate_y,
        )
    }
}

impl Default for Affine2D {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl From<&Transform> for Affine2D {
    fn from(transform: &Transform) -> Self {
        let radians = transform.rotation.to_radians();
        let (sin, cos) = radians.sin_cos();
        let scale_x = cos * transform.scale.x;
        let skew_x = -sin * transform.scale.y;
        let skew_y = sin * transform.scale.x;
        let scale_y = cos * transform.scale.y;
        Self {
            scale_x,
            skew_x,
            translate_x: transform.position.x
                - scale_x * transform.anchor.x
                - skew_x * transform.anchor.y,
            skew_y,
            scale_y,
            translate_y: transform.position.y
                - skew_y * transform.anchor.x
                - scale_y * transform.anchor.y,
        }
    }
}

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

#[derive(Clone, Copy)]
pub struct TextRasterRequest<'a> {
    pub text: &'a str,
    pub size: f64,
    pub font_name: &'a str,
    pub styles: &'a [StyleConfig],
    pub ensemble: Option<&'a crate::core::ensemble::EnsembleData>,
    pub transform: Affine2D,
    pub current_time: f64,
}

#[derive(Clone, Copy)]
pub struct ShapeRasterRequest<'a> {
    pub path_data: &'a str,
    pub styles: &'a [StyleConfig],
    pub path_effects: &'a [PathEffect],
    pub ensemble: Option<&'a crate::core::ensemble::EnsembleData>,
    pub transform: Affine2D,
}

pub trait Renderer {
    fn draw_layer(
        &mut self,
        layer: &RenderOutput,
        transform: &Transform,
    ) -> Result<(), LibraryError> {
        self.draw_layer_with_blend(layer, transform, BlendMode::Normal)
    }

    fn draw_layer_with_blend(
        &mut self,
        layer: &RenderOutput,
        transform: &Transform,
        blend_mode: BlendMode,
    ) -> Result<(), LibraryError> {
        self.draw_layer_affine_with_blend(
            layer,
            &Affine2D::from(transform),
            transform.opacity,
            blend_mode,
        )
    }

    fn draw_layer_affine_with_blend(
        &mut self,
        layer: &RenderOutput,
        transform: &Affine2D,
        opacity: f64,
        blend_mode: BlendMode,
    ) -> Result<(), LibraryError>;

    /// Start an isolated transparent/image group render target.
    fn begin_group(
        &mut self,
        width: u32,
        height: u32,
        background_color: &Color,
    ) -> Result<(), LibraryError>;

    /// Finish the current group without compositing it. The caller may apply
    /// effects before drawing this output into its parent target.
    fn end_group(&mut self) -> Result<RenderOutput, LibraryError>;

    fn rasterize_text_layer(
        &mut self,
        request: TextRasterRequest<'_>,
    ) -> Result<RenderOutput, LibraryError>;

    fn rasterize_shape_layer(
        &mut self,
        request: ShapeRasterRequest<'_>,
    ) -> Result<RenderOutput, LibraryError>;

    fn rasterize_sksl_layer(
        &mut self,
        shader_code: &str,
        resolution: (f32, f32),
        time: f32,
        transform: &Affine2D,
    ) -> Result<RenderOutput, LibraryError>;

    fn read_surface(&mut self, output: &RenderOutput) -> Result<Image, LibraryError>;

    fn finalize(&mut self) -> Result<RenderOutput, LibraryError>;
    fn clear(&mut self) -> Result<(), LibraryError>;
    fn get_gpu_context(&mut self) -> Option<&mut crate::rendering::skia_utils::GpuContext> {
        None
    }

    fn set_sharing_context(
        &mut self,
        _handle: usize,
        _hwnd: Option<isize>,
    ) -> Result<(), LibraryError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Affine2D;
    use crate::model::frame::transform::{Position, Scale, Transform};

    #[test]
    fn affine_transform_maps_anchor_to_position_with_rotation_and_anisotropic_scale() {
        let transform = Transform {
            position: Position { x: 130.0, y: 75.0 },
            scale: Scale { x: 2.5, y: 0.4 },
            anchor: Position { x: 17.0, y: -9.0 },
            rotation: 37.0,
            opacity: 0.6,
        };
        let affine = Affine2D::from(&transform);
        let mapped = affine.map_point(transform.anchor.x, transform.anchor.y);
        assert!((mapped.0 - transform.position.x).abs() < 1.0e-10);
        assert!((mapped.1 - transform.position.y).abs() < 1.0e-10);
    }

    #[test]
    fn affine_composition_matches_sequential_container_and_child_mapping() {
        let parent = Affine2D::from(&Transform {
            position: Position { x: 42.0, y: -11.0 },
            scale: Scale { x: 1.8, y: 0.55 },
            anchor: Position { x: 8.0, y: 3.0 },
            rotation: -28.0,
            opacity: 1.0,
        });
        let child = Affine2D::from(&Transform {
            position: Position { x: 19.0, y: 24.0 },
            scale: Scale { x: 0.7, y: 2.2 },
            anchor: Position { x: -4.0, y: 6.0 },
            rotation: 13.0,
            opacity: 1.0,
        });
        let point = (5.5, -7.25);
        let child_mapped = child.map_point(point.0, point.1);
        let sequential = parent.map_point(child_mapped.0, child_mapped.1);
        let composed = parent.compose(child).map_point(point.0, point.1);
        assert!((sequential.0 - composed.0).abs() < 1.0e-10);
        assert!((sequential.1 - composed.1).abs() < 1.0e-10);
    }
}
