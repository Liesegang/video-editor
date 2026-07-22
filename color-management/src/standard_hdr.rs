use crate::{
    ColorContext, ColorManagementError, TransformPurpose,
    standard_spaces::{StandardColorSpaceMetadata, StandardTransfer},
};

/// Diffuse/reference white used to normalize absolute PQ luminance.
pub const REFERENCE_WHITE_NITS_CONTEXT_KEY: &str = "RUVIE_REFERENCE_WHITE_NITS";
/// Required opt-in policy for the deliberately limited built-in PQ path.
pub const PQ_LINEARIZATION_POLICY_CONTEXT_KEY: &str = "RUVIE_PQ_LINEARIZATION_POLICY";
/// Treat PQ as reference-white-relative display luminance, not recovered scene light.
pub const RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY: &str = "relative-display-luminance";

const ST2084_MAX_NITS: f64 = 10_000.0;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct HdrTransformContext {
    pq_reference_white_nits: Option<f64>,
}

impl HdrTransformContext {
    pub(crate) fn compile(
        source: StandardColorSpaceMetadata,
        destination: StandardColorSpaceMetadata,
        purpose: TransformPurpose,
        context: &ColorContext,
    ) -> Result<Self, ColorManagementError> {
        if source.id == destination.id {
            return Ok(Self::default());
        }
        validate_hdr_purpose(source, destination, purpose)?;
        let uses_pq = is_pq(source.transfer) || is_pq(destination.transfer);
        let pq_reference_white_nits = if uses_pq {
            let pq_space = if is_pq(source.transfer) {
                source
            } else {
                destination
            };
            Some(validate_pq_context(pq_space, context)?)
        } else {
            None
        };
        Ok(Self {
            pq_reference_white_nits,
        })
    }

    pub(crate) fn decode_if_hdr(
        self,
        transfer: StandardTransfer,
        rgb: [f64; 3],
    ) -> Result<Option<[f64; 3]>, ColorManagementError> {
        match transfer {
            StandardTransfer::St2084Pq => {
                let reference_white_nits = self.required_pq_reference_white()?;
                let mut relative_display_luminance = [0.0; 3];
                for (output, value) in relative_display_luminance.iter_mut().zip(rgb) {
                    *output = st2084_to_nits(value)? / reference_white_nits;
                }
                Ok(Some(relative_display_luminance))
            }
            StandardTransfer::Bt2100Hlg => Ok(Some(rgb.map(hlg_inverse_oetf))),
            _ => Ok(None),
        }
    }

    pub(crate) fn encode_if_hdr(
        self,
        transfer: StandardTransfer,
        relative_rgb: [f64; 3],
    ) -> Result<Option<[f64; 3]>, ColorManagementError> {
        match transfer {
            StandardTransfer::St2084Pq => {
                let reference_white_nits = self.required_pq_reference_white()?;
                let mut encoded = [0.0; 3];
                for (output, value) in encoded.iter_mut().zip(relative_rgb) {
                    *output = nits_to_st2084(value * reference_white_nits)?;
                }
                Ok(Some(encoded))
            }
            StandardTransfer::Bt2100Hlg => Ok(Some(relative_rgb.map(hlg_oetf))),
            _ => Ok(None),
        }
    }

    fn required_pq_reference_white(self) -> Result<f64, ColorManagementError> {
        self.pq_reference_white_nits
            .ok_or_else(internal_context_error)
    }

    pub(crate) const fn program_id_suffix(self) -> &'static str {
        if self.pq_reference_white_nits.is_some() {
            ":pq-relative-display-luminance"
        } else {
            ""
        }
    }
}

pub(crate) fn validate_standard_space_context(
    space: StandardColorSpaceMetadata,
    context: &ColorContext,
) -> Result<(), ColorManagementError> {
    if is_pq(space.transfer) {
        validate_pq_context(space, context)?;
    }
    Ok(())
}

