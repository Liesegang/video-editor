pub mod ffmpeg_video;
pub mod native_image;

mod decoded_color;
mod ffmpeg_color_metadata;
mod ffmpeg_frame_cache;
mod ffmpeg_pixel_decode;
#[cfg(test)]
mod ffmpeg_rgb_decode_tests;
mod ffmpeg_runtime;
#[cfg(test)]
mod ffmpeg_video_loader_tests;
mod ffmpeg_yuv_color;
mod ffmpeg_yuv_decode;
#[cfg(test)]
mod ffmpeg_yuv_decode_tests;
mod native_image_decode;
mod native_png_chunk_inventory;
mod native_png_metadata;
mod native_still_format_probe;
mod source_file_identity;

pub(crate) use source_file_identity::FileIdentity;

pub use self::decoded_color::{
    AppliedYuvChromaLocation, AppliedYuvToRgb, ConfigOwnedColorSpace, ConfigOwnedColorSpaceError,
    DecodedColorSpace, DecodedPixelDescription, DecodedRgbConversion, UntaggedSrgbAssumption,
    UntaggedSrgbAssumptionError, UntaggedSrgbPolicy, YuvChromaLocation, YuvChromaLocationSource,
    YuvToRgbOperation,
};
pub use self::ffmpeg_video::FfmpegVideoLoader;
pub use self::native_image::NativeImageLoader;

use crate::cache::CacheManager;
use crate::error::LibraryError;
use crate::model::asset::AssetKind;
use crate::model::asset::{DecoderSourceColorAuthority, SourceColorDescription};
use crate::model::frame::Image;
use crate::plugin::{Plugin, PluginCategory};
use half::f16;
use ruvie_color_management::AlphaRepresentation;
use std::collections::HashMap;
use std::mem::size_of;
use std::sync::Arc;
use thiserror::Error;

/// CPU loader payload budget. 8K RGBA32F fits; larger sources must use a
/// tiled/GPU decode path instead of adopting an unbounded allocation.
const MAX_DECODED_IMAGE_BYTES: usize = 512 * 1024 * 1024;

/// Concrete storage adopted at the loader boundary.
///
/// Callers must validate the storage they will actually return, rather than
/// the (often smaller) encoded or decoder-native representation. For example,
/// a Gray16 still becomes straight RGBA32F and is therefore charged at
/// sixteen bytes per pixel before decoding starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DecodedPixelStorage {
    StraightRgba8,
    StraightRgba16F,
    StraightRgba32F,
}

impl DecodedPixelStorage {
    const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::StraightRgba8 => size_of::<[u8; 4]>(),
            Self::StraightRgba16F => size_of::<[f16; 4]>(),
            Self::StraightRgba32F => size_of::<[f32; 4]>(),
        }
    }
}

/// Allocation-free proof that a decoded payload fits the CPU loader budget.
///
/// The layout is intentionally typed by [`DecodedPixelStorage`]. Accepting an
/// arbitrary byte count here would let a high-precision caller accidentally
/// validate RGBA8 and then allocate RGBA32F.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DecodedPixelLayout {
    width: u32,
    height: u32,
    storage: DecodedPixelStorage,
    pixel_count: usize,
    byte_len: usize,
}

impl DecodedPixelLayout {
    pub(crate) const fn width(self) -> u32 {
        self.width
    }

    pub(crate) const fn height(self) -> u32 {
        self.height
    }

    pub(crate) const fn storage(self) -> DecodedPixelStorage {
        self.storage
    }

    pub(crate) const fn pixel_count(self) -> usize {
        self.pixel_count
    }

    pub(crate) const fn byte_len(self) -> usize {
        self.byte_len
    }
}

