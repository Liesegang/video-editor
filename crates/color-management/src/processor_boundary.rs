use crate::{
    AlphaRepresentation, BackendBuild, ColorContext, ColorManagementError, CpuColorProcessor,
    LinearWorkingImage, LinearWorkingImageError, ProcessorCacheKey, TransformPurpose,
    TransformSpec, VerifiedSourceSpace, VerifiedWorkingSpace,
};

/// Verified two-stage Preview terminal for a named display/view followed by
/// an exact config-local native surface encoding.
///
/// A display/view's reported output space is not proof that its numeric output
/// is encoded for the native surface. Construction therefore requires a
/// second explicit processor and verifies that both stages share one real
/// backend/config/context and join at the exact declared view-output space.
pub struct DisplayViewSurfaceProcessor {
    display_view: Box<dyn CpuColorProcessor>,
    view_output_to_surface: Box<dyn CpuColorProcessor>,
    view_output_space: String,
    surface_space: String,
}

impl std::fmt::Debug for DisplayViewSurfaceProcessor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DisplayViewSurfaceProcessor")
            .field("view_output_space", &self.view_output_space)
            .field("surface_space", &self.surface_space)
            .finish_non_exhaustive()
    }
}

impl DisplayViewSurfaceProcessor {
    pub fn new(
        display_view: Box<dyn CpuColorProcessor>,
        view_output_space: impl Into<String>,
        surface_space: impl Into<String>,
        view_output_to_surface: Box<dyn CpuColorProcessor>,
    ) -> Result<Self, ColorManagementError> {
        let view_output_space = view_output_space.into();
        let surface_space = surface_space.into();
        if view_output_space.trim().is_empty() || surface_space.trim().is_empty() {
            return Err(ColorManagementError::EmptyColorSpace);
        }

        let view_identity = display_view.compiled_transform_identity();
        let view_key = view_identity.cache_key();
        if view_key.purpose != TransformPurpose::WorkingToDisplay
            || !matches!(view_key.spec, TransformSpec::DisplayView { .. })
        {
            return Err(processor_contract_error(
                "named display/view to surface conversion",
                "first processor is not a WorkingToDisplay named view",
            ));
        }

        let surface_identity = view_output_to_surface.compiled_transform_identity();
        let surface_key = surface_identity.cache_key();
        if surface_key.purpose != TransformPurpose::Explicit {
            return Err(processor_contract_error(
                "named display/view to surface conversion",
                "second processor is not an explicit color-space conversion",
            ));
        }
        let TransformSpec::ColorSpace {
            source,
            destination,
        } = &surface_key.spec
        else {
            return Err(processor_contract_error(
                "named display/view to surface conversion",
                "second processor is not a color-space transform",
            ));
        };
        if source != &view_output_space || destination != &surface_space {
            return Err(processor_contract_error(
                "named display/view to surface conversion",
                format!(
                    "second processor is '{source}' -> '{destination}', expected '{view_output_space}' -> '{surface_space}'"
                ),
            ));
        }
        if view_identity.backend_build() != surface_identity.backend_build()
            || view_key.backend_id != surface_key.backend_id
            || view_key.config_fingerprint != surface_key.config_fingerprint
            || view_key.context != surface_key.context
        {
            return Err(processor_contract_error(
                "named display/view to surface conversion",
                "display/view and surface processors do not share one backend/config/context",
            ));
        }

        Ok(Self {
            display_view,
            view_output_to_surface,
            view_output_space,
            surface_space,
        })
    }

    pub fn view_output_space(&self) -> &str {
        &self.view_output_space
    }

    pub fn surface_space(&self) -> &str {
        &self.surface_space
    }
}

