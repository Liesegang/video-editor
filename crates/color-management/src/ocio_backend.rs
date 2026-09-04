//! Optional real OpenColorIO CPU backend.
//!
//! The published `ocio-rs` crate defaults to an API-compatible stub. This
//! adapter refuses to construct in that mode, so enabling RuViE's Cargo feature
//! alone can never turn a color transform into a silent no-op.

use std::{path::Path, sync::Mutex};

use ocio_rs::{Config, Context, Processor, ReferenceSpaceType, TransformDirection};
use sha2::{Digest, Sha256};

use crate::ocio_config::{OcioConfigSource, deterministic_context, ensure_runtime_version};
use crate::ocio_error::{OcioBackendError, map_ocio};
use crate::transform::sealed;
use crate::{
    BackendBuild, BackendCapabilities, ColorContext, ColorLinearity, ColorManagementError,
    ColorReferenceSpace, ColorSpaceInfo, ColorTransformBackend, ColorTransformRequest,
    CompiledTransformIdentity, CpuColorProcessor, CpuSamplePrecision, GpuColorTransform,
    GpuShaderLanguage, ProcessorCacheKey, TransformSpec,
};

const BACKEND_ID: &str = "opencolorio-2.5-via-ocio-rs-0.2.1";

/// A real, exact-config OpenColorIO backend.
///
/// `ocio-rs` 0.2.1 does not expose `Config` or `Context` as `Send + Sync`, so
/// this type retains only an immutable exact source. It reconstructs the
/// handles on the calling thread. A CPU processor is `Send` but not `Sync`, and
/// is therefore protected by a mutex rather than given an unsound `Sync` impl.
///
/// `from_exact_bytes` and `from_exact_path` accept only self-contained configs.
/// `ocio-rs` 0.2.1 does not expose the complete context-specialized external
/// file closure required for a trustworthy resource manifest, so any authored
/// `FileTransform` is rejected rather than pretending the `.ocio` checksum
/// also identifies its LUT sidecars. Exact built-in registry configs are
/// accepted only when the same audit proves they are self-contained.
#[derive(Clone, Debug)]
pub struct OcioColorTransformBackend {
    source: OcioConfigSource,
    expected_ocio_version: String,
    base_fingerprint: String,
}

impl sealed::Backend for OcioColorTransformBackend {}

impl OcioColorTransformBackend {
    pub fn from_exact_bytes(
        bytes: &[u8],
        expected_ocio_version: impl Into<String>,
    ) -> Result<Self, OcioBackendError> {
        let source = OcioConfigSource::from_exact_bytes(bytes)?;
        Self::initialize(source, expected_ocio_version.into())
    }

    pub fn from_exact_path(
        path: impl AsRef<Path>,
        expected_sha256: impl AsRef<str>,
        expected_ocio_version: impl Into<String>,
    ) -> Result<Self, OcioBackendError> {
        let source = OcioConfigSource::from_exact_path(path.as_ref(), expected_sha256.as_ref())?;
        Self::initialize(source, expected_ocio_version.into())
    }

    /// Opens only a name returned by OCIO's built-in registry, expressed as
    /// `ocio://<exact-registry-name>`. `expected_ocio_version` must exactly
    /// match both persisted Project intent and the linked runtime version.
    pub fn from_builtin_registry_uri(
        uri: impl Into<String>,
        expected_ocio_version: impl Into<String>,
    ) -> Result<Self, OcioBackendError> {
        let source = OcioConfigSource::from_builtin_registry_uri(uri.into())?;
        Self::initialize(source, expected_ocio_version.into())
    }

    pub fn exact_config_identity(&self) -> String {
        self.source.exact_identity()
    }

    pub fn expected_ocio_version(&self) -> &str {
        &self.expected_ocio_version
    }

    fn initialize(
        source: OcioConfigSource,
        expected_ocio_version: String,
    ) -> Result<Self, OcioBackendError> {
        ensure_runtime_version(&expected_ocio_version)?;
        let config = source.load_and_validate()?;
        let context = deterministic_context(&config, &ColorContext::default())?;
        let config_cache_id = required_identity(
            "config cache ID",
            map_ocio(
                "read config cache ID",
                config.try_cache_id_for_context(&context),
            )?,
        )?;
        let base_fingerprint = fingerprint(&[
            BACKEND_ID,
            &expected_ocio_version,
            &source.exact_identity(),
            &config_cache_id,
        ]);
        Ok(Self {
            source,
            expected_ocio_version,
            base_fingerprint,
        })
    }