pub(crate) fn validate_decoded_pixel_layout(
    width: u32,
    height: u32,
    storage: DecodedPixelStorage,
) -> Result<DecodedPixelLayout, DecodedPixelBufferError> {
    if width == 0 || height == 0 {
        return Err(DecodedPixelBufferError::EmptyDimensions { width, height });
    }
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|count| usize::try_from(count).ok())
        .ok_or(DecodedPixelBufferError::DimensionOverflow { width, height })?;
    let bytes = pixels
        .checked_mul(storage.bytes_per_pixel())
        .ok_or(DecodedPixelBufferError::DimensionOverflow { width, height })?;
    if bytes > MAX_DECODED_IMAGE_BYTES {
        return Err(DecodedPixelBufferError::ImageBudgetExceeded {
            bytes,
            maximum: MAX_DECODED_IMAGE_BYTES,
        });
    }
    Ok(DecodedPixelLayout {
        width,
        height,
        storage,
        pixel_count: pixels,
        byte_len: bytes,
    })
}

#[derive(Debug, Clone)]
pub enum LoadRequest {
    /// Load a static image.
    Image { path: String },
    /// Load a video frame.
    VideoFrame {
        path: String,
        /// Source-local seconds. This is the sole decode authority; the
        /// selected loader converts it into the stream's time-base/PTS.
        source_time: f64,
        stream_index: Option<usize>,
        /// Project-owned source-color authority. Complete user overrides and
        /// conditional import assumptions are different variants so a stale
        /// assumption can never override newly tagged decoded frames.
        source_color_authority: Option<DecoderSourceColorAuthority>,
    },
}

impl LoadRequest {
    pub fn path(&self) -> &str {
        match self {
            LoadRequest::Image { path } => path,
            LoadRequest::VideoFrame { path, .. } => path,
        }
    }
}

#[derive(Debug)]
pub struct LoadResponse {
    /// Pixel storage and its actual component type. Storage is intentionally
    /// encoded by this enum rather than repeated in [`DecodedPixelDescription`]
    /// so a loader cannot claim float pixels while returning an RGBA8 image.
    pixels: DecodedPixelBuffer,
    /// Semantics of the pixels after loader/decoder processing. Source-file
    /// CICP/ICC metadata alone is not a substitute: a decoder may already have
    /// expanded YUV matrix/range or applied a color transform.
    decoded: DecodedPixelDescription,
}

impl LoadResponse {
    pub fn new(pixels: DecodedPixelBuffer, decoded: DecodedPixelDescription) -> Self {
        Self { pixels, decoded }
    }

    pub fn rgba8(
        image: Image,
        decoded: DecodedPixelDescription,
    ) -> Result<Self, DecodedPixelBufferError> {
        Ok(Self::new(
            DecodedPixelBuffer::StraightRgba8(DecodedStraightRgba8::new(image)?),
            decoded,
        ))
    }

    /// Explicit compatibility boundary for the versioned native Loader ABI.
    /// ABI v1 can transport only straight, CPU RGBA8 pixels and defines those
    /// pixels as sRGB.
    pub fn abi_v1_srgb_rgba8(image: Image) -> Result<Self, DecodedPixelBufferError> {
        Self::rgba8(image, DecodedPixelDescription::abi_v1_srgb())
    }

    pub fn as_rgba8(&self) -> Option<&Image> {
        self.pixels.as_rgba8()
    }

    pub fn pixels(&self) -> &DecodedPixelBuffer {
        &self.pixels
    }

    pub fn decoded(&self) -> &DecodedPixelDescription {
        &self.decoded
    }

    pub fn into_parts(self) -> (DecodedPixelBuffer, DecodedPixelDescription) {
        (self.pixels, self.decoded)
    }

    /// Compatibility accessor for consumers that have not yet gained a typed
    /// float ingress. It rejects rather than quantizes float loader output.
    pub fn into_rgba8(self) -> Result<Image, LibraryError> {
        self.pixels.into_rgba8().map_err(|pixels| {
            LibraryError::Plugin(format!(
                "consumer requires RGBA8 but loader returned {}",
                pixels.storage_name()
            ))
        })
    }
}

