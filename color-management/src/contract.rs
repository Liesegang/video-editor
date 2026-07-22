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
    pub cpu_straight_rgba_f64: bool,
    pub gpu_shader_lut: bool,
    pub extended_range_rgb: bool,
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

/// Intended end-to-end rendering contract.
///
/// This is a target contract, not a claim about the current RGBA8 renderer.
/// Source adapters decode tagged straight pixels, render storage is
/// scene-linear and premultiplied, and only a display or output boundary
/// applies a view/encoding transform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorPipelineContract {
    pub authoring_alpha: AlphaRepresentation,
    pub working_space: &'static str,
    pub working_alpha: AlphaRepresentation,
    pub preferred_storage: ComponentStorage,
    pub high_precision_storage: ComponentStorage,
    pub default_display_space: &'static str,
}

pub const TARGET_COLOR_PIPELINE: ColorPipelineContract = ColorPipelineContract {
    authoring_alpha: AlphaRepresentation::Straight,
    working_space: LINEAR_SRGB_SPACE_ID,
    working_alpha: AlphaRepresentation::Premultiplied,
    preferred_storage: ComponentStorage::Float16,
    high_precision_storage: ComponentStorage::Float32,
    default_display_space: SRGB_SPACE_ID,
};

#[cfg(test)]
mod tests {
    use super::{AlphaRepresentation, ComponentStorage, TARGET_COLOR_PIPELINE};
    use crate::{LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID};

    #[test]
    fn target_pipeline_has_one_explicit_linear_working_boundary() {
        assert_eq!(TARGET_COLOR_PIPELINE.working_space, LINEAR_SRGB_SPACE_ID);
        assert_eq!(
            TARGET_COLOR_PIPELINE.working_alpha,
            AlphaRepresentation::Premultiplied
        );
        assert_eq!(
            TARGET_COLOR_PIPELINE.preferred_storage,
            ComponentStorage::Float16
        );
        assert_eq!(TARGET_COLOR_PIPELINE.default_display_space, SRGB_SPACE_ID);
    }
}
