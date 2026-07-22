use std::fmt;

use crate::{
    AlphaRepresentation, BackendBuild, ColorManagementError, ComponentStorage, CpuColorProcessor,
};

/// Per-image CPU safety budget. 8K RGBA32F fits; larger working images must
/// use tiled/GPU storage instead of risking a process-aborting allocation.
const MAX_SCENE_LINEAR_IMAGE_BYTES: usize = 512 * 1024 * 1024;

/// Stable identity required at every composite/cache boundary.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WorkingColorIdentity {
    pub project_config_identity: String,
    pub backend_id: String,
    pub backend_build: BackendBuild,
    pub backend_config_fingerprint: String,
    pub working_space: String,
    pub alpha: AlphaRepresentation,
    pub storage: ComponentStorage,
}

impl WorkingColorIdentity {
    pub fn scene_linear_f32(
        project_config_identity: impl Into<String>,
        backend_id: impl Into<String>,
        backend_build: BackendBuild,
        backend_config_fingerprint: impl Into<String>,
        working_space: impl Into<String>,
    ) -> Self {
        Self {
            project_config_identity: project_config_identity.into(),
            backend_id: backend_id.into(),
            backend_build,
            backend_config_fingerprint: backend_config_fingerprint.into(),
            working_space: working_space.into(),
            alpha: AlphaRepresentation::Premultiplied,
            storage: ComponentStorage::Float32,
        }
    }
}

/// Low-level CPU buffer for the scene-linear working pipeline.
///
/// RGB is premultiplied by alpha and deliberately retains negative and
/// greater-than-one values. Alpha remains finite in `[0, 1]`. GPU rendering
/// may store the same contract as RGBA16F. This buffer intentionally has no
/// color-space identity and must be wrapped in [`ManagedSceneLinearImage`]
/// before crossing a render or cache boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneLinearImage {
    width: u32,
    height: u32,
    pixels: Vec<[f32; 4]>,
}

/// Owner-bearing scene-linear image whose working identity cannot be dropped
/// accidentally at a composite/cache boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct ManagedSceneLinearImage {
    identity: WorkingColorIdentity,
    pixels: SceneLinearImage,
}

impl ManagedSceneLinearImage {
    pub fn new(identity: WorkingColorIdentity, pixels: SceneLinearImage) -> Self {
        Self { identity, pixels }
    }

    pub fn identity(&self) -> &WorkingColorIdentity {
        &self.identity
    }

    pub fn pixels(&self) -> &SceneLinearImage {
        &self.pixels
    }

    pub fn into_parts(self) -> (WorkingColorIdentity, SceneLinearImage) {
        (self.identity, self.pixels)
    }

    pub fn composite_source_over(&mut self, source: &Self) -> Result<(), SceneLinearImageError> {
        if self.identity != source.identity {
            return Err(SceneLinearImageError::WorkingIdentityMismatch {
                background: Box::new(self.identity.clone()),
                source: Box::new(source.identity.clone()),
            });
        }
        self.pixels.composite_source_over(&source.pixels)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SceneLinearImageError {
    DimensionOverflow {
        width: u32,
        height: u32,
    },
    ImageBudgetExceeded {
        bytes: usize,
        maximum: usize,
    },
    AllocationFailed {
        bytes: usize,
    },
    InvalidBufferLength {
        expected: usize,
        actual: usize,
    },
    NonFiniteComponent {
        pixel_index: usize,
        component: &'static str,
    },
    AlphaOutOfRange {
        pixel_index: usize,
        alpha: f32,
    },
    DimensionMismatch {
        background: (u32, u32),
        source: (u32, u32),
    },
    WorkingIdentityMismatch {
        background: Box<WorkingColorIdentity>,
        source: Box<WorkingColorIdentity>,
    },
    Transform(ColorManagementError),
}

impl fmt::Display for SceneLinearImageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DimensionOverflow { width, height } => {
                write!(
                    formatter,
                    "image dimensions {width}x{height} overflow addressable memory"
                )
            }
            Self::ImageBudgetExceeded { bytes, maximum } => write!(
                formatter,
                "scene-linear image requires {bytes} bytes; per-image limit is {maximum} bytes"
            ),
            Self::AllocationFailed { bytes } => {
                write!(
                    formatter,
                    "cannot allocate {bytes} bytes for scene-linear image"
                )
            }
            Self::InvalidBufferLength { expected, actual } => {
                write!(
                    formatter,
                    "image buffer has {actual} components; expected {expected}"
                )
            }
            Self::NonFiniteComponent {
                pixel_index,
                component,
            } => write!(
                formatter,
                "scene-linear pixel {pixel_index} has non-finite {component}"
            ),
            Self::AlphaOutOfRange { pixel_index, alpha } => write!(
                formatter,
                "scene-linear pixel {pixel_index} has alpha {alpha}; expected 0 through 1"
            ),
            Self::DimensionMismatch { background, source } => write!(
                formatter,
                "cannot composite source {}x{} over background {}x{}",
                source.0, source.1, background.0, background.1
            ),
            Self::WorkingIdentityMismatch { background, source } => write!(
                formatter,
                "cannot composite working image {source:?} into {background:?}"
            ),
            Self::Transform(error) => write!(formatter, "color transform failed: {error}"),
        }
    }
}

