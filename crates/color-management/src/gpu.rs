use crate::{
    AlphaRepresentation, ColorManagementError, CompiledTransformIdentity, ComponentStorage,
    TransformPurpose, WorkingColorIdentity,
};

/// Shader language of an extracted immutable color program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

/// Required host behavior when either an input or transformed component is
/// outside the processor's finite/domain contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GpuInvalidPixelPolicy {
    RejectFrame,
}

/// Numeric boundary of the extracted pure RGB program.
///
/// The program accepts and returns straight RGB. A renderer whose authoritative
/// working surface is premultiplied must validate RGBA and alpha, canonicalize
/// transparent pixels, and unpremultiply before invoking it. Terminal packing
/// then preserves alpha as straight output. Both the domain function and a
/// finite-result check are mandatory; a failed pixel rejects the whole frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GpuTransformPixelContract {
    pub input_alpha: AlphaRepresentation,
    pub output_alpha: AlphaRepresentation,
    pub component_storage: ComponentStorage,
    pub invalid_pixel_policy: GpuInvalidPixelPolicy,
}

impl GpuTransformPixelContract {
    pub const STRAIGHT_F32_REJECT_FRAME: Self = Self {
        input_alpha: AlphaRepresentation::Straight,
        output_alpha: AlphaRepresentation::Straight,
        component_storage: ComponentStorage::Float32,
        invalid_pixel_policy: GpuInvalidPixelPolicy::RejectFrame,
    };
}

/// Immutable GPU form of the exact backend processor identified by
/// [`CompiledTransformIdentity`].
///
/// `source` is a declaration-only shader fragment. It defines the named pure
/// `vec3` transform and `bool(vec3)` domain functions but no shader entry point
/// such as `main`, so the renderer remains the sole owner of dispatch, alpha,
/// validation, and publication.
#[derive(Clone, Debug, PartialEq)]
pub struct GpuColorTransform {
    identity: CompiledTransformIdentity,
    language: GpuShaderLanguage,
    source: String,
    entry_point: String,
    domain_entry_point: String,
    input_color_space: String,
    output_color_space: String,
    luts: Vec<GpuLut3d>,
    pixel_contract: GpuTransformPixelContract,
}

pub(crate) struct GpuShaderProgram {
    pub(crate) source: String,
    pub(crate) entry_point: String,
    pub(crate) domain_entry_point: String,
    pub(crate) luts: Vec<GpuLut3d>,
}

impl GpuColorTransform {
    pub(crate) fn new(
        identity: CompiledTransformIdentity,
        language: GpuShaderLanguage,
        input_color_space: String,
        output_color_space: String,
        program: GpuShaderProgram,
    ) -> Result<Self, ColorManagementError> {
        if program.source.trim().is_empty()
            || program.entry_point.trim().is_empty()
            || program.domain_entry_point.trim().is_empty()
        {
            return Err(contract_error("GPU shader declarations are incomplete"));
        }
        if input_color_space.trim().is_empty() || output_color_space.trim().is_empty() {
            return Err(ColorManagementError::EmptyColorSpace);
        }
        match &identity.cache_key().spec {
            crate::TransformSpec::ColorSpace {
                source,
                destination,
            } if source == &input_color_space && destination == &output_color_space => {}
            crate::TransformSpec::DisplayView { source, .. } if source == &input_color_space => {}
            _ => {
                return Err(contract_error(
                    "GPU stage color spaces do not match its compiled identity",
                ));
            }
        }
        Ok(Self {
            identity,
            language,
            source: program.source,
            entry_point: program.entry_point,
            domain_entry_point: program.domain_entry_point,
            input_color_space,
            output_color_space,
            luts: program.luts,
            pixel_contract: GpuTransformPixelContract::STRAIGHT_F32_REJECT_FRAME,
        })
    }

    pub const fn compiled_transform_identity(&self) -> &CompiledTransformIdentity {
        &self.identity
    }

