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
//! free to select another scene-linear working space.

mod contract;
mod transform;

pub use contract::{
    AlphaRepresentation, AuthoringScalarPrecision, BackendBuild, BackendCapabilities,
    ColorPipelineContract, ComponentStorage, CpuSamplePrecision, ProcessorCacheKey,
    TARGET_COLOR_PIPELINE,
};
pub use transform::{
    BuiltinColorTransform, ColorManagementError, ColorSpaceInfo, ColorTransformBackend,
    ColorTransformRequest, CpuColorProcessor, GpuColorTransform, GpuLut3d, GpuShaderLanguage,
    LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID, TransformRole,
};
