use crate::model::ColorConfigIdentity;
use crate::model::asset::{SourceColorDescription, SourceColorRange, SourceMatrixCoefficients};
use thiserror::Error;

/// A color-space name together with the exact configuration that owns it.
/// A bare name is not globally meaningful in OpenColorIO and must never cross
/// the decoded-pixel boundary without this identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigOwnedColorSpace {
    config: ColorConfigIdentity,
    name: String,
}

impl ConfigOwnedColorSpace {
    /// Constructs a color-space identity from the exact persisted config
    /// resource identity. Preview, export, and working-space settings are not
    /// part of ownership because changing them cannot rename this space.
    pub fn new(
        config: ColorConfigIdentity,
        name: impl Into<String>,
    ) -> Result<Self, ConfigOwnedColorSpaceError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(ConfigOwnedColorSpaceError::BlankName);
        }
        Ok(Self { config, name })
    }

    pub fn config(&self) -> &ColorConfigIdentity {
        &self.config
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ConfigOwnedColorSpaceError {
    #[error("config-owned color-space name must not be blank")]
    BlankName,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodedColorSpace {
    /// Versioned Loader ABI v1 contract and other proven straight sRGB output.
    Srgb,
    /// An untagged still image interpreted as sRGB by an explicit, versioned
    /// loader policy. This is deliberately distinct from [`Self::Srgb`]: the
    /// assumption remains inspectable and an authored Project interpretation
    /// always takes precedence at managed ingress.
    AssumedSrgb(UntaggedSrgbAssumption),
    /// Samples retain transfer/primaries and original source precision after
    /// any explicitly recorded YUV matrix/range expansion. Project overrides
    /// may replace color interpretation, but never erase precision provenance.
    SourceEncoded(SourceColorDescription),
    /// A loader explicitly transformed samples into a space owned by the
    /// indicated exact color configuration.
    ConfigOwned(ConfigOwnedColorSpace),
    /// The loader cannot prove how its output samples should be interpreted.
    /// The reason preserves provenance for a fail-closed managed ingress.
    Unknown { reason: String },
}

/// Stable identity of the policy that supplied a missing still-image color
/// interpretation. Adding or changing assumptions requires a new variant and
/// policy id; existing decoded provenance must never silently change meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UntaggedSrgbPolicy {
    NativeStillImageV1,
}

impl UntaggedSrgbPolicy {
    pub const fn id(self) -> &'static str {
        match self {
            Self::NativeStillImageV1 => "ruvie.native-image.untagged-srgb/v1",
        }
    }
}

/// Provenance retained when a native still-image decoder applies the standard
/// untagged-image sRGB convention. The detected description is preserved so a
/// stale Project probe or a precision-losing decode can still be rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UntaggedSrgbAssumption {
    policy: UntaggedSrgbPolicy,
    detected_source: SourceColorDescription,
}

impl UntaggedSrgbAssumption {
    pub fn policy(&self) -> UntaggedSrgbPolicy {
        self.policy
    }

    pub fn detected_source(&self) -> &SourceColorDescription {
        &self.detected_source
    }

    pub fn diagnostic(&self) -> String {
        format!(
            "untagged still image interpreted as sRGB by policy '{}' (detected bit depth {:?})",
            self.policy.id(),
            self.detected_source.bit_depth
        )
    }
}

/// Exact source-domain operation that produced full-range RGB samples.
///
/// Construction is crate-private so a loader cannot manufacture the verified
/// state without going through [`DecodedPixelDescription`]'s checked boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedYuvToRgb {
    operation: YuvToRgbOperation,
    chroma_location: Option<AppliedYuvChromaLocation>,
    source_matrix: SourceMatrixCoefficients,
    source_range: SourceColorRange,
}

impl AppliedYuvToRgb {
    pub fn operation(&self) -> YuvToRgbOperation {
        self.operation
    }

    pub fn chroma_location(&self) -> Option<AppliedYuvChromaLocation> {
        self.chroma_location
    }

    pub fn source_matrix(&self) -> &SourceMatrixCoefficients {
        &self.source_matrix
    }