/// Stable color identity required at every composite/cache boundary.
///
/// Component storage deliberately does not participate in this identity. An
/// RGBA16F GPU surface and an RGBA32F CPU surface can represent the same
/// working-color contract and may cross a storage-conversion boundary without
/// becoming color-incompatible. The verified space and exact context preserve
/// whether source samples were scene-derived or adopted from a display-derived
/// domain through an explicit policy. The owning image/resource type records
/// its actual storage instead.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WorkingColorIdentity {
    project_config_identity: String,
    verified: VerifiedWorkingSpace,
    alpha: AlphaRepresentation,
}

impl WorkingColorIdentity {
    /// Bind a backend-verified linear working space to one Project config.
    ///
    /// An arbitrary space name or stub backend cannot be supplied here: the
    /// caller must first obtain [`VerifiedWorkingSpace`] from a real backend.
    pub fn from_verified(
        project_config_identity: impl Into<String>,
        verified: VerifiedWorkingSpace,
    ) -> Result<Self, ColorManagementError> {
        let project_config_identity = project_config_identity.into();
        if project_config_identity.trim().is_empty() {
            return Err(ColorManagementError::EmptyProjectConfigIdentity);
        }
        Ok(Self {
            project_config_identity,
            verified,
            alpha: AlphaRepresentation::Premultiplied,
        })
    }

    pub fn project_config_identity(&self) -> &str {
        &self.project_config_identity
    }

    pub fn backend_id(&self) -> &str {
        self.verified.backend_id()
    }

    pub const fn backend_build(&self) -> BackendBuild {
        self.verified.backend_build()
    }

    pub fn backend_config_fingerprint(&self) -> &str {
        self.verified.backend_config_fingerprint()
    }

    pub const fn context(&self) -> &ColorContext {
        self.verified.context()
    }

    pub fn working_space(&self) -> &str {
        self.verified.color_space_id()
    }

    pub const fn alpha(&self) -> AlphaRepresentation {
        self.alpha
    }

    fn verified(&self) -> &VerifiedWorkingSpace {
        &self.verified
    }
}

/// Owner-bearing linear working image whose identity cannot be dropped
/// accidentally at a composite/cache boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct ManagedLinearWorkingImage {
    identity: WorkingColorIdentity,
    pixels: LinearWorkingImage,
}

impl ManagedLinearWorkingImage {
    /// Reattach pixels produced by an external working-domain renderer to the
    /// exact identity of its inputs.
    ///
    /// # Safety
    ///
    /// `pixels` must be the result of operations that preserve the RGB
    /// encoding represented by `identity`. Every color-bearing input to that
    /// operation must carry the same `WorkingColorIdentity`; the operation may
    /// not apply a transfer function, gamut conversion, display transform, or
    /// reinterpret bare numeric samples. Alpha must remain premultiplied.
    pub unsafe fn from_working_pixels_unchecked(
        identity: WorkingColorIdentity,
        pixels: LinearWorkingImage,
    ) -> Self {
        Self { identity, pixels }
    }

    pub fn identity(&self) -> &WorkingColorIdentity {
        &self.identity
    }

    pub fn pixels(&self) -> &LinearWorkingImage {
        &self.pixels
    }

    pub fn into_parts(self) -> (WorkingColorIdentity, LinearWorkingImage) {
        (self.identity, self.pixels)
    }

    /// Decode a straight RGBA8 source through a processor compiled specifically
    /// for this image's source-to-working boundary.
    pub fn from_straight_rgba8(
        identity: WorkingColorIdentity,
        source: &VerifiedSourceSpace,
        width: u32,
        height: u32,
        rgba: &[u8],
        source_to_working: &dyn CpuColorProcessor,
    ) -> Result<Self, LinearWorkingImageError> {
        validate_source_to_working(&identity, source, source_to_working)?;
        let pixels =
            LinearWorkingImage::from_straight_rgba8(width, height, rgba, source_to_working)?;
        Ok(Self { identity, pixels })
    }

