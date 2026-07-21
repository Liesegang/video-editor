//! Transient Sound analysis values and deterministic short-window math.
//!
//! `Spectrum` deliberately has no Serde implementation: it exists only while
//! a frame graph is evaluated and can never become an intermediate Project
//! model. FFT magnitudes use a Hann window and coherent-gain normalization;
//! Band Energy is the root-sum-square of magnitudes in the inclusive band.
//! Smoothing, attack/release envelopes and LUFS remain future analysis Nodes.

use std::f64::consts::TAU;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Spectrum {
    pub(crate) sample_rate: u32,
    pub(crate) bin_width_hz: f64,
    pub(crate) magnitudes: Vec<f64>,
}

pub(crate) fn rms(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum = samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>();
    (sum / samples.len() as f64).sqrt()
}

pub(crate) fn peak(samples: &[f32]) -> f64 {
    samples
        .iter()
        .map(|sample| f64::from(*sample).abs())
        .fold(0.0, f64::max)
}

#[derive(Clone, Copy, Default)]
struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    fn magnitude(self) -> f64 {
        self.re.hypot(self.im)
    }

    fn mul(self, other: Self) -> Self {
        Self {
            re: self.re * other.re - self.im * other.im,
            im: self.re * other.im + self.im * other.re,
        }
    }
}

pub(crate) fn spectrum(samples: &[f32], sample_rate: u32) -> Spectrum {
    let fft_len = samples.len().max(2).next_power_of_two();
    let mut bins = vec![Complex::default(); fft_len];
    let denominator = samples.len().saturating_sub(1).max(1) as f64;
    let mut coherent_sum = 0.0;
    for (index, sample) in samples.iter().enumerate() {
        let window = 0.5 - 0.5 * (TAU * index as f64 / denominator).cos();
        coherent_sum += window;
        bins[index].re = f64::from(*sample) * window;
    }
    fft_in_place(&mut bins);
    let scale = coherent_sum.max(f64::EPSILON);
    let magnitudes = bins[..=fft_len / 2]
        .iter()
        .enumerate()
        .map(|(index, bin)| {
            let edge = index == 0 || index == fft_len / 2;
            bin.magnitude() * if edge { 1.0 } else { 2.0 } / scale
        })
        .collect();
    Spectrum {
        sample_rate,
        bin_width_hz: f64::from(sample_rate) / fft_len as f64,
        magnitudes,
    }
}

fn fft_in_place(values: &mut [Complex]) {
    debug_assert!(values.len().is_power_of_two());
    let mut reversed = 0usize;
    for index in 1..values.len() {
        let mut bit = values.len() >> 1;
        while reversed & bit != 0 {
            reversed ^= bit;
            bit >>= 1;
        }
        reversed ^= bit;
        if index < reversed {
            values.swap(index, reversed);
        }
    }

    let mut width = 2;
    while width <= values.len() {
        let angle = -TAU / width as f64;
        let root = Complex {
            re: angle.cos(),
            im: angle.sin(),
        };
        for block in values.chunks_exact_mut(width) {
            let (left, right) = block.split_at_mut(width / 2);
            let mut weight = Complex { re: 1.0, im: 0.0 };
            for (even, odd) in left.iter_mut().zip(right) {
                let rotated = odd.mul(weight);
                let original = *even;
                *even = Complex {
                    re: original.re + rotated.re,
                    im: original.im + rotated.im,
                };
                *odd = Complex {
                    re: original.re - rotated.re,
                    im: original.im - rotated.im,
                };
                weight = weight.mul(root);
            }
        }
        width *= 2;
    }
}

pub(crate) fn band_energy(spectrum: &Spectrum, low_hz: f64, high_hz: f64) -> f64 {
    if spectrum.bin_width_hz <= 0.0 || !low_hz.is_finite() || !high_hz.is_finite() {
        return 0.0;
    }
    let low = low_hz.max(0.0).min(high_hz.max(0.0));
    let high = high_hz.max(low).min(f64::from(spectrum.sample_rate) * 0.5);
    spectrum
        .magnitudes
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            let hz = *index as f64 * spectrum.bin_width_hz;
            hz >= low && hz <= high
        })
        .map(|(_, magnitude)| magnitude.powi(2))
        .sum::<f64>()
        .sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(frequency: f64, amplitude: f64, sample_rate: u32, count: usize) -> Vec<f32> {
        (0..count)
            .map(|index| {
                (amplitude * (TAU * frequency * index as f64 / f64::from(sample_rate)).sin()) as f32
            })
            .collect()
    }

    #[test]
    fn sine_amplitude_has_deterministic_rms_peak_and_band_energy() {
        let samples = sine(1_000.0, 0.5, 48_000, 4_800);
        assert!((rms(&samples) - 0.5 / 2.0_f64.sqrt()).abs() < 1.0e-6);
        assert!((peak(&samples) - 0.5).abs() < 1.0e-6);

        let spectrum = spectrum(&samples, 48_000);
        assert!(band_energy(&spectrum, 950.0, 1_050.0) > 0.49);
        assert!(band_energy(&spectrum, 2_000.0, 3_000.0) < 1.0e-4);
    }

    #[test]
    fn silence_is_a_valid_zero_analysis_value() {
        let samples = vec![0.0; 128];
        assert_eq!(rms(&samples), 0.0);
        assert_eq!(peak(&samples), 0.0);
        assert_eq!(band_energy(&spectrum(&samples, 48_000), 0.0, 24_000.0), 0.0);
    }
}
