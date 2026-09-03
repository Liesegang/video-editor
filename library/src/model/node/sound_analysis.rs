//! Authoritative native Sound-analysis descriptors.
//!
//! This module owns the persisted operation identity, typed ports, authored
//! property metadata, and complete Node factory. Runtime PCM/Spectrum values
//! remain transient in `core::audio::analysis`.

use std::sync::LazyLock;

use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

use super::{Node, NodeContent};
use crate::model::project::connection::{
    ANALYSIS_HOP_MS_PROPERTY, ANALYSIS_SAMPLE_RATE_PROPERTY, ANALYSIS_WINDOW_MS_PROPERTY,
    BAND_HIGH_HZ_PROPERTY, BAND_LOW_HZ_PROPERTY, NUMBER_RESULT_OUTPUT_PORT, PortDataType,
    PortDefinition, PortExposure, PortSide, SOUND_INPUT_PORT, SPECTRUM_INPUT_PORT,
    SPECTRUM_OUTPUT_PORT,
};
use crate::model::project::property::{
    PropertyDefinition, PropertyMap, PropertyUiType, PropertyValue,
};

static SOUND_WINDOW_PROPERTY_DEFINITIONS: LazyLock<[PropertyDefinition; 3]> = LazyLock::new(|| {
    [
        PropertyDefinition::new(
            ANALYSIS_WINDOW_MS_PROPERTY,
            PropertyUiType::Float {
                min: 1.0,
                max: 2_000.0,
                step: 1.0,
                suffix: " ms".to_string(),
                min_hard_limit: true,
                max_hard_limit: true,
            },
            "Window",
            PropertyValue::Number(OrderedFloat(50.0)),
        ),
        PropertyDefinition::new(
            ANALYSIS_HOP_MS_PROPERTY,
            PropertyUiType::Float {
                min: 1.0,
                max: 2_000.0,
                step: 1.0,
                suffix: " ms".to_string(),
                min_hard_limit: true,
                max_hard_limit: true,
            },
            "Hop",
            PropertyValue::Number(OrderedFloat(10.0)),
        ),
        PropertyDefinition::new(
            ANALYSIS_SAMPLE_RATE_PROPERTY,
            PropertyUiType::Integer {
                min: 8_000,
                max: 192_000,
                suffix: " Hz".to_string(),
                min_hard_limit: true,
                max_hard_limit: true,
            },
            "Sample Rate",
            PropertyValue::Integer(48_000),
        ),
    ]
});

static BAND_ENERGY_PROPERTY_DEFINITIONS: LazyLock<[PropertyDefinition; 2]> = LazyLock::new(|| {
    [
        PropertyDefinition::new(
            BAND_LOW_HZ_PROPERTY,
            hz_ui(),
            "Low",
            PropertyValue::Number(OrderedFloat(20.0)),
        ),
        PropertyDefinition::new(
            BAND_HIGH_HZ_PROPERTY,
            hz_ui(),
            "High",
            PropertyValue::Number(OrderedFloat(20_000.0)),
        ),
    ]
});

fn hz_ui() -> PropertyUiType {
    PropertyUiType::Float {
        min: 0.0,
        max: 96_000.0,
        step: 1.0,
        suffix: " Hz".to_string(),
        min_hard_limit: true,
        max_hard_limit: true,
    }
}

static SOUND_SCALAR_PORT_DEFINITIONS: LazyLock<[PortDefinition; 5]> = LazyLock::new(|| {
    [
        PortDefinition::input(SOUND_INPUT_PORT, "Sound", PortDataType::Audio),
        analysis_number_input(ANALYSIS_WINDOW_MS_PROPERTY, "Window"),
        analysis_number_input(ANALYSIS_HOP_MS_PROPERTY, "Hop"),
        analysis_number_input(ANALYSIS_SAMPLE_RATE_PROPERTY, "Sample Rate"),
        PortDefinition::output(
            NUMBER_RESULT_OUTPUT_PORT,
            "Result",
            PortDataType::Number,
            PortSide::Right,
            PortExposure::Graph,
        ),
    ]
});

static SPECTRUM_PORT_DEFINITIONS: LazyLock<[PortDefinition; 5]> = LazyLock::new(|| {
    [
        PortDefinition::input(SOUND_INPUT_PORT, "Sound", PortDataType::Audio),
        analysis_number_input(ANALYSIS_WINDOW_MS_PROPERTY, "Window"),
        analysis_number_input(ANALYSIS_HOP_MS_PROPERTY, "Hop"),
        analysis_number_input(ANALYSIS_SAMPLE_RATE_PROPERTY, "Sample Rate"),
        PortDefinition::output(
            SPECTRUM_OUTPUT_PORT,
            "Spectrum",
            PortDataType::Spectrum,
            PortSide::Right,
            PortExposure::Graph,
        ),
    ]
});

static BAND_ENERGY_PORT_DEFINITIONS: LazyLock<[PortDefinition; 4]> = LazyLock::new(|| {
    [
        PortDefinition::input(SPECTRUM_INPUT_PORT, "Spectrum", PortDataType::Spectrum),
        analysis_number_input(BAND_LOW_HZ_PROPERTY, "Low"),
        analysis_number_input(BAND_HIGH_HZ_PROPERTY, "High"),
        PortDefinition::output(
            NUMBER_RESULT_OUTPUT_PORT,
            "Result",
            PortDataType::Number,
            PortSide::Right,
            PortExposure::Graph,
        ),
    ]
});