    fn load_with_context(
        &self,
        requested: &ColorContext,
    ) -> Result<(Config, Context, String), OcioBackendError> {
        ensure_runtime_version(&self.expected_ocio_version)?;
        let config = self.source.load_and_validate()?;
        let context = deterministic_context(&config, requested)?;
        let config_cache_id = required_identity(
            "context-specialized config cache ID",
            map_ocio(
                "read context-specialized config cache ID",
                config.try_cache_id_for_context(&context),
            )?,
        )?;
        Ok((config, context, config_cache_id))
    }

    fn prepare_processor(
        &self,
        request: &ColorTransformRequest,
    ) -> Result<(ocio_rs::CPUProcessor, CompiledTransformIdentity), OcioBackendError> {
        validate_request(request).map_err(|error| OcioBackendError::Ocio {
            operation: "validate transform request",
            detail: error.to_string(),
        })?;
        let (config, context, context_config_cache_id) =
            self.load_with_context(request.context())?;
        let processor = processor_for_request(&config, &context, request.spec())?;
        let processor_cache_id = required_identity(
            "processor cache ID",
            map_ocio("read processor cache ID", processor.try_cache_id())?,
        )?;
        let cpu = map_ocio("create CPU processor", processor.default_cpu_processor())?;
        let cpu_cache_id = required_identity(
            "CPU processor cache ID",
            map_ocio("read CPU processor cache ID", cpu.try_cache_id())?,
        )?;
        let key = cache_key(request, self.base_fingerprint.clone());
        let program_id =
            fingerprint(&[&context_config_cache_id, &processor_cache_id, &cpu_cache_id]);
        let identity = CompiledTransformIdentity::new(BackendBuild::Real, key, program_id)
            .map_err(|error| OcioBackendError::Ocio {
                operation: "record compiled processor identity",
                detail: error.to_string(),
            })?;
        Ok((cpu, identity))
    }

    fn enumerate_color_spaces(&self) -> Result<Vec<ColorSpaceInfo>, OcioBackendError> {
        ensure_runtime_version(&self.expected_ocio_version)?;
        let config = self.source.load_and_validate()?;
        let count = config.num_color_spaces();
        let count = usize::try_from(count).map_err(|_| OcioBackendError::Ocio {
            operation: "enumerate color spaces",
            detail: format!("OCIO returned a negative color-space count ({count})"),
        })?;
        let mut spaces = Vec::with_capacity(count);
        for index in 0..count {
            let index = i32::try_from(index).map_err(|_| OcioBackendError::Ocio {
                operation: "enumerate color spaces",
                detail: "color-space index exceeds OCIO's i32 range".to_string(),
            })?;
            let name = map_ocio(
                "read color-space name",
                config.try_color_space_name_by_index(index),
            )?
            .ok_or_else(|| OcioBackendError::Ocio {
                operation: "read color-space name",
                detail: format!("OCIO returned no name for index {index}"),
            })?;
            let color_space = map_ocio("read color-space metadata", config.try_color_space(&name))?
                .ok_or_else(|| OcioBackendError::Ocio {
                    operation: "read color-space metadata",
                    detail: format!("OCIO returned no ColorSpace object for '{name}'"),
                })?;
            let (reference_space, ocio_reference) = match color_space.reference_space_type() {
                ReferenceSpaceType::Scene => {
                    (ColorReferenceSpace::Scene, ReferenceSpaceType::Scene)
                }
                ReferenceSpaceType::Display => {
                    (ColorReferenceSpace::Display, ReferenceSpaceType::Display)
                }
            };
            let linearity = if config.is_color_space_linear(&name, ocio_reference) {
                ColorLinearity::Linear
            } else {
                ColorLinearity::Encoded
            };
            spaces.push(ColorSpaceInfo {
                id: name.clone(),
                label: color_space.name().unwrap_or(name),
                reference_space,
                linearity,
                is_data: color_space.is_data(),
            });
        }
        Ok(spaces)
    }

    fn map_contract_error(
        operation: &'static str,
        error: OcioBackendError,
    ) -> ColorManagementError {
        match error {
            OcioBackendError::StubBuild => ColorManagementError::StubBackend {
                backend_id: BACKEND_ID.to_string(),
            },
            other => ColorManagementError::ProcessorContractMismatch {
                operation,
                detail: other.to_string(),
            },
        }
    }
}

