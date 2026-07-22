use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use super::super::color_management::ColorConfigIdentity;

/// Stream/codec or still-image color metadata retained without guessing an
/// untagged source.
///
/// These values describe encoded source samples. They do not imply that the
/// current RGBA8 loader has applied a color transform yet.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceColorDescription {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primaries: Option<SourceColorPrimaries>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transfer: Option<SourceTransferCharacteristic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix: Option<SourceMatrixCoefficients>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<SourceColorRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bit_depth: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<SourceColorProfile>,
}

impl SourceColorDescription {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// A source color-space name bound to the exact configuration that owns it.
///
/// OpenColorIO color-space names are config-local identifiers. Persisting only
/// `"ACEScg"` would allow a later Project config to silently reinterpret that
/// name, so an authored source assignment always carries the config identity
/// under which the name was selected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetSourceColorSpaceBinding {
    config: ColorConfigIdentity,
    color_space: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetSourceColorSpaceBindingError {
    EmptyColorSpace,
}

impl std::fmt::Display for AssetSourceColorSpaceBindingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyColorSpace => formatter.write_str("source color space must not be blank"),
        }
    }
}

impl std::error::Error for AssetSourceColorSpaceBindingError {}

/// Lossless persisted form used so a future/invalid binding cannot make the
/// containing Project unloadable or get erased by a save-for-repair cycle.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PersistedAssetSourceColorSpaceBinding {
    Binding(AssetSourceColorSpaceBinding),
    Malformed { raw: Value, detail: String },
}

impl Serialize for PersistedAssetSourceColorSpaceBinding {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Binding(binding) => binding.serialize(serializer),
            Self::Malformed { raw, .. } => raw.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for PersistedAssetSourceColorSpaceBinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Value::deserialize(deserializer)?;
        Ok(
            match serde_json::from_value::<AssetSourceColorSpaceBinding>(raw.clone()) {
                Ok(binding) => Self::Binding(binding),
                Err(error) => Self::Malformed {
                    raw,
                    detail: error.to_string(),
                },
            },
        )
    }
}

impl AssetSourceColorSpaceBinding {
    pub(crate) fn new(
        config: ColorConfigIdentity,
        color_space: impl Into<String>,
    ) -> Result<Self, AssetSourceColorSpaceBindingError> {
        let color_space = color_space.into();
        if color_space.trim().is_empty() {
            return Err(AssetSourceColorSpaceBindingError::EmptyColorSpace);
        }
        Ok(Self {
            config,
            color_space,
        })
    }

    pub fn config(&self) -> &ColorConfigIdentity {
        &self.config
    }

    pub fn color_space(&self) -> &str {
        &self.color_space
    }
}

/// Persisted Asset metadata keeps automatic detection, authored CICP/profile
/// corrections, and a config-bound source-space assignment distinct.
///
/// A CICP/profile override is a complete source description, so an
/// intentionally untagged override (`Some(Default::default())`) is
/// representable too. A config-bound assignment takes precedence at the color
/// conversion boundary, but never erases detected metadata needed for repair
/// or reassignment.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetSourceColorMetadata {
    #[serde(default, skip_serializing_if = "SourceColorDescription::is_empty")]
    detected: SourceColorDescription,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user_override: Option<SourceColorDescription>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    assigned_space: Option<PersistedAssetSourceColorSpaceBinding>,
}

impl AssetSourceColorMetadata {
    pub fn is_empty(&self) -> bool {
        self.detected.is_empty() && self.user_override.is_none() && self.assigned_space.is_none()
    }

    pub fn effective(&self) -> &SourceColorDescription {
        self.user_override.as_ref().unwrap_or(&self.detected)
    }

    pub fn detected(&self) -> &SourceColorDescription {
        &self.detected
    }

    pub fn replace_detected(&mut self, detected: SourceColorDescription) {
        self.detected = detected;
    }

    pub fn user_override(&self) -> Option<&SourceColorDescription> {
        self.user_override.as_ref()
    }

    /// Atomically edits the effective description as a complete override.
    ///
    /// The first edit starts from detected metadata. Later edits start from
    /// the existing authored override, so independent corrections accumulate
    /// instead of resetting earlier changes.
    pub fn edit_override(&mut self, edit: impl FnOnce(&mut SourceColorDescription)) {
        let mut complete_override = self
            .user_override
            .clone()
            .unwrap_or_else(|| self.detected.clone());
        edit(&mut complete_override);
        self.user_override = Some(complete_override);
    }