    /// Decode straight RGBA32F source samples through a processor compiled for
    /// the exact resolved source-space token and this working identity.
    pub fn from_straight_rgba_f32(
        identity: WorkingColorIdentity,
        source: &VerifiedSourceSpace,
        width: u32,
        height: u32,
        rgba: &[[f32; 4]],
        source_to_working: &dyn CpuColorProcessor,
    ) -> Result<Self, LinearWorkingImageError> {
        validate_source_to_working(&identity, source, source_to_working)?;
        let pixels =
            LinearWorkingImage::from_straight_rgba_f32(width, height, rgba, source_to_working)?;
        Ok(Self { identity, pixels })
    }

    /// Create a solid working image without expanding an encoded RGBA8 canvas.
    pub fn solid_from_straight_rgba8(
        identity: WorkingColorIdentity,
        source: &VerifiedSourceSpace,
        width: u32,
        height: u32,
        rgba: [u8; 4],
        source_to_working: &dyn CpuColorProcessor,
    ) -> Result<Self, LinearWorkingImageError> {
        validate_source_to_working(&identity, source, source_to_working)?;
        let pixels =
            LinearWorkingImage::solid_from_straight_rgba8(width, height, rgba, source_to_working)?;
        Ok(Self { identity, pixels })
    }

    /// Apply the matching working-to-display/output processor without packing.
    pub fn to_straight_rgba_f32(
        &self,
        working_to_output: &dyn CpuColorProcessor,
    ) -> Result<Vec<[f32; 4]>, LinearWorkingImageError> {
        validate_working_to_output(&self.identity, working_to_output)?;
        self.pixels.to_straight_rgba_f32(working_to_output)
    }

    /// Apply the matching terminal transform and pack to RGBA8.
    pub fn to_straight_rgba8(
        &self,
        working_to_output: &dyn CpuColorProcessor,
    ) -> Result<Vec<u8>, LinearWorkingImageError> {
        validate_working_to_output(&self.identity, working_to_output)?;
        self.pixels.to_straight_rgba8(working_to_output)
    }

    /// Apply a named OCIO display/view and then convert its exact config-local
    /// output into the Project-bound native surface encoding before packing.
    pub fn to_straight_rgba8_via_display_surface(
        &self,
        terminal: &DisplayViewSurfaceProcessor,
    ) -> Result<Vec<u8>, LinearWorkingImageError> {
        validate_working_to_output(&self.identity, terminal.display_view.as_ref())?;
        let mut straight = self
            .pixels
            .to_straight_rgba_f32(terminal.display_view.as_ref())?;
        let rgb_bytes = straight
            .len()
            .checked_mul(std::mem::size_of::<[f32; 3]>())
            .ok_or(LinearWorkingImageError::DimensionOverflow {
                width: self.pixels.width(),
                height: self.pixels.height(),
            })?;
        let mut rgb = Vec::new();
        rgb.try_reserve_exact(straight.len())
            .map_err(|_| LinearWorkingImageError::AllocationFailed { bytes: rgb_bytes })?;
        rgb.extend(straight.iter().map(|pixel| [pixel[0], pixel[1], pixel[2]]));
        terminal
            .view_output_to_surface
            .transform_rgb_f32_in_place(&mut rgb)?;
        crate::image::validate_rgb_pixels(&rgb)?;
        for (pixel, transformed) in straight.iter_mut().zip(rgb) {
            if pixel[3] == 0.0 {
                *pixel = [0.0; 4];
            } else {
                pixel[..3].copy_from_slice(&transformed);
            }
        }
        crate::image::pack_straight_rgba8(self.pixels.width(), self.pixels.height(), &straight)
    }

    pub fn composite_source_over(&mut self, source: &Self) -> Result<(), LinearWorkingImageError> {
        if self.identity != source.identity {
            return Err(LinearWorkingImageError::WorkingIdentityMismatch {
                background: Box::new(self.identity.clone()),
                source: Box::new(source.identity.clone()),
            });
        }
        self.pixels.composite_source_over(&source.pixels)
    }
}

