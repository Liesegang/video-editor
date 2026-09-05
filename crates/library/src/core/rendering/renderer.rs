use crate::error::LibraryError;
use crate::model::frame::Image;
use crate::model::frame::color::Color;

use crate::model::BlendMode;
use crate::model::frame::draw_type::PathEffect;
use crate::model::frame::entity::SkSLColorDomain;
use crate::model::frame::entity::StyleConfig;
use crate::model::frame::transform::Transform;
use ruvie_color_management::{
    CpuColorProcessor, ManagedLinearWorkingImage, VerifiedSourceSpace, WorkingColorIdentity,
};
use std::sync::Arc;

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

    /// Return the exact inverse mapping when the affine basis is finite and
    /// non-singular.
    pub fn inverse(self) -> Option<Self> {
        let determinant = self.scale_x * self.scale_y - self.skew_x * self.skew_y;
        if !determinant.is_finite() || determinant.abs() <= f64::EPSILON {
            return None;
        }
        let scale_x = self.scale_y / determinant;
        let skew_x = -self.skew_x / determinant;
        let skew_y = -self.skew_y / determinant;
        let scale_y = self.scale_x / determinant;
        let inverse = Self {
            scale_x,
            skew_x,
            translate_x: -scale_x * self.translate_x - skew_x * self.translate_y,
            skew_y,
            scale_y,
            translate_y: -skew_y * self.translate_x - scale_y * self.translate_y,
        };
        [
            inverse.scale_x,
            inverse.skew_x,
            inverse.translate_x,
            inverse.skew_y,
            inverse.scale_y,
            inverse.translate_y,
        ]
        .into_iter()
        .all(f64::is_finite)
        .then_some(inverse)
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
    /// Legacy, straight-alpha, encoded sRGBA8 compatibility output.
    Image(Image),
    /// Project-authoritative, premultiplied RGBAF32 working-domain output.
    ///
    /// This variant is never a terminal Preview/export surface. The root
    /// Project render applies exactly one destination processor before it
    /// becomes [`Self::Image`].
    Working(ManagedLinearWorkingImage),
    /// Legacy untyped GPU texture. Project working frames deliberately do not
    /// cross this boundary until an owner-bearing typed GPU resource exists.
    Texture(TextureInfo),
}

/// Opaque ownership token for a renderer-native isolated layer.
///
/// The layer remains in its backend storage (including a GPU texture) until a
/// consuming composite operation or explicit release. It must never be
/// serialized or exposed as a user-editable resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RetainedRenderLayer(pub(crate) u64);

/// Verified authoring-to-working contract installed for one Project frame.
///
/// Construction is crate-private: only the exact Project color pipeline can
/// bind the source processor to the Project's verified working identity.
#[derive(Clone)]
pub struct WorkingSurfaceContract {
    identity: WorkingColorIdentity,
    authoring_srgb: VerifiedSourceSpace,
    authoring_to_working: Arc<dyn CpuColorProcessor>,
}

impl std::fmt::Debug for WorkingSurfaceContract {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkingSurfaceContract")
            .field("identity", &self.identity)
            .field("authoring_srgb", &self.authoring_srgb)
            .finish_non_exhaustive()
    }
}

impl WorkingSurfaceContract {
    pub(crate) fn new(
        identity: WorkingColorIdentity,
        authoring_srgb: VerifiedSourceSpace,
        authoring_to_working: Box<dyn CpuColorProcessor>,
    ) -> Result<Self, LibraryError> {
        // Use the managed constructor once to prove that the processor has the
        // exact source/config/context/direction required by this identity.
        ManagedLinearWorkingImage::solid_from_straight_rgba8(
            identity.clone(),
            &authoring_srgb,
            1,
            1,
            [0, 0, 0, 0],
            authoring_to_working.as_ref(),
        )
        .map_err(|error| {
            LibraryError::Render(format!(
                "cannot bind Project authoring colors to the working surface: {error}"
            ))
        })?;
        Ok(Self {
            identity,
            authoring_srgb,
            authoring_to_working: Arc::from(authoring_to_working),
        })
    }

    pub fn identity(&self) -> &WorkingColorIdentity {
        &self.identity
    }

