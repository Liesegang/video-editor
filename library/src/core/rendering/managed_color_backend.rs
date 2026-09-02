//! Exact Project color-config resolution and verified processor construction.

use std::path::Path;

use ruvie_color_management::{
    BuiltinColorTransform, ColorContext, ColorTransformBackend, ColorTransformRequest,
    CpuColorProcessor, DisplayViewSurfaceProcessor, LegacySrgbV1ColorTransform,
    ManagedLinearWorkingImage, PQ_LINEARIZATION_POLICY_CONTEXT_KEY,
    REFERENCE_WHITE_NITS_CONTEXT_KEY, RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY, VerifiedSourceSpace,
    WorkingColorIdentity,
};

use crate::error::LibraryError;
use crate::model::authoring::AuthoringProject;
use crate::model::frame::Image;
#[cfg(test)]
use crate::model::project::Project;
use crate::model::project::asset::Asset;
use crate::model::project::{
    ColorConfigIdentity, DEFAULT_BUNDLED_COLOR_CONFIG_ID, LEGACY_BUNDLED_COLOR_CONFIG_V1_ID,
    ModelValidatedColorManagementConfig, ResolvedColorManagementConfig,
};
use crate::model::property::{ColorSpaceRef, ColorValue};
use crate::plugin::{DecodedPixelBuffer, DecodedStraightRgba16F};
use crate::rendering::renderer::WorkingSurfaceContract;

const MAX_MEDIA_INGRESS_TRANSIENT_BYTES: u64 = 768 * 1024 * 1024;

pub(crate) trait ProjectColorAuthority {
    fn assets(&self) -> &[Asset];
    fn resolved_color_management(&self) -> ResolvedColorManagementConfig;
}

#[cfg(test)]
impl ProjectColorAuthority for Project {
    fn assets(&self) -> &[Asset] {
        &self.assets
    }

    fn resolved_color_management(&self) -> ResolvedColorManagementConfig {
        Project::resolved_color_management(self)
    }
}

impl ProjectColorAuthority for AuthoringProject {
    fn assets(&self) -> &[Asset] {
        &self.assets
    }