    pub fn source_range(&self) -> &SourceColorRange {
        &self.source_range
    }
}

/// Chroma sample location consumed by a subsampled YUV conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppliedYuvChromaLocation {
    location: YuvChromaLocation,
    source: YuvChromaLocationSource,
}

impl AppliedYuvChromaLocation {
    pub(crate) const fn new(location: YuvChromaLocation, source: YuvChromaLocationSource) -> Self {
        Self { location, source }
    }

    pub const fn location(self) -> YuvChromaLocation {
        self.location
    }

    pub const fn source(self) -> YuvChromaLocationSource {
        self.source
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YuvChromaLocation {
    Left,
    Center,
    TopLeft,
    Top,
    BottomLeft,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YuvChromaLocationSource {
    Frame,
    Decoder,
}

/// Versioned implementation that expanded source YUV code values into RGB.
///
/// Managed color ingress records this operation explicitly so an unclamped
/// RuViE floating-point conversion cannot be confused with an opaque decoder
/// or libswscale conversion with different precision or clipping behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YuvToRgbOperation {
    /// Descriptor-driven planar integer YUV to straight RGBA32F.
    H273PlanarF32V1,
}

impl YuvToRgbOperation {
    pub const fn id(self) -> &'static str {
        match self {
            Self::H273PlanarF32V1 => "ruvie.h273-planar-yuv-to-rgb-f32-v1",
        }
    }
}

/// Whether decoded pixels are proven to be full-range RGB.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodedRgbConversion {
    /// The loader input was already full-range RGB (for example a PNG decode
    /// or the versioned native loader ABI).
    AlreadyFullRangeRgb,
    /// A decoder adapter applied this exact YUV matrix/range expansion.
    AppliedYuvToFullRangeRgb(AppliedYuvToRgb),
    /// Legacy pixels may still be displayed, but managed ingress must reject
    /// them because the conversion cannot be proved.
    Unverified { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedPixelDescription {
    color_space: DecodedColorSpace,
    rgb_conversion: DecodedRgbConversion,
}

impl DecodedPixelDescription {
    pub fn full_range_rgb(color_space: DecodedColorSpace) -> Self {
        let color_space = match color_space {
            DecodedColorSpace::SourceEncoded(source)
                if source.matrix.is_some() || source.range.is_some() =>
            {
                return Self::unverified(
                    "full-range RGB loader output retained unconsumed source matrix/range metadata",
                );
            }
            other => sanitize_rgb_color_space(other),
        };
        Self {
            color_space,
            rgb_conversion: DecodedRgbConversion::AlreadyFullRangeRgb,
        }
    }

    pub(crate) fn already_full_range_rgb_from_source(mut source: SourceColorDescription) -> Self {
        if source.matrix != Some(SourceMatrixCoefficients::Identity)
            || source.range != Some(SourceColorRange::Full)
        {
            return Self::unverified(
                "decoded RGB source did not prove identity matrix and full range",
            );
        }
        source.matrix = None;
        source.range = None;
        Self {
            color_space: DecodedColorSpace::SourceEncoded(source),
            rgb_conversion: DecodedRgbConversion::AlreadyFullRangeRgb,
        }
    }

    /// Applies the native still-image policy only when the decoder has proved
    /// that no embedded/profile color identity exists and the source precision
    /// is at most 8 bits per channel. ICC-tagged, high-bit, or unknown-precision
    /// input stays source-encoded and therefore cannot be quantized and passed
    /// off as an ordinary sRGB image.
    pub(crate) fn assumed_untagged_still_srgb_v1(
        source: SourceColorDescription,
    ) -> Result<Self, UntaggedSrgbAssumptionError> {
        if source.primaries.is_some()
            || source.transfer.is_some()
            || source.matrix.is_some()
            || source.range.is_some()
            || source.profile.is_some()
        {
            return Err(UntaggedSrgbAssumptionError::TaggedSource);
        }
        match source.bit_depth {
            Some(1..=8) => {}
            Some(bit_depth) => {
                return Err(UntaggedSrgbAssumptionError::UnsupportedBitDepth(bit_depth));
            }
            None => return Err(UntaggedSrgbAssumptionError::UnknownBitDepth),
        }
        Ok(Self::full_range_rgb(DecodedColorSpace::AssumedSrgb(
            UntaggedSrgbAssumption {
                policy: UntaggedSrgbPolicy::NativeStillImageV1,
                detected_source: source,
            },
        )))
    }

    /// Records the exact matrix/range operation performed by a decoder adapter.
    /// The encoded description retained on the RGB samples contains only
    /// transfer/primaries/profile and source-precision provenance. Matrix and
    /// range alone have been consumed by this operation.
    pub(crate) fn applied_yuv_to_full_range_rgb(
        mut source: SourceColorDescription,
        operation: YuvToRgbOperation,
        chroma_location: Option<AppliedYuvChromaLocation>,
        source_matrix: SourceMatrixCoefficients,
        source_range: SourceColorRange,
    ) -> Self {
        if source.matrix.as_ref() != Some(&source_matrix)
            || source.range.as_ref() != Some(&source_range)
        {
            return Self::unverified(
                "applied YUV conversion does not match the decoded source matrix/range metadata",
            );
        }
        source.matrix = None;
        source.range = None;
        Self {
            color_space: DecodedColorSpace::SourceEncoded(source),
            rgb_conversion: DecodedRgbConversion::AppliedYuvToFullRangeRgb(AppliedYuvToRgb {
                operation,
                chroma_location,
                source_matrix,
                source_range,
            }),
        }
    }

    /// Describes legacy output whose matrix/range conversion was not verified.
    /// Keeping both the color identity and conversion state unverified makes it
    /// impossible for a managed consumer to accept the pixels accidentally.
    pub(crate) fn unverified(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            color_space: DecodedColorSpace::Unknown {
                reason: reason.clone(),
            },
            rgb_conversion: DecodedRgbConversion::Unverified { reason },
        }
    }

    pub fn abi_v1_srgb() -> Self {
        Self::full_range_rgb(DecodedColorSpace::Srgb)
    }

    pub fn color_space(&self) -> &DecodedColorSpace {
        &self.color_space
    }

    pub fn rgb_conversion(&self) -> &DecodedRgbConversion {
        &self.rgb_conversion
    }

    pub fn rgb_matrix_applied(&self) -> bool {
        !matches!(self.rgb_conversion, DecodedRgbConversion::Unverified { .. })
    }

    pub fn full_range(&self) -> bool {
        !matches!(self.rgb_conversion, DecodedRgbConversion::Unverified { .. })
    }
}

fn sanitize_rgb_color_space(color_space: DecodedColorSpace) -> DecodedColorSpace {
    match color_space {
        DecodedColorSpace::SourceEncoded(mut source) => {
            source.matrix = None;
            source.range = None;
            DecodedColorSpace::SourceEncoded(source)
        }
        other => other,
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum UntaggedSrgbAssumptionError {
    #[error("untagged sRGB policy cannot replace embedded or explicit color metadata")]
    TaggedSource,
    #[error("untagged sRGB policy requires a proven source bit depth")]
    UnknownBitDepth,
    #[error("untagged sRGB policy cannot quantize a {0}-bit source to RGBA8")]
    UnsupportedBitDepth(u8),
}

#[cfg(test)]
mod tests {
    use super::{
        DecodedColorSpace, DecodedPixelDescription, DecodedRgbConversion,
        UntaggedSrgbAssumptionError, UntaggedSrgbPolicy, YuvToRgbOperation,
    };
    use crate::model::asset::{
        SourceColorDescription, SourceColorProfile, SourceColorRange, SourceMatrixCoefficients,
        SourceTransferCharacteristic,
    };

    #[test]
    fn applied_yuv_conversion_consumes_only_matrix_range_and_storage_precision() {
        let decoded = DecodedPixelDescription::applied_yuv_to_full_range_rgb(
            SourceColorDescription {
                transfer: Some(SourceTransferCharacteristic::Bt709),
                matrix: Some(SourceMatrixCoefficients::Bt709),
                range: Some(SourceColorRange::Limited),
                bit_depth: Some(10),
                ..SourceColorDescription::default()
            },
            YuvToRgbOperation::H273PlanarF32V1,
            None,
            SourceMatrixCoefficients::Bt709,
            SourceColorRange::Limited,
        );

        let DecodedRgbConversion::AppliedYuvToFullRangeRgb(applied) = decoded.rgb_conversion()
        else {
            panic!("verified conversion was not retained");
        };
        assert_eq!(applied.source_matrix(), &SourceMatrixCoefficients::Bt709);
        assert_eq!(applied.source_range(), &SourceColorRange::Limited);
        assert_eq!(applied.operation(), YuvToRgbOperation::H273PlanarF32V1);
        assert_eq!(
            applied.operation().id(),
            "ruvie.h273-planar-yuv-to-rgb-f32-v1"
        );
        let DecodedColorSpace::SourceEncoded(source) = decoded.color_space() else {
            panic!("source encoding was not retained");
        };
        assert_eq!(source.transfer, Some(SourceTransferCharacteristic::Bt709));
        assert_eq!(source.matrix, None);
        assert_eq!(source.range, None);
        assert_eq!(source.bit_depth, Some(10));
    }

    #[test]
    fn unverified_conversion_cannot_claim_full_range_rgb() {
        let decoded = DecodedPixelDescription::unverified("missing frame matrix");
        assert!(!decoded.rgb_matrix_applied());
        assert!(!decoded.full_range());
        assert!(matches!(
            decoded.color_space(),
            DecodedColorSpace::Unknown { reason } if reason == "missing frame matrix"
        ));
    }

    #[test]
    fn generic_full_range_constructor_rejects_unconsumed_yuv_metadata() {
        let decoded = DecodedPixelDescription::full_range_rgb(DecodedColorSpace::SourceEncoded(
            SourceColorDescription {
                matrix: Some(SourceMatrixCoefficients::Bt709),
                range: Some(SourceColorRange::Limited),
                ..SourceColorDescription::default()
            },
        ));

        assert!(matches!(
            decoded.rgb_conversion(),
            DecodedRgbConversion::Unverified { .. }
        ));
        assert!(matches!(
            decoded.color_space(),
            DecodedColorSpace::Unknown { .. }
        ));
    }

    #[test]
    fn untagged_still_assumption_retains_versioned_provenance() {
        let decoded =
            DecodedPixelDescription::assumed_untagged_still_srgb_v1(SourceColorDescription {
                bit_depth: Some(8),
                ..SourceColorDescription::default()
            })
            .expect("8-bit untagged still is eligible for the policy");
        let DecodedColorSpace::AssumedSrgb(assumption) = decoded.color_space() else {
            panic!("policy provenance was discarded");
        };
        assert_eq!(assumption.policy(), UntaggedSrgbPolicy::NativeStillImageV1);
        assert_eq!(
            assumption.policy().id(),
            "ruvie.native-image.untagged-srgb/v1"
        );
        assert_eq!(assumption.detected_source().bit_depth, Some(8));
        assert!(assumption.diagnostic().contains("untagged still image"));
    }

    #[test]
    fn untagged_still_assumption_rejects_profiles_and_precision_loss() {
        let tagged = SourceColorDescription {
            bit_depth: Some(8),
            profile: Some(SourceColorProfile::Other {
                profile_kind: "test".to_string(),
                identity: "tagged".to_string(),
            }),
            ..SourceColorDescription::default()
        };
        assert_eq!(
            DecodedPixelDescription::assumed_untagged_still_srgb_v1(tagged),
            Err(UntaggedSrgbAssumptionError::TaggedSource)
        );
        assert_eq!(
            DecodedPixelDescription::assumed_untagged_still_srgb_v1(SourceColorDescription {
                bit_depth: Some(16),
                ..SourceColorDescription::default()
            }),
            Err(UntaggedSrgbAssumptionError::UnsupportedBitDepth(16))
        );
        assert_eq!(
            DecodedPixelDescription::assumed_untagged_still_srgb_v1(
                SourceColorDescription::default()
            ),
            Err(UntaggedSrgbAssumptionError::UnknownBitDepth)
        );
    }
}