    /// Replaces the override as one complete authored description.
    ///
    /// `Some(SourceColorDescription::default())` intentionally declares the
    /// entire source untagged. It does not inherit omitted detected fields.
    pub fn replace_complete_override(&mut self, complete: SourceColorDescription) {
        self.user_override = Some(complete);
    }

    pub fn clear_override(&mut self) {
        self.user_override = None;
    }

    pub fn assigned_space(&self) -> Option<&AssetSourceColorSpaceBinding> {
        match self.assigned_space.as_ref()? {
            PersistedAssetSourceColorSpaceBinding::Binding(binding) => Some(binding),
            PersistedAssetSourceColorSpaceBinding::Malformed { .. } => None,
        }
    }

    /// Returns the exact unrecognized value and its parse diagnostic for a
    /// repair UI. Saving the Project writes `raw` back unchanged.
    pub fn malformed_assigned_space(&self) -> Option<(&Value, &str)> {
        match self.assigned_space.as_ref()? {
            PersistedAssetSourceColorSpaceBinding::Binding(_) => None,
            PersistedAssetSourceColorSpaceBinding::Malformed { raw, detail } => Some((raw, detail)),
        }
    }

    /// Assigns a named source space without mutating detected CICP/profile
    /// metadata. Project color diagnostics reject a different owning config.
    pub fn assign_space(&mut self, binding: AssetSourceColorSpaceBinding) {
        self.assigned_space = Some(PersistedAssetSourceColorSpaceBinding::Binding(binding));
    }