pub(crate) fn validate_source_direction(
    processor: &dyn CpuColorProcessor,
) -> Result<(), ColorManagementError> {
    let key = processor.compiled_transform_identity().cache_key();
    if key.purpose != TransformPurpose::SourceToWorking {
        return Err(processor_contract_error(
            "source-to-working conversion",
            format!(
                "compiled purpose is {:?}, expected SourceToWorking",
                key.purpose
            ),
        ));
    }
    if !matches!(key.spec, TransformSpec::ColorSpace { .. }) {
        return Err(processor_contract_error(
            "source-to-working conversion",
            "compiled transform is not a color-space transform",
        ));
    }
    Ok(())
}

pub(crate) fn validate_output_direction(
    processor: &dyn CpuColorProcessor,
) -> Result<(), ColorManagementError> {
    let key = processor.compiled_transform_identity().cache_key();
    if !matches!(
        key.purpose,
        TransformPurpose::WorkingToDisplay | TransformPurpose::WorkingToOutput
    ) {
        return Err(processor_contract_error(
            "working-to-display/output conversion",
            format!(
                "compiled purpose is {:?}, expected WorkingToDisplay or WorkingToOutput",
                key.purpose
            ),
        ));
    }
    Ok(())
}

fn validate_source_to_working(
    working: &WorkingColorIdentity,
    source: &VerifiedSourceSpace,
    processor: &dyn CpuColorProcessor,
) -> Result<(), ColorManagementError> {
    validate_source_direction(processor)?;
    if !working.verified().has_same_backend_context(source) {
        return Err(processor_contract_error(
            "source-to-working conversion",
            format!(
                "resolved source backend {:?}/'{}'/'{}'/{} does not match working backend {:?}/'{}'/'{}'/{}",
                source.backend_build(),
                source.backend_id(),
                source.backend_config_fingerprint(),
                source.context().fingerprint(),
                working.backend_build(),
                working.backend_id(),
                working.backend_config_fingerprint(),
                working.context().fingerprint(),
            ),
        ));
    }
    let compiled = processor.compiled_transform_identity();
    validate_backend_and_context(working, compiled.backend_build(), compiled.cache_key())?;
    let TransformSpec::ColorSpace {
        source: processor_source,
        destination,
    } = &compiled.cache_key().spec
    else {
        return Err(processor_contract_error(
            "source-to-working conversion",
            "compiled transform is not a color-space transform",
        ));
    };
    if processor_source != source.color_space_id() {
        return Err(processor_contract_error(
            "source-to-working conversion",
            format!(
                "processor source is '{processor_source}', but resolved source is '{}'",
                source.color_space_id()
            ),
        ));
    }
    if destination != working.working_space() {
        return Err(processor_contract_error(
            "source-to-working conversion",
            format!(
                "processor destination is '{destination}', but image working space is '{}'",
                working.working_space()
            ),
        ));
    }
    Ok(())
}

fn validate_working_to_output(
    working: &WorkingColorIdentity,
    processor: &dyn CpuColorProcessor,
) -> Result<(), ColorManagementError> {
    validate_output_direction(processor)?;
    let compiled = processor.compiled_transform_identity();
    validate_backend_and_context(working, compiled.backend_build(), compiled.cache_key())?;
    let source = match &compiled.cache_key().spec {
        TransformSpec::ColorSpace { source, .. } | TransformSpec::DisplayView { source, .. } => {
            source
        }
    };
    if source != working.working_space() {
        return Err(processor_contract_error(
            "working-to-display/output conversion",
            format!(
                "processor source is '{source}', but image working space is '{}'",
                working.working_space()
            ),
        ));
    }
    Ok(())
}

