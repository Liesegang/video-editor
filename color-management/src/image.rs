use std::fmt;

use crate::processor_boundary::{validate_output_direction, validate_source_direction};
use crate::{ColorManagementError, CpuColorProcessor, WorkingColorIdentity};

/// Per-image CPU safety budget. 8K RGBA32F fits; larger working images must
/// use tiled/GPU storage instead of risking a process-aborting allocation.
const MAX_LINEAR_WORKING_IMAGE_BYTES: usize = 512 * 1024 * 1024;

/// Low-level CPU buffer in the Project-selected linear working domain.
///
/// RGB is premultiplied by alpha and deliberately retains negative and
/// greater-than-one values. Alpha remains finite in `[0, 1]`. GPU rendering
/// may store the same contract as RGBA16F. This buffer intentionally has no
/// color-space identity and must be wrapped in [`crate::ManagedLinearWorkingImage`]
/// before crossing a render or cache boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct LinearWorkingImage {
    width: u32,
    height: u32,
    pixels: Vec<[f32; 4]>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LinearWorkingImageError {
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
    InvalidPixelCount {
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

impl fmt::Display for LinearWorkingImageError {
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
                "linear working image requires {bytes} bytes; per-image limit is {maximum} bytes"
            ),
            Self::AllocationFailed { bytes } => {
                write!(
                    formatter,
                    "cannot allocate {bytes} bytes for linear working image"
                )
            }
            Self::InvalidBufferLength { expected, actual } => {
                write!(
                    formatter,
                    "image buffer has {actual} components; expected {expected}"
                )
            }
            Self::InvalidPixelCount { expected, actual } => {
                write!(
                    formatter,
                    "image buffer has {actual} pixels; expected {expected}"
                )
            }
            Self::NonFiniteComponent {
                pixel_index,
                component,
            } => write!(
                formatter,
                "image pixel {pixel_index} has non-finite {component}"
            ),
            Self::AlphaOutOfRange { pixel_index, alpha } => write!(
                formatter,
                "linear working pixel {pixel_index} has alpha {alpha}; expected 0 through 1"
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

impl std::error::Error for LinearWorkingImageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transform(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ColorManagementError> for LinearWorkingImageError {
    fn from(error: ColorManagementError) -> Self {
        Self::Transform(error)
    }
}

impl LinearWorkingImage {
    /// Create a solid image from one straight encoded color without expanding
    /// an intermediate RGBA8 canvas.
    pub fn solid_from_straight_rgba8(
        width: u32,
        height: u32,
        rgba: [u8; 4],
        source_to_working: &dyn CpuColorProcessor,
    ) -> Result<Self, LinearWorkingImageError> {
        validate_source_direction(source_to_working)?;
        Self::solid_from_straight_rgba8_unchecked(width, height, rgba, source_to_working)
    }

    fn solid_from_straight_rgba8_unchecked(
        width: u32,
        height: u32,
        rgba: [u8; 4],
        source_to_working: &dyn CpuColorProcessor,
    ) -> Result<Self, LinearWorkingImageError> {
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
    ) -> Result<Self, LinearWorkingImageError> {
        let expected_pixels = checked_pixel_count(width, height)?;
        if pixels.len() != expected_pixels {
            return Err(LinearWorkingImageError::InvalidBufferLength {
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
    ) -> Result<Self, LinearWorkingImageError> {
        validate_source_direction(source_to_working)?;
        Self::from_straight_rgba8_unchecked(width, height, rgba, source_to_working)
    }

    fn from_straight_rgba8_unchecked(
        width: u32,
        height: u32,
        rgba: &[u8],
        source_to_working: &dyn CpuColorProcessor,
    ) -> Result<Self, LinearWorkingImageError> {
        let pixel_count = checked_pixel_count(width, height)?;
        let expected_components = pixel_count
            .checked_mul(4)
            .ok_or(LinearWorkingImageError::DimensionOverflow { width, height })?;
        if rgba.len() != expected_components {
            return Err(LinearWorkingImageError::InvalidBufferLength {
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

    /// Decode straight RGBA32F samples through an explicit source-to-working
    /// processor, then premultiply in the working space.
    ///
    /// RGB remains extended range. Alpha is never passed to the color backend
    /// and must be finite in `[0, 1]`; fully transparent output is canonical
    /// transparent black.
    pub fn from_straight_rgba_f32(
        width: u32,
        height: u32,
        rgba: &[[f32; 4]],
        source_to_working: &dyn CpuColorProcessor,
    ) -> Result<Self, LinearWorkingImageError> {
        validate_source_direction(source_to_working)?;
        Self::from_straight_rgba_f32_unchecked(width, height, rgba, source_to_working)
    }

    fn from_straight_rgba_f32_unchecked(
        width: u32,
        height: u32,
        rgba: &[[f32; 4]],
        source_to_working: &dyn CpuColorProcessor,
    ) -> Result<Self, LinearWorkingImageError> {
        let pixel_count = checked_pixel_count(width, height)?;
        if rgba.len() != pixel_count {
            return Err(LinearWorkingImageError::InvalidPixelCount {
                expected: pixel_count,
                actual: rgba.len(),
            });
        }

        validate_straight_rgba_f32(rgba)?;
        let mut straight = allocate_rgb_pixels(width, height)?;
        straight.extend(rgba.iter().map(|pixel| [pixel[0], pixel[1], pixel[2]]));
        source_to_working.transform_rgb_f32_in_place(&mut straight)?;
        validate_rgb_pixels(&straight)?;

        let mut pixels = allocate_pixels(width, height)?;
        for (rgb, source) in straight.into_iter().zip(rgba) {
            let alpha = source[3];
            if alpha == 0.0 {
                pixels.push([0.0; 4]);
            } else {
                pixels.push([rgb[0] * alpha, rgb[1] * alpha, rgb[2] * alpha, alpha]);
            }
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
    ) -> Result<(), LinearWorkingImageError> {
        if (self.width, self.height) != (source.width, source.height) {
            return Err(LinearWorkingImageError::DimensionMismatch {
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

    /// Apply a working-to-display/output transform without quantizing.
    ///
    /// The returned RGB remains extended-range f32 (negative and greater than
    /// one values are preserved), while alpha is copied exactly and never
    /// submitted to the color backend. Fully transparent pixels remain
    /// canonical transparent black. Backend output is validated even when the
    /// processor overrides its bulk method.
    pub fn to_straight_rgba_f32(
        &self,
        working_to_output: &dyn CpuColorProcessor,
    ) -> Result<Vec<[f32; 4]>, LinearWorkingImageError> {
        validate_output_direction(working_to_output)?;
        self.to_straight_rgba_f32_unchecked(working_to_output)
    }

    fn to_straight_rgba_f32_unchecked(
        &self,
        working_to_output: &dyn CpuColorProcessor,
    ) -> Result<Vec<[f32; 4]>, LinearWorkingImageError> {
        let mut straight = allocate_rgb_pixels(self.width, self.height)?;
        for pixel in &self.pixels {
            let alpha = pixel[3];
            let straight_rgb = if alpha == 0.0 {
                [0.0; 3]
            } else {
                [pixel[0] / alpha, pixel[1] / alpha, pixel[2] / alpha]
            };
            straight.push(straight_rgb);
        }
        validate_rgb_pixels(&straight)?;
        working_to_output.transform_rgb_f32_in_place(&mut straight)?;
        validate_rgb_pixels(&straight)?;

        let mut rgba = allocate_pixels(self.width, self.height)?;
        for (transformed, pixel) in straight.into_iter().zip(&self.pixels) {
            if pixel[3] == 0.0 {
                rgba.push([0.0; 4]);
            } else {
                rgba.push([transformed[0], transformed[1], transformed[2], pixel[3]]);
            }
        }
        Ok(rgba)
    }

    /// Apply a working-to-display/output transform and quantize at the terminal
    /// RGBA8 boundary. RGB is clipped only by the packing step.
    pub fn to_straight_rgba8(
        &self,
        working_to_output: &dyn CpuColorProcessor,
    ) -> Result<Vec<u8>, LinearWorkingImageError> {
        validate_output_direction(working_to_output)?;
        let straight = self.to_straight_rgba_f32_unchecked(working_to_output)?;
        pack_straight_rgba8(self.width, self.height, &straight)
    }
}

pub(crate) fn pack_straight_rgba8(
    width: u32,
    height: u32,
    straight: &[[f32; 4]],
) -> Result<Vec<u8>, LinearWorkingImageError> {
    let component_count = straight
        .len()
        .checked_mul(4)
        .ok_or(LinearWorkingImageError::DimensionOverflow { width, height })?;
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(component_count).map_err(|_| {
        LinearWorkingImageError::AllocationFailed {
            bytes: component_count,
        }
    })?;
    for pixel in straight {
        rgba.extend([
            quantize_unorm8(f64::from(pixel[0])),
            quantize_unorm8(f64::from(pixel[1])),
            quantize_unorm8(f64::from(pixel[2])),
            quantize_unorm8(f64::from(pixel[3])),
        ]);
    }
    Ok(rgba)
}

fn checked_pixel_count(width: u32, height: u32) -> Result<usize, LinearWorkingImageError> {
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(LinearWorkingImageError::DimensionOverflow { width, height })?;
    let pixels = usize::try_from(pixels)
        .map_err(|_| LinearWorkingImageError::DimensionOverflow { width, height })?;
    let bytes = pixels
        .checked_mul(std::mem::size_of::<[f32; 4]>())
        .ok_or(LinearWorkingImageError::DimensionOverflow { width, height })?;
    if bytes > MAX_LINEAR_WORKING_IMAGE_BYTES {
        return Err(LinearWorkingImageError::ImageBudgetExceeded {
            bytes,
            maximum: MAX_LINEAR_WORKING_IMAGE_BYTES,
        });
    }
    Ok(pixels)
}

fn allocate_pixels(width: u32, height: u32) -> Result<Vec<[f32; 4]>, LinearWorkingImageError> {
    let pixel_count = checked_pixel_count(width, height)?;
    let bytes = pixel_count * std::mem::size_of::<[f32; 4]>();
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(pixel_count)
        .map_err(|_| LinearWorkingImageError::AllocationFailed { bytes })?;
    Ok(pixels)
}

fn allocate_rgb_pixels(width: u32, height: u32) -> Result<Vec<[f32; 3]>, LinearWorkingImageError> {
    let pixel_count = checked_pixel_count(width, height)?;
    let bytes = pixel_count * std::mem::size_of::<[f32; 3]>();
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(pixel_count)
        .map_err(|_| LinearWorkingImageError::AllocationFailed { bytes })?;
    Ok(pixels)
}

fn validate_and_canonicalize(pixels: &mut [[f32; 4]]) -> Result<(), LinearWorkingImageError> {
    for (pixel_index, pixel) in pixels.iter_mut().enumerate() {
        for (component, value) in ["r", "g", "b", "a"].into_iter().zip(pixel.iter()) {
            if !value.is_finite() {
                return Err(LinearWorkingImageError::NonFiniteComponent {
                    pixel_index,
                    component,
                });
            }
        }
        if !(0.0..=1.0).contains(&pixel[3]) {
            return Err(LinearWorkingImageError::AlphaOutOfRange {
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

pub(crate) fn validate_rgb_pixels(pixels: &[[f32; 3]]) -> Result<(), LinearWorkingImageError> {
    for (pixel_index, pixel) in pixels.iter().enumerate() {
        for (component, value) in ["r", "g", "b"].into_iter().zip(pixel) {
            if !value.is_finite() {
                return Err(LinearWorkingImageError::NonFiniteComponent {
                    pixel_index,
                    component,
                });
            }
        }
    }
    Ok(())
}

fn validate_straight_rgba_f32(pixels: &[[f32; 4]]) -> Result<(), LinearWorkingImageError> {
    for (pixel_index, pixel) in pixels.iter().enumerate() {
        for (component, value) in ["r", "g", "b", "a"].into_iter().zip(pixel) {
            if !value.is_finite() {
                return Err(LinearWorkingImageError::NonFiniteComponent {
                    pixel_index,
                    component,
                });
            }
        }
        if !(0.0..=1.0).contains(&pixel[3]) {
            return Err(LinearWorkingImageError::AlphaOutOfRange {
                pixel_index,
                alpha: pixel[3],
            });
        }
    }
    Ok(())
}

fn quantize_unorm8(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
#[path = "pq_image_tests.rs"]
mod pq_image_tests;

#[cfg(test)]
mod tests {
    use super::{LinearWorkingImage, LinearWorkingImageError};
    use crate::{
        BuiltinColorTransform, ColorContext, ColorManagementError, ColorTransformBackend,
        ColorTransformRequest, CompiledTransformIdentity, CpuColorProcessor, LINEAR_BT709_SPACE_ID,
        LINEAR_SRGB_SPACE_ID, ManagedLinearWorkingImage, SRGB_SPACE_ID, VerifiedSourceSpace,
        WorkingColorIdentity,
    };

    fn source_processor(source: &str, target: &str) -> Box<dyn crate::CpuColorProcessor> {
        BuiltinColorTransform
            .create_cpu_processor(&ColorTransformRequest::source_to_working(source, target))
            .unwrap()
    }

    fn output_processor(source: &str, target: &str) -> Box<dyn crate::CpuColorProcessor> {
        BuiltinColorTransform
            .create_cpu_processor(&ColorTransformRequest::working_to_output(source, target))
            .unwrap()
    }

    fn working_identity(project: &str) -> WorkingColorIdentity {
        let backend = BuiltinColorTransform;
        let verified = backend
            .verify_working_space(LINEAR_SRGB_SPACE_ID, &ColorContext::default())
            .unwrap();
        WorkingColorIdentity::from_verified(project, verified).unwrap()
    }

    fn source_space(space: &str) -> VerifiedSourceSpace {
        BuiltinColorTransform
            .verify_source_space(space, &ColorContext::default())
            .unwrap()
    }

    #[test]
    fn rgba8_round_trip_transforms_rgb_and_preserves_alpha() {
        let to_working = source_processor(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        let to_display = output_processor(LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID);
        let image = LinearWorkingImage::from_straight_rgba8(
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
    fn managed_rgba32f_ingress_preserves_extended_rgb_and_canonicalizes_transparency() {
        let source = source_space(LINEAR_SRGB_SPACE_ID);
        let processor = source_processor(LINEAR_SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        let image = ManagedLinearWorkingImage::from_straight_rgba_f32(
            working_identity("project-config"),
            &source,
            2,
            1,
            &[[-0.5, 2.0, 0.25, 0.25], [8.0, -4.0, 2.0, 0.0]],
            processor.as_ref(),
        )
        .unwrap();

        assert_eq!(image.pixels().pixels()[0], [-0.125, 0.5, 0.0625, 0.25]);
        assert_eq!(image.pixels().pixels()[1], [0.0; 4]);
    }

    #[test]
    fn rgba32f_ingress_rejects_invalid_length_components_and_alpha() {
        let processor = source_processor(LINEAR_SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        assert!(matches!(
            LinearWorkingImage::from_straight_rgba_f32(2, 1, &[[0.0; 4]], processor.as_ref()),
            Err(LinearWorkingImageError::InvalidPixelCount {
                expected: 2,
                actual: 1
            })
        ));
        assert!(matches!(
            LinearWorkingImage::from_straight_rgba_f32(
                1,
                1,
                &[[f32::NAN, 0.0, 0.0, 1.0]],
                processor.as_ref(),
            ),
            Err(LinearWorkingImageError::NonFiniteComponent {
                pixel_index: 0,
                component: "r"
            })
        ));
        assert!(matches!(
            LinearWorkingImage::from_straight_rgba_f32(
                1,
                1,
                &[[0.0, 0.0, 0.0, 1.5]],
                processor.as_ref(),
            ),
            Err(LinearWorkingImageError::AlphaOutOfRange { pixel_index: 0, .. })
        ));
    }

    #[test]
    fn solid_constructor_transforms_once_and_canonicalizes_transparent_rgb() {
        let to_working = source_processor(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        let image = LinearWorkingImage::solid_from_straight_rgba8(
            2,
            1,
            [90, 80, 70, 0],
            to_working.as_ref(),
        )
        .unwrap();
        assert_eq!(image.pixels(), &[[0.0; 4], [0.0; 4]]);
    }

    #[test]
    fn source_over_blends_in_linear_light_instead_of_encoded_srgb() {
        let to_working = source_processor(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        let to_display = output_processor(LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID);
        let mut background =
            LinearWorkingImage::from_straight_rgba8(1, 1, &[0, 0, 0, 255], to_working.as_ref())
                .unwrap();
        let source = LinearWorkingImage::from_straight_rgba8(
            1,
            1,
            &[255, 255, 255, 128],
            to_working.as_ref(),
        )
        .unwrap();

        background.composite_source_over(&source).unwrap();

        let encoded = background.to_straight_rgba8(to_display.as_ref()).unwrap();
        assert_eq!(encoded, [188, 188, 188, 255]);
    }

    #[test]
    fn working_storage_retains_hdr_and_negative_rgb_until_output_boundary() {
        let image =
            LinearWorkingImage::from_premultiplied_rgba_f32(1, 1, vec![[-0.25, 2.0, 0.5, 1.0]])
                .unwrap();
        assert_eq!(image.pixels()[0], [-0.25, 2.0, 0.5, 1.0]);

        let identity = output_processor(LINEAR_SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        assert_eq!(
            image.to_straight_rgba_f32(identity.as_ref()).unwrap(),
            [[-0.25, 2.0, 0.5, 1.0]]
        );
        assert_eq!(
            image.to_straight_rgba8(identity.as_ref()).unwrap(),
            [0, 255, 128, 255]
        );
    }

    #[test]
    fn invalid_alpha_and_buffer_lengths_fail_closed() {
        assert!(matches!(
            LinearWorkingImage::from_premultiplied_rgba_f32(1, 1, vec![[0.0, 0.0, 0.0, 1.1]]),
            Err(LinearWorkingImageError::AlphaOutOfRange { .. })
        ));
        assert!(matches!(
            LinearWorkingImage::from_straight_rgba8(
                1,
                1,
                &[0, 0, 0],
                source_processor(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID).as_ref(),
            ),
            Err(LinearWorkingImageError::InvalidBufferLength { .. })
        ));
    }

    #[test]
    fn working_identity_mismatch_cannot_be_composited() {
        let processor = source_processor(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        let source_space = source_space(SRGB_SPACE_ID);
        let mut background = ManagedLinearWorkingImage::solid_from_straight_rgba8(
            working_identity("project-a"),
            &source_space,
            1,
            1,
            [128, 128, 128, 255],
            processor.as_ref(),
        )
        .unwrap();
        let source = ManagedLinearWorkingImage::solid_from_straight_rgba8(
            working_identity("project-b"),
            &source_space,
            1,
            1,
            [128, 128, 128, 255],
            processor.as_ref(),
        )
        .unwrap();
        assert!(matches!(
            background.composite_source_over(&source),
            Err(LinearWorkingImageError::WorkingIdentityMismatch { .. })
        ));
    }

    #[test]
    fn managed_ingress_rejects_wrong_direction_destination_and_context() {
        let identity = working_identity("project-config");
        let backend = BuiltinColorTransform;
        let source = source_space(SRGB_SPACE_ID);
        let explicit = backend
            .create_cpu_processor(&ColorTransformRequest::explicit(
                SRGB_SPACE_ID,
                LINEAR_SRGB_SPACE_ID,
            ))
            .unwrap();
        assert!(matches!(
            ManagedLinearWorkingImage::from_straight_rgba8(
                identity.clone(),
                &source,
                1,
                1,
                &[0, 0, 0, 255],
                explicit.as_ref(),
            ),
            Err(LinearWorkingImageError::Transform(
                ColorManagementError::ProcessorContractMismatch { .. }
            ))
        ));

        let wrong_destination = source_processor(SRGB_SPACE_ID, LINEAR_BT709_SPACE_ID);
        assert!(matches!(
            ManagedLinearWorkingImage::from_straight_rgba8(
                identity.clone(),
                &source,
                1,
                1,
                &[0, 0, 0, 255],
                wrong_destination.as_ref(),
            ),
            Err(LinearWorkingImageError::Transform(
                ColorManagementError::ProcessorContractMismatch { .. }
            ))
        ));

        let contextual = backend
            .create_cpu_processor(
                &ColorTransformRequest::source_to_working(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID)
                    .with_context(ColorContext::default().with_variable("SHOT", "010")),
            )
            .unwrap();
        assert!(matches!(
            ManagedLinearWorkingImage::from_straight_rgba8(
                identity,
                &source,
                1,
                1,
                &[0, 0, 0, 255],
                contextual.as_ref(),
            ),
            Err(LinearWorkingImageError::Transform(
                ColorManagementError::ProcessorContractMismatch { .. }
            ))
        ));
    }

    #[test]
    fn managed_ingress_rejects_processor_for_a_different_resolved_source() {
        let identity = working_identity("project-config");
        let resolved_source = source_space(SRGB_SPACE_ID);
        let wrong_source_processor = source_processor(LINEAR_SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);

        assert!(matches!(
            ManagedLinearWorkingImage::from_straight_rgba8(
                identity,
                &resolved_source,
                1,
                1,
                &[128, 64, 32, 255],
                wrong_source_processor.as_ref(),
            ),
            Err(LinearWorkingImageError::Transform(
                ColorManagementError::ProcessorContractMismatch { .. }
            ))
        ));
    }

    #[test]
    fn managed_ingress_rejects_source_token_from_a_different_config_or_context() {
        let identity = working_identity("project-config");
        let context = ColorContext::default().with_variable("SHOT", "010");
        let source = BuiltinColorTransform
            .verify_source_space(SRGB_SPACE_ID, &context)
            .unwrap();
        let processor = BuiltinColorTransform
            .create_cpu_processor(
                &ColorTransformRequest::source_to_working(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID)
                    .with_context(context),
            )
            .unwrap();

        assert!(matches!(
            ManagedLinearWorkingImage::from_straight_rgba8(
                identity,
                &source,
                1,
                1,
                &[128, 64, 32, 255],
                processor.as_ref(),
            ),
            Err(LinearWorkingImageError::Transform(
                ColorManagementError::ProcessorContractMismatch { .. }
            ))
        ));

        let valid_source = source_space(SRGB_SPACE_ID);
        let wrong_config_source = VerifiedSourceSpace::new(
            valid_source.backend_id().to_string(),
            valid_source.backend_build(),
            "mismatched-config".to_string(),
            valid_source.context().clone(),
            valid_source.color_space().clone(),
        );
        let processor = source_processor(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        assert!(matches!(
            ManagedLinearWorkingImage::from_straight_rgba8(
                working_identity("project-config"),
                &wrong_config_source,
                1,
                1,
                &[128, 64, 32, 255],
                processor.as_ref(),
            ),
            Err(LinearWorkingImageError::Transform(
                ColorManagementError::ProcessorContractMismatch { .. }
            ))
        ));
    }

    #[test]
    fn managed_egress_rejects_a_processor_from_the_wrong_working_space() {
        let to_working = source_processor(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        let image = ManagedLinearWorkingImage::solid_from_straight_rgba8(
            working_identity("project-config"),
            &source_space(SRGB_SPACE_ID),
            1,
            1,
            [128, 128, 128, 255],
            to_working.as_ref(),
        )
        .unwrap();
        let wrong_source = output_processor(LINEAR_BT709_SPACE_ID, SRGB_SPACE_ID);

        assert!(matches!(
            image.to_straight_rgba8(wrong_source.as_ref()),
            Err(LinearWorkingImageError::Transform(
                ColorManagementError::ProcessorContractMismatch { .. }
            ))
        ));
    }

    #[test]
    fn oversized_image_is_rejected_before_allocation() {
        assert!(matches!(
            LinearWorkingImage::solid_from_straight_rgba8(
                100_000,
                100_000,
                [0, 0, 0, 0],
                source_processor(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID).as_ref(),
            ),
            Err(LinearWorkingImageError::ImageBudgetExceeded { .. })
        ));
    }

    struct InvalidBulkProcessor {
        identity: CompiledTransformIdentity,
    }

    impl crate::transform::sealed::Processor for InvalidBulkProcessor {}

    impl CpuColorProcessor for InvalidBulkProcessor {
        fn compiled_transform_identity(&self) -> &CompiledTransformIdentity {
            &self.identity
        }

        fn transform_rgb(&self, rgb: [f64; 3]) -> Result<[f64; 3], ColorManagementError> {
            Ok(rgb)
        }

        fn transform_rgb_f32_in_place(
            &self,
            pixels: &mut [[f32; 3]],
        ) -> Result<(), ColorManagementError> {
            pixels[0][1] = f32::NAN;
            Ok(())
        }
    }

    #[test]
    fn processor_api_never_receives_alpha() {
        let processor = source_processor(LINEAR_SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        let image = LinearWorkingImage::solid_from_straight_rgba8(
            1,
            1,
            [10, 20, 30, 128],
            processor.as_ref(),
        )
        .unwrap();
        assert_eq!(image.pixels()[0][3], 128.0 / 255.0);
    }

    #[test]
    fn float_terminal_unpremultiplies_without_clipping_and_preserves_alpha() {
        let image = LinearWorkingImage::from_premultiplied_rgba_f32(
            2,
            1,
            vec![[-0.125, 0.5, 0.25, 0.25], [0.0; 4]],
        )
        .unwrap();
        let processor = output_processor(LINEAR_SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);

        assert_eq!(
            image.to_straight_rgba_f32(processor.as_ref()).unwrap(),
            [[-0.5, 2.0, 1.0, 0.25], [0.0; 4]]
        );
    }

    #[test]
    fn float_terminal_rejects_invalid_bulk_backend_output() {
        let image =
            LinearWorkingImage::from_premultiplied_rgba_f32(1, 1, vec![[0.25, 0.25, 0.25, 1.0]])
                .unwrap();
        let valid = output_processor(LINEAR_SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        let invalid = InvalidBulkProcessor {
            identity: valid.compiled_transform_identity().clone(),
        };

        assert!(matches!(
            image.to_straight_rgba_f32(&invalid),
            Err(LinearWorkingImageError::NonFiniteComponent {
                pixel_index: 0,
                component: "g"
            })
        ));
    }

    #[test]
    fn working_color_identity_is_independent_of_image_storage() {
        let color_identity = working_identity("project-config");
        let to_working = source_processor(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        let image_f32 = ManagedLinearWorkingImage::solid_from_straight_rgba8(
            color_identity.clone(),
            &source_space(SRGB_SPACE_ID),
            1,
            1,
            [128, 128, 128, 255],
            to_working.as_ref(),
        )
        .unwrap();
        assert_eq!(image_f32.identity(), &color_identity);
    }
}
