//! Project-independent color-management contracts.
//!
//! This crate owns transform semantics, not Project persistence or UI state.
//! The renderer, explicit graph operations, pickers, Preview, and exporters
//! can therefore share one backend without duplicating transfer functions.

mod contract;
mod transform;

pub use contract::{
    AlphaRepresentation, BackendBuild, BackendCapabilities, ColorPipelineContract,
    ComponentStorage, ProcessorCacheKey, TARGET_COLOR_PIPELINE,
};
pub use transform::{
    BuiltinColorTransform, ColorManagementError, ColorSpaceInfo, ColorTransformBackend,
    ColorTransformRequest, CpuColorProcessor, GpuColorTransform, GpuLut3d, GpuShaderLanguage,
    LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID, TransformRole,
};
