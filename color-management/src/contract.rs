use crate::transform::{LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID};

/// Whether a backend contains a real implementation or a build-time stub.
///
/// Hosts must reject [`BackendBuild::Stub`] before accepting Project work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendBuild {
    Real,
    Stub,
}

/// Operations a backend can honestly provide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendCapabilities {
    pub enumerate_color_spaces: bool,
    /// `None` means that no CPU processor is available. The precision describes
    /// the processor's internal math, not the f64 authoring values at its API
    /// boundary.
    pub cpu_processor_compute_precision: Option<CpuComputePrecision>,
    pub gpu_shader_lut: bool,
    pub extended_range_rgb: bool,
}

/// Internal arithmetic precision used by a CPU color processor.
///
/// This is deliberately independent of Project/authoring scalar precision and
/// image-buffer component storage. For example, an OpenColorIO processor may
/// accept f64 authoring values at this crate's boundary while computing in f32.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuComputePrecision {
    Float32,
    Float64,
}

/// Stable identity for reusing a CPU processor.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProcessorCacheKey {
    pub backend_id: String,
    pub config_fingerprint: String,
    pub source_space: String,
    pub target_space: String,
    pub role: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlphaRepresentation {
    Straight,
    Premultiplied,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentStorage {
    Float16,
    Float32,
}

/// Scalar precision retained by Project data and authoring APIs.
///
/// This does not prescribe image-buffer storage or backend arithmetic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthoringScalarPrecision {
    Float32,
    Float64,
}

/// Intended end-to-end rendering contract.
///
/// This is a target contract, not a claim about the current RGBA8 renderer.
/// Source adapters decode tagged straight pixels, render storage is
/// scene-linear and premultiplied, and only a display or output boundary
/// applies a view/encoding transform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorPipelineContract {
    pub authoring_alpha: AlphaRepresentation,
    pub authoring_scalar_precision: AuthoringScalarPrecision,
    /// Fallback for a Project or color profile that does not choose a working
    /// space. It is not a process-wide fixed working space.
    pub default_working_space: &'static str,
    pub working_alpha: AlphaRepresentation,
    pub preferred_image_storage: ComponentStorage,
    pub high_precision_image_storage: ComponentStorage,
    pub default_display_space: &'static str,
}

pub const TARGET_COLOR_PIPELINE: ColorPipelineContract = ColorPipelineContract {
    authoring_alpha: AlphaRepresentation::Straight,
    authoring_scalar_precision: AuthoringScalarPrecision::Float64,
    default_working_space: LINEAR_SRGB_SPACE_ID,
    working_alpha: AlphaRepresentation::Premultiplied,
    preferred_image_storage: ComponentStorage::Float16,
    high_precision_image_storage: ComponentStorage::Float32,
    default_display_space: SRGB_SPACE_ID,
};

#[cfg(test)]
mod tests {
    use super::{
        AlphaRepresentation, AuthoringScalarPrecision, BackendCapabilities, ComponentStorage,
        CpuComputePrecision, TARGET_COLOR_PIPELINE,
    };
    use crate::{LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID};

    #[test]
    fn target_pipeline_keeps_precision_domains_and_working_default_explicit() {
        assert_eq!(
            TARGET_COLOR_PIPELINE.authoring_scalar_precision,
            AuthoringScalarPrecision::Float64
        );
        assert_eq!(
            TARGET_COLOR_PIPELINE.default_working_space,
            LINEAR_SRGB_SPACE_ID
        );
        assert_eq!(
            TARGET_COLOR_PIPELINE.working_alpha,
            AlphaRepresentation::Premultiplied
        );
        assert_eq!(
            TARGET_COLOR_PIPELINE.preferred_image_storage,
            ComponentStorage::Float16
        );
        assert_eq!(
            TARGET_COLOR_PIPELINE.high_precision_image_storage,
            ComponentStorage::Float32
        );
        assert_eq!(TARGET_COLOR_PIPELINE.default_display_space, SRGB_SPACE_ID);
    }

    #[test]
    fn f64_authoring_does_not_imply_f64_backend_compute() {
        let capabilities = BackendCapabilities {
            enumerate_color_spaces: true,
            cpu_processor_compute_precision: Some(CpuComputePrecision::Float32),
            gpu_shader_lut: false,
            extended_range_rgb: true,
        };

        assert_eq!(
            TARGET_COLOR_PIPELINE.authoring_scalar_precision,
            AuthoringScalarPrecision::Float64
        );
        assert_eq!(
            capabilities.cpu_processor_compute_precision,
            Some(CpuComputePrecision::Float32)
        );
    }
}
