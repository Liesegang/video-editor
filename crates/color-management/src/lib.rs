//! Project-independent color-management contracts.
//!
//! This crate owns transform semantics, not Project persistence or UI state.
//! The renderer, explicit graph operations, pickers, Preview, and exporters
//! can therefore share one backend without duplicating transfer functions.
//!
//! Project floating-point authoring scalars use f64 at the public boundary.
//! The target image storage is f16 or f32, while the current renderer remains
//! RGBA8. Each CPU backend reports its sample-boundary precision without
//! claiming an internal arithmetic precision. The target working-space
//! identifier is only a profile fallback; a Project or color profile remains
//! free to select another linear working space. A managed image's identity and
//! context preserve the source-domain policy used to enter that space.

mod contract;
mod exact_color_config_file;
mod gpu;
mod gpu_standard;
mod image;
mod legacy_srgb_v1;
#[cfg(feature = "opencolorio")]
mod ocio_backend;
#[cfg(feature = "opencolorio")]
mod ocio_config;
#[cfg(feature = "opencolorio")]
mod ocio_error;
mod processor_boundary;
mod request;
mod standard_hdr;
mod standard_spaces;
mod terminal_pack;
mod transform;
mod verified_space;

pub use contract::{
    AlphaRepresentation, AuthoringScalarPrecision, BackendBuild, BackendCapabilities,
    ColorPipelineContract, ComponentStorage, CpuSamplePrecision, TARGET_COLOR_PIPELINE,
};
pub use exact_color_config_file::{
    ExactColorConfigFile, ExactColorConfigFileError, MAX_EXACT_COLOR_CONFIG_BYTES,
};
pub use gpu::{
    GpuColorTransform, GpuInvalidPixelPolicy, GpuLut3d, GpuShaderLanguage, GpuTerminalChain,
    GpuTransformPixelContract,
};
pub use image::{LinearWorkingImage, LinearWorkingImageError};
pub use legacy_srgb_v1::LegacySrgbV1ColorTransform;
#[cfg(feature = "opencolorio")]
pub use ocio_backend::OcioColorTransformBackend;
#[cfg(feature = "opencolorio")]
pub use ocio_error::OcioBackendError;
pub use processor_boundary::{
    DisplayViewSurfaceProcessor, ManagedLinearWorkingImage, WorkingColorIdentity,
};
pub use request::{
    ColorContext, ColorTransformRequest, ProcessorCacheKey, TransformPurpose, TransformSpec,
};
pub use standard_hdr::{
    PQ_LINEARIZATION_POLICY_CONTEXT_KEY, REFERENCE_WHITE_NITS_CONTEXT_KEY,
    RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY,
};
pub use standard_spaces::{
    BT709_SPACE_ID, DISPLAY_P3_SPACE_ID, LINEAR_BT709_SPACE_ID, LINEAR_DISPLAY_P3_SPACE_ID,
    LINEAR_REC2020_SPACE_ID, LINEAR_SRGB_SPACE_ID, REC2020_SDR_10_SPACE_ID,
    REC2020_SDR_12_SPACE_ID, REC2020_SDR_EXACT_SPACE_ID, REC2100_HLG_SPACE_ID, REC2100_PQ_SPACE_ID,
    SRGB_SPACE_ID, StandardColorSpaceId, StandardColorSpaceMetadata, StandardPrimaries,
    StandardTransfer,
};
pub use transform::{
    BuiltinColorTransform, ColorManagementError, ColorTransformBackend, CompiledTransformIdentity,
    CpuColorProcessor,
};
pub use verified_space::{
    ColorLinearity, ColorReferenceSpace, ColorSpaceInfo, VerifiedSourceSpace, VerifiedWorkingSpace,
};
