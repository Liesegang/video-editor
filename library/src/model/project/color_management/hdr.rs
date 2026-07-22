//! Typed PQ reference-luminance intent persisted with Project color.

use ordered_float::OrderedFloat;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

const MAX_REFERENCE_WHITE_NITS: f64 = 10_000.0;
const RELATIVE_DISPLAY_LUMINANCE: &str = "relative-display-luminance";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HdrColorSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reference_white_nits: Option<OrderedFloat<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pq_linearization_policy: Option<PqLinearizationPolicy>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HdrColorField {
    ReferenceWhiteNits,
    PqLinearizationPolicy,
}

impl std::fmt::Display for HdrColorField {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReferenceWhiteNits => formatter.write_str("HDR reference white nits"),
            Self::PqLinearizationPolicy => formatter.write_str("PQ linearization policy"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PqLinearizationPolicy {
    RelativeDisplayLuminance,
    Unsupported(String),
}

impl PqLinearizationPolicy {
    pub const fn context_value(&self) -> &str {
        match self {
            Self::RelativeDisplayLuminance => RELATIVE_DISPLAY_LUMINANCE,
            Self::Unsupported(value) => value.as_str(),
        }
    }

    pub const fn is_supported(&self) -> bool {
        matches!(self, Self::RelativeDisplayLuminance)
    }
}

impl Serialize for PqLinearizationPolicy {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.context_value())
    }
}

impl<'de> Deserialize<'de> for PqLinearizationPolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(if value == RELATIVE_DISPLAY_LUMINANCE {
            Self::RelativeDisplayLuminance
        } else {
            Self::Unsupported(value)
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct HdrColorSettingsError {
    value: f64,
}

impl std::fmt::Display for HdrColorSettingsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "HDR reference white {} must be finite, positive, and no greater than {} nits",
            self.value, MAX_REFERENCE_WHITE_NITS
        )
    }
}

impl std::error::Error for HdrColorSettingsError {}

impl HdrColorSettings {
    /// Construct the complete, supported PQ linearization contract atomically.
    pub fn for_pq(reference_white_nits: f64) -> Result<Self, HdrColorSettingsError> {
        validate_reference_white(reference_white_nits)?;
        Ok(Self {
            reference_white_nits: Some(OrderedFloat(reference_white_nits)),
            pq_linearization_policy: Some(PqLinearizationPolicy::RelativeDisplayLuminance),
        })
    }

    pub fn reference_white_nits(&self) -> Option<f64> {
        self.reference_white_nits.map(OrderedFloat::into_inner)
    }

    pub fn pq_linearization_policy(&self) -> Option<&PqLinearizationPolicy> {
        self.pq_linearization_policy.as_ref()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    pub(crate) fn semantic_issues(&self) -> Vec<(HdrColorField, String)> {
        let mut issues = self
            .reference_white_nits()
            .and_then(|value| validate_reference_white(value).err())
            .map(|error| vec![(HdrColorField::ReferenceWhiteNits, error.to_string())])
            .unwrap_or_default();
        if let Some(policy) = self
            .pq_linearization_policy()
            .filter(|policy| !policy.is_supported())
        {
            issues.push((
                HdrColorField::PqLinearizationPolicy,
                format!("unsupported policy '{}'", policy.context_value()),
            ));
        }
        issues
    }
}

fn validate_reference_white(value: f64) -> Result<(), HdrColorSettingsError> {
    if !value.is_finite() || value <= 0.0 || value > MAX_REFERENCE_WHITE_NITS {
        return Err(HdrColorSettingsError { value });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{HdrColorSettings, PqLinearizationPolicy};

    #[test]
    fn pq_constructor_sets_reference_and_policy_atomically() {
        let settings = HdrColorSettings::for_pq(203.0).expect("valid reference white");
        assert_eq!(settings.reference_white_nits(), Some(203.0));
        assert_eq!(
            settings.pq_linearization_policy(),
            Some(&PqLinearizationPolicy::RelativeDisplayLuminance)
        );

        let json = serde_json::to_string(&settings).expect("serialize HDR settings");
        assert_eq!(
            serde_json::from_str::<HdrColorSettings>(&json).expect("deserialize HDR settings"),
            settings
        );
    }

    #[test]
    fn invalid_reference_is_rejected_by_the_typed_constructor() {
        for invalid in [f64::NAN, 0.0, -1.0, 10_001.0] {
            assert!(HdrColorSettings::for_pq(invalid).is_err());
        }
    }

    #[test]
    fn unknown_persisted_policy_remains_repairable_and_diagnostic() {
        let settings: HdrColorSettings = serde_json::from_str(
            r#"{"reference_white_nits":203.0,"pq_linearization_policy":"future-policy"}"#,
        )
        .expect("unknown policy must remain loadable for repair");
        assert!(matches!(
            settings.pq_linearization_policy(),
            Some(PqLinearizationPolicy::Unsupported(value)) if value == "future-policy"
        ));
        assert_eq!(settings.semantic_issues().len(), 1);
    }
}