fn validate_backend_and_context(
    working: &WorkingColorIdentity,
    processor_build: BackendBuild,
    key: &ProcessorCacheKey,
) -> Result<(), ColorManagementError> {
    if processor_build != working.backend_build()
        || key.backend_id != working.backend_id()
        || key.config_fingerprint != working.backend_config_fingerprint()
    {
        return Err(processor_contract_error(
            "working-image color conversion",
            format!(
                "processor backend {:?}/'{}'/'{}' does not match working backend {:?}/'{}'/'{}'",
                processor_build,
                key.backend_id,
                key.config_fingerprint,
                working.backend_build(),
                working.backend_id(),
                working.backend_config_fingerprint(),
            ),
        ));
    }
    if &key.context != working.context() {
        return Err(processor_contract_error(
            "working-image color conversion",
            format!(
                "processor context {} does not match working context {}",
                key.context.fingerprint(),
                working.context().fingerprint()
            ),
        ));
    }
    Ok(())
}

fn processor_contract_error(
    operation: &'static str,
    detail: impl Into<String>,
) -> ColorManagementError {
    ColorManagementError::ProcessorContractMismatch {
        operation,
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BuiltinColorTransform, ColorTransformBackend, CompiledTransformIdentity,
        LINEAR_SRGB_SPACE_ID,
    };

    struct TestProcessor {
        identity: CompiledTransformIdentity,
        invalid_bulk_output: bool,
    }

    impl crate::transform::sealed::Processor for TestProcessor {}

    impl CpuColorProcessor for TestProcessor {
        fn compiled_transform_identity(&self) -> &CompiledTransformIdentity {
            &self.identity
        }

        fn transform_rgb(&self, rgb: [f64; 3]) -> Result<[f64; 3], ColorManagementError> {
            Ok(rgb)
        }

        fn transform_rgb_f32_in_place(
            &self,
            pixels: &mut [[f32; 3]],
        ) -> Result<(), ColorManagementError> {
            if self.invalid_bulk_output
                && let Some(pixel) = pixels.first_mut()
            {
                pixel[0] = f32::NAN;
            }
            Ok(())
        }
    }

    #[test]
    fn display_surface_terminal_rejects_non_finite_second_stage_output() {
        let backend = BuiltinColorTransform;
        let context = ColorContext::default();
        let verified = backend
            .verify_working_space(LINEAR_SRGB_SPACE_ID, &context)
            .expect("built-in linear working space");
        let working = WorkingColorIdentity::from_verified("test-project", verified)
            .expect("working identity");
        let pixels =
            LinearWorkingImage::from_premultiplied_rgba_f32(1, 1, vec![[0.25, 0.25, 0.25, 1.0]])
                .expect("working pixels");
        // SAFETY: the fixture pixels are authored directly in linear-sRGB.
        let image =
            unsafe { ManagedLinearWorkingImage::from_working_pixels_unchecked(working, pixels) };

        let processor = |purpose, spec, program, invalid_bulk_output| TestProcessor {
            identity: CompiledTransformIdentity::new(
                backend.build(),
                ProcessorCacheKey {
                    backend_id: backend.backend_id().to_string(),
                    config_fingerprint: backend.config_fingerprint(),
                    purpose,
                    spec,
                    context: context.clone(),
                },
                program,
            )
            .expect("test processor identity"),
            invalid_bulk_output,
        };
        let view = processor(
            TransformPurpose::WorkingToDisplay,
            TransformSpec::DisplayView {
                source: LINEAR_SRGB_SPACE_ID.to_string(),
                display: "fixture-display".to_string(),
                view: "fixture-view".to_string(),
                looks_bypass: false,
                data_bypass: false,
            },
            "fixture-view-program",
            false,
        );
        let surface = processor(
            TransformPurpose::Explicit,
            TransformSpec::ColorSpace {
                source: "fixture-view-output".to_string(),
                destination: "fixture-srgb-surface".to_string(),
            },
            "fixture-surface-program",
            true,
        );
        let terminal = DisplayViewSurfaceProcessor::new(
            Box::new(view),
            "fixture-view-output",
            "fixture-srgb-surface",
            Box::new(surface),
        )
        .expect("valid chained terminal contract");

        assert!(matches!(
            image.to_straight_rgba8_via_display_surface(&terminal),
            Err(LinearWorkingImageError::NonFiniteComponent {
                pixel_index: 0,
                component: "r"
            })
        ));
    }
}
