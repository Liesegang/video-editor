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

static SOUND_SCALAR_PORT_DEFINITIONS: LazyLock<[PortDefinition; 2]> = LazyLock::new(|| {
    [
        PortDefinition::input(SOUND_INPUT_PORT, "Sound", PortDataType::Audio),
        PortDefinition::output(
            NUMBER_RESULT_OUTPUT_PORT,
            "Result",
            PortDataType::Number,
            PortSide::Right,
            PortExposure::Graph,
        ),
    ]
});

static SPECTRUM_PORT_DEFINITIONS: LazyLock<[PortDefinition; 2]> = LazyLock::new(|| {
    [
        PortDefinition::input(SOUND_INPUT_PORT, "Sound", PortDataType::Audio),
        PortDefinition::output(
            SPECTRUM_OUTPUT_PORT,
            "Spectrum",
            PortDataType::Spectrum,
            PortSide::Right,
            PortExposure::Graph,
        ),
    ]
});

static BAND_ENERGY_PORT_DEFINITIONS: LazyLock<[PortDefinition; 2]> = LazyLock::new(|| {
    [
        PortDefinition::input(SPECTRUM_INPUT_PORT, "Spectrum", PortDataType::Spectrum),
        PortDefinition::output(
            NUMBER_RESULT_OUTPUT_PORT,
            "Result",
            PortDataType::Number,
            PortSide::Right,
            PortExposure::Graph,
        ),
    ]
});

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