impl ColorTransformBackend for OcioColorTransformBackend {
    fn backend_id(&self) -> &'static str {
        BACKEND_ID
    }

    fn build(&self) -> BackendBuild {
        BackendBuild::Real
    }

    fn config_fingerprint(&self) -> String {
        self.base_fingerprint.clone()
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            enumerate_color_spaces: true,
            cpu_processor_sample_precision: Some(CpuSamplePrecision::Float32),
            gpu_shader_lut: false,
            extended_range_rgb: true,
        }
    }

    fn available_color_spaces(&self) -> Result<Vec<ColorSpaceInfo>, ColorManagementError> {
        self.enumerate_color_spaces()
            .map_err(|error| Self::map_contract_error("enumerate color spaces", error))
    }

    fn processor_cache_key(
        &self,
        request: &ColorTransformRequest,
    ) -> Result<ProcessorCacheKey, ColorManagementError> {
        validate_request(request)?;
        // Reload and specialize the native context to fail closed on an
        // unavailable resource. The structural cache key keeps the same base
        // config fingerprint issued by verify_working_space; the complete
        // ColorContext is already a typed key field, while OCIO's specialized
        // config/cache ID is captured in CompiledTransformIdentity's program ID.
        let (_, _, _) = self
            .load_with_context(request.context())
            .map_err(|error| Self::map_contract_error("build processor cache key", error))?;
        Ok(cache_key(request, self.base_fingerprint.clone()))
    }

    fn create_cpu_processor(
        &self,
        request: &ColorTransformRequest,
    ) -> Result<Box<dyn CpuColorProcessor>, ColorManagementError> {
        let (processor, identity) = self
            .prepare_processor(request)
            .map_err(|error| Self::map_contract_error("create CPU processor", error))?;
        Ok(Box::new(OcioCpuProcessor {
            processor: Mutex::new(processor),
            identity,
        }))
    }

    fn extract_gpu_transform(
        &self,
        _request: &ColorTransformRequest,
        _language: GpuShaderLanguage,
    ) -> Result<GpuColorTransform, ColorManagementError> {
        Err(ColorManagementError::GpuTransformUnavailable {
            backend_id: BACKEND_ID.to_string(),
        })
    }

    fn display_view_output_space(
        &self,
        display: &str,
        view: &str,
        context: &ColorContext,
    ) -> Result<String, ColorManagementError> {
        let (config, _, _) = self
            .load_with_context(context)
            .map_err(|error| Self::map_contract_error("resolve display/view output", error))?;
        let output = config
            .display_view_color_space_name(display, view)
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| ColorManagementError::UnsupportedDisplayView {
                backend_id: BACKEND_ID.to_string(),
                display: display.to_string(),
                view: view.to_string(),
            })?;
        let color_space = map_ocio(
            "read display/view output color space",
            config.try_color_space(&output),
        )
        .map_err(|error| {
            Self::map_contract_error("resolve display/view output color space", error)
        })?;
        if color_space.is_none() {
            return Err(ColorManagementError::ColorSpaceUnavailable { space: output });
        }
        Ok(output)
    }
}

struct OcioCpuProcessor {
    processor: Mutex<ocio_rs::CPUProcessor>,
    identity: CompiledTransformIdentity,
}

impl sealed::Processor for OcioCpuProcessor {}

impl CpuColorProcessor for OcioCpuProcessor {
    fn compiled_transform_identity(&self) -> &CompiledTransformIdentity {
        &self.identity
    }

    fn transform_rgb(&self, rgb: [f64; 3]) -> Result<[f64; 3], ColorManagementError> {
        if !rgb.into_iter().all(f64::is_finite) {
            return Err(ColorManagementError::NonFiniteComponent { component: "rgb" });
        }
        let mut sample = rgb.map(|component| component as f32);
        if !sample.into_iter().all(f32::is_finite) {
            return Err(ColorManagementError::NonFiniteComponent { component: "rgb" });
        }
        self.with_processor("transform one RGB sample", |processor| {
            map_ocio(
                "transform one RGB sample",
                processor.try_apply_rgb(&mut sample),
            )
        })?;
        if !sample.into_iter().all(f32::is_finite) {
            return Err(ColorManagementError::NonFiniteComponent { component: "rgb" });
        }
        Ok(sample.map(f64::from))
    }

