use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

/// Exact authored media time. Equivalent fractions are normalized at every
/// construction boundary, so equality and hashing remain structural.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[serde(try_from = "MediaTimeWire", deny_unknown_fields)]
pub struct MediaTime {
    value: i64,
    timescale: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MediaTimeWire {
    value: i64,
    timescale: u32,
}

impl TryFrom<MediaTimeWire> for MediaTime {
    type Error = String;

    fn try_from(value: MediaTimeWire) -> Result<Self, Self::Error> {
        Self::new(value.value, value.timescale)
    }
}

impl MediaTime {
    pub fn new(value: i64, timescale: u32) -> Result<Self, String> {
        normalized_ratio(value as i128, timescale as i128)
            .map(|(value, timescale)| Self { value, timescale })
    }

    /// Constructs an exact integral number of seconds without a fallible
    /// rational-normalization round trip.
    pub const fn from_whole_seconds(value: i64) -> Self {
        Self {
            value,
            timescale: 1,
        }
    }

    pub const fn zero() -> Self {
        Self::from_whole_seconds(0)
    }

    pub const fn value(self) -> i64 {
        self.value
    }

    pub const fn timescale(self) -> u32 {
        self.timescale
    }

    pub fn is_negative(self) -> bool {
        self.value < 0
    }

    pub fn to_seconds_f64(self) -> f64 {
        self.value as f64 / f64::from(self.timescale)
    }

    /// Explicit UI/import boundary. Core authored arithmetic never uses this.
    pub fn from_seconds_f64(seconds: f64, timescale: u32) -> Result<Self, String> {
        if !seconds.is_finite() || timescale == 0 {
            return Err("Media time seconds must be finite and timescale non-zero".to_string());
        }
        let scaled = seconds * f64::from(timescale);
        if scaled < i64::MIN as f64 || scaled > i64::MAX as f64 {
            return Err("Media time is outside the i64 range".to_string());
        }
        Self::new(scaled.round() as i64, timescale)
    }

    pub fn checked_add(self, other: Self) -> Result<Self, String> {
        let numerator = i128::from(self.value) * i128::from(other.timescale)
            + i128::from(other.value) * i128::from(self.timescale);
        let denominator = i128::from(self.timescale) * i128::from(other.timescale);
        normalized_ratio(numerator, denominator).map(|(value, timescale)| Self { value, timescale })
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, String> {
        let numerator = i128::from(self.value) * i128::from(other.timescale)
            - i128::from(other.value) * i128::from(self.timescale);
        let denominator = i128::from(self.timescale) * i128::from(other.timescale);
        normalized_ratio(numerator, denominator).map(|(value, timescale)| Self { value, timescale })
    }

    pub fn checked_mul_rate(self, rate: RationalRate) -> Result<Self, String> {
        let numerator = i128::from(self.value) * i128::from(rate.numerator);
        let denominator = i128::from(self.timescale) * i128::from(rate.denominator);
        normalized_ratio(numerator, denominator).map(|(value, timescale)| Self { value, timescale })
    }

    pub fn checked_div_rate(self, rate: RationalRate) -> Result<Self, String> {
        if !rate.is_positive() {
            return Err("Rate divisor must be greater than zero".to_string());
        }
        let numerator = i128::from(self.value) * i128::from(rate.denominator);
        let denominator = i128::from(self.timescale) * i128::from(rate.numerator);
        normalized_ratio(numerator, denominator).map(|(value, timescale)| Self { value, timescale })
    }

    /// Returns the zero-based frame containing this time at `frame_rate`.
    /// Negative values use Euclidean floor semantics rather than truncating
    /// toward zero.
    pub fn checked_frame_index(self, frame_rate: RationalRate) -> Result<i64, String> {
        if !frame_rate.is_positive() {
            return Err("Frame rate must be greater than zero".to_string());
        }
        let numerator = i128::from(self.value) * i128::from(frame_rate.numerator);
        let denominator = i128::from(self.timescale) * i128::from(frame_rate.denominator);
        let frame = numerator.div_euclid(denominator);
        i64::try_from(frame).map_err(|_| "Frame index exceeds the i64 range".to_string())
    }

    /// Converts an exact frame boundary into authored media time.
    pub fn from_frame_index(frame: i64, frame_rate: RationalRate) -> Result<Self, String> {
        if !frame_rate.is_positive() {
            return Err("Frame rate must be greater than zero".to_string());
        }
        let numerator = i128::from(frame) * i128::from(frame_rate.denominator);
        let denominator = i128::from(frame_rate.numerator);
        normalized_ratio(numerator, denominator).map(|(value, timescale)| Self { value, timescale })
    }
}

impl Default for MediaTime {
    fn default() -> Self {
        Self::zero()
    }
}

impl PartialOrd for MediaTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MediaTime {
    fn cmp(&self, other: &Self) -> Ordering {
        (i128::from(self.value) * i128::from(other.timescale))
            .cmp(&(i128::from(other.value) * i128::from(self.timescale)))
    }
}

/// Exact authored rate used for FPS and time mapping.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[serde(try_from = "RationalRateWire", deny_unknown_fields)]
pub struct RationalRate {
    numerator: i64,
    denominator: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RationalRateWire {
    numerator: i64,
    denominator: u32,
}

impl TryFrom<RationalRateWire> for RationalRate {
    type Error = String;