/// Typed pixel payload returned by a loader.
///
/// Every variant explicitly carries straight-alpha pixels. Float variants
/// store one array per RGBA pixel. Their dimensions travel in the same variant
/// as the data, preventing component storage or alpha semantics from becoming
/// separately claimable metadata.
#[derive(Clone, Debug)]
pub enum DecodedPixelBuffer {
    StraightRgba8(DecodedStraightRgba8),
    StraightRgba16F(DecodedStraightRgba16F),
    StraightRgba32F(DecodedStraightRgba32F),
}

impl DecodedPixelBuffer {
    pub fn width(&self) -> u32 {
        match self {
            Self::StraightRgba8(image) => image.width(),
            Self::StraightRgba16F(image) => image.width(),
            Self::StraightRgba32F(image) => image.width(),
        }
    }

    pub fn height(&self) -> u32 {
        match self {
            Self::StraightRgba8(image) => image.height(),
            Self::StraightRgba16F(image) => image.height(),
            Self::StraightRgba32F(image) => image.height(),
        }
    }

    pub fn as_rgba8(&self) -> Option<&Image> {
        match self {
            Self::StraightRgba8(image) => Some(image.image()),
            Self::StraightRgba16F(_) | Self::StraightRgba32F(_) => None,
        }
    }

    pub fn into_rgba8(self) -> Result<Image, Self> {
        match self {
            Self::StraightRgba8(image) => Ok(image.into_image()),
            other @ (Self::StraightRgba16F(_) | Self::StraightRgba32F(_)) => Err(other),
        }
    }

    pub fn storage_name(&self) -> &'static str {
        match self {
            Self::StraightRgba8(_) => "straight RGBA8",
            Self::StraightRgba16F(_) => "straight RGBA16F",
            Self::StraightRgba32F(_) => "straight RGBA32F",
        }
    }

    /// Exact resident bytes owned by this payload. Decode caches use this
    /// instead of assuming four RGBA8 bytes per pixel, so high-precision
    /// frames remain subject to the same hard memory budget.
    pub fn byte_len(&self) -> usize {
        match self {
            Self::StraightRgba8(image) => image.data().len(),
            Self::StraightRgba16F(image) => std::mem::size_of_val(image.data()),
            Self::StraightRgba32F(image) => std::mem::size_of_val(image.data()),
        }
    }

    /// Alpha representation is a property of the typed payload, not loader
    /// metadata that can contradict the bytes.
    pub fn alpha_representation(&self) -> AlphaRepresentation {
        AlphaRepresentation::Straight
    }
}

#[derive(Clone, Debug)]
pub struct DecodedStraightRgba8 {
    image: Image,
}

impl DecodedStraightRgba8 {
    pub fn new(mut image: Image) -> Result<Self, DecodedPixelBufferError> {
        let expected = validate_decoded_pixel_layout(
            image.width,
            image.height,
            DecodedPixelStorage::StraightRgba8,
        )?
        .byte_len();
        if image.data.len() != expected {
            return Err(DecodedPixelBufferError::InvalidRgba8DataLength {
                width: image.width,
                height: image.height,
                expected,
                actual: image.data.len(),
            });
        }
        Image::canonicalize_transparent_rgb(&mut image.data);
        Ok(Self { image })
    }

    pub fn width(&self) -> u32 {
        self.image.width
    }

    pub fn height(&self) -> u32 {
        self.image.height
    }

    pub fn data(&self) -> &[u8] {
        &self.image.data
    }

    pub fn image(&self) -> &Image {
        &self.image
    }

    pub fn into_image(self) -> Image {
        self.image
    }
}