    fn transform_rgb_f32_in_place(
        &self,
        pixels: &mut [[f32; 3]],
    ) -> Result<(), ColorManagementError> {
        if !all_finite(pixels) {
            return Err(ColorManagementError::NonFiniteComponent { component: "rgb" });
        }
        let pixel_count = i64::try_from(pixels.len()).map_err(|_| {
            OcioColorTransformBackend::map_contract_error(
                "transform RGB image",
                OcioBackendError::PixelCountOverflow,
            )
        })?;
        self.with_processor("transform RGB image", |processor| {
            map_ocio(
                "transform RGB image",
                processor.try_apply_rgb_pixels(pixels.as_flattened_mut(), pixel_count, 3),
            )
        })?;
        if !all_finite(pixels) {
            return Err(ColorManagementError::NonFiniteComponent { component: "rgb" });
        }
        Ok(())
    }
}

impl OcioCpuProcessor {
    fn with_processor<T>(
        &self,
        operation: &'static str,
        apply: impl FnOnce(&ocio_rs::CPUProcessor) -> Result<T, OcioBackendError>,
    ) -> Result<T, ColorManagementError> {
        let processor = self.processor.lock().map_err(|_| {
            OcioColorTransformBackend::map_contract_error(
                operation,
                OcioBackendError::ProcessorLockPoisoned,
            )
        })?;
        apply(&processor)
            .map_err(|error| OcioColorTransformBackend::map_contract_error(operation, error))
    }
}

fn processor_for_request(
    config: &Config,
    context: &Context,
    spec: &TransformSpec,
) -> Result<Processor, OcioBackendError> {
    match spec {
        TransformSpec::ColorSpace {
            source,
            destination,
        } => map_ocio(
            "create color-space processor",
            config.processor_with_context(source, destination, context),
        ),
        TransformSpec::DisplayView {
            source,
            display,
            view,
            looks_bypass,
            data_bypass,
        } => {
            let transform = map_ocio(
                "create display/view transform",
                ocio_rs::transform::DisplayViewTransform::create(),
            )?;
            map_ocio("set display/view source", transform.set_src(source))?;
            map_ocio("set display", transform.set_display(display))?;
            map_ocio("set view", transform.set_view(view))?;
            map_ocio(
                "set looks bypass",
                transform.try_set_looks_bypass(*looks_bypass),
            )?;
            map_ocio(
                "set data bypass",
                transform.try_set_data_bypass(*data_bypass),
            )?;
            map_ocio(
                "set display/view direction",
                transform.try_set_direction(TransformDirection::Forward),
            )?;
            map_ocio("validate display/view transform", transform.validate())?;
            map_ocio(
                "create display/view processor",
                config.processor_from_transform_with_context(
                    context,
                    &transform,
                    TransformDirection::Forward,
                ),
            )
        }
    }
}

fn cache_key(request: &ColorTransformRequest, config_fingerprint: String) -> ProcessorCacheKey {
    ProcessorCacheKey {
        backend_id: BACKEND_ID.to_string(),
        config_fingerprint,
        purpose: request.purpose(),
        spec: request.spec().clone(),
        context: request.context().clone(),
    }
}

fn validate_request(request: &ColorTransformRequest) -> Result<(), ColorManagementError> {
    let all_non_empty = match request.spec() {
        TransformSpec::ColorSpace {
            source,
            destination,
        } => [source.as_str(), destination.as_str()]
            .into_iter()
            .all(|value| !value.trim().is_empty()),
        TransformSpec::DisplayView {
            source,
            display,
            view,
            ..
        } => [source.as_str(), display.as_str(), view.as_str()]
            .into_iter()
            .all(|value| !value.trim().is_empty()),
    };
    if all_non_empty {
        Ok(())
    } else {
        Err(ColorManagementError::EmptyColorSpace)
    }
}

fn all_finite(pixels: &[[f32; 3]]) -> bool {
    pixels.iter().flatten().copied().all(f32::is_finite)
}

fn required_identity(
    identity: &'static str,
    value: Option<String>,
) -> Result<String, OcioBackendError> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or(OcioBackendError::MissingRuntimeIdentity { identity })
}

