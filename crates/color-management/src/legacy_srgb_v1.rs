//! Frozen compatibility backend for RuViE's former bundled sRGB-only config.
//!
//! This catalog is deliberately independent from the current built-in
//! standard-space backend. A Project that persisted the v1 config identity
//! must never gain new color-space meanings merely because a newer build
//! opened it.

use crate::{
    BackendBuild, BackendCapabilities, ColorManagementError, ColorSpaceInfo, ColorTransformBackend,
    ColorTransformRequest, CompiledTransformIdentity, CpuColorProcessor, CpuSamplePrecision,
    GpuColorTransform, GpuShaderLanguage, ProcessorCacheKey, StandardColorSpaceId, TransformSpec,
    standard_spaces::CompiledStandardTransform, transform::sealed,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct LegacySrgbV1ColorTransform;

impl sealed::Backend for LegacySrgbV1ColorTransform {}

impl LegacySrgbV1ColorTransform {
    fn validate_request(
        &self,
        request: &ColorTransformRequest,
    ) -> Result<CompiledStandardTransform, ColorManagementError> {
        match request.spec() {
            TransformSpec::ColorSpace {
                source,
                destination,
            } if is_v1_space(source) && is_v1_space(destination) => {
                CompiledStandardTransform::compile(request)
            }
            TransformSpec::ColorSpace {
                source,
                destination,
            } => Err(ColorManagementError::UnsupportedTransform {
                source: source.clone(),
                target: destination.clone(),
            }),
            TransformSpec::DisplayView { display, view, .. } => {
                Err(ColorManagementError::UnsupportedDisplayView {
                    backend_id: self.backend_id().to_string(),
                    display: display.clone(),
                    view: view.clone(),
                })
            }
        }
    }
}

impl ColorTransformBackend for LegacySrgbV1ColorTransform {
    fn backend_id(&self) -> &'static str {
        "builtin.extended-srgb"
    }

    fn build(&self) -> BackendBuild {
        BackendBuild::Real
    }

    fn config_fingerprint(&self) -> String {
        "builtin-extended-srgb-v1".to_string()
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            enumerate_color_spaces: true,
            cpu_processor_sample_precision: Some(CpuSamplePrecision::Float64),
            gpu_shader_lut: false,
            extended_range_rgb: true,
        }
    }

    fn available_color_spaces(&self) -> Result<Vec<ColorSpaceInfo>, ColorManagementError> {
        Ok(
            [StandardColorSpaceId::Srgb, StandardColorSpaceId::LinearSrgb]
                .into_iter()
                .map(|space| space.metadata().color_space_info())
                .collect(),
        )
    }

    fn processor_cache_key(
        &self,
        request: &ColorTransformRequest,
    ) -> Result<ProcessorCacheKey, ColorManagementError> {
        let _ = self.validate_request(request)?;
        Ok(ProcessorCacheKey {
            backend_id: self.backend_id().to_string(),
            config_fingerprint: self.config_fingerprint(),
            purpose: request.purpose(),
            spec: request.spec().clone(),
            context: request.context().clone(),
        })
    }

    fn create_cpu_processor(
        &self,
        request: &ColorTransformRequest,
    ) -> Result<Box<dyn CpuColorProcessor>, ColorManagementError> {
        let transform = self.validate_request(request)?;
        let program_id = transform.program_id();
        Ok(Box::new(LegacySrgbV1CpuProcessor {
            transform,
            identity: CompiledTransformIdentity::new(
                self.build(),
                self.processor_cache_key(request)?,
                program_id,
            )?,
        }))
    }

    fn extract_gpu_transform(
        &self,
        request: &ColorTransformRequest,
        _language: GpuShaderLanguage,
    ) -> Result<GpuColorTransform, ColorManagementError> {
        let _ = self.validate_request(request)?;
        Err(ColorManagementError::GpuTransformUnavailable {
            backend_id: self.backend_id().to_string(),
        })
    }
}

struct LegacySrgbV1CpuProcessor {
    transform: CompiledStandardTransform,
    identity: CompiledTransformIdentity,
}

impl sealed::Processor for LegacySrgbV1CpuProcessor {}

impl CpuColorProcessor for LegacySrgbV1CpuProcessor {
    fn compiled_transform_identity(&self) -> &CompiledTransformIdentity {
        &self.identity
    }

    fn transform_rgb(&self, rgb: [f64; 3]) -> Result<[f64; 3], ColorManagementError> {
        validate_finite(rgb)?;
        let transformed = self.transform.apply(rgb)?;
        validate_finite(transformed)?;
        Ok(transformed)
    }
}

fn is_v1_space(space: &str) -> bool {
    matches!(
        StandardColorSpaceId::from_id(space),
        Some(StandardColorSpaceId::Srgb | StandardColorSpaceId::LinearSrgb)
    )
}

fn validate_finite(rgb: [f64; 3]) -> Result<(), ColorManagementError> {
    for (component, value) in ["r", "g", "b"].into_iter().zip(rgb) {
        if !value.is_finite() {
            return Err(ColorManagementError::NonFiniteComponent { component });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID};

    #[test]
    fn keeps_exact_two_space_catalog_identity_and_transform_scope() {
        let backend = LegacySrgbV1ColorTransform;
        assert_eq!(backend.backend_id(), "builtin.extended-srgb");
        assert_eq!(backend.config_fingerprint(), "builtin-extended-srgb-v1");
        assert_eq!(
            backend
                .available_color_spaces()
                .unwrap()
                .into_iter()
                .map(|space| space.id)
                .collect::<Vec<_>>(),
            [SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID]
        );

        let supported =
            ColorTransformRequest::source_to_working(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        assert!(backend.create_cpu_processor(&supported).is_ok());

        let newer_v3_space = StandardColorSpaceId::DisplayP3.as_str();
        assert!(matches!(
            backend.resolve_color_space(newer_v3_space),
            Err(ColorManagementError::ColorSpaceUnavailable { .. })
        ));
        assert!(matches!(
            backend.create_cpu_processor(&ColorTransformRequest::source_to_working(
                newer_v3_space,
                LINEAR_SRGB_SPACE_ID,
            )),
            Err(ColorManagementError::UnsupportedTransform { .. })
        ));
    }
}