fn validate_hdr_purpose(
    source: StandardColorSpaceMetadata,
    destination: StandardColorSpaceMetadata,
    purpose: TransformPurpose,
) -> Result<(), ColorManagementError> {
    if is_pq(source.transfer) || is_pq(destination.transfer) {
        let supported = matches!(purpose, TransformPurpose::SourceToWorking)
            && is_pq(source.transfer)
            || matches!(purpose, TransformPurpose::WorkingToOutput) && is_pq(destination.transfer);
        if !supported {
            return Err(unsupported(source, destination));
        }
    }
    if is_hlg(source.transfer) || is_hlg(destination.transfer) {
        let supported = matches!(purpose, TransformPurpose::SourceToWorking)
            && is_hlg(source.transfer)
            || matches!(purpose, TransformPurpose::WorkingToOutput) && is_hlg(destination.transfer);
        if !supported {
            return Err(unsupported(source, destination));
        }
    }
    Ok(())
}

fn unsupported(
    source: StandardColorSpaceMetadata,
    destination: StandardColorSpaceMetadata,
) -> ColorManagementError {
    ColorManagementError::UnsupportedTransform {
        source: source.id.as_str().to_string(),
        target: destination.id.as_str().to_string(),
    }
}

const fn is_pq(transfer: StandardTransfer) -> bool {
    matches!(transfer, StandardTransfer::St2084Pq)
}

const fn is_hlg(transfer: StandardTransfer) -> bool {
    matches!(transfer, StandardTransfer::Bt2100Hlg)
}

fn validate_pq_context(
    space: StandardColorSpaceMetadata,
    context: &ColorContext,
) -> Result<f64, ColorManagementError> {
    let policy = context
        .variables()
        .get(PQ_LINEARIZATION_POLICY_CONTEXT_KEY)
        .ok_or_else(|| ColorManagementError::MissingContextVariable {
            variable: PQ_LINEARIZATION_POLICY_CONTEXT_KEY,
            space: space.id.as_str().to_string(),
        })?;
    if policy != RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY {
        return Err(ColorManagementError::InvalidContextVariable {
            variable: PQ_LINEARIZATION_POLICY_CONTEXT_KEY,
            value: policy.clone(),
            reason: "unsupported PQ linearization policy",
        });
    }
    let reference_white_nits =
        context_positive(context, REFERENCE_WHITE_NITS_CONTEXT_KEY, space.id.as_str())?;
    if reference_white_nits > ST2084_MAX_NITS {
        return Err(invalid_context(
            REFERENCE_WHITE_NITS_CONTEXT_KEY,
            reference_white_nits,
            "must not exceed 10000 nits",
        ));
    }
    Ok(reference_white_nits)
}

fn context_positive(
    context: &ColorContext,
    variable: &'static str,
    space: &str,
) -> Result<f64, ColorManagementError> {
    let raw = context.variables().get(variable).ok_or_else(|| {
        ColorManagementError::MissingContextVariable {
            variable,
            space: space.to_string(),
        }
    })?;
    let parsed = raw
        .parse::<f64>()
        .map_err(|_| ColorManagementError::InvalidContextVariable {
            variable,
            value: raw.clone(),
            reason: "must be a finite positive number",
        })?;
    if !parsed.is_finite() || parsed <= 0.0 {
        return Err(ColorManagementError::InvalidContextVariable {
            variable,
            value: raw.clone(),
            reason: "must be a finite positive number",
        });
    }
    Ok(parsed)
}

fn invalid_context(
    variable: &'static str,
    value: f64,
    reason: &'static str,
) -> ColorManagementError {
    ColorManagementError::InvalidContextVariable {
        variable,
        value: value.to_string(),
        reason,
    }
}

fn internal_context_error() -> ColorManagementError {
    ColorManagementError::ProcessorContractMismatch {
        operation: "PQ color transform",
        detail: "compiled PQ context is incomplete".to_string(),
    }
}

