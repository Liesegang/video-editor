use crate::{
    ColorLinearity, ColorManagementError, ColorReferenceSpace, ColorSpaceInfo,
    ColorTransformRequest, TransformPurpose, TransformSpec, standard_hdr::HdrTransformContext,
};

pub const SRGB_SPACE_ID: &str = "srgb";
pub const LINEAR_SRGB_SPACE_ID: &str = "linear-srgb";
pub const BT709_SPACE_ID: &str = "bt709";
pub const LINEAR_BT709_SPACE_ID: &str = "linear-bt709";
pub const DISPLAY_P3_SPACE_ID: &str = "display-p3";
pub const LINEAR_DISPLAY_P3_SPACE_ID: &str = "linear-display-p3";
/// Analytic smooth BT.2020 OETF; never inferred from a bit-depth CICP code.
pub const REC2020_SDR_EXACT_SPACE_ID: &str = "rec2020-sdr-exact";
/// BT.2020's practical 10-bit OETF coefficients (`alpha=1.099`, `beta=0.018`).
pub const REC2020_SDR_10_SPACE_ID: &str = "rec2020-sdr-10";
/// BT.2020's practical 12-bit OETF coefficients (`alpha=1.0993`, `beta=0.0181`).
pub const REC2020_SDR_12_SPACE_ID: &str = "rec2020-sdr-12";
pub const LINEAR_REC2020_SPACE_ID: &str = "linear-rec2020";
pub const REC2100_PQ_SPACE_ID: &str = "rec2100-pq";
pub const REC2100_HLG_SPACE_ID: &str = "rec2100-hlg";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StandardPrimaries {
    Bt709,
    DisplayP3,
    Rec2020,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StandardTransfer {
    Linear,
    Srgb,
    Bt709,
    Bt2020Exact,
    Bt2020TenBit,
    Bt2020TwelveBit,
    St2084Pq,
    Bt2100Hlg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StandardColorSpaceId {
    Srgb,
    LinearSrgb,
    Bt709,
    LinearBt709,
    DisplayP3,
    LinearDisplayP3,
    Rec2020SdrExact,
    Rec2020Sdr10,
    Rec2020Sdr12,
    LinearRec2020,
    Rec2100Pq,
    Rec2100Hlg,
}

impl StandardColorSpaceId {
    pub const ALL: [Self; 12] = [
        Self::Srgb,
        Self::LinearSrgb,
        Self::Bt709,
        Self::LinearBt709,
        Self::DisplayP3,
        Self::LinearDisplayP3,
        Self::Rec2020SdrExact,
        Self::Rec2020Sdr10,
        Self::Rec2020Sdr12,
        Self::LinearRec2020,
        Self::Rec2100Pq,
        Self::Rec2100Hlg,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Srgb => SRGB_SPACE_ID,
            Self::LinearSrgb => LINEAR_SRGB_SPACE_ID,
            Self::Bt709 => BT709_SPACE_ID,
            Self::LinearBt709 => LINEAR_BT709_SPACE_ID,
            Self::DisplayP3 => DISPLAY_P3_SPACE_ID,
            Self::LinearDisplayP3 => LINEAR_DISPLAY_P3_SPACE_ID,
            Self::Rec2020SdrExact => REC2020_SDR_EXACT_SPACE_ID,
            Self::Rec2020Sdr10 => REC2020_SDR_10_SPACE_ID,
            Self::Rec2020Sdr12 => REC2020_SDR_12_SPACE_ID,
            Self::LinearRec2020 => LINEAR_REC2020_SPACE_ID,
            Self::Rec2100Pq => REC2100_PQ_SPACE_ID,
            Self::Rec2100Hlg => REC2100_HLG_SPACE_ID,
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.as_str() == id)
    }

    pub const fn metadata(self) -> StandardColorSpaceMetadata {
        match self {
            Self::Srgb => metadata(
                self,
                "sRGB (encoded)",
                StandardPrimaries::Bt709,
                StandardTransfer::Srgb,
                ColorReferenceSpace::Display,
            ),
            Self::LinearSrgb => metadata(
                self,
                "Linear sRGB",
                StandardPrimaries::Bt709,
                StandardTransfer::Linear,
                ColorReferenceSpace::Scene,
            ),
            Self::Bt709 => metadata(
                self,
                "BT.709 (encoded)",
                StandardPrimaries::Bt709,
                StandardTransfer::Bt709,
                ColorReferenceSpace::Scene,
            ),
            Self::LinearBt709 => metadata(
                self,
                "Linear BT.709",
                StandardPrimaries::Bt709,
                StandardTransfer::Linear,
                ColorReferenceSpace::Scene,
            ),
            Self::DisplayP3 => metadata(
                self,
                "Display P3 (encoded)",
                StandardPrimaries::DisplayP3,
                StandardTransfer::Srgb,
                ColorReferenceSpace::Display,
            ),
            Self::LinearDisplayP3 => metadata(
                self,
                "Linear Display P3",
                StandardPrimaries::DisplayP3,
                StandardTransfer::Linear,
                ColorReferenceSpace::Scene,
            ),
            Self::Rec2020SdrExact => metadata(
                self,
                "Rec.2020 SDR exact smooth OETF",
                StandardPrimaries::Rec2020,
                StandardTransfer::Bt2020Exact,
                ColorReferenceSpace::Scene,
            ),
            Self::Rec2020Sdr10 => metadata(
                self,
                "Rec.2020 SDR 10-bit OETF",
                StandardPrimaries::Rec2020,
                StandardTransfer::Bt2020TenBit,
                ColorReferenceSpace::Scene,
            ),
            Self::Rec2020Sdr12 => metadata(
                self,
                "Rec.2020 SDR 12-bit OETF",
                StandardPrimaries::Rec2020,
                StandardTransfer::Bt2020TwelveBit,
                ColorReferenceSpace::Scene,
            ),
            Self::LinearRec2020 => metadata(
                self,
                "Linear Rec.2020",
                StandardPrimaries::Rec2020,
                StandardTransfer::Linear,
                ColorReferenceSpace::Scene,
            ),
            Self::Rec2100Pq => metadata(
                self,
                "Rec.2100 PQ (absolute display)",
                StandardPrimaries::Rec2020,
                StandardTransfer::St2084Pq,
                ColorReferenceSpace::Display,
            ),
            Self::Rec2100Hlg => metadata(
                self,
                "Rec.2100 HLG",
                StandardPrimaries::Rec2020,
                StandardTransfer::Bt2100Hlg,
                ColorReferenceSpace::Scene,
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StandardColorSpaceMetadata {
    pub id: StandardColorSpaceId,
    pub label: &'static str,
    pub primaries: StandardPrimaries,
    pub transfer: StandardTransfer,
    pub reference_space: ColorReferenceSpace,
}

impl StandardColorSpaceMetadata {
    pub const fn linearity(self) -> ColorLinearity {
        if matches!(self.transfer, StandardTransfer::Linear) {
            ColorLinearity::Linear
        } else {
            ColorLinearity::Encoded
        }
    }

    pub fn color_space_info(self) -> ColorSpaceInfo {
        ColorSpaceInfo {
            id: self.id.as_str().to_string(),
            label: self.label.to_string(),
            reference_space: self.reference_space,
            linearity: self.linearity(),
            is_data: false,
        }
    }
}

const fn metadata(
    id: StandardColorSpaceId,
    label: &'static str,
    primaries: StandardPrimaries,
    transfer: StandardTransfer,
    reference_space: ColorReferenceSpace,
) -> StandardColorSpaceMetadata {
    StandardColorSpaceMetadata {
        id,
        label,
        primaries,
        transfer,
        reference_space,
    }
}

pub(crate) struct CompiledStandardTransform {
    source: StandardColorSpaceMetadata,
    destination: StandardColorSpaceMetadata,
    purpose: TransformPurpose,
    hdr_context: HdrTransformContext,
}

impl CompiledStandardTransform {
    pub(crate) fn compile(request: &ColorTransformRequest) -> Result<Self, ColorManagementError> {
        let TransformSpec::ColorSpace {
            source,
            destination,
        } = request.spec()
        else {
            return Err(ColorManagementError::UnsupportedDisplayView {
                backend_id: "builtin.standard-spaces".to_string(),
                display: "named display".to_string(),
                view: "named view".to_string(),
            });
        };
        if source.trim().is_empty() || destination.trim().is_empty() {
            return Err(ColorManagementError::EmptyColorSpace);
        }
        let source_id = StandardColorSpaceId::from_id(source).ok_or_else(|| {
            ColorManagementError::UnsupportedTransform {
                source: source.clone(),
                target: destination.clone(),
            }
        })?;
        let destination_id = StandardColorSpaceId::from_id(destination).ok_or_else(|| {
            ColorManagementError::UnsupportedTransform {
                source: source.clone(),
                target: destination.clone(),
            }
        })?;
        let compiled = Self {
            source: source_id.metadata(),
            destination: destination_id.metadata(),
            purpose: request.purpose(),
            hdr_context: HdrTransformContext::compile(
                source_id.metadata(),
                destination_id.metadata(),
                request.purpose(),
                request.context(),
            )?,
        };
        compiled.validate_boundary()?;
        Ok(compiled)
    }

    pub(crate) fn apply(&self, rgb: [f64; 3]) -> Result<[f64; 3], ColorManagementError> {
        if self.source.id == self.destination.id {
            return Ok(rgb);
        }
        let source_linear = self
            .hdr_context
            .decode_if_hdr(self.source.transfer, rgb)?
            .unwrap_or_else(|| rgb.map(|value| decode_sdr(self.source.transfer, value)));
        let destination_linear = convert_primaries(
            self.source.primaries,
            self.destination.primaries,
            source_linear,
        );
        Ok(self
            .hdr_context
            .encode_if_hdr(self.destination.transfer, destination_linear)?
            .unwrap_or_else(|| {
                destination_linear.map(|value| encode_sdr(self.destination.transfer, value))
            }))
    }

    pub(crate) fn program_id(&self) -> String {
        match (self.source.id, self.destination.id) {
            (StandardColorSpaceId::Srgb, StandardColorSpaceId::LinearSrgb) => {
                "builtin.extended-srgb-to-linear.v1".to_string()
            }
            (StandardColorSpaceId::LinearSrgb, StandardColorSpaceId::Srgb) => {
                "builtin.extended-linear-to-srgb.v1".to_string()
            }
            (source, destination) if source == destination => "builtin.identity.v1".to_string(),
            (source, destination) => format!(
                "builtin.standard-spaces.v3:{:?}:{}>{}{}",
                self.purpose,
                source.as_str(),
                destination.as_str(),
                self.hdr_context.program_id_suffix()
            ),
        }
    }

    fn validate_boundary(&self) -> Result<(), ColorManagementError> {
        let valid = match self.purpose {
            TransformPurpose::Explicit => true,
            TransformPurpose::SourceToWorking => {
                self.destination.linearity() == ColorLinearity::Linear
                    && self.destination.reference_space == ColorReferenceSpace::Scene
            }
            TransformPurpose::WorkingToDisplay | TransformPurpose::WorkingToOutput => {
                self.source.linearity() == ColorLinearity::Linear
                    && self.source.reference_space == ColorReferenceSpace::Scene
            }
        };
        if valid {
            Ok(())
        } else {
            Err(ColorManagementError::ProcessorContractMismatch {
                operation: "standard color transform compilation",
                detail: format!(
                    "purpose {:?} is incompatible with '{}' -> '{}'",
                    self.purpose,
                    self.source.id.as_str(),
                    self.destination.id.as_str()
                ),
            })
        }
    }
}

fn decode_sdr(transfer: StandardTransfer, value: f64) -> f64 {
    match transfer {
        StandardTransfer::Linear => value,
        StandardTransfer::Srgb => signed_transfer(value, |magnitude| {
            if magnitude <= 0.040_45 {
                magnitude / 12.92
            } else {
                ((magnitude + 0.055) / 1.055).powf(2.4)
            }
        }),
        StandardTransfer::Bt709 => signed_transfer(value, |magnitude| {
            if magnitude <= 0.081 {
                magnitude / 4.5
            } else {
                ((magnitude + 0.099) / 1.099).powf(1.0 / 0.45)
            }
        }),
        StandardTransfer::Bt2020Exact => decode_bt2020(value, BT2020_EXACT),
        StandardTransfer::Bt2020TenBit => decode_bt2020(value, BT2020_10_BIT),
        StandardTransfer::Bt2020TwelveBit => decode_bt2020(value, BT2020_12_BIT),
        StandardTransfer::St2084Pq | StandardTransfer::Bt2100Hlg => value,
    }
}

fn encode_sdr(transfer: StandardTransfer, value: f64) -> f64 {
    match transfer {
        StandardTransfer::Linear => value,
        StandardTransfer::Srgb => signed_transfer(value, |magnitude| {
            if magnitude <= 0.003_130_8 {
                magnitude * 12.92
            } else {
                1.055 * magnitude.powf(1.0 / 2.4) - 0.055
            }
        }),
        StandardTransfer::Bt709 => signed_transfer(value, |magnitude| {
            if magnitude <= 0.018 {
                magnitude * 4.5
            } else {
                1.099 * magnitude.powf(0.45) - 0.099
            }
        }),
        StandardTransfer::Bt2020Exact => encode_bt2020(value, BT2020_EXACT),
        StandardTransfer::Bt2020TenBit => encode_bt2020(value, BT2020_10_BIT),
        StandardTransfer::Bt2020TwelveBit => encode_bt2020(value, BT2020_12_BIT),
        StandardTransfer::St2084Pq | StandardTransfer::Bt2100Hlg => value,
    }
}

#[derive(Clone, Copy)]
struct Bt2020TransferCoefficients {
    alpha: f64,
    beta: f64,
}

const BT2020_EXACT: Bt2020TransferCoefficients = Bt2020TransferCoefficients {
    alpha: 1.099_296_826_809_44,
    beta: 0.018_053_968_510_807,
};
const BT2020_10_BIT: Bt2020TransferCoefficients = Bt2020TransferCoefficients {
    alpha: 1.099,
    beta: 0.018,
};
const BT2020_12_BIT: Bt2020TransferCoefficients = Bt2020TransferCoefficients {
    alpha: 1.0993,
    beta: 0.0181,
};

fn decode_bt2020(value: f64, coefficients: Bt2020TransferCoefficients) -> f64 {
    signed_transfer(value, |magnitude| {
        if magnitude < coefficients.beta * 4.5 {
            magnitude / 4.5
        } else {
            ((magnitude + coefficients.alpha - 1.0) / coefficients.alpha).powf(1.0 / 0.45)
        }
    })
}

fn encode_bt2020(value: f64, coefficients: Bt2020TransferCoefficients) -> f64 {
    signed_transfer(value, |magnitude| {
        if magnitude < coefficients.beta {
            magnitude * 4.5
        } else {
            coefficients.alpha * magnitude.powf(0.45) - (coefficients.alpha - 1.0)
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

fn convert_primaries(
    source: StandardPrimaries,
    destination: StandardPrimaries,
    rgb: [f64; 3],
) -> [f64; 3] {
    if source == destination {
        return rgb;
    }
    multiply_matrix(
        from_xyz_matrix(destination),
        multiply_matrix(to_xyz_matrix(source), rgb),
    )
}

fn multiply_matrix(matrix: [[f64; 3]; 3], value: [f64; 3]) -> [f64; 3] {
    matrix.map(|row| row[0] * value[0] + row[1] * value[1] + row[2] * value[2])
}

fn to_xyz_matrix(primaries: StandardPrimaries) -> [[f64; 3]; 3] {
    match primaries {
        StandardPrimaries::Bt709 => [
            [
                506_752.0 / 1_228_815.0,
                87_881.0 / 245_763.0,
                12_673.0 / 70_218.0,
            ],
            [
                87_098.0 / 409_605.0,
                175_762.0 / 245_763.0,
                12_673.0 / 175_545.0,
            ],
            [
                7_918.0 / 409_605.0,
                87_881.0 / 737_289.0,
                1_001_167.0 / 1_053_270.0,
            ],
        ],
        StandardPrimaries::DisplayP3 => [
            [
                608_311.0 / 1_250_200.0,
                189_793.0 / 714_400.0,
                198_249.0 / 1_000_160.0,
            ],
            [
                35_783.0 / 156_275.0,
                247_089.0 / 357_200.0,
                198_249.0 / 2_500_400.0,
            ],
            [0.0, 32_229.0 / 714_400.0, 5_220_557.0 / 5_000_800.0],
        ],
        StandardPrimaries::Rec2020 => [
            [
                63_426_534.0 / 99_577_255.0,
                20_160_776.0 / 139_408_157.0,
                47_086_771.0 / 278_816_314.0,
            ],
            [
                26_158_966.0 / 99_577_255.0,
                472_592_308.0 / 697_040_785.0,
                8_267_143.0 / 139_408_157.0,
            ],
            [
                0.0,
                19_567_812.0 / 697_040_785.0,
                295_819_943.0 / 278_816_314.0,
            ],
        ],
    }
}

fn from_xyz_matrix(primaries: StandardPrimaries) -> [[f64; 3]; 3] {
    match primaries {
        StandardPrimaries::Bt709 => [
            [12_831.0 / 3_959.0, -329.0 / 214.0, -1_974.0 / 3_959.0],
            [
                -851_781.0 / 878_810.0,
                1_648_619.0 / 878_810.0,
                36_519.0 / 878_810.0,
            ],
            [705.0 / 12_673.0, -2_585.0 / 12_673.0, 705.0 / 667.0],
        ],
        StandardPrimaries::DisplayP3 => [
            [
                446_124.0 / 178_915.0,
                -333_277.0 / 357_830.0,
                -72_051.0 / 178_915.0,
            ],
            [-14_852.0 / 17_905.0, 63_121.0 / 35_810.0, 423.0 / 17_905.0],
            [
                11_844.0 / 330_415.0,
                -50_337.0 / 660_830.0,
                316_169.0 / 330_415.0,
            ],
        ],
        StandardPrimaries::Rec2020 => [
            [
                30_757_411.0 / 17_917_100.0,
                -6_372_589.0 / 17_917_100.0,
                -4_539_589.0 / 17_917_100.0,
            ],
            [
                -19_765_991.0 / 29_648_200.0,
                47_925_759.0 / 29_648_200.0,
                467_509.0 / 29_648_200.0,
            ],
            [
                792_561.0 / 44_930_125.0,
                -1_921_689.0 / 44_930_125.0,
                42_328_811.0 / 44_930_125.0,
            ],
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ColorContext, PQ_LINEARIZATION_POLICY_CONTEXT_KEY, REFERENCE_WHITE_NITS_CONTEXT_KEY,
        RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY,
    };

    fn transform(source: &str, destination: &str, rgb: [f64; 3]) -> [f64; 3] {
        CompiledStandardTransform::compile(&ColorTransformRequest::explicit(source, destination))
            .unwrap()
            .apply(rgb)
            .unwrap()
    }

    fn assert_rgb_near(actual: [f64; 3], expected: [f64; 3], tolerance: f64) {
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!(
                (actual - expected).abs() <= tolerance,
                "{actual} != {expected}"
            );
        }
    }

    fn pq_context(reference_white_nits: &str) -> ColorContext {
        ColorContext::default()
            .with_variable(REFERENCE_WHITE_NITS_CONTEXT_KEY, reference_white_nits)
            .with_variable(
                PQ_LINEARIZATION_POLICY_CONTEXT_KEY,
                RELATIVE_DISPLAY_LUMINANCE_PQ_POLICY,
            )
    }

    #[test]
    fn catalog_ids_are_exact_unique_and_metadata_is_typed() {
        let ids = StandardColorSpaceId::ALL.map(StandardColorSpaceId::as_str);
        let mut sorted = ids.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), StandardColorSpaceId::ALL.len());
        assert_eq!(StandardColorSpaceId::from_id("sRGB"), None);
        assert_eq!(
            StandardColorSpaceId::from_id("rec2020-sdr"),
            None,
            "the ambiguous legacy ID must never alias a bit-depth-specific transfer"
        );
        assert_eq!(
            StandardColorSpaceId::from_id(SRGB_SPACE_ID),
            Some(StandardColorSpaceId::Srgb)
        );
        assert_eq!(
            StandardColorSpaceId::Rec2100Pq.metadata().primaries,
            StandardPrimaries::Rec2020
        );
    }

    #[test]
    fn bt709_and_all_bt2020_transfer_reference_breakpoints_match() {
        assert_rgb_near(
            transform(LINEAR_BT709_SPACE_ID, BT709_SPACE_ID, [0.018; 3]),
            [0.081; 3],
            1.0e-12,
        );
        let variants = [
            (
                REC2020_SDR_EXACT_SPACE_ID,
                BT2020_EXACT,
                0.081_242_858_298_633_9,
                0.408_848_108_891_225,
            ),
            (
                REC2020_SDR_10_SPACE_ID,
                BT2020_10_BIT,
                0.081_247_944_035_140_49,
                0.409_007_728_864_150_4,
            ),
            (
                REC2020_SDR_12_SPACE_ID,
                BT2020_12_BIT,
                0.081_447_203_498_534_24,
                0.408_846_402_493_503_7,
            ),
        ];
        for (space, coefficients, encoded_beta, encoded_eighteen_percent) in variants {
            assert_rgb_near(
                transform(LINEAR_REC2020_SPACE_ID, space, [coefficients.beta; 3]),
                [encoded_beta; 3],
                1.0e-14,
            );
            assert_rgb_near(
                transform(LINEAR_REC2020_SPACE_ID, space, [coefficients.beta * 0.5; 3]),
                [coefficients.beta * 2.25; 3],
                1.0e-14,
            );
            assert_rgb_near(
                transform(LINEAR_REC2020_SPACE_ID, space, [0.18; 3]),
                [encoded_eighteen_percent; 3],
                1.0e-14,
            );
        }
    }

    #[test]
    fn p3_and_rec2020_primary_conversion_matches_w3c_reference_values() {
        assert_rgb_near(
            transform(
                LINEAR_DISPLAY_P3_SPACE_ID,
                LINEAR_SRGB_SPACE_ID,
                [1.0, 0.0, 0.0],
            ),
            [
                1.224_940_176_280_559_8,
                -0.042_056_954_709_688_2,
                -0.019_637_554_590_334_4,
            ],
            2.0e-12,
        );
        let rec2020 = transform(
            LINEAR_SRGB_SPACE_ID,
            LINEAR_REC2020_SPACE_ID,
            [0.25, 0.5, 2.0],
        );
        let roundtrip = transform(LINEAR_REC2020_SPACE_ID, LINEAR_SRGB_SPACE_ID, rec2020);
        assert_rgb_near(roundtrip, [0.25, 0.5, 2.0], 2.0e-12);
    }

    #[test]
    fn every_sdr_pair_roundtrips_extended_rgb_without_clipping() {
        let spaces = [
            SRGB_SPACE_ID,
            LINEAR_SRGB_SPACE_ID,
            BT709_SPACE_ID,
            LINEAR_BT709_SPACE_ID,
            DISPLAY_P3_SPACE_ID,
            LINEAR_DISPLAY_P3_SPACE_ID,
            REC2020_SDR_EXACT_SPACE_ID,
            REC2020_SDR_10_SPACE_ID,
            REC2020_SDR_12_SPACE_ID,
            LINEAR_REC2020_SPACE_ID,
        ];
        for source in spaces {
            for destination in spaces {
                let input = [-0.125, 0.5, 1.5];
                let converted = transform(source, destination, input);
                let roundtrip = transform(destination, source, converted);
                assert_rgb_near(roundtrip, input, 2.0e-10);
            }
        }
    }

    #[test]
    fn pq_maps_itu_absolute_luminance_to_explicit_relative_working_scale() {
        let context = pq_context("100");
        let decode = CompiledStandardTransform::compile(
            &ColorTransformRequest::source_to_working(REC2100_PQ_SPACE_ID, LINEAR_REC2020_SPACE_ID)
                .with_context(context.clone()),
        )
        .unwrap();
        assert_rgb_near(
            decode.apply([0.508_078_421_517_399; 3]).unwrap(),
            [1.0; 3],
            1.0e-10,
        );
        assert_rgb_near(
            decode.apply([0.751_827_096_247_041; 3]).unwrap(),
            [10.0; 3],
            1.0e-9,
        );

        let encode = CompiledStandardTransform::compile(
            &ColorTransformRequest::working_to_output(LINEAR_REC2020_SPACE_ID, REC2100_PQ_SPACE_ID)
                .with_context(context),
        )
        .unwrap();
        assert_rgb_near(
            encode.apply([1.0; 3]).unwrap(),
            [0.508_078_421_517_399; 3],
            1.0e-12,
        );
    }

    #[test]
    fn hlg_source_and_output_use_scene_oetf_without_a_display_ootf() {
        let decode = CompiledStandardTransform::compile(&ColorTransformRequest::source_to_working(
            REC2100_HLG_SPACE_ID,
            LINEAR_REC2020_SPACE_ID,
        ))
        .unwrap();
        assert_rgb_near(
            decode.apply([0.75; 3]).unwrap(),
            [0.264_962_559_786_400_15; 3],
            1.0e-12,
        );

        let encode = CompiledStandardTransform::compile(&ColorTransformRequest::working_to_output(
            LINEAR_REC2020_SPACE_ID,
            REC2100_HLG_SPACE_ID,
        ))
        .unwrap();
        assert_rgb_near(
            encode.apply([1.0; 3]).unwrap(),
            [0.999_999_995_536_568_6; 3],
            1.0e-11,
        );
    }

    #[test]
    fn hdr_requires_explicit_luminance_context_and_roundtrips_without_clipping() {
        let missing = CompiledStandardTransform::compile(
            &ColorTransformRequest::source_to_working(REC2100_PQ_SPACE_ID, LINEAR_REC2020_SPACE_ID),
        );
        assert!(matches!(
            missing,
            Err(ColorManagementError::MissingContextVariable {
                variable: PQ_LINEARIZATION_POLICY_CONTEXT_KEY,
                ..
            })
        ));

        let context = pq_context("203");
        let decode = CompiledStandardTransform::compile(
            &ColorTransformRequest::source_to_working(REC2100_PQ_SPACE_ID, LINEAR_REC2020_SPACE_ID)
                .with_context(context.clone()),
        )
        .unwrap();
        let encode = CompiledStandardTransform::compile(
            &ColorTransformRequest::working_to_output(LINEAR_REC2020_SPACE_ID, REC2100_PQ_SPACE_ID)
                .with_context(context),
        )
        .unwrap();
        let encoded = [0.1, 0.75, 1.0];
        let roundtrip = encode.apply(decode.apply(encoded).unwrap()).unwrap();
        assert_rgb_near(roundtrip, encoded, 2.0e-10);

        let direct_display =
            ColorTransformRequest::working_to_display(LINEAR_REC2020_SPACE_ID, REC2100_PQ_SPACE_ID)
                .with_context(pq_context("203"));
        assert!(matches!(
            CompiledStandardTransform::compile(&direct_display),
            Err(ColorManagementError::UnsupportedTransform { .. })
        ));
        assert!(matches!(
            CompiledStandardTransform::compile(&ColorTransformRequest::working_to_display(
                LINEAR_REC2020_SPACE_ID,
                REC2100_HLG_SPACE_ID,
            ),),
            Err(ColorManagementError::UnsupportedTransform { .. })
        ));
        assert!(matches!(
            CompiledStandardTransform::compile(&ColorTransformRequest::explicit(
                REC2100_HLG_SPACE_ID,
                LINEAR_REC2020_SPACE_ID,
            )),
            Err(ColorManagementError::UnsupportedTransform { .. })
        ));
        assert!(matches!(
            CompiledStandardTransform::compile(
                &ColorTransformRequest::explicit(REC2100_PQ_SPACE_ID, LINEAR_REC2020_SPACE_ID,)
                    .with_context(pq_context("203")),
            ),
            Err(ColorManagementError::UnsupportedTransform { .. })
        ));
    }
}