    pub const fn language(&self) -> GpuShaderLanguage {
        self.language
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn entry_point(&self) -> &str {
        &self.entry_point
    }

    pub fn domain_entry_point(&self) -> &str {
        &self.domain_entry_point
    }

    pub fn input_color_space(&self) -> &str {
        &self.input_color_space
    }

    pub fn output_color_space(&self) -> &str {
        &self.output_color_space
    }

    pub fn luts(&self) -> &[GpuLut3d] {
        &self.luts
    }

    pub const fn pixel_contract(&self) -> GpuTransformPixelContract {
        self.pixel_contract
    }
}

/// A fully supported terminal chain rooted in one exact Project working-color
/// identity. Partial chains are rejected at construction, so callers may only
/// select this path when every CPU terminal stage has an identical GPU owner.
#[derive(Clone, Debug, PartialEq)]
pub struct GpuTerminalChain {
    working_identity: WorkingColorIdentity,
    stages: Vec<GpuColorTransform>,
    language: GpuShaderLanguage,
}

impl GpuTerminalChain {
    pub fn new(
        working_identity: WorkingColorIdentity,
        stages: Vec<GpuColorTransform>,
    ) -> Result<Self, ColorManagementError> {
        let Some(first) = stages.first() else {
            return Err(contract_error("GPU terminal chain has no stages"));
        };
        if working_identity.alpha() != AlphaRepresentation::Premultiplied {
            return Err(contract_error(
                "GPU terminal working identity is not premultiplied",
            ));
        }
        if first.input_color_space() != working_identity.working_space() {
            return Err(contract_error(
                "GPU terminal chain does not start in the Project working space",
            ));
        }
        let language = first.language();
        let mut previous_output: Option<&str> = None;
        for (index, stage) in stages.iter().enumerate() {
            validate_stage_owner(&working_identity, stage)?;
            if stage.language() != language {
                return Err(contract_error("GPU terminal chain mixes shader languages"));
            }
            if stage.pixel_contract() != GpuTransformPixelContract::STRAIGHT_F32_REJECT_FRAME {
                return Err(contract_error(
                    "GPU terminal stage has an incompatible pixel contract",
                ));
            }
            if let Some(expected_input) = previous_output
                && stage.input_color_space() != expected_input
            {
                return Err(contract_error(
                    "GPU terminal stages have a color-space discontinuity",
                ));
            }
            let purpose = stage.compiled_transform_identity().cache_key().purpose;
            let valid_purpose = if index == 0 {
                matches!(
                    purpose,
                    TransformPurpose::WorkingToDisplay | TransformPurpose::WorkingToOutput
                )
            } else {
                purpose == TransformPurpose::Explicit
            };
            if !valid_purpose {
                return Err(contract_error(
                    "GPU terminal stage has an incompatible transform purpose",
                ));
            }
            previous_output = Some(stage.output_color_space());
        }
        Ok(Self {
            working_identity,
            stages,
            language,
        })
    }

    pub const fn working_identity(&self) -> &WorkingColorIdentity {
        &self.working_identity
    }

    pub fn stages(&self) -> &[GpuColorTransform] {
        &self.stages
    }

    pub const fn language(&self) -> GpuShaderLanguage {
        self.language
    }
}

fn validate_stage_owner(
    working_identity: &WorkingColorIdentity,
    stage: &GpuColorTransform,
) -> Result<(), ColorManagementError> {
    let identity = stage.compiled_transform_identity();
    let key = identity.cache_key();
    if identity.backend_build() != working_identity.backend_build()
        || key.backend_id != working_identity.backend_id()
        || key.config_fingerprint != working_identity.backend_config_fingerprint()
        || &key.context != working_identity.context()
    {
        return Err(contract_error(
            "GPU terminal stage does not share the Project color owner",
        ));
    }
    Ok(())
}

fn contract_error(detail: &'static str) -> ColorManagementError {
    ColorManagementError::ProcessorContractMismatch {
        operation: "GPU terminal chain",
        detail: detail.to_string(),
    }
}