fn fingerprint(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"ruvie-ocio-backend-v1\0");
    for part in parts {
        hasher.update(part.len().to_le_bytes());
        hasher.update(part.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::{OcioBackendError, OcioColorTransformBackend};
    use crate::{
        ColorContext, ColorTransformBackend, ColorTransformRequest, ManagedLinearWorkingImage,
        WorkingColorIdentity,
    };

    const RAW_CONFIG: &[u8] = b"ocio_profile_version: 2\nroles:\n  default: raw\ncolorspaces:\n  - !<ColorSpace> {name: raw, isdata: true}\n";

    #[test]
    fn stub_build_is_rejected_instead_of_becoming_a_no_op_backend() {
        if ocio_rs::is_stub_build() {
            assert!(matches!(
                OcioColorTransformBackend::from_exact_bytes(RAW_CONFIG, "2.5.2"),
                Err(OcioBackendError::StubBuild)
            ));
        }
    }

    #[test]
    fn real_runtime_executes_bulk_rgb_when_available() -> Result<(), Box<dyn Error>> {
        if ocio_rs::is_stub_build() {
            eprintln!("skipped: ocio-rs is a stub build");
            return Ok(());
        }
        let runtime_version = ocio_rs::version().ok_or("runtime version unavailable")?;
        let wrong_version = format!("{runtime_version}-mismatch");
        assert!(matches!(
            OcioColorTransformBackend::from_exact_bytes(RAW_CONFIG, wrong_version),
            Err(OcioBackendError::RuntimeVersionMismatch { .. })
        ));
        let raw_config = ocio_rs::Config::raw()?
            .serialize()?
            .ok_or("real OCIO did not serialize its built-in raw config")?;
        let backend = OcioColorTransformBackend::from_exact_bytes(
            raw_config.as_bytes(),
            runtime_version.as_str(),
        )?;
        let spaces = backend.available_color_spaces()?;
        assert!(
            spaces
                .iter()
                .any(|space| space.id == "raw" && space.is_data)
        );

        let request = ColorTransformRequest::explicit("raw", "raw");
        let processor = backend.create_cpu_processor(&request)?;
        let mut pixels = [[0.25, 0.5, 2.0], [-0.25, 1.5, 0.0]];
        processor.transform_rgb_f32_in_place(&mut pixels)?;
        assert_eq!(pixels, [[0.25, 0.5, 2.0], [-0.25, 1.5, 0.0]]);
        assert_eq!(
            processor.compiled_transform_identity().cache_key().context,
            *request.context()
        );
        Ok(())
    }

    /// Opt-in real-config contract gate. CI or a developer may provide an ACES
    /// or show config without pretending that the default ocio-rs stub ran it.
    #[test]
    fn real_non_srgb_managed_ingress_when_fixture_is_configured() -> Result<(), Box<dyn Error>> {
        if ocio_rs::is_stub_build() {
            eprintln!("skipped: ocio-rs is a stub build");
            return Ok(());
        }
        let fixture = (
            std::env::var("RUVIE_OCIO_TEST_CONFIG"),
            std::env::var("RUVIE_OCIO_TEST_CONFIG_SHA256"),
            std::env::var("RUVIE_OCIO_TEST_VERSION"),
            std::env::var("RUVIE_OCIO_TEST_SOURCE_SPACE"),
            std::env::var("RUVIE_OCIO_TEST_WORKING_SPACE"),
        );
        let (Ok(path), Ok(checksum), Ok(version), Ok(source), Ok(working)) = fixture else {
            eprintln!("skipped: RUVIE_OCIO_TEST_* real-config fixture is not configured");
            return Ok(());
        };
        assert!(!source.eq_ignore_ascii_case("srgb"));
        assert!(!working.eq_ignore_ascii_case("srgb"));

        let backend = OcioColorTransformBackend::from_exact_path(path, checksum, version)?;
        let context = ColorContext::default();
        let verified_source = backend.verify_source_space(&source, &context)?;
        let verified = backend.verify_working_space(&working, &context)?;
        let identity = WorkingColorIdentity::from_verified("real-ocio-fixture", verified)?;
        let request = ColorTransformRequest::source_to_working(&source, &working)
            .with_context(context.clone());
        let processor = backend.create_cpu_processor(&request)?;
        let image = ManagedLinearWorkingImage::solid_from_straight_rgba8(
            identity,
            &verified_source,
            1,
            1,
            [128, 64, 32, 255],
            processor.as_ref(),
        )?;
        assert_eq!(image.identity().context(), &context);
        assert_eq!(image.identity().working_space(), working);
        Ok(())
    }
}
