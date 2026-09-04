use std::fmt;

use crate::{
    ColorLinearity, ColorReferenceSpace, ColorSpaceInfo, VerifiedSourceSpace, VerifiedWorkingSpace,
    contract::{BackendBuild, BackendCapabilities, CpuSamplePrecision},
    request::{ColorContext, ColorTransformRequest, ProcessorCacheKey, TransformSpec},
    standard_spaces::{CompiledStandardTransform, StandardColorSpaceId},
};

pub use crate::standard_spaces::{LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID};

/// Stable identity of the immutable program compiled for a CPU processor.
///
/// Processor instances may eventually carry mutable dynamic-property state;
/// this identity deliberately describes only the immutable request and
/// backend program that can safely participate in caches and validation.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CompiledTransformIdentity {
    backend_build: BackendBuild,
    cache_key: ProcessorCacheKey,
    backend_program_cache_id: String,
}

impl CompiledTransformIdentity {
    pub(crate) fn new(
        backend_build: BackendBuild,
        cache_key: ProcessorCacheKey,
        backend_program_cache_id: impl Into<String>,
    ) -> Result<Self, ColorManagementError> {
        if backend_build == BackendBuild::Stub {
            return Err(ColorManagementError::StubBackend {
                backend_id: cache_key.backend_id,
            });
        }
        let backend_program_cache_id = backend_program_cache_id.into();
        if backend_program_cache_id.trim().is_empty() {
            return Err(ColorManagementError::EmptyProcessorProgramIdentity);
        }
        Ok(Self {
            backend_build,
            cache_key,
            backend_program_cache_id,
        })
    }

    pub const fn backend_build(&self) -> BackendBuild {
        self.backend_build
    }

    pub const fn cache_key(&self) -> &ProcessorCacheKey {
        &self.cache_key
    }