macro_rules! decoded_float_image {
    ($name:ident, $component:ty, $storage:expr) => {
        #[derive(Clone, Debug)]
        pub struct $name {
            width: u32,
            height: u32,
            data: Vec<[$component; 4]>,
        }

        impl $name {
            pub fn new(
                width: u32,
                height: u32,
                mut data: Vec<[$component; 4]>,
            ) -> Result<Self, DecodedPixelBufferError> {
                let expected =
                    validate_decoded_pixel_layout(width, height, $storage)?.pixel_count();
                if data.len() != expected {
                    return Err(DecodedPixelBufferError::InvalidDataLength {
                        width,
                        height,
                        expected,
                        actual: data.len(),
                    });
                }
                validate_straight_float_pixels(&mut data)?;
                Ok(Self {
                    width,
                    height,
                    data,
                })
            }

            pub fn width(&self) -> u32 {
                self.width
            }

            pub fn height(&self) -> u32 {
                self.height
            }

            pub fn data(&self) -> &[[$component; 4]] {
                &self.data
            }

            pub fn into_data(self) -> Vec<[$component; 4]> {
                self.data
            }
        }
    };
}

decoded_float_image!(
    DecodedStraightRgba16F,
    f16,
    DecodedPixelStorage::StraightRgba16F
);
decoded_float_image!(
    DecodedStraightRgba32F,
    f32,
    DecodedPixelStorage::StraightRgba32F
);

trait DecodedFloatComponent: Copy {
    const ZERO: Self;

    fn as_f32(self) -> f32;
}

impl DecodedFloatComponent for f16 {
    const ZERO: Self = f16::ZERO;

    fn as_f32(self) -> f32 {
        self.to_f32()
    }
}

impl DecodedFloatComponent for f32 {
    const ZERO: Self = 0.0;

    fn as_f32(self) -> f32 {
        self
    }
}