impl std::error::Error for SceneLinearImageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transform(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ColorManagementError> for SceneLinearImageError {
    fn from(error: ColorManagementError) -> Self {
        Self::Transform(error)
    }
}

impl SceneLinearImage {
    /// Create a solid image from one straight encoded color without expanding
    /// an intermediate RGBA8 canvas.
    pub fn solid_from_straight_rgba8(
        width: u32,
        height: u32,
        rgba: [u8; 4],
        source_to_working: &dyn CpuColorProcessor,
    ) -> Result<Self, SceneLinearImageError> {
        let pixel_count = checked_pixel_count(width, height)?;
        let alpha = f32::from(rgba[3]) / 255.0;
        let mut straight = [[
            f32::from(rgba[0]) / 255.0,
            f32::from(rgba[1]) / 255.0,
            f32::from(rgba[2]) / 255.0,
        ]];
        source_to_working.transform_rgb_f32_in_place(&mut straight)?;
        let straight = straight[0];
        let pixel = [
            straight[0] * alpha,
            straight[1] * alpha,
            straight[2] * alpha,
            alpha,
        ];
        let mut pixels = allocate_pixels(width, height)?;
        pixels.resize(pixel_count, pixel);
        Self::from_premultiplied_rgba_f32(width, height, pixels)
    }

    pub fn from_premultiplied_rgba_f32(
        width: u32,
        height: u32,
        mut pixels: Vec<[f32; 4]>,
    ) -> Result<Self, SceneLinearImageError> {
        let expected_pixels = checked_pixel_count(width, height)?;
        if pixels.len() != expected_pixels {
            return Err(SceneLinearImageError::InvalidBufferLength {
                expected: expected_pixels,
                actual: pixels.len(),
            });
        }
        validate_and_canonicalize(&mut pixels)?;
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    /// Decode straight RGBA8 samples through an explicit source-to-working
    /// processor, then premultiply in the working space.
    pub fn from_straight_rgba8(
        width: u32,
        height: u32,
        rgba: &[u8],
        source_to_working: &dyn CpuColorProcessor,
    ) -> Result<Self, SceneLinearImageError> {
        let pixel_count = checked_pixel_count(width, height)?;
        let expected_components = pixel_count
            .checked_mul(4)
            .ok_or(SceneLinearImageError::DimensionOverflow { width, height })?;
        if rgba.len() != expected_components {
            return Err(SceneLinearImageError::InvalidBufferLength {
                expected: expected_components,
                actual: rgba.len(),
            });
        }

        let mut straight = allocate_rgb_pixels(width, height)?;
        for encoded in rgba.chunks_exact(4) {
            straight.push([
                f32::from(encoded[0]) / 255.0,
                f32::from(encoded[1]) / 255.0,
                f32::from(encoded[2]) / 255.0,
            ]);
        }
        source_to_working.transform_rgb_f32_in_place(&mut straight)?;
        let mut pixels = allocate_pixels(width, height)?;
        for (rgb, encoded) in straight.into_iter().zip(rgba.chunks_exact(4)) {
            let alpha = f32::from(encoded[3]) / 255.0;
            pixels.push([rgb[0] * alpha, rgb[1] * alpha, rgb[2] * alpha, alpha]);
        }
        Self::from_premultiplied_rgba_f32(width, height, pixels)
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[[f32; 4]] {
        &self.pixels
    }

    pub fn into_pixels(self) -> Vec<[f32; 4]> {
        self.pixels
    }

    /// Composite this premultiplied background with `source` using source-over.
    pub(crate) fn composite_source_over(
        &mut self,
        source: &Self,
    ) -> Result<(), SceneLinearImageError> {
        if (self.width, self.height) != (source.width, source.height) {
            return Err(SceneLinearImageError::DimensionMismatch {
                background: (self.width, self.height),
                source: (source.width, source.height),
            });
        }
        for (background, source) in self.pixels.iter_mut().zip(&source.pixels) {
            let background_weight = 1.0 - source[3];
            background[0] = source[0] + background[0] * background_weight;
            background[1] = source[1] + background[1] * background_weight;
            background[2] = source[2] + background[2] * background_weight;
            background[3] = source[3] + background[3] * background_weight;
        }
        validate_and_canonicalize(&mut self.pixels)
    }

    /// Apply a working-to-display/output transform and quantize at the terminal
    /// RGBA8 boundary. RGB is clipped only here; alpha is never transformed.
    pub fn to_straight_rgba8(
        &self,
        working_to_output: &dyn CpuColorProcessor,
    ) -> Result<Vec<u8>, SceneLinearImageError> {
        let component_count =
            self.pixels
                .len()
                .checked_mul(4)
                .ok_or(SceneLinearImageError::DimensionOverflow {
                    width: self.width,
                    height: self.height,
                })?;
        let mut straight = allocate_rgb_pixels(self.width, self.height)?;
        for pixel in &self.pixels {
            let alpha = f64::from(pixel[3]);
            let straight_rgb = if alpha == 0.0 {
                [0.0; 3]
            } else {
                [
                    f64::from(pixel[0]) / alpha,
                    f64::from(pixel[1]) / alpha,
                    f64::from(pixel[2]) / alpha,
                ]
            };
            straight.push(straight_rgb.map(|component| component as f32));
        }
        working_to_output.transform_rgb_f32_in_place(&mut straight)?;
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(component_count).map_err(|_| {
            SceneLinearImageError::AllocationFailed {
                bytes: component_count,
            }
        })?;
        for (transformed, pixel) in straight.iter().zip(&self.pixels) {
            let alpha = f64::from(pixel[3]);
            rgba.extend([
                quantize_unorm8(f64::from(transformed[0])),
                quantize_unorm8(f64::from(transformed[1])),
                quantize_unorm8(f64::from(transformed[2])),
                quantize_unorm8(alpha),
            ]);
        }
        Ok(rgba)
    }
}

fn checked_pixel_count(width: u32, height: u32) -> Result<usize, SceneLinearImageError> {
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(SceneLinearImageError::DimensionOverflow { width, height })?;
    let pixels = usize::try_from(pixels)
        .map_err(|_| SceneLinearImageError::DimensionOverflow { width, height })?;
    let bytes = pixels
        .checked_mul(std::mem::size_of::<[f32; 4]>())
        .ok_or(SceneLinearImageError::DimensionOverflow { width, height })?;
    if bytes > MAX_SCENE_LINEAR_IMAGE_BYTES {
        return Err(SceneLinearImageError::ImageBudgetExceeded {
            bytes,
            maximum: MAX_SCENE_LINEAR_IMAGE_BYTES,
        });
    }
    Ok(pixels)
}

fn allocate_pixels(width: u32, height: u32) -> Result<Vec<[f32; 4]>, SceneLinearImageError> {
    let pixel_count = checked_pixel_count(width, height)?;
    let bytes = pixel_count * std::mem::size_of::<[f32; 4]>();
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(pixel_count)
        .map_err(|_| SceneLinearImageError::AllocationFailed { bytes })?;
    Ok(pixels)
}

fn allocate_rgb_pixels(width: u32, height: u32) -> Result<Vec<[f32; 3]>, SceneLinearImageError> {
    let pixel_count = checked_pixel_count(width, height)?;
    let bytes = pixel_count * std::mem::size_of::<[f32; 3]>();
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(pixel_count)
        .map_err(|_| SceneLinearImageError::AllocationFailed { bytes })?;
    Ok(pixels)
}

fn validate_and_canonicalize(pixels: &mut [[f32; 4]]) -> Result<(), SceneLinearImageError> {
    for (pixel_index, pixel) in pixels.iter_mut().enumerate() {
        for (component, value) in ["r", "g", "b", "a"].into_iter().zip(pixel.iter()) {
            if !value.is_finite() {
                return Err(SceneLinearImageError::NonFiniteComponent {
                    pixel_index,
                    component,
                });
            }
        }
        if !(0.0..=1.0).contains(&pixel[3]) {
            return Err(SceneLinearImageError::AlphaOutOfRange {
                pixel_index,
                alpha: pixel[3],
            });
        }
        if pixel[3] == 0.0 {
            *pixel = [0.0; 4];
        }
    }
    Ok(())
}

fn quantize_unorm8(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::{
        ManagedSceneLinearImage, SceneLinearImage, SceneLinearImageError, WorkingColorIdentity,
    };
    use crate::{
        BackendBuild, BuiltinColorTransform, ColorManagementError, ColorTransformBackend,
        ColorTransformRequest, CpuColorProcessor, LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID,
    };

    fn processor(source: &str, target: &str) -> Box<dyn crate::CpuColorProcessor> {
        BuiltinColorTransform
            .create_cpu_processor(&ColorTransformRequest::explicit(source, target))
            .unwrap()
    }

    #[test]
    fn rgba8_round_trip_transforms_rgb_and_preserves_alpha() {
        let to_working = processor(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        let to_display = processor(LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID);
        let image = SceneLinearImage::from_straight_rgba8(
            2,
            1,
            &[128, 64, 32, 128, 90, 80, 70, 0],
            to_working.as_ref(),
        )
        .unwrap();

        assert_eq!(image.pixels()[1], [0.0; 4]);
        assert_eq!(
            image.to_straight_rgba8(to_display.as_ref()).unwrap(),
            [128, 64, 32, 128, 0, 0, 0, 0]
        );
    }

    #[test]
    fn solid_constructor_transforms_once_and_canonicalizes_transparent_rgb() {
        let to_working = processor(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        let image =
            SceneLinearImage::solid_from_straight_rgba8(2, 1, [90, 80, 70, 0], to_working.as_ref())
                .unwrap();
        assert_eq!(image.pixels(), &[[0.0; 4], [0.0; 4]]);
    }

    #[test]
    fn source_over_blends_in_linear_light_instead_of_encoded_srgb() {
        let to_working = processor(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        let to_display = processor(LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID);
        let mut background =
            SceneLinearImage::from_straight_rgba8(1, 1, &[0, 0, 0, 255], to_working.as_ref())
                .unwrap();
        let source =
            SceneLinearImage::from_straight_rgba8(1, 1, &[255, 255, 255, 128], to_working.as_ref())
                .unwrap();

        background.composite_source_over(&source).unwrap();

        let encoded = background.to_straight_rgba8(to_display.as_ref()).unwrap();
        assert_eq!(encoded, [188, 188, 188, 255]);
    }

    #[test]
    fn working_storage_retains_hdr_and_negative_rgb_until_output_boundary() {
        let image =
            SceneLinearImage::from_premultiplied_rgba_f32(1, 1, vec![[-0.25, 2.0, 0.5, 1.0]])
                .unwrap();
        assert_eq!(image.pixels()[0], [-0.25, 2.0, 0.5, 1.0]);

        let identity = processor(LINEAR_SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        assert_eq!(
            image.to_straight_rgba8(identity.as_ref()).unwrap(),
            [0, 255, 128, 255]
        );
    }

    #[test]
    fn invalid_alpha_and_buffer_lengths_fail_closed() {
        assert!(matches!(
            SceneLinearImage::from_premultiplied_rgba_f32(1, 1, vec![[0.0, 0.0, 0.0, 1.1]]),
            Err(SceneLinearImageError::AlphaOutOfRange { .. })
        ));
        assert!(matches!(
            SceneLinearImage::from_straight_rgba8(
                1,
                1,
                &[0, 0, 0],
                processor(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID).as_ref(),
            ),
            Err(SceneLinearImageError::InvalidBufferLength { .. })
        ));
    }

    #[test]
    fn working_identity_mismatch_cannot_be_composited() {
        let image =
            SceneLinearImage::from_premultiplied_rgba_f32(1, 1, vec![[0.25, 0.25, 0.25, 1.0]])
                .unwrap();
        let identity = |space: &str| {
            WorkingColorIdentity::scene_linear_f32(
                "project-config",
                "backend",
                BackendBuild::Real,
                "fingerprint",
                space,
            )
        };
        let mut background = ManagedSceneLinearImage::new(identity("linear-srgb"), image.clone());
        let source = ManagedSceneLinearImage::new(identity("acescg"), image);
        assert!(matches!(
            background.composite_source_over(&source),
            Err(SceneLinearImageError::WorkingIdentityMismatch { .. })
        ));
    }

    #[test]
    fn oversized_image_is_rejected_before_allocation() {
        assert!(matches!(
            SceneLinearImage::solid_from_straight_rgba8(
                100_000,
                100_000,
                [0, 0, 0, 0],
                processor(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID).as_ref(),
            ),
            Err(SceneLinearImageError::ImageBudgetExceeded { .. })
        ));
    }

    struct IdentityProcessor;

    impl CpuColorProcessor for IdentityProcessor {
        fn transform_rgb(&self, rgb: [f64; 3]) -> Result<[f64; 3], ColorManagementError> {
            Ok(rgb)
        }
    }

    #[test]
    fn processor_api_never_receives_alpha() {
        let image = SceneLinearImage::solid_from_straight_rgba8(
            1,
            1,
            [10, 20, 30, 128],
            &IdentityProcessor,
        )
        .unwrap();
        assert_eq!(image.pixels()[0][3], 128.0 / 255.0);
    }
}
