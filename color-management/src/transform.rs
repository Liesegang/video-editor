use std::fmt;

use crate::contract::{BackendBuild, BackendCapabilities, CpuComputePrecision, ProcessorCacheKey};

pub const SRGB_SPACE_ID: &str = "srgb";
pub const LINEAR_SRGB_SPACE_ID: &str = "linear-srgb";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorSpaceInfo {
    pub id: String,
    pub label: String,
    pub scene_linear: bool,
}

/// Why a transform is being requested. Source and target identities remain
/// explicit even for Preview and output transforms, so no role silently
/// changes the mathematical contract.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TransformRole {
    Explicit,
    SourceToWorking,
    WorkingToDisplay { view: Option<String> },
    WorkingToOutput,
}

impl TransformRole {
    fn cache_fragment(&self) -> String {
        match self {
            Self::Explicit => "explicit".to_string(),
            Self::SourceToWorking => "source-to-working".to_string(),
            Self::WorkingToDisplay { view: None } => "working-to-display".to_string(),
            Self::WorkingToDisplay { view: Some(view) } => {
                format!("working-to-display:{view}")
            }
            Self::WorkingToOutput => "working-to-output".to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ColorTransformRequest {
    pub source_space: String,
    pub target_space: String,
    pub role: TransformRole,
}

impl ColorTransformRequest {
    pub fn explicit(source_space: impl Into<String>, target_space: impl Into<String>) -> Self {
        Self {
            source_space: source_space.into(),
            target_space: target_space.into(),
            role: TransformRole::Explicit,
        }
    }

    pub fn source_to_working(
        source_space: impl Into<String>,
        working_space: impl Into<String>,
    ) -> Self {
        Self {
            source_space: source_space.into(),
            target_space: working_space.into(),
            role: TransformRole::SourceToWorking,
        }
    }

    pub fn working_to_display(
        working_space: impl Into<String>,
        display_space: impl Into<String>,
        view: Option<String>,
    ) -> Self {
        Self {
            source_space: working_space.into(),
            target_space: display_space.into(),
            role: TransformRole::WorkingToDisplay { view },
        }
    }

    pub fn working_to_output(
        working_space: impl Into<String>,
        output_space: impl Into<String>,
    ) -> Self {
        Self {
            source_space: working_space.into(),
            target_space: output_space.into(),
            role: TransformRole::WorkingToOutput,
        }
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
    StubBackend { backend_id: String },
    EmptyColorSpace,
    NonFiniteComponent { component: &'static str },
    AlphaOutOfRange,
    UnsupportedTransform { source: String, target: String },
    GpuTransformUnavailable { backend_id: String },
}

impl fmt::Display for ColorManagementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StubBackend { backend_id } => {
                write!(formatter, "color backend '{backend_id}' is a stub build")
            }
            Self::EmptyColorSpace => formatter.write_str("color space must not be empty"),
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

pub trait CpuColorProcessor: Send + Sync {
    /// Transform one straight-alpha authoring color without clipping RGB.
    ///
    /// f64 is the lossless Project/API interchange representation. A backend
    /// may compute internally at the precision reported by
    /// [`BackendCapabilities::cpu_processor_compute_precision`].
    fn transform_straight_rgba(&self, rgba: [f64; 4]) -> Result<[f64; 4], ColorManagementError>;
}

/// Replaceable backend boundary for built-in transfers and a future pinned
/// OpenColorIO implementation.
pub trait ColorTransformBackend: Send + Sync {
    fn backend_id(&self) -> &'static str;
    fn build(&self) -> BackendBuild;
    fn config_fingerprint(&self) -> String;
    fn capabilities(&self) -> BackendCapabilities;
    fn available_color_spaces(&self) -> Result<Vec<ColorSpaceInfo>, ColorManagementError>;
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
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BuiltinColorTransform;

impl BuiltinColorTransform {
    pub fn transform_rgba(
        &self,
        rgba: [f64; 4],
        source_space: &str,
        target_space: &str,
    ) -> Result<[f64; 4], ColorManagementError> {
        let request = ColorTransformRequest::explicit(source_space, target_space);
        self.create_cpu_processor(&request)?
            .transform_straight_rgba(rgba)
    }

    fn validate_request(
        &self,
        request: &ColorTransformRequest,
    ) -> Result<TransformKind, ColorManagementError> {
        ensure_real_backend(self)?;
        if request.source_space.trim().is_empty() || request.target_space.trim().is_empty() {
            return Err(ColorManagementError::EmptyColorSpace);
        }
        let source_supported = matches!(
            request.source_space.as_str(),
            SRGB_SPACE_ID | LINEAR_SRGB_SPACE_ID
        );
        let target_supported = matches!(
            request.target_space.as_str(),
            SRGB_SPACE_ID | LINEAR_SRGB_SPACE_ID
        );
        if source_supported && target_supported && request.source_space == request.target_space {
            return Ok(TransformKind::Identity);
        }
        match (request.source_space.as_str(), request.target_space.as_str()) {
            (SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID) => Ok(TransformKind::SrgbToLinear),
            (LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID) => Ok(TransformKind::LinearToSrgb),
            _ => Err(ColorManagementError::UnsupportedTransform {
                source: request.source_space.clone(),
                target: request.target_space.clone(),
            }),
        }
    }
}

impl ColorTransformBackend for BuiltinColorTransform {
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
            cpu_processor_compute_precision: Some(CpuComputePrecision::Float64),
            gpu_shader_lut: false,
            extended_range_rgb: true,
        }
    }

    fn available_color_spaces(&self) -> Result<Vec<ColorSpaceInfo>, ColorManagementError> {
        ensure_real_backend(self)?;
        Ok(vec![
            ColorSpaceInfo {
                id: SRGB_SPACE_ID.to_string(),
                label: "sRGB (encoded)".to_string(),
                scene_linear: false,
            },
            ColorSpaceInfo {
                id: LINEAR_SRGB_SPACE_ID.to_string(),
                label: "Linear sRGB".to_string(),
                scene_linear: true,
            },
        ])
    }

    fn processor_cache_key(
        &self,
        request: &ColorTransformRequest,
    ) -> Result<ProcessorCacheKey, ColorManagementError> {
        let _ = self.validate_request(request)?;
        Ok(ProcessorCacheKey {
            backend_id: self.backend_id().to_string(),
            config_fingerprint: self.config_fingerprint(),
            source_space: request.source_space.clone(),
            target_space: request.target_space.clone(),
            role: request.role.cache_fragment(),
        })
    }

    fn create_cpu_processor(
        &self,
        request: &ColorTransformRequest,
    ) -> Result<Box<dyn CpuColorProcessor>, ColorManagementError> {
        Ok(Box::new(BuiltinCpuProcessor {
            kind: self.validate_request(request)?,
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

fn ensure_real_backend(backend: &dyn ColorTransformBackend) -> Result<(), ColorManagementError> {
    if backend.build() == BackendBuild::Stub {
        return Err(ColorManagementError::StubBackend {
            backend_id: backend.backend_id().to_string(),
        });
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransformKind {
    Identity,
    SrgbToLinear,
    LinearToSrgb,
}

struct BuiltinCpuProcessor {
    kind: TransformKind,
}

impl CpuColorProcessor for BuiltinCpuProcessor {
    fn transform_straight_rgba(&self, rgba: [f64; 4]) -> Result<[f64; 4], ColorManagementError> {
        validate_rgba(rgba)?;
        let [r, g, b, a] = rgba;
        let transform = match self.kind {
            TransformKind::Identity => return Ok(rgba),
            TransformKind::SrgbToLinear => extended_srgb_to_linear,
            TransformKind::LinearToSrgb => extended_linear_to_srgb,
        };
        Ok([transform(r), transform(g), transform(b), a])
    }
}

fn validate_rgba(rgba: [f64; 4]) -> Result<(), ColorManagementError> {
    for (component, value) in ["r", "g", "b", "a"].into_iter().zip(rgba) {
        if !value.is_finite() {
            return Err(ColorManagementError::NonFiniteComponent { component });
        }
    }
    if !(0.0..=1.0).contains(&rgba[3]) {
        return Err(ColorManagementError::AlphaOutOfRange);
    }
    Ok(())
}

/// IEC 61966-2-1 transfer extended outside `[0, 1]` by odd symmetry.
///
/// Odd extension keeps negative scene values finite and makes the transform
/// reversible. Values above one use the same power segment and are not clipped.
fn extended_srgb_to_linear(value: f64) -> f64 {
    signed_transfer(value, |magnitude| {
        if magnitude <= 0.040_45 {
            magnitude / 12.92
        } else {
            ((magnitude + 0.055) / 1.055).powf(2.4)
        }
    })
}

fn extended_linear_to_srgb(value: f64) -> f64 {
    signed_transfer(value, |magnitude| {
        if magnitude <= 0.003_130_8 {
            magnitude * 12.92
        } else {
            1.055 * magnitude.powf(1.0 / 2.4) - 0.055
        }
    })
}

fn signed_transfer(value: f64, transfer: impl FnOnce(f64) -> f64) -> f64 {
    let transformed = transfer(value.abs());
    if value.is_sign_negative() {
        -transformed
    } else {
        transformed
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BuiltinColorTransform, ColorManagementError, ColorTransformBackend, ColorTransformRequest,
        GpuShaderLanguage, LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID,
    };
    use crate::{BackendBuild, CpuComputePrecision, TransformRole};

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
        let capabilities = backend.capabilities();
        assert!(capabilities.enumerate_color_spaces);
        assert_eq!(
            capabilities.cpu_processor_compute_precision,
            Some(CpuComputePrecision::Float64)
        );
        assert!(!capabilities.gpu_shader_lut);
        assert!(capabilities.extended_range_rgb);
        assert_eq!(backend.available_color_spaces().unwrap().len(), 2);
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
    fn role_participates_in_processor_cache_identity() {
        let backend = BuiltinColorTransform;
        let explicit = ColorTransformRequest::explicit(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        let display = ColorTransformRequest::working_to_display(
            SRGB_SPACE_ID,
            LINEAR_SRGB_SPACE_ID,
            Some("qa-view".to_string()),
        );
        assert_eq!(explicit.role, TransformRole::Explicit);
        assert_ne!(
            backend.processor_cache_key(&explicit).unwrap(),
            backend.processor_cache_key(&display).unwrap()
        );
    }

    #[test]
    fn invalid_values_and_unavailable_gpu_path_fail_closed() {
        let backend = BuiltinColorTransform;
        let request = ColorTransformRequest::explicit(SRGB_SPACE_ID, LINEAR_SRGB_SPACE_ID);
        let processor = backend.create_cpu_processor(&request).unwrap();
        assert!(matches!(
            processor.transform_straight_rgba([f64::NAN, 0.0, 0.0, 1.0]),
            Err(ColorManagementError::NonFiniteComponent { component: "r" })
        ));
        assert!(matches!(
            processor.transform_straight_rgba([0.0, 0.0, 0.0, 2.0]),
            Err(ColorManagementError::AlphaOutOfRange)
        ));
        assert!(matches!(
            backend.extract_gpu_transform(&request, GpuShaderLanguage::SkSl),
            Err(ColorManagementError::GpuTransformUnavailable { .. })
        ));
    }
}