    fn resolved_color_management(&self) -> ResolvedColorManagementConfig {
        AuthoringProject::resolved_color_management(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ManagedRenderDestination {
    Preview,
    Export,
}

pub(crate) struct ProjectColorPipeline {
    backend: Box<dyn ColorTransformBackend>,
    intent: ModelValidatedColorManagementConfig,
    context: ColorContext,
    working: WorkingColorIdentity,
    authoring_srgb: VerifiedSourceSpace,
    terminal: ProjectTerminal,
}

enum ProjectTerminal {
    Direct(Box<dyn CpuColorProcessor>),
    NamedView(DisplayViewSurfaceProcessor),
}

impl ProjectColorPipeline {
    pub(crate) fn for_project(
        project: &dyn ProjectColorAuthority,
        destination: ManagedRenderDestination,
    ) -> Result<Self, LibraryError> {
        let intent = resolved_intent(project)?;
        validate_terminal_storage(&intent, destination)?;
        let backend = backend_for_project(project, &intent)?;
        let context = project_color_context(&intent);
        validate_terminal_backend_binding(backend.as_ref(), &intent, destination, &context)?;
        let srgb_surface_binding = intent.srgb_surface_space().map_err(|issue| {
            LibraryError::Render(format!(
                "Project sRGB surface binding is unavailable: {issue}"
            ))
        })?;
        let srgb_surface_space =
            resolve_exact_backend_space(backend.as_ref(), srgb_surface_binding.color_space())?;
        let authoring_srgb = backend
            .verify_source_space(&srgb_surface_space, &context)
            .map_err(color_error)?;
        let working_space =
            resolve_backend_space(backend.as_ref(), intent.config().working_space())?;
        let verified = backend
            .verify_working_space(&working_space, &context)
            .map_err(color_error)?;
        let working = WorkingColorIdentity::from_verified(intent.cache_identity(), verified)
            .map_err(color_error)?;
        let terminal = create_terminal(
            backend.as_ref(),
            &intent,
            destination,
            &working_space,
            &srgb_surface_space,
            &context,
        )?;
        Ok(Self {
            backend,
            intent,
            context,
            working,
            authoring_srgb,
            terminal,
        })
    }

    pub(super) fn intent(&self) -> &ModelValidatedColorManagementConfig {
        &self.intent
    }

    pub(super) fn resolve_source_space(
        &self,
        requested: &str,
    ) -> Result<VerifiedSourceSpace, LibraryError> {
        let resolved = resolve_backend_space(self.backend.as_ref(), requested)?;
        self.backend
            .verify_source_space(&resolved, &self.context)
            .map_err(color_error)
    }

    pub(super) fn ingest_pixels(
        &self,
        source: &VerifiedSourceSpace,
        pixels: DecodedPixelBuffer,
    ) -> Result<ManagedLinearWorkingImage, LibraryError> {
        validate_media_ingress_budget(&pixels)?;
        let processor = self.source_processor(source)?;
        match pixels {
            DecodedPixelBuffer::StraightRgba8(image) => {
                ManagedLinearWorkingImage::from_straight_rgba8(
                    self.working.clone(),
                    source,
                    image.width(),
                    image.height(),
                    image.data(),
                    processor.as_ref(),
                )
                .map_err(linear_working_error)
            }
            DecodedPixelBuffer::StraightRgba16F(image) => {
                self.ingest_rgba16f(source, &image, processor.as_ref())
            }
            DecodedPixelBuffer::StraightRgba32F(image) => {
                ManagedLinearWorkingImage::from_straight_rgba_f32(
                    self.working.clone(),
                    source,
                    image.width(),
                    image.height(),
                    image.data(),
                    processor.as_ref(),
                )
                .map_err(linear_working_error)
            }
        }
    }

    pub(crate) fn terminal_image(
        &self,
        image: &ManagedLinearWorkingImage,
    ) -> Result<Image, LibraryError> {
        let rgba = match &self.terminal {
            ProjectTerminal::Direct(processor) => image.to_straight_rgba8(processor.as_ref()),
            ProjectTerminal::NamedView(processor) => {
                image.to_straight_rgba8_via_display_surface(processor)
            }
        }
        .map_err(linear_working_error)?;
        Ok(Image::new(
            image.pixels().width(),
            image.pixels().height(),
            rgba,
        ))
    }

    pub(crate) fn working_surface_contract(&self) -> Result<WorkingSurfaceContract, LibraryError> {
        WorkingSurfaceContract::new(
            self.working.clone(),
            self.authoring_srgb.clone(),
            self.source_processor(&self.authoring_srgb)?,
        )
    }

    /// Convert a graph-authored straight color into this Project's exact
    /// working space for a project-linear Effect uniform.
    ///
    /// The canonical `srgb` model tag is resolved through the Project's exact
    /// sRGB authoring binding, including OCIO configs whose local space has a
    /// different name. Other tags are resolved by the selected backend.
    pub(crate) fn effect_color_to_working(
        &self,
        color: &ColorValue,
    ) -> Result<ColorValue, LibraryError> {
        let source = if color.color_space() == &ColorSpaceRef::srgb() {
            self.authoring_srgb.clone()
        } else {
            self.resolve_source_space(color.color_space().as_str())?
        };
        let [r, g, b, a] = color.rgba();
        let rgb = self
            .source_processor(&source)?
            .transform_rgb([r, g, b])
            .map_err(color_error)?;
        let working_space = ColorSpaceRef::new(self.working.working_space()).map_err(|error| {
            LibraryError::Render(format!(
                "Project working color-space identity is invalid: {error}"
            ))
        })?;
        ColorValue::new(working_space, [rgb[0], rgb[1], rgb[2], a]).map_err(|error| {
            LibraryError::Render(format!(
                "Effect color conversion produced an invalid Project working value: {error}"
            ))
        })
    }

    fn ingest_rgba16f(
        &self,
        source: &VerifiedSourceSpace,
        image: &DecodedStraightRgba16F,
        processor: &dyn CpuColorProcessor,
    ) -> Result<ManagedLinearWorkingImage, LibraryError> {
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(image.data().len()).map_err(|_| {
            LibraryError::Render(format!(
                "cannot allocate RGBAF32 conversion buffer for {}x{} RGBA16F media",
                image.width(),
                image.height()
            ))
        })?;
        rgba.extend(
            image
                .data()
                .iter()
                .map(|pixel| (*pixel).map(half::f16::to_f32)),
        );
        ManagedLinearWorkingImage::from_straight_rgba_f32(
            self.working.clone(),
            source,
            image.width(),
            image.height(),
            &rgba,
            processor,
        )
        .map_err(linear_working_error)
    }

    fn source_processor(
        &self,
        source: &VerifiedSourceSpace,
    ) -> Result<Box<dyn CpuColorProcessor>, LibraryError> {
        self.backend
            .create_cpu_processor(
                &ColorTransformRequest::source_to_working(
                    source.color_space_id(),
                    self.working.working_space(),
                )
                .with_context(self.context.clone()),
            )
            .map_err(color_error)
    }
}

fn validate_media_ingress_budget(pixels: &DecodedPixelBuffer) -> Result<(), LibraryError> {
    // Peak estimates include the resident decoded source, the processor's
    // straight RGB scratch, and final premultiplied RGBAF32 storage. RGBA16F
    // additionally expands once to straight f32 before the common boundary.
    let bytes_per_pixel = match pixels {
        DecodedPixelBuffer::StraightRgba8(_) => 4_u64 + 12 + 16,
        DecodedPixelBuffer::StraightRgba16F(_) => 8_u64 + 16 + 12 + 16,
        DecodedPixelBuffer::StraightRgba32F(_) => 16_u64 + 12 + 16,
    };
    let estimated = u64::from(pixels.width())
        .checked_mul(u64::from(pixels.height()))
        .and_then(|count| count.checked_mul(bytes_per_pixel))
        .ok_or_else(|| LibraryError::Render("media ingress size overflows".to_string()))?;
    if estimated > MAX_MEDIA_INGRESS_TRANSIENT_BYTES {
        return Err(LibraryError::Render(format!(
            "{} {}x{} media needs an estimated {estimated} transient bytes for verified Project ingress; limit is {MAX_MEDIA_INGRESS_TRANSIENT_BYTES}",
            pixels.storage_name(),
            pixels.width(),
            pixels.height()
        )));
    }
    Ok(())
}

fn project_color_context(intent: &ModelValidatedColorManagementConfig) -> ColorContext {
    let hdr = intent.config().hdr();
    let mut variables = Vec::new();
    if let Some(reference_white_nits) = hdr.reference_white_nits() {
        variables.push((
            REFERENCE_WHITE_NITS_CONTEXT_KEY,
            reference_white_nits.to_string(),
        ));
    }
    if let Some(policy) = hdr.pq_linearization_policy() {
        let value = if policy.is_supported() {
            RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY
        } else {
            policy.context_value()
        };
        variables.push((PQ_LINEARIZATION_POLICY_CONTEXT_KEY, value.to_string()));
    }
    ColorContext::from_variables(variables)
}

fn validate_terminal_storage(
    intent: &ModelValidatedColorManagementConfig,
    destination: ManagedRenderDestination,
) -> Result<(), LibraryError> {
    let config = intent.config();
    let surface_space = intent
        .srgb_surface_space()
        .map_err(|issue| {
            LibraryError::Render(format!(
                "terminal has no exact active-config sRGB surface binding: {issue}"
            ))
        })?
        .color_space();
    match destination {
        ManagedRenderDestination::Preview => {
            if !config.preview().surface_encoding().is_srgb() {
                return Err(LibraryError::Render(format!(
                    "Preview surface encoding '{}' cannot be presented through the current egui sRGB texture boundary",
                    config.preview().surface_encoding().as_str()
                )));
            }
            if config.preview().view().is_none() && config.preview().display() != surface_space {
                return Err(LibraryError::Render(format!(
                    "direct Preview destination '{}' does not match its exact Project-bound sRGB surface space '{}' at the egui texture boundary",
                    config.preview().display(),
                    surface_space
                )));
            }
            if config.preview().view().is_some()
                && config
                    .preview()
                    .view_output_color_space()
                    .is_none_or(|space| space.trim().is_empty())
            {
                return Err(LibraryError::Render(
                    "named OCIO Preview requires an exact config-local view_output_color_space binding before writing egui sRGB bytes"
                        .to_string(),
                ));
            }
            Ok(())
        }
        ManagedRenderDestination::Export if config.export().output_space() == surface_space => {
            Ok(())
        }
        ManagedRenderDestination::Export => Err(LibraryError::Render(format!(
            "color output space '{}' is not the exact active-config sRGB surface space '{surface_space}' and cannot be presented through the current untagged RGBA8 export RenderOutput; a typed output color/profile boundary is required",
            config.export().output_space(),
        ))),
    }
}

fn validate_terminal_backend_binding(
    backend: &dyn ColorTransformBackend,
    intent: &ModelValidatedColorManagementConfig,
    destination: ManagedRenderDestination,
    context: &ColorContext,
) -> Result<(), LibraryError> {
    if destination != ManagedRenderDestination::Preview {
        return Ok(());
    }
    let preview = intent.config().preview();
    let Some(view) = preview.view() else {
        return Ok(());
    };
    let expected = preview.view_output_color_space().ok_or_else(|| {
        LibraryError::Render("named OCIO Preview has no exact output-space binding".to_string())
    })?;
    let actual = backend
        .display_view_output_space(preview.display(), view, context)
        .map_err(color_error)?;
    if actual != expected {
        return Err(LibraryError::Render(format!(
            "OCIO display/view '{}/{view}' resolves output color space '{actual}', not the Project-bound exact space '{expected}' for its sRGB Preview surface",
            preview.display()
        )));
    }
    Ok(())
}

fn resolved_intent(
    project: &dyn ProjectColorAuthority,
) -> Result<ModelValidatedColorManagementConfig, LibraryError> {
    match project.resolved_color_management() {
        ResolvedColorManagementConfig::Ready(intent) => Ok(*intent),
        ResolvedColorManagementConfig::Unavailable { diagnostics, .. } => {
            Err(LibraryError::Render(format!(
                "Project color configuration is unavailable: {}",
                diagnostics
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")
            )))
        }
    }
}

fn backend_for_project(
    project: &dyn ProjectColorAuthority,
    intent: &ModelValidatedColorManagementConfig,
) -> Result<Box<dyn ColorTransformBackend>, LibraryError> {
    match intent.config().config() {
        ColorConfigIdentity::Bundled { id } if id == DEFAULT_BUNDLED_COLOR_CONFIG_ID => {
            Ok(Box::new(BuiltinColorTransform))
        }
        ColorConfigIdentity::Bundled { id } if id == LEGACY_BUNDLED_COLOR_CONFIG_V1_ID => {
            Ok(Box::new(LegacySrgbV1ColorTransform))
        }
        ColorConfigIdentity::Bundled { id } => Err(LibraryError::Render(format!(
            "bundled color config '{id}' is not available in this build"
        ))),
        ColorConfigIdentity::OcioBuiltin { uri, ocio_version } => {
            ocio_builtin_backend(uri, ocio_version)
        }
        ColorConfigIdentity::ProjectAsset {
            asset_id,
            sha256,
            ocio_version,
        } => {
            let asset = project
                .assets()
                .iter()
                .find(|asset| asset.id == *asset_id)
                .ok_or_else(|| {
                    LibraryError::Render(format!(
                        "OpenColorIO config Asset {asset_id} no longer exists"
                    ))
                })?;
            ocio_project_backend(Path::new(&asset.path), sha256, ocio_version)
        }
    }
}

#[cfg(feature = "opencolorio")]
fn ocio_builtin_backend(
    uri: &str,
    ocio_version: &str,
) -> Result<Box<dyn ColorTransformBackend>, LibraryError> {
    ruvie_color_management::OcioColorTransformBackend::from_builtin_registry_uri(uri, ocio_version)
        .map(|backend| Box::new(backend) as Box<dyn ColorTransformBackend>)
        .map_err(|error| LibraryError::Render(format!("Cannot open exact OCIO config: {error}")))
}

#[cfg(not(feature = "opencolorio"))]
fn ocio_builtin_backend(
    uri: &str,
    _ocio_version: &str,
) -> Result<Box<dyn ColorTransformBackend>, LibraryError> {
    Err(LibraryError::Render(format!(
        "Project requires OpenColorIO config '{uri}', but this build has no real OpenColorIO runtime"
    )))
}

#[cfg(feature = "opencolorio")]
fn ocio_project_backend(
    path: &Path,
    sha256: &str,
    ocio_version: &str,
) -> Result<Box<dyn ColorTransformBackend>, LibraryError> {
    ruvie_color_management::OcioColorTransformBackend::from_exact_path(path, sha256, ocio_version)
        .map(|backend| Box::new(backend) as Box<dyn ColorTransformBackend>)
        .map_err(|error| LibraryError::Render(format!("Cannot open exact OCIO config: {error}")))
}

#[cfg(not(feature = "opencolorio"))]
fn ocio_project_backend(
    path: &Path,
    _sha256: &str,
    _ocio_version: &str,
) -> Result<Box<dyn ColorTransformBackend>, LibraryError> {
    Err(LibraryError::Render(format!(
        "Project requires OpenColorIO config '{}', but this build has no real OpenColorIO runtime",
        path.display()
    )))
}

fn terminal_request(
    intent: &ModelValidatedColorManagementConfig,
    destination: ManagedRenderDestination,
    working_space: &str,
    context: &ColorContext,
) -> ColorTransformRequest {
    let config = intent.config();
    let request = match destination {
        ManagedRenderDestination::Preview => match config.preview().view() {
            Some(view) => ColorTransformRequest::working_to_display_view(
                working_space,
                config.preview().display(),
                view,
            ),
            None => {
                ColorTransformRequest::working_to_display(working_space, config.preview().display())
            }
        },
        ManagedRenderDestination::Export => {
            ColorTransformRequest::working_to_output(working_space, config.export().output_space())
        }
    };
    request.with_context(context.clone())
}

fn create_terminal(
    backend: &dyn ColorTransformBackend,
    intent: &ModelValidatedColorManagementConfig,
    destination: ManagedRenderDestination,
    working_space: &str,
    srgb_surface_space: &str,
    context: &ColorContext,
) -> Result<ProjectTerminal, LibraryError> {
    let first_request = terminal_request(intent, destination, working_space, context);
    let first = backend
        .create_cpu_processor(&first_request)
        .map_err(color_error)?;
    if destination != ManagedRenderDestination::Preview {
        return Ok(ProjectTerminal::Direct(first));
    }
    let preview = intent.config().preview();
    let Some(_) = preview.view() else {
        return Ok(ProjectTerminal::Direct(first));
    };
    let view_output_space = preview.view_output_color_space().ok_or_else(|| {
        LibraryError::Render("named OCIO Preview has no exact view output binding".to_string())
    })?;
    let view_output_space = resolve_exact_backend_space(backend, view_output_space)?;
    let surface_request = ColorTransformRequest::explicit(&view_output_space, srgb_surface_space)
        .with_context(context.clone());
    let surface = backend
        .create_cpu_processor(&surface_request)
        .map_err(color_error)?;
    DisplayViewSurfaceProcessor::new(first, view_output_space, srgb_surface_space, surface)
        .map(ProjectTerminal::NamedView)
        .map_err(color_error)
}

fn resolve_backend_space(
    backend: &dyn ColorTransformBackend,
    requested: &str,
) -> Result<String, LibraryError> {
    let spaces = backend.available_color_spaces().map_err(color_error)?;
    if let Some(exact) = spaces.iter().find(|space| space.id == requested) {
        return Ok(exact.id.clone());
    }
    let mut insensitive = spaces
        .iter()
        .filter(|space| space.id.eq_ignore_ascii_case(requested));
    if let Some(first) = insensitive.next()
        && insensitive.next().is_none()
    {
        return Ok(first.id.clone());
    }
    Err(LibraryError::Render(format!(
        "color space '{requested}' is not uniquely available in backend '{}'",
        backend.backend_id()
    )))
}

fn resolve_exact_backend_space(
    backend: &dyn ColorTransformBackend,
    requested: &str,
) -> Result<String, LibraryError> {
    backend
        .available_color_spaces()
        .map_err(color_error)?
        .into_iter()
        .find(|space| space.id == requested)
        .map(|space| space.id)
        .ok_or_else(|| {
            LibraryError::Render(format!(
                "exact color space '{requested}' is unavailable in backend '{}'",
                backend.backend_id()
            ))
        })
}

fn color_error(error: ruvie_color_management::ColorManagementError) -> LibraryError {
    LibraryError::Render(format!("Cannot create verified color processor: {error}"))
}

fn linear_working_error(error: ruvie_color_management::LinearWorkingImageError) -> LibraryError {
    LibraryError::Render(format!("linear working image operation failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruvie_color_management::TransformPurpose;

    #[test]
    fn preview_and_export_build_distinct_terminal_purposes() {
        let project = Project::new("terminal transform purposes");
        let intent = resolved_intent(&project).expect("default color intent");
        let context = project_color_context(&intent);
        let working_space = intent.config().working_space();

        assert_eq!(
            terminal_request(
                &intent,
                ManagedRenderDestination::Preview,
                working_space,
                &context,
            )
            .purpose(),
            TransformPurpose::WorkingToDisplay
        );
        assert_eq!(
            terminal_request(
                &intent,
                ManagedRenderDestination::Export,
                working_space,
                &context,
            )
            .purpose(),
            TransformPurpose::WorkingToOutput
        );
    }
}