fn analysis_number_input(key: &str, label: &str) -> PortDefinition {
    // Authored metadata may use an Integer UI (sample rate), but graph math
    // deliberately accepts every scalar numeric source. Runtime converts only
    // finite in-range scalar values and otherwise produces NoOutput.
    PortDefinition::input(key, label, PortDataType::Numeric)
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SoundAnalysisContent {
    Rms,
    Peak,
    Spectrum,
    BandEnergy,
}

impl SoundAnalysisContent {
    pub const ALL: [Self; 4] = [Self::Rms, Self::Peak, Self::Spectrum, Self::BandEnergy];

    pub fn operation_key(self) -> &'static str {
        match self {
            Self::Rms => "sound_rms",
            Self::Peak => "sound_peak",
            Self::Spectrum => "sound_spectrum",
            Self::BandEnergy => "sound_band_energy",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Rms => "RMS",
            Self::Peak => "Peak",
            Self::Spectrum => "FFT / Spectrum",
            Self::BandEnergy => "Band Energy",
        }
    }

    pub fn property_definitions(self) -> &'static [PropertyDefinition] {
        match self {
            Self::Rms | Self::Peak | Self::Spectrum => SOUND_WINDOW_PROPERTY_DEFINITIONS.as_slice(),
            Self::BandEnergy => BAND_ENERGY_PROPERTY_DEFINITIONS.as_slice(),
        }
    }

    pub fn port_definitions(self) -> &'static [PortDefinition] {
        match self {
            Self::Rms | Self::Peak => SOUND_SCALAR_PORT_DEFINITIONS.as_slice(),
            Self::Spectrum => SPECTRUM_PORT_DEFINITIONS.as_slice(),
            Self::BandEnergy => BAND_ENERGY_PORT_DEFINITIONS.as_slice(),
        }
    }

    pub(crate) fn numeric_property_is_in_hard_limits(self, key: &str, value: f64) -> bool {
        if !value.is_finite() {
            return false;
        }
        let Some(definition) = self
            .property_definitions()
            .iter()
            .find(|definition| definition.name() == key)
        else {
            return false;
        };
        match definition.ui_type() {
            PropertyUiType::Float {
                min,
                max,
                min_hard_limit,
                max_hard_limit,
                ..
            } => (!min_hard_limit || value >= *min) && (!max_hard_limit || value <= *max),
            PropertyUiType::Integer {
                min,
                max,
                min_hard_limit,
                max_hard_limit,
                ..
            } => {
                (!min_hard_limit || value >= *min as f64)
                    && (!max_hard_limit || value <= *max as f64)
            }
            _ => false,
        }
    }
}

impl Node {
    pub fn new_sound_analysis(name: &str, content: SoundAnalysisContent) -> Self {
        Self::with_properties(
            name,
            NodeContent::SoundAnalysis(content),
            PropertyMap::from_definitions(content.property_definitions()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::node::SOUND_MERGE_OPERATION_KEY;

    const NODE_DESIGN: &str = include_str!("../../../../node_list.yml");

    #[test]
    fn authoritative_sound_operation_keys_match_node_design() {
        let node_design = NODE_DESIGN.replace("\r\n", "\n");
        assert!(node_design.contains(&format!(
            "model: SoundMerge\n  operation_key: {SOUND_MERGE_OPERATION_KEY}"
        )));
        for (content, model) in [
            (SoundAnalysisContent::Rms, "SoundAnalysis.Rms"),
            (SoundAnalysisContent::Peak, "SoundAnalysis.Peak"),
            (SoundAnalysisContent::Spectrum, "SoundAnalysis.Spectrum"),
            (SoundAnalysisContent::BandEnergy, "SoundAnalysis.BandEnergy"),
        ] {
            assert!(node_design.contains(&format!(
                "model: {model}\n  operation_key: {}",
                content.operation_key()
            )));
        }
        assert!(node_design.contains("model: PortDataType.Audio"));
        assert!(node_design.contains("model: PortDataType.Spectrum"));
        assert!(node_design.contains("TODO(sound-analysis): LUFS loudness and envelope/follower"));
    }

    #[test]
    fn graph_driven_settings_share_the_descriptor_hard_limits() {
        let rms = SoundAnalysisContent::Rms;
        assert!(rms.numeric_property_is_in_hard_limits(ANALYSIS_WINDOW_MS_PROPERTY, 1.0));
        assert!(rms.numeric_property_is_in_hard_limits(ANALYSIS_WINDOW_MS_PROPERTY, 2_000.0));
        assert!(!rms.numeric_property_is_in_hard_limits(ANALYSIS_WINDOW_MS_PROPERTY, 0.99));
        assert!(!rms.numeric_property_is_in_hard_limits(ANALYSIS_WINDOW_MS_PROPERTY, 2_000.01));
        assert!(rms.numeric_property_is_in_hard_limits(ANALYSIS_SAMPLE_RATE_PROPERTY, 48_000.0));
        assert!(!rms.numeric_property_is_in_hard_limits(ANALYSIS_SAMPLE_RATE_PROPERTY, 7_999.0));

        let band = SoundAnalysisContent::BandEnergy;
        assert!(band.numeric_property_is_in_hard_limits(BAND_LOW_HZ_PROPERTY, 0.0));
        assert!(band.numeric_property_is_in_hard_limits(BAND_HIGH_HZ_PROPERTY, 96_000.0));
        assert!(!band.numeric_property_is_in_hard_limits(BAND_HIGH_HZ_PROPERTY, f64::NAN));
    }
}
