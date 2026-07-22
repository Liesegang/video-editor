use serde::{Deserialize, Serialize};

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

/// Persisted Asset metadata keeps automatic detection and authored intent
/// distinct. An override is a complete source description, so an intentionally
/// untagged override (`Some(Default::default())`) is representable too.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetSourceColorMetadata {
    #[serde(default, skip_serializing_if = "SourceColorDescription::is_empty")]
    detected: SourceColorDescription,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user_override: Option<SourceColorDescription>,
}

impl AssetSourceColorMetadata {
    pub fn is_empty(&self) -> bool {
        self.detected.is_empty() && self.user_override.is_none()
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

    /// Atomically builds a complete override from the current detection.
    ///
    /// Use this when correcting individual detected fields: fields not edited
    /// by `edit` retain their detected values instead of becoming unknown.
    pub fn replace_override_from_detected(
        &mut self,
        edit: impl FnOnce(&mut SourceColorDescription),
    ) {
        let mut complete_override = self.detected.clone();
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
        AssetSourceColorMetadata, SourceColorDescription, SourceColorPrimaries,
        SourceTransferCharacteristic,
    };

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
    fn field_correction_starts_from_complete_detection() {
        let detected = SourceColorDescription {
            primaries: Some(SourceColorPrimaries::Bt709),
            transfer: Some(SourceTransferCharacteristic::Bt709),
            bit_depth: Some(10),
            ..SourceColorDescription::default()
        };
        let mut metadata = AssetSourceColorMetadata::default();
        metadata.replace_detected(detected);
        metadata.replace_override_from_detected(|source| {
            source.primaries = Some(SourceColorPrimaries::Bt2020);
        });

        let authored = metadata.user_override().expect("override must exist");
        assert_eq!(authored.primaries, Some(SourceColorPrimaries::Bt2020));
        assert_eq!(authored.transfer, Some(SourceTransferCharacteristic::Bt709));
        assert_eq!(authored.bit_depth, Some(10));
    }
}
