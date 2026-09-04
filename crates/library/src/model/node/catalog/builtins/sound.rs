use super::super::super::SoundAnalysisContent;
use super::super::descriptor::{DescriptorIdentity, DescriptorSpec, NativeNodeFactory};

const SPECS: &[DescriptorSpec] = &[
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.sound.rms",
            "RMS",
            "Sound / Analysis",
            "node_editor.menu.create.sound_rms",
            &["sound", "audio", "rms", "level", "amplitude"],
        ),
        NativeNodeFactory::SoundAnalysis(SoundAnalysisContent::Rms),
        &[],
        &[],
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.sound.peak",
            "Peak",
            "Sound / Analysis",
            "node_editor.menu.create.sound_peak",
            &["sound", "audio", "peak", "level", "amplitude"],
        ),
        NativeNodeFactory::SoundAnalysis(SoundAnalysisContent::Peak),
        &[],
        &[],
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.sound.spectrum",
            "FFT / Spectrum",
            "Sound / Analysis",
            "node_editor.menu.create.sound_spectrum",
            &["sound", "audio", "fft", "spectrum", "frequency"],
        ),
        NativeNodeFactory::SoundAnalysis(SoundAnalysisContent::Spectrum),
        &[],
        &[],
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.sound.band-energy",
            "Band Energy",
            "Sound / Analysis",
            "node_editor.menu.create.sound_band_energy",
            &["sound", "audio", "band", "frequency", "energy"],
        ),
        NativeNodeFactory::SoundAnalysis(SoundAnalysisContent::BandEnergy),
        &[],
        &[],
    ),
];

pub(super) const fn specs() -> &'static [DescriptorSpec] {
    SPECS
}