    pub fn backend_program_cache_id(&self) -> &str {
        &self.backend_program_cache_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GpuShaderLanguage {
    SkSl,
    Glsl,
    Wgsl,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpuLut3d {
    pub edge_length: u32,
    pub rgba_f32: Vec<[f32; 4]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpuColorTransform {
    pub language: GpuShaderLanguage,
    pub source: String,
    pub entry_point: String,
    pub luts: Vec<GpuLut3d>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColorManagementError {
    StubBackend {
        backend_id: String,
    },
    EmptyColorSpace,
    EmptyProjectConfigIdentity,
    EmptyProcessorProgramIdentity,
    ColorSpaceUnavailable {
        space: String,
    },
    InvalidWorkingSpace {
        space: String,
        reason: &'static str,
    },
    InvalidSourceSpace {
        space: String,
        reason: &'static str,
    },
    MissingContextVariable {
        variable: &'static str,
        space: String,
    },
    InvalidContextVariable {
        variable: &'static str,
        value: String,
        reason: &'static str,
    },
    InvalidTransferDomain {
        transfer: &'static str,
        reason: &'static str,
    },
    ProcessorContractMismatch {
        operation: &'static str,
        detail: String,
    },
    NonFiniteComponent {
        component: &'static str,
    },
    AlphaOutOfRange,
    UnsupportedTransform {
        source: String,
        target: String,
    },
    UnsupportedDisplayView {
        backend_id: String,
        display: String,
        view: String,
    },
    GpuTransformUnavailable {
        backend_id: String,
    },
}

impl fmt::Display for ColorManagementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StubBackend { backend_id } => {
                write!(formatter, "color backend '{backend_id}' is a stub build")
            }
            Self::EmptyColorSpace => formatter.write_str("color space must not be empty"),
            Self::EmptyProjectConfigIdentity => {
                formatter.write_str("Project color configuration identity must not be empty")
            }
            Self::EmptyProcessorProgramIdentity => {
                formatter.write_str("compiled color processor program identity must not be empty")
            }
            Self::ColorSpaceUnavailable { space } => {
                write!(
                    formatter,
                    "color space '{space}' is not available in the backend"
                )
            }
            Self::InvalidWorkingSpace { space, reason } => {
                write!(
                    formatter,
                    "color space '{space}' is not a valid working space: {reason}"
                )
            }
            Self::InvalidSourceSpace { space, reason } => {
                write!(
                    formatter,
                    "color space '{space}' is not a valid image source: {reason}"
                )
            }
            Self::MissingContextVariable { variable, space } => {
                write!(
                    formatter,
                    "color space '{space}' requires context variable '{variable}'"
                )
            }
            Self::InvalidContextVariable {
                variable,
                value,
                reason,
            } => {
                write!(
                    formatter,
                    "color context variable '{variable}' has invalid value '{value}': {reason}"
                )
            }
            Self::InvalidTransferDomain { transfer, reason } => {
                write!(
                    formatter,
                    "value is outside the {transfer} transfer domain: {reason}"
                )
            }
            Self::ProcessorContractMismatch { operation, detail } => {
                write!(
                    formatter,
                    "color processor cannot perform {operation}: {detail}"
                )
            }
            Self::NonFiniteComponent { component } => {
                write!(formatter, "color component {component} must be finite")
            }
            Self::AlphaOutOfRange => {
                formatter.write_str("straight alpha must be between 0 and 1 inclusive")
            }
            Self::UnsupportedTransform { source, target } => {
                write!(
                    formatter,
                    "unsupported color transform '{source}' -> '{target}'"
                )
            }
            Self::UnsupportedDisplayView {
                backend_id,
                display,
                view,
            } => {
                write!(
                    formatter,
                    "color backend '{backend_id}' does not support display/view '{display}/{view}'"
                )
            }
            Self::GpuTransformUnavailable { backend_id } => {
                write!(
                    formatter,
                    "color backend '{backend_id}' has no GPU transform"
                )
            }
        }
    }
}

impl std::error::Error for ColorManagementError {}

/// Private implementation boundary for trusted in-crate color backends.
///
/// The public traits remain usable as trait objects, but cannot be implemented
/// by downstream code that could otherwise self-report `BackendBuild::Real`
/// and forge processor or verified-space authority.
pub(crate) mod sealed {
    pub trait Backend {}
    pub trait Processor {}
}

pub trait CpuColorProcessor: sealed::Processor + Send + Sync {
    /// Immutable request/program identity compiled by the backend.
    fn compiled_transform_identity(&self) -> &CompiledTransformIdentity;

    /// Transform one RGB authoring color without clipping.
    ///
    /// f64 is the canonical Project/API interchange representation. A backend
    /// may quantize RGB at the sample boundary reported by
    /// [`BackendCapabilities::cpu_processor_sample_precision`]; that capability
    /// does not claim the backend's internal arithmetic precision.
    fn transform_rgb(&self, rgb: [f64; 3]) -> Result<[f64; 3], ColorManagementError>;

    /// Transform a packed straight-alpha f32 image in place.
    ///
    /// Backends with a packed image API (including OpenColorIO) should
    /// override this method. The default keeps small/custom processors source
    /// compatible while avoiding a second image-transform abstraction.
    fn transform_rgb_f32_in_place(
        &self,
        pixels: &mut [[f32; 3]],
    ) -> Result<(), ColorManagementError> {
        for pixel in pixels {
            let transformed = self.transform_rgb([
                f64::from(pixel[0]),
                f64::from(pixel[1]),
                f64::from(pixel[2]),
            ])?;
            let transformed = transformed.map(|component| component as f32);
            validate_rgb_f32(transformed)?;
            *pixel = transformed;
        }
        Ok(())
    }
}

/// Replaceable backend boundary for built-in transfers and a future pinned
/// OpenColorIO implementation.
pub trait ColorTransformBackend: sealed::Backend + Send + Sync {
    fn backend_id(&self) -> &'static str;
    fn build(&self) -> BackendBuild;
    fn config_fingerprint(&self) -> String;
    fn capabilities(&self) -> BackendCapabilities;
    fn available_color_spaces(&self) -> Result<Vec<ColorSpaceInfo>, ColorManagementError>;

    /// Resolve a source color space in this exact backend configuration and
    /// context. Data spaces cannot enter the managed color pipeline.
    fn verify_source_space(
        &self,
        space: &str,
        context: &ColorContext,
    ) -> Result<VerifiedSourceSpace, ColorManagementError> {
        let color_space = self.resolve_color_space(space)?;
        self.validate_color_space_context(&color_space, context)?;
        if color_space.is_data {
            return Err(ColorManagementError::InvalidSourceSpace {
                space: color_space.id,
                reason: "data spaces cannot enter the managed color pipeline",
            });
        }
        Ok(VerifiedSourceSpace::new(
            self.backend_id().to_string(),
            self.build(),
            self.config_fingerprint(),
            context.clone(),
            color_space,
        ))
    }

    /// Verify that `space` is scene-referred, linear, and non-data in this
    /// backend configuration. The returned opaque token also captures the
    /// context that processors must use at working-image boundaries.
    fn verify_working_space(
        &self,
        space: &str,
        context: &ColorContext,
    ) -> Result<VerifiedWorkingSpace, ColorManagementError> {
        let color_space = self.resolve_color_space(space)?;
        self.validate_color_space_context(&color_space, context)?;
        let reason = match (
            color_space.reference_space,
            color_space.linearity,
            color_space.is_data,
        ) {
            (_, _, true) => Some("data spaces cannot be used for image compositing"),
            (ColorReferenceSpace::Display, _, false) => {
                Some("the space is display-referred, not scene-referred")
            }
            (ColorReferenceSpace::Scene, ColorLinearity::Encoded, false) => {
                Some("the scene-referred space is not linear")
            }
            (ColorReferenceSpace::Scene, ColorLinearity::Linear, false) => None,
        };
        if let Some(reason) = reason {
            return Err(ColorManagementError::InvalidWorkingSpace {
                space: color_space.id,
                reason,
            });
        }
        Ok(VerifiedWorkingSpace::new(
            self.backend_id().to_string(),
            self.build(),
            self.config_fingerprint(),
            context.clone(),
            color_space,
        ))
    }
    fn processor_cache_key(
        &self,
        request: &ColorTransformRequest,
    ) -> Result<ProcessorCacheKey, ColorManagementError>;
    fn create_cpu_processor(
        &self,
        request: &ColorTransformRequest,
    ) -> Result<Box<dyn CpuColorProcessor>, ColorManagementError>;
    fn extract_gpu_transform(
        &self,
        request: &ColorTransformRequest,
        language: GpuShaderLanguage,
    ) -> Result<GpuColorTransform, ColorManagementError>;

    /// Resolve the exact config-local output color-space name selected by a
    /// display/view pair. Backends without named display/view semantics must
    /// reject instead of inferring a surface encoding from display labels.
    fn display_view_output_space(
        &self,
        display: &str,
        view: &str,
        _context: &ColorContext,
    ) -> Result<String, ColorManagementError> {
        Err(ColorManagementError::UnsupportedDisplayView {
            backend_id: self.backend_id().to_string(),
            display: display.to_string(),
            view: view.to_string(),
        })
    }

    fn validate_color_space_context(
        &self,
        _space: &ColorSpaceInfo,
        _context: &ColorContext,
    ) -> Result<(), ColorManagementError> {
        Ok(())
    }

    fn resolve_color_space(&self, space: &str) -> Result<ColorSpaceInfo, ColorManagementError> {
        ensure_real_backend_identity(self.backend_id(), self.build())?;
        if space.trim().is_empty() {
            return Err(ColorManagementError::EmptyColorSpace);
        }
        self.available_color_spaces()?
            .into_iter()
            .find(|candidate| candidate.id == space)
            .ok_or_else(|| ColorManagementError::ColorSpaceUnavailable {
                space: space.to_string(),
            })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BuiltinColorTransform;

impl sealed::Backend for BuiltinColorTransform {}

impl BuiltinColorTransform {
    pub fn transform_rgba(
        &self,
        rgba: [f64; 4],
        source_space: &str,
        target_space: &str,
    ) -> Result<[f64; 4], ColorManagementError> {
        validate_rgb([rgba[0], rgba[1], rgba[2]])?;
        if !rgba[3].is_finite() {
            return Err(ColorManagementError::NonFiniteComponent { component: "a" });
        }
        if !(0.0..=1.0).contains(&rgba[3]) {
            return Err(ColorManagementError::AlphaOutOfRange);
        }
        let request = ColorTransformRequest::explicit(source_space, target_space);
        self.create_cpu_processor(&request)?
            .transform_rgb([rgba[0], rgba[1], rgba[2]])
            .map(|rgb| [rgb[0], rgb[1], rgb[2], rgba[3]])
    }

    fn validate_request(
        &self,
        request: &ColorTransformRequest,
    ) -> Result<CompiledStandardTransform, ColorManagementError> {
        ensure_real_backend(self)?;
        match request.spec() {
            TransformSpec::ColorSpace { .. } => CompiledStandardTransform::compile(request),
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

impl ColorTransformBackend for BuiltinColorTransform {
    fn backend_id(&self) -> &'static str {
        "builtin.standard-spaces.v3"
    }

    fn build(&self) -> BackendBuild {
        BackendBuild::Real
    }

    fn config_fingerprint(&self) -> String {
        "builtin.standard-spaces.config.v3".to_string()
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
        ensure_real_backend(self)?;
        Ok(StandardColorSpaceId::ALL
            .into_iter()
            .map(|space| space.metadata().color_space_info())
            .collect())
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
        Ok(Box::new(BuiltinCpuProcessor {
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

    fn validate_color_space_context(
        &self,
        space: &ColorSpaceInfo,
        context: &ColorContext,
    ) -> Result<(), ColorManagementError> {
        let standard_space = StandardColorSpaceId::from_id(&space.id).ok_or_else(|| {
            ColorManagementError::ColorSpaceUnavailable {
                space: space.id.clone(),
            }
        })?;
        crate::standard_hdr::validate_standard_space_context(standard_space.metadata(), context)
    }
}

fn ensure_real_backend(backend: &dyn ColorTransformBackend) -> Result<(), ColorManagementError> {
    ensure_real_backend_identity(backend.backend_id(), backend.build())
}

fn ensure_real_backend_identity(
    backend_id: &str,
    build: BackendBuild,
) -> Result<(), ColorManagementError> {
    if build == BackendBuild::Stub {
        return Err(ColorManagementError::StubBackend {
            backend_id: backend_id.to_string(),
        });
    }
    Ok(())
}

struct BuiltinCpuProcessor {
    transform: CompiledStandardTransform,
    identity: CompiledTransformIdentity,
}

impl sealed::Processor for BuiltinCpuProcessor {}

impl CpuColorProcessor for BuiltinCpuProcessor {
    fn compiled_transform_identity(&self) -> &CompiledTransformIdentity {
        &self.identity
    }

    fn transform_rgb(&self, rgb: [f64; 3]) -> Result<[f64; 3], ColorManagementError> {
        validate_rgb(rgb)?;
        let transformed = self.transform.apply(rgb)?;
        validate_rgb(transformed)?;
        Ok(transformed)
    }

    fn transform_rgb_f32_in_place(
        &self,
        pixels: &mut [[f32; 3]],
    ) -> Result<(), ColorManagementError> {
        for pixel in pixels {
            validate_rgb_f32(*pixel)?;
            let transformed = self
                .transform
                .apply(pixel.map(f64::from))?
                .map(|component| component as f32);
            *pixel = transformed;
            validate_rgb_f32(*pixel)?;
        }
        Ok(())
    }
}

fn validate_rgb(rgb: [f64; 3]) -> Result<(), ColorManagementError> {
    for (component, value) in ["r", "g", "b"].into_iter().zip(rgb) {
        if !value.is_finite() {
            return Err(ColorManagementError::NonFiniteComponent { component });
        }
    }
    Ok(())
}

fn validate_rgb_f32(rgb: [f32; 3]) -> Result<(), ColorManagementError> {
    for (component, value) in ["r", "g", "b"].into_iter().zip(rgb) {
        if !value.is_finite() {
            return Err(ColorManagementError::NonFiniteComponent { component });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        BuiltinColorTransform, ColorLinearity, ColorManagementError, ColorReferenceSpace,
        ColorTransformBackend, ColorTransformRequest, CompiledTransformIdentity, GpuShaderLanguage,
        LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID,
    };
    use crate::{
        BackendBuild, ColorContext, CpuSamplePrecision, PQ_LINEARIZATION_POLICY_CONTEXT_KEY,
        REC2100_HLG_SPACE_ID, REC2100_PQ_SPACE_ID, REFERENCE_WHITE_NITS_CONTEXT_KEY,
        RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY, StandardColorSpaceId, TransformPurpose,
        TransformSpec,
    };

    struct DataOnlyBackend;

    impl super::sealed::Backend for DataOnlyBackend {}

    impl ColorTransformBackend for DataOnlyBackend {
        fn backend_id(&self) -> &'static str {
            "test.data-only"
        }

        fn build(&self) -> BackendBuild {
            BackendBuild::Real
        }

        fn config_fingerprint(&self) -> String {
            "test-data-config-v1".to_string()
        }

        fn capabilities(&self) -> crate::BackendCapabilities {
            crate::BackendCapabilities {
                enumerate_color_spaces: true,
                cpu_processor_sample_precision: None,
                gpu_shader_lut: false,
                extended_range_rgb: false,
            }
        }

        fn available_color_spaces(
            &self,
        ) -> Result<Vec<crate::ColorSpaceInfo>, ColorManagementError> {
            Ok(vec![crate::ColorSpaceInfo {
                id: "raw".to_string(),
                label: "Raw data".to_string(),
                reference_space: ColorReferenceSpace::Scene,
                linearity: ColorLinearity::Linear,
                is_data: true,
            }])
        }

        fn processor_cache_key(
            &self,
            _request: &ColorTransformRequest,
        ) -> Result<crate::ProcessorCacheKey, ColorManagementError> {
            data_backend_has_no_processor()
        }

        fn create_cpu_processor(
            &self,
            _request: &ColorTransformRequest,
        ) -> Result<Box<dyn crate::CpuColorProcessor>, ColorManagementError> {
            data_backend_has_no_processor()
        }

        fn extract_gpu_transform(
            &self,
            _request: &ColorTransformRequest,
            _language: GpuShaderLanguage,
        ) -> Result<crate::GpuColorTransform, ColorManagementError> {
            data_backend_has_no_processor()
        }
    }

    fn data_backend_has_no_processor<T>() -> Result<T, ColorManagementError> {
        Err(ColorManagementError::UnsupportedTransform {
            source: "raw".to_string(),
            target: "raw".to_string(),
        })
    }

    fn assert_near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= 1.0e-12,
            "actual {actual}, expected {expected}"
        );
    }

    #[test]
    fn builtin_advertises_only_implemented_capabilities() {
        let backend = BuiltinColorTransform;
        assert_eq!(backend.build(), BackendBuild::Real);
        assert_eq!(backend.backend_id(), "builtin.standard-spaces.v3");
        assert_eq!(
            backend.config_fingerprint(),
            "builtin.standard-spaces.config.v3"
        );
        let capabilities = backend.capabilities();
        assert!(capabilities.enumerate_color_spaces);
        assert_eq!(
            capabilities.cpu_processor_sample_precision,
            Some(CpuSamplePrecision::Float64)
        );
        assert!(!capabilities.gpu_shader_lut);
        assert!(capabilities.extended_range_rgb);
        let spaces = backend.available_color_spaces().unwrap();
        assert_eq!(spaces.len(), StandardColorSpaceId::ALL.len());
        assert!(spaces.iter().any(|space| {
            space.id == LINEAR_SRGB_SPACE_ID
                && space.reference_space == ColorReferenceSpace::Scene
                && space.linearity == ColorLinearity::Linear
                && !space.is_data
                && space.is_valid_working_space()
        }));
    }

    #[test]
    fn only_scene_linear_non_data_space_receives_a_verified_working_token() {
        let backend = BuiltinColorTransform;
        let context = ColorContext::default().with_variable("SHOT", "010");
        let verified = backend
            .verify_working_space(LINEAR_SRGB_SPACE_ID, &context)
            .unwrap();
        assert_eq!(verified.backend_id(), backend.backend_id());
        assert_eq!(verified.backend_build(), BackendBuild::Real);
        assert_eq!(verified.context(), &context);
        assert_eq!(verified.color_space_id(), LINEAR_SRGB_SPACE_ID);

        assert!(matches!(
            backend.verify_working_space(SRGB_SPACE_ID, &ColorContext::default()),
            Err(ColorManagementError::InvalidWorkingSpace { .. })
        ));
        assert!(matches!(
            backend.verify_working_space("missing", &ColorContext::default()),
            Err(ColorManagementError::ColorSpaceUnavailable { .. })
        ));
    }

    #[test]
    fn source_token_captures_exact_backend_config_context_and_space() {
        let backend = BuiltinColorTransform;
        let context = ColorContext::default().with_variable("SHOT", "010");
        let verified = backend
            .verify_source_space(SRGB_SPACE_ID, &context)
            .unwrap();

        assert_eq!(verified.backend_id(), backend.backend_id());
        assert_eq!(verified.backend_build(), BackendBuild::Real);
        assert_eq!(
            verified.backend_config_fingerprint(),
            backend.config_fingerprint()
        );
        assert_eq!(verified.context(), &context);
        assert_eq!(verified.color_space_id(), SRGB_SPACE_ID);
    }

    #[test]
    fn hdr_source_tokens_and_processors_fail_closed_without_explicit_context() {
        let backend = BuiltinColorTransform;
        assert!(matches!(
            backend.verify_source_space(REC2100_PQ_SPACE_ID, &ColorContext::default()),
            Err(ColorManagementError::MissingContextVariable {
                variable: PQ_LINEARIZATION_POLICY_CONTEXT_KEY,
                ..
            })
        ));

        let missing_reference = ColorContext::default().with_variable(
            PQ_LINEARIZATION_POLICY_CONTEXT_KEY,
            RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY,
        );
        assert!(matches!(
            backend.verify_source_space(REC2100_PQ_SPACE_ID, &missing_reference),
            Err(ColorManagementError::MissingContextVariable {
                variable: REFERENCE_WHITE_NITS_CONTEXT_KEY,
                ..
            })
        ));

        assert!(
            backend
                .verify_source_space(REC2100_HLG_SPACE_ID, &ColorContext::default())
                .is_ok()
        );
    }

    #[test]
    fn data_space_cannot_receive_a_managed_source_token() {
        assert!(matches!(
            DataOnlyBackend.verify_source_space("raw", &ColorContext::default()),
            Err(ColorManagementError::InvalidSourceSpace { .. })
        ));
    }

    #[test]
    fn standard_srgb_breakpoints_and_alpha_are_exactly_respected() {
        let backend = BuiltinColorTransform;
        let output = backend
            .transform_rgba(
                [0.040_45, 0.5, 1.0, 0.375],
                SRGB_SPACE_ID,
                LINEAR_SRGB_SPACE_ID,
            )
            .unwrap();
        assert_near(output[0], 0.040_45 / 12.92);
        assert_near(output[1], 0.214_041_140_482_232_55);
        assert_near(output[2], 1.0);
        assert_eq!(output[3], 0.375);
    }

    #[test]
    fn negative_and_hdr_rgb_round_trip_without_clipping() {
        let backend = BuiltinColorTransform;
        let encoded = [-0.25, 0.5, 2.0, 0.125];
        let linear = backend
            .transform_rgba(encoded, SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID)
            .unwrap();
        assert!(linear[0] < 0.0);
        assert!(linear[2] > 1.0);
        assert_eq!(linear[3], encoded[3]);
        let round_trip = backend
            .transform_rgba(linear, LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID)
            .unwrap();
        for (actual, expected) in round_trip.into_iter().zip(encoded) {
            assert_near(actual, expected);
        }
    }

    #[test]
    fn supported_identity_is_lossless_and_unknown_spaces_are_rejected() {
        let backend = BuiltinColorTransform;
        let value = [-3.0, 4.0, 0.25, 0.5];
        assert_eq!(
            backend.transform_rgba(value, LINEAR_SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID),
            Ok(value)
        );
        assert!(matches!(
            backend.transform_rgba(value, "acescg", LINEAR_SRGB_SPACE_ID),
            Err(ColorManagementError::UnsupportedTransform { .. })
        ));
        assert!(matches!(
            backend.transform_rgba(value, "acescg", "acescg"),
            Err(ColorManagementError::UnsupportedTransform { .. })
        ));
    }

    #[test]
    fn supported_purpose_participates_in_processor_cache_identity() {
        let backend = BuiltinColorTransform;
        let explicit = ColorTransformRequest::explicit(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        let display =
            ColorTransformRequest::working_to_display(LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID);
        assert_eq!(explicit.purpose(), TransformPurpose::Explicit);
        assert_ne!(
            backend.processor_cache_key(&explicit).unwrap(),
            backend.processor_cache_key(&display).unwrap()
        );
    }

    #[test]
    fn cpu_processor_exposes_the_exact_immutable_request_and_program_identity() {
        let backend = BuiltinColorTransform;
        let request = ColorTransformRequest::source_to_working(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID)
            .with_context(ColorContext::default().with_variable("SHOT", "010"));
        let processor = backend.create_cpu_processor(&request).unwrap();
        let compiled = processor.compiled_transform_identity();

        assert_eq!(compiled.backend_build(), BackendBuild::Real);
        assert_eq!(
            compiled.cache_key().purpose,
            TransformPurpose::SourceToWorking
        );
        assert_eq!(compiled.cache_key().context, *request.context());
        assert!(matches!(
            compiled.cache_key().spec,
            TransformSpec::ColorSpace {
                ref source,
                ref destination,
            } if source == SRGB_SPACE_ID && destination == LINEAR_SRGB_SPACE_ID
        ));
        assert_eq!(
            compiled.backend_program_cache_id(),
            "builtin.extended-srgb-to-linear.v1"
        );

        assert!(matches!(
            CompiledTransformIdentity::new(
                BackendBuild::Stub,
                compiled.cache_key().clone(),
                "stub-program",
            ),
            Err(ColorManagementError::StubBackend { .. })
        ));
    }

    #[test]
    fn builtin_rejects_named_display_views_instead_of_ignoring_them() {
        let backend = BuiltinColorTransform;
        let request = ColorTransformRequest::working_to_display_view(
            LINEAR_SRGB_SPACE_ID,
            "qa-display",
            "qa-view",
        );

        assert!(matches!(
            backend.processor_cache_key(&request),
            Err(ColorManagementError::UnsupportedDisplayView {
                ref backend_id,
                ref display,
                ref view,
            }) if backend_id == "builtin.standard-spaces.v3"
                && display == "qa-display"
                && view == "qa-view"
        ));
        assert!(matches!(
            backend.create_cpu_processor(&request),
            Err(ColorManagementError::UnsupportedDisplayView {
                ref backend_id,
                ref display,
                ref view,
            }) if backend_id == "builtin.standard-spaces.v3"
                && display == "qa-display"
                && view == "qa-view"
        ));
    }

    #[test]
    fn invalid_values_and_unavailable_gpu_path_fail_closed() {
        let backend = BuiltinColorTransform;
        let request = ColorTransformRequest::explicit(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        let processor = backend.create_cpu_processor(&request).unwrap();
        assert!(matches!(
            processor.transform_rgb([f64::NAN, 0.0, 0.0]),
            Err(ColorManagementError::NonFiniteComponent { component: "r" })
        ));
        assert!(matches!(
            backend.transform_rgba([0.0, 0.0, 0.0, 2.0], SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID),
            Err(ColorManagementError::AlphaOutOfRange)
        ));
        assert!(matches!(
            backend.extract_gpu_transform(&request, GpuShaderLanguage::SkSl),
            Err(ColorManagementError::GpuTransformUnavailable { .. })
        ));
    }
}