    pub(crate) fn authoring_color_to_working(
        &self,
        color: &Color,
        opacity: f32,
    ) -> Result<[f32; 4], LibraryError> {
        let alpha = (f32::from(color.a) / 255.0 * opacity).clamp(0.0, 1.0);
        let rgb = self
            .authoring_to_working
            .transform_rgb([
                f64::from(color.r) / 255.0,
                f64::from(color.g) / 255.0,
                f64::from(color.b) / 255.0,
            ])
            .map_err(|error| {
                LibraryError::Render(format!(
                    "cannot convert an authored sRGB color into Project working space '{}': {error}",
                    self.identity.working_space()
                ))
            })?;
        let rgba = [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, alpha];
        if rgba.iter().all(|component| component.is_finite()) {
            Ok(rgba)
        } else {
            Err(LibraryError::Render(
                "authoring color conversion produced a non-finite working value".to_string(),
            ))
        }
    }
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
    /// Exact native geometry when available. The SVG string remains only a
    /// legacy/fallback boundary and must not replace weighted conics.
    pub canonical_path: Option<&'a crate::model::path::PathValue>,
    /// Ordered child paths whose composed body owns the layer-style mask.
    /// Empty keeps the aggregate path fast path.
    pub parts: &'a [crate::model::frame::entity::FramePathPart],
    pub styles: &'a [StyleConfig],
    pub path_effects: &'a [PathEffect],
    pub ensemble: Option<&'a crate::core::ensemble::EnsembleData>,
    pub transform: Affine2D,
}

#[derive(Clone, Copy)]
pub struct SkSLRasterRequest<'a> {
    pub shader_code: &'a str,
    pub resolution: (f32, f32),
    pub time: f32,
    pub transform: &'a Affine2D,
    pub color_domain: SkSLColorDomain,
}

#[derive(Clone, Copy)]
pub struct ParticleRasterRequest<'a> {
    pub scene: &'a crate::model::frame::particle::ParticleSceneFrame,
    pub transform: &'a Affine2D,
}

pub trait Renderer {
    /// Select the Project-free encoded-sRGBA8 compatibility contract.
    /// Implementations which have only one legacy surface may keep the
    /// default no-op behavior.
    fn use_unmanaged_srgba8_surface(&mut self) -> Result<(), LibraryError> {
        Ok(())
    }

    /// Select a Project-authoritative RGBAF32 scene-linear contract.
    /// Renderers must fail rather than pretending that an encoded surface is
    /// working-linear.
    fn use_project_linear_surface(
        &mut self,
        _contract: WorkingSurfaceContract,
    ) -> Result<(), LibraryError> {
        Err(LibraryError::Render(
            "renderer does not implement the Project linear-working surface contract".to_string(),
        ))
    }

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

    /// Mix two already-rasterized Timeline transition sources in the active
    /// render domain. Project renderers must keep this operation in their
    /// scene-linear working surface.
    fn draw_cross_dissolve(
        &mut self,
        _from: &RenderOutput,
        _to: &RenderOutput,
        _progress: f32,
        _blend_mode: BlendMode,
    ) -> Result<(), LibraryError> {
        Err(LibraryError::Render(
            "renderer does not implement Timeline Cross Dissolve".to_string(),
        ))
    }

    /// Finish the current group while retaining its native backing store.
    /// Backends which cannot guarantee native lifetime ownership fail closed;
    /// callers must not silently fall back through a CPU readback.
    fn end_group_retained(&mut self) -> Result<RetainedRenderLayer, LibraryError> {
        let _ = self.end_group()?;
        Err(LibraryError::Render(
            "renderer does not support retained Timeline transition layers".to_string(),
        ))
    }

    fn release_retained_layer(&mut self, _layer: RetainedRenderLayer) -> Result<(), LibraryError> {
        Err(LibraryError::Render(
            "renderer does not support retained Timeline transition layers".to_string(),
        ))
    }

    /// Consume two retained layers and mix them into the active target.
    fn draw_cross_dissolve_retained(
        &mut self,
        _from: RetainedRenderLayer,
        _to: RetainedRenderLayer,
        _progress: f32,
        _blend_mode: BlendMode,
    ) -> Result<(), LibraryError> {
        Err(LibraryError::Render(
            "renderer does not support retained Timeline Cross Dissolve".to_string(),
        ))
    }

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

    /// Finish the current group and composite it into its parent target.
    /// Backends override this combined boundary when they can keep the group
    /// in native storage; the default remains correct for CPU renderers.
    fn end_group_and_draw(
        &mut self,
        transform: &Affine2D,
        opacity: f64,
        blend_mode: BlendMode,
    ) -> Result<(), LibraryError> {
        let layer = self.end_group()?;
        self.draw_layer_affine_with_blend(&layer, transform, opacity, blend_mode)
    }