fn st2084_to_nits(code: f64) -> Result<f64, ColorManagementError> {
    const M1: f64 = 2610.0 / 16_384.0;
    const M2: f64 = (2523.0 / 4096.0) * 128.0;
    const C1: f64 = 3424.0 / 4096.0;
    const C2: f64 = (2413.0 / 4096.0) * 32.0;
    const C3: f64 = (2392.0 / 4096.0) * 32.0;

    if !(0.0..=1.0).contains(&code) {
        return Err(ColorManagementError::InvalidTransferDomain {
            transfer: "ST 2084 PQ",
            reason: "encoded value must be between 0 and 1 inclusive",
        });
    }
    let magnitude = code.powf(1.0 / M2);
    let denominator = C2 - C3 * magnitude;
    let normalized = ((magnitude - C1).max(0.0) / denominator).powf(1.0 / M1);
    Ok(ST2084_MAX_NITS * normalized)
}

fn nits_to_st2084(nits: f64) -> Result<f64, ColorManagementError> {
    const M1: f64 = 2610.0 / 16_384.0;
    const M2: f64 = (2523.0 / 4096.0) * 128.0;
    const C1: f64 = 3424.0 / 4096.0;
    const C2: f64 = (2413.0 / 4096.0) * 32.0;
    const C3: f64 = (2392.0 / 4096.0) * 32.0;

    if !(0.0..=ST2084_MAX_NITS).contains(&nits) {
        return Err(ColorManagementError::InvalidTransferDomain {
            transfer: "ST 2084 PQ",
            reason: "absolute luminance must be between 0 and 10000 nits inclusive",
        });
    }
    let normalized = (nits / ST2084_MAX_NITS).powf(M1);
    Ok(((C1 + C2 * normalized) / (1.0 + C3 * normalized)).powf(M2))
}

fn hlg_inverse_oetf(signal: f64) -> f64 {
    // BT.2100 defines the nominal non-negative branch. The working pipeline
    // deliberately uses an odd-symmetric extension and leaves >1 headroom
    // unclipped so grading and graph math do not destroy recoverable values.
    signed_transfer(signal, |magnitude| {
        const A: f64 = 0.178_832_77;
        const B: f64 = 0.284_668_92;
        const C: f64 = 0.559_910_73;
        if magnitude <= 0.5 {
            magnitude * magnitude / 3.0
        } else {
            (((magnitude - C) / A).exp() + B) / 12.0
        }
    })
}

fn hlg_oetf(scene: f64) -> f64 {
    signed_transfer(scene, |magnitude| {
        const A: f64 = 0.178_832_77;
        const B: f64 = 0.284_668_92;
        const C: f64 = 0.559_910_73;
        if magnitude <= 1.0 / 12.0 {
            (3.0 * magnitude).sqrt()
        } else {
            A * (12.0 * magnitude - B).ln() + C
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
    use super::*;

    fn assert_near(actual: f64, expected: f64, tolerance: f64) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "actual {actual}, expected {expected}"
        );
    }

    #[test]
    fn st2084_matches_itu_absolute_luminance_vectors_and_rejects_extensions() {
        assert_near(
            st2084_to_nits(0.508_078_421_517_399).unwrap(),
            100.0,
            1.0e-8,
        );
        assert_near(
            st2084_to_nits(0.751_827_096_247_041).unwrap(),
            1000.0,
            1.0e-7,
        );
        assert_near(nits_to_st2084(10_000.0).unwrap(), 1.0, 1.0e-12);
        assert!(st2084_to_nits(-0.01).is_err());
        assert!(st2084_to_nits(1.01).is_err());
        assert!(nits_to_st2084(-0.01).is_err());
        assert!(nits_to_st2084(10_000.01).is_err());
    }

    #[test]
    fn hlg_scene_oetf_matches_itu_piecewise_vectors_without_an_ootf() {
        assert_near(hlg_oetf(1.0 / 12.0), 0.5, 1.0e-12);
        assert_near(hlg_inverse_oetf(0.5), 1.0 / 12.0, 1.0e-12);
        assert_near(hlg_inverse_oetf(0.75), 0.264_962_559_786_400_15, 1.0e-12);
        assert_near(hlg_inverse_oetf(-0.75), -0.264_962_559_786_400_15, 1.0e-12);
        let extended = hlg_inverse_oetf(1.25);
        assert!(extended > 1.0);
        assert_near(hlg_oetf(extended), 1.25, 1.0e-12);
    }
}