    pub fn clear_assigned_space(&mut self) {
        self.assigned_space = None;
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceColorPrimaries {
    Bt709,
    Bt470M,
    Bt470Bg,
    Smpte170M,
    Smpte240M,
    Film,
    Bt2020,
    Smpte428,
    DciP3,
    DisplayP3,
    Ebu3213,
    UnknownCode(i32),
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceTransferCharacteristic {
    Bt709,
    Gamma22,
    Gamma28,
    Smpte170M,
    Smpte240M,
    Linear,
    Log100,
    Log316,
    Iec61966_2_4,
    Bt1361,
    Srgb,
    Bt2020_10,
    Bt2020_12,
    Pq,
    Smpte428,
    Hlg,
    UnknownCode(i32),
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceMatrixCoefficients {
    Identity,
    Bt709,
    Fcc,
    Bt470Bg,
    Smpte170M,
    Smpte240M,
    YCgCo,
    Bt2020NonConstantLuminance,
    Bt2020ConstantLuminance,
    Smpte2085,
    ChromaDerivedNonConstantLuminance,
    ChromaDerivedConstantLuminance,
    ICtCp,
    UnknownCode(i32),
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceColorRange {
    Limited,
    Full,
    UnknownCode(i32),
    Other(String),
}

/// Stable identity for an embedded source profile. The source bytes remain in
/// the media file; persisting them in every Project would duplicate assets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceColorProfile {
    Icc {
        sha256: String,
        byte_length: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile_id: Option<String>,
    },
    Other {
        profile_kind: String,
        identity: String,
    },
}

#[cfg(test)]
mod tests {
    use super::{
        AssetSourceColorMetadata, AssetSourceColorSpaceBinding, SourceColorDescription,
        SourceColorPrimaries, SourceTransferCharacteristic,
    };
    use crate::model::project::ColorConfigIdentity;

    #[test]
    fn authored_override_does_not_mutate_detection() {
        let detected = SourceColorDescription {
            primaries: Some(SourceColorPrimaries::Bt709),
            transfer: Some(SourceTransferCharacteristic::Bt709),
            ..SourceColorDescription::default()
        };
        let authored = SourceColorDescription {
            primaries: Some(SourceColorPrimaries::Bt2020),
            transfer: Some(SourceTransferCharacteristic::Pq),
            ..SourceColorDescription::default()
        };
        let mut metadata = AssetSourceColorMetadata::default();
        metadata.replace_detected(detected.clone());
        metadata.replace_complete_override(authored.clone());

        assert_eq!(metadata.detected, detected);
        assert_eq!(metadata.effective(), &authored);
    }

    #[test]
    fn an_explicit_untagged_override_is_not_auto_detection() {
        let mut metadata = AssetSourceColorMetadata::default();
        metadata.replace_detected(SourceColorDescription {
            primaries: Some(SourceColorPrimaries::Bt709),
            ..SourceColorDescription::default()
        });
        metadata.replace_complete_override(SourceColorDescription::default());

        assert!(metadata.effective().is_empty());
        assert!(!metadata.detected().is_empty());
    }

    #[test]
    fn consecutive_field_corrections_accumulate_on_the_effective_description() {
        let detected = SourceColorDescription {
            primaries: Some(SourceColorPrimaries::Bt709),
            transfer: Some(SourceTransferCharacteristic::Bt709),
            bit_depth: Some(10),
            ..SourceColorDescription::default()
        };
        let mut metadata = AssetSourceColorMetadata::default();
        metadata.replace_detected(detected);
        metadata.edit_override(|source| {
            source.primaries = Some(SourceColorPrimaries::Bt2020);
        });
        metadata.edit_override(|source| {
            source.bit_depth = Some(12);
        });

        let authored = metadata.user_override().expect("override must exist");
        assert_eq!(authored.primaries, Some(SourceColorPrimaries::Bt2020));
        assert_eq!(authored.transfer, Some(SourceTransferCharacteristic::Bt709));
        assert_eq!(authored.bit_depth, Some(12));
    }

    #[test]
    fn explicit_empty_override_survives_as_json_object() {
        let mut metadata = AssetSourceColorMetadata::default();
        metadata.replace_detected(SourceColorDescription {
            primaries: Some(SourceColorPrimaries::Bt709),
            ..SourceColorDescription::default()
        });
        metadata.replace_complete_override(SourceColorDescription::default());

        let json = serde_json::to_string(&metadata).unwrap();
        assert!(json.contains(r#""user_override":{}"#));
        let restored: AssetSourceColorMetadata = serde_json::from_str(&json).unwrap();
        assert!(restored.user_override().is_some());
        assert!(restored.effective().is_empty());
    }

    #[test]
    fn unknown_codes_survive_serde_round_trip() {
        let source = SourceColorDescription {
            primaries: Some(SourceColorPrimaries::UnknownCode(90)),
            transfer: Some(super::SourceTransferCharacteristic::UnknownCode(91)),
            matrix: Some(super::SourceMatrixCoefficients::UnknownCode(92)),
            range: Some(super::SourceColorRange::UnknownCode(93)),
            ..SourceColorDescription::default()
        };

        let json = serde_json::to_string(&source).unwrap();
        let restored: SourceColorDescription = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, source);
    }

    #[test]
    fn config_bound_assignment_preserves_detected_metadata() {
        let detected = SourceColorDescription {
            primaries: Some(SourceColorPrimaries::Bt2020),
            transfer: Some(SourceTransferCharacteristic::Pq),
            ..SourceColorDescription::default()
        };
        let binding = AssetSourceColorSpaceBinding::new(
            ColorConfigIdentity::OcioBuiltin {
                uri: "ocio://studio-config-v4.0.0_aces-v2.0_ocio-v2.5".to_string(),
                ocio_version: "2.5.2".to_string(),
            },
            "Input - Rec.2100-PQ",
        )
        .unwrap();
        let mut metadata = AssetSourceColorMetadata::default();
        metadata.replace_detected(detected.clone());
        metadata.assign_space(binding.clone());

        let json = serde_json::to_string(&metadata).unwrap();
        let restored: AssetSourceColorMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.detected(), &detected);
        assert_eq!(restored.assigned_space(), Some(&binding));
    }

    #[test]
    fn malformed_binding_round_trips_for_repair() {
        let raw = serde_json::json!({
            "config": { "kind": "future_config", "identity": "show-v9" },
            "color_space": "Future Log"
        });
        let json = serde_json::json!({ "assigned_space": raw });

        let metadata: AssetSourceColorMetadata = serde_json::from_value(json).unwrap();
        assert!(metadata.assigned_space().is_none());
        assert_eq!(
            metadata
                .malformed_assigned_space()
                .map(|(persisted, _)| persisted),
            Some(&raw)
        );
        assert_eq!(
            serde_json::to_value(&metadata).unwrap()["assigned_space"],
            raw
        );
    }
}