fn validate_straight_float_pixels<T: DecodedFloatComponent>(
    pixels: &mut [[T; 4]],
) -> Result<(), DecodedPixelBufferError> {
    for (pixel_index, pixel) in pixels.iter_mut().enumerate() {
        for (component, value) in ["r", "g", "b", "a"].into_iter().zip(pixel.iter()) {
            if !value.as_f32().is_finite() {
                return Err(DecodedPixelBufferError::NonFiniteComponent {
                    pixel_index,
                    component,
                });
            }
        }
        let alpha = pixel[3].as_f32();
        if !(0.0..=1.0).contains(&alpha) {
            return Err(DecodedPixelBufferError::AlphaOutOfRange { pixel_index, alpha });
        }
        if alpha == 0.0 {
            *pixel = [T::ZERO; 4];
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum DecodedPixelBufferError {
    #[error("decoded pixel dimensions must be non-zero, got {width}x{height}")]
    EmptyDimensions { width: u32, height: u32 },
    #[error("decoded pixel dimensions {width}x{height} overflow addressable memory")]
    DimensionOverflow { width: u32, height: u32 },
    #[error("decoded image requires {bytes} bytes; per-image limit is {maximum} bytes")]
    ImageBudgetExceeded { bytes: usize, maximum: usize },
    #[error("decoded RGBA8 buffer for {width}x{height} requires {expected} bytes, got {actual}")]
    InvalidRgba8DataLength {
        width: u32,
        height: u32,
        expected: usize,
        actual: usize,
    },
    #[error(
        "decoded pixel buffer for {width}x{height} requires {expected} RGBA pixels, got {actual}"
    )]
    InvalidDataLength {
        width: u32,
        height: u32,
        expected: usize,
        actual: usize,
    },
    #[error("decoded pixel {pixel_index} has non-finite {component}")]
    NonFiniteComponent {
        pixel_index: usize,
        component: &'static str,
    },
    #[error("decoded pixel {pixel_index} has alpha {alpha}; expected 0 through 1")]
    AlphaOutOfRange { pixel_index: usize, alpha: f32 },
}

impl From<DecodedPixelBufferError> for LibraryError {
    fn from(error: DecodedPixelBufferError) -> Self {
        Self::Plugin(format!(
            "loader returned an invalid decoded pixel buffer: {error}"
        ))
    }
}

/// A loader can either decline a request without error or report a real load
/// failure. This prevents decode errors from being misreported as a missing
/// plugin after the manager tries the next loader.
#[derive(Debug, Error)]
pub enum LoadPluginError {
    #[error("request is not supported by this loader")]
    Unsupported,
    #[error(transparent)]
    Failed(#[from] LibraryError),
}

impl From<DecodedPixelBufferError> for LoadPluginError {
    fn from(error: DecodedPixelBufferError) -> Self {
        Self::Failed(error.into())
    }
}

pub type LoadPluginResult<T> = Result<T, LoadPluginError>;

#[derive(Debug, Clone)]
pub struct AssetMetadata {
    pub kind: AssetKind,
    pub duration: Option<f64>,
    pub fps: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub stream_index: Option<usize>,
    pub frame_count: Option<u64>,
    pub time_base: Option<(i32, i32)>,
    /// Color tags detected from this source stream/codec, or from authoritative
    /// still-image metadata. Empty fields remain unknown and must not be
    /// replaced with guessed defaults by the loader. Loader ABI v1 cannot
    /// expose arbitrary source tags, but [`LoadResponse::decoded`] always
    /// describes the pixels that actually cross the loader boundary.
    pub source_color: SourceColorDescription,
}

pub trait LoadPlugin: Plugin {
    /// Probe a file and return metadata for all available streams.
    ///
    /// Probing must not require a video decoder: audio-only resources and
    /// metadata-only import paths must work independently from frame loading.
    /// Implementations may initialize their state lazily in [`Self::load`].
    /// Returns [`LoadPluginError::Unsupported`] when the plugin does not handle
    /// the file and [`LoadPluginError::Failed`] when it does but opening fails.
    fn open(&self, path: &str) -> LoadPluginResult<Vec<AssetMetadata>>;

    /// Load a frame from a file.
    /// The plugin uses internally cached reader if available.
    /// Unsupported request types must not be encoded as generic plugin errors.
    fn load(&self, request: &LoadRequest, cache: &CacheManager) -> LoadPluginResult<LoadResponse>;

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Load
    }
}

#[derive(Default)]
pub struct LoadRepository {
    pub plugins: HashMap<String, Arc<dyn LoadPlugin>>,
    /// Plugin IDs in priority order (first = highest priority).
    priority_order: Vec<String>,
}

impl LoadRepository {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a loader under an identity resolved before the manager takes
    /// its registry lock. Plugin callbacks must never run under that lock.
    pub fn register(
        &mut self,
        id: String,
        plugin: Arc<dyn LoadPlugin>,
    ) -> Option<Arc<dyn LoadPlugin>> {
        if !self.priority_order.contains(&id) {
            self.priority_order.push(id.clone());
        }
        self.plugins.insert(id, plugin)
    }

    pub fn get(&self, id: &str) -> Option<&Arc<dyn LoadPlugin>> {
        self.plugins.get(id)
    }

    /// Set plugin priority order. IDs not in the list will be appended at the end.
    pub fn set_priority_order(&mut self, order: Vec<String>) {
        // Start with the given order, then append any missing plugins
        let mut new_order = order;
        for id in &self.priority_order {
            if !new_order.contains(id) {
                new_order.push(id.clone());
            }
        }
        self.priority_order = new_order;
    }

    /// Get priority order (for UI display).
    pub fn get_priority_order(&self) -> &[String] {
        &self.priority_order
    }

    /// Iterate plugins in priority order.
    pub fn values_by_priority(&self) -> impl Iterator<Item = &Arc<dyn LoadPlugin>> {
        self.priority_order
            .iter()
            .filter_map(|id| self.plugins.get(id))
    }

    pub fn values(&self) -> impl Iterator<Item = &Arc<dyn LoadPlugin>> {
        self.values_by_priority()
    }

    /// Clones immutable endpoints in dispatch order so callers can release
    /// the manager registry lock before invoking loader code.
    pub fn snapshot(&self) -> Vec<(String, Arc<dyn LoadPlugin>)> {
        self.priority_order
            .iter()
            .filter_map(|id| {
                self.plugins
                    .get(id)
                    .map(|plugin| (id.clone(), Arc::clone(plugin)))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConfigOwnedColorSpace, ConfigOwnedColorSpaceError, DecodedColorSpace, DecodedPixelBuffer,
        DecodedPixelBufferError, DecodedPixelDescription, DecodedPixelStorage,
        DecodedStraightRgba16F, DecodedStraightRgba32F, LoadResponse,
        validate_decoded_pixel_layout,
    };
    use crate::model::frame::Image;
    use crate::model::project::{
        ColorConfigIdentity, ColorManagementConfig, ExportColorConfig, PreviewColorConfig,
    };
    use half::f16;

    #[test]
    fn rgba8_response_rejects_dimension_data_mismatch() {
        let result = LoadResponse::rgba8(
            Image::new(2, 1, vec![0; 4]),
            DecodedPixelDescription::full_range_rgb(DecodedColorSpace::Srgb),
        );
        assert!(matches!(
            result,
            Err(DecodedPixelBufferError::InvalidRgba8DataLength {
                expected: 8,
                actual: 4,
                ..
            })
        ));
    }

    #[test]
    fn rgba8_response_restores_transparent_pixel_canonical_form() {
        let response = LoadResponse::rgba8(
            Image {
                width: 1,
                height: 1,
                data: vec![200, 100, 50, 0],
            },
            DecodedPixelDescription::full_range_rgb(DecodedColorSpace::Srgb),
        );
        assert_eq!(
            response
                .as_ref()
                .ok()
                .and_then(LoadResponse::as_rgba8)
                .map(|image| image.data.as_slice()),
            Some([0, 0, 0, 0].as_slice())
        );
    }

    #[test]
    fn float_buffers_reject_dimension_data_mismatch() {
        assert!(matches!(
            DecodedStraightRgba16F::new(2, 1, vec![[f16::ZERO; 4]]),
            Err(DecodedPixelBufferError::InvalidDataLength {
                expected: 2,
                actual: 1,
                ..
            })
        ));
        assert!(matches!(
            DecodedStraightRgba32F::new(1, 2, vec![[0.0; 4]]),
            Err(DecodedPixelBufferError::InvalidDataLength {
                expected: 2,
                actual: 1,
                ..
            })
        ));
    }

    #[test]
    fn decoded_buffers_reject_empty_dimensions() {
        assert!(matches!(
            DecodedStraightRgba32F::new(0, 1, Vec::new()),
            Err(DecodedPixelBufferError::EmptyDimensions {
                width: 0,
                height: 1
            })
        ));
        assert!(matches!(
            LoadResponse::rgba8(
                Image::new(1, 0, Vec::new()),
                DecodedPixelDescription::full_range_rgb(DecodedColorSpace::Srgb),
            ),
            Err(DecodedPixelBufferError::EmptyDimensions {
                width: 1,
                height: 0
            })
        ));
    }

    #[test]
    fn float_buffers_reject_non_finite_components_and_invalid_alpha() {
        assert!(matches!(
            DecodedStraightRgba32F::new(1, 1, vec![[f32::NAN, 0.0, 0.0, 1.0]]),
            Err(DecodedPixelBufferError::NonFiniteComponent {
                pixel_index: 0,
                component: "r"
            })
        ));
        assert!(matches!(
            DecodedStraightRgba16F::new(
                1,
                1,
                vec![[f16::ZERO, f16::ZERO, f16::ZERO, f16::from_f32(1.5)]],
            ),
            Err(DecodedPixelBufferError::AlphaOutOfRange { pixel_index: 0, .. })
        ));
    }

    #[test]
    fn float_buffers_canonicalize_transparent_rgb() {
        let pixels = DecodedStraightRgba32F::new(1, 1, vec![[2.0, -1.0, 0.5, 0.0]])
            .expect("finite straight RGBA32F should be accepted");

        assert_eq!(pixels.data(), &[[0.0; 4]]);
    }

    #[test]
    fn decoded_buffers_enforce_a_checked_byte_budget() {
        assert!(matches!(
            DecodedStraightRgba32F::new(32_768, 2_048, Vec::new()),
            Err(DecodedPixelBufferError::ImageBudgetExceeded { .. })
        ));
    }

    #[test]
    fn typed_layout_charges_the_final_storage_without_allocating() {
        let boundary =
            validate_decoded_pixel_layout(16_384, 2_048, DecodedPixelStorage::StraightRgba32F)
                .expect("exactly 512 MiB of RGBA32F must fit");
        assert_eq!(boundary.pixel_count(), 33_554_432);
        assert_eq!(boundary.byte_len(), 512 * 1024 * 1024);

        assert!(matches!(
            validate_decoded_pixel_layout(16_384, 2_049, DecodedPixelStorage::StraightRgba32F,),
            Err(DecodedPixelBufferError::ImageBudgetExceeded { .. })
        ));
        assert!(
            validate_decoded_pixel_layout(16_384, 2_049, DecodedPixelStorage::StraightRgba8,)
                .is_ok(),
            "the validator must distinguish RGBA8 from the larger RGBA32F target"
        );
    }

    #[test]
    fn typed_layout_rejects_arithmetic_overflow_without_allocating() {
        assert!(matches!(
            validate_decoded_pixel_layout(u32::MAX, u32::MAX, DecodedPixelStorage::StraightRgba32F,),
            Err(DecodedPixelBufferError::DimensionOverflow { .. })
        ));
    }

    #[test]
    fn legacy_rgba8_accessor_rejects_float_pixels_without_quantizing() {
        let pixels = DecodedStraightRgba16F::new(1, 1, vec![[f16::ZERO; 4]]);
        assert!(pixels.is_ok(), "valid RGBA16F fixture was rejected");
        if let Ok(pixels) = pixels {
            let response = LoadResponse::new(
                DecodedPixelBuffer::StraightRgba16F(pixels),
                DecodedPixelDescription::full_range_rgb(DecodedColorSpace::Srgb),
            );
            let error = response.into_rgba8();
            assert!(
                error
                    .as_ref()
                    .is_err_and(|error| error.to_string().contains("RGBA16F"))
            );
        }
    }

    #[test]
    fn config_owned_color_space_rejects_blank_names() {
        assert_eq!(
            ConfigOwnedColorSpace::new(ColorConfigIdentity::default(), "  "),
            Err(ConfigOwnedColorSpaceError::BlankName)
        );
    }

    #[test]
    fn config_space_ownership_ignores_preview_and_export_settings() {
        let config_identity = ColorConfigIdentity::default();
        let first = ColorManagementConfig::new(
            config_identity.clone(),
            "linear-srgb",
            PreviewColorConfig::direct("srgb"),
            ExportColorConfig::new("srgb"),
        );
        let changed_terminals = ColorManagementConfig::new(
            config_identity.clone(),
            "linear-srgb",
            PreviewColorConfig::direct("display-p3"),
            ExportColorConfig::new("rec2020"),
        );

        let owned = ConfigOwnedColorSpace::new(first.config().clone(), "texture-space")
            .expect("non-blank color-space name should be accepted");

        assert_eq!(owned.config(), changed_terminals.config());
        assert_eq!(owned.config(), &config_identity);
    }

    #[test]
    fn decoded_alpha_is_derived_from_the_typed_pixel_buffer() {
        let response = LoadResponse::rgba8(
            Image::new(1, 1, vec![255, 255, 255, 255]),
            DecodedPixelDescription::full_range_rgb(DecodedColorSpace::Srgb),
        )
        .expect("valid straight RGBA8 response");

        assert_eq!(
            response.pixels().alpha_representation(),
            ruvie_color_management::AlphaRepresentation::Straight
        );
    }
}