    fn rasterize_text_layer(
        &mut self,
        request: TextRasterRequest<'_>,
    ) -> Result<RenderOutput, LibraryError>;

    /// Render and composite one text layer into the active target. Backends
    /// override this combined boundary when they can retain native storage;
    /// the default remains correct for CPU renderers.
    fn draw_text_layer(
        &mut self,
        request: TextRasterRequest<'_>,
        opacity: f64,
        blend_mode: BlendMode,
    ) -> Result<(), LibraryError> {
        let layer = self.rasterize_text_layer(request)?;
        self.draw_layer_affine_with_blend(&layer, &Affine2D::IDENTITY, opacity, blend_mode)
    }

    fn rasterize_shape_layer(
        &mut self,
        request: ShapeRasterRequest<'_>,
    ) -> Result<RenderOutput, LibraryError>;

    /// Render and composite one shape layer without requiring an intermediate
    /// CPU image when the backend can retain its native surface.
    fn draw_shape_layer(
        &mut self,
        request: ShapeRasterRequest<'_>,
        opacity: f64,
        blend_mode: BlendMode,
    ) -> Result<(), LibraryError> {
        let layer = self.rasterize_shape_layer(request)?;
        self.draw_layer_affine_with_blend(&layer, &Affine2D::IDENTITY, opacity, blend_mode)
    }

    fn rasterize_sksl_layer(
        &mut self,
        request: SkSLRasterRequest<'_>,
    ) -> Result<RenderOutput, LibraryError>;

    /// Render and composite one SkSL layer without requiring an intermediate
    /// CPU image when the backend can retain its native surface.
    fn draw_sksl_layer(
        &mut self,
        request: SkSLRasterRequest<'_>,
        opacity: f64,
        blend_mode: BlendMode,
    ) -> Result<(), LibraryError> {
        let layer = self.rasterize_sksl_layer(request)?;
        self.draw_layer_affine_with_blend(&layer, &Affine2D::IDENTITY, opacity, blend_mode)
    }

    /// Stateful GPU scene boundary. Non-GPU renderers fail closed instead of
    /// substituting a CPU implementation with different behavior.
    fn rasterize_particle_layer(
        &mut self,
        _request: ParticleRasterRequest<'_>,
    ) -> Result<RenderOutput, LibraryError> {
        Err(LibraryError::Render(
            "GPU Particle requires an OpenGL 4.3 SceneRuntime; this renderer has no compatible GPU boundary"
                .to_string(),
        ))
    }

    /// Prove that the complete stateful Particle backend is usable before an
    /// exporter creates any externally visible output. Implementations must
    /// validate their real execution/storage path, not merely the presence of
    /// a nominal GPU context. `target_sizes` are the distinct render targets
    /// reached by Particle scenes in the requested export range.
    fn preflight_particle_backend(
        &mut self,
        _target_sizes: &[(u32, u32)],
    ) -> Result<(), LibraryError> {
        Err(LibraryError::Render(
            "GPU Particle requires an OpenGL 4.3 SceneRuntime; this renderer cannot preflight that backend"
                .to_string(),
        ))
    }

    /// Render and composite one stateful Particle scene into the active
    /// backend target. GPU renderers override this boundary so the scene
    /// texture remains backend-native instead of round-tripping through a
    /// full-frame CPU image before the immediately following composite.
    fn draw_particle_layer(
        &mut self,
        request: ParticleRasterRequest<'_>,
        opacity: f64,
        blend_mode: BlendMode,
    ) -> Result<(), LibraryError> {
        let layer = self.rasterize_particle_layer(request)?;
        self.draw_layer_affine_with_blend(&layer, &Affine2D::IDENTITY, opacity, blend_mode)
    }

    fn read_surface(&mut self, output: &RenderOutput) -> Result<Image, LibraryError>;

    fn finalize(&mut self) -> Result<RenderOutput, LibraryError>;
    /// Apply a complete Project-authorized terminal chain while working pixels
    /// remain backend-native. `None` means unsupported, with no frame mutation;
    /// execution failures are errors, never an invitation to ignore color.
    fn finalize_gpu_terminal(
        &mut self,
        _chain: &ruvie_color_management::GpuTerminalChain,
    ) -> Result<Option<Image>, LibraryError> {
        Ok(None)
    }
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