    fn try_from(value: RationalRateWire) -> Result<Self, Self::Error> {
        Self::new(value.numerator, value.denominator)
    }
}

impl RationalRate {
    pub fn new(numerator: i64, denominator: u32) -> Result<Self, String> {
        normalized_ratio(numerator as i128, denominator as i128).map(|(numerator, denominator)| {
            Self {
                numerator,
                denominator,
            }
        })
    }

    pub const fn one() -> Self {
        Self {
            numerator: 1,
            denominator: 1,
        }
    }

    pub const fn numerator(self) -> i64 {
        self.numerator
    }

    pub const fn denominator(self) -> u32 {
        self.denominator
    }

    pub fn is_positive(self) -> bool {
        self.numerator > 0
    }

    pub fn to_f64(self) -> f64 {
        self.numerator as f64 / f64::from(self.denominator)
    }

    /// Explicit UI/import boundary. Core authored arithmetic never uses this.
    pub fn from_f64(value: f64, denominator: u32) -> Result<Self, String> {
        if !value.is_finite() || denominator == 0 {
            return Err("Rate must be finite and denominator non-zero".to_string());
        }
        let scaled = value * f64::from(denominator);
        if scaled < i64::MIN as f64 || scaled > i64::MAX as f64 {
            return Err("Rate is outside the i64 range".to_string());
        }
        Self::new(scaled.round() as i64, denominator)
    }

    pub fn checked_mul(self, other: Self) -> Result<Self, String> {
        let numerator = i128::from(self.numerator) * i128::from(other.numerator);
        let denominator = i128::from(self.denominator) * i128::from(other.denominator);
        normalized_ratio(numerator, denominator).map(|(numerator, denominator)| Self {
            numerator,
            denominator,
        })
    }
}

impl Default for RationalRate {
    fn default() -> Self {
        Self::one()
    }
}

impl PartialOrd for RationalRate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RationalRate {
    fn cmp(&self, other: &Self) -> Ordering {
        (i128::from(self.numerator) * i128::from(other.denominator))
            .cmp(&(i128::from(other.numerator) * i128::from(self.denominator)))
    }
}

fn normalized_ratio(numerator: i128, denominator: i128) -> Result<(i64, u32), String> {
    if denominator <= 0 {
        return Err("Rational denominator must be greater than zero".to_string());
    }
    if numerator == 0 {
        return Ok((0, 1));
    }
    let divisor = gcd(numerator.unsigned_abs(), denominator as u128);
    let numerator = numerator / divisor as i128;
    let denominator = denominator / divisor as i128;
    let numerator = i64::try_from(numerator)
        .map_err(|_| "Rational numerator exceeds the i64 range".to_string())?;
    let denominator = u32::try_from(denominator)
        .map_err(|_| "Rational denominator exceeds the u32 range".to_string())?;
    Ok((numerator, denominator))
}

fn gcd(mut left: u128, mut right: u128) -> u128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}
