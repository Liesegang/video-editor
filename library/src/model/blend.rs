use serde::{Deserialize, Serialize};

/// Authoritative six-way grouping used by both the editor catalog and QA.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlendModeGroup {
    Normal,
    Darken,
    Lighten,
    Contrast,
    Comparative,
    Hsl,
}

impl BlendModeGroup {
    pub const ALL: [Self; 6] = [
        Self::Normal,
        Self::Darken,
        Self::Lighten,
        Self::Contrast,
        Self::Comparative,
        Self::Hsl,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Darken => "Darken",
            Self::Lighten => "Lighten",
            Self::Contrast => "Contrast",
            Self::Comparative => "Comparative",
            Self::Hsl => "HSL",
        }
    }

    pub const fn qa_key(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Darken => "darken",
            Self::Lighten => "lighten",
            Self::Contrast => "contrast",
            Self::Comparative => "comparative",
            Self::Hsl => "hsl",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlendModeInfo {
    pub mode: BlendMode,
    pub group: BlendModeGroup,
    pub label: &'static str,
    pub qa_key: &'static str,
}

/// Pixel compositing behavior authored independently on each Merge input.
///
/// This is the complete pre-v1 catalog. Variant names are the persisted JSON
/// contract; intentionally no legacy aliases or migration paths are accepted.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Dissolve,
    Behind,
    Clear,
    Darken,
    Multiply,
    ColorBurn,
    LinearBurn,
    DarkerColor,
    Lighten,
    Screen,
    ColorDodge,
    LinearDodge,
    LighterColor,
    Overlay,
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    Difference,
    Exclusion,
    Subtract,
    Divide,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    pub const ALL: [Self; 29] = [
        Self::Normal,
        Self::Dissolve,
        Self::Behind,
        Self::Clear,
        Self::Darken,
        Self::Multiply,
        Self::ColorBurn,
        Self::LinearBurn,
        Self::DarkerColor,
        Self::Lighten,
        Self::Screen,
        Self::ColorDodge,
        Self::LinearDodge,
        Self::LighterColor,
        Self::Overlay,
        Self::SoftLight,
        Self::HardLight,
        Self::VividLight,
        Self::LinearLight,
        Self::PinLight,
        Self::HardMix,
        Self::Difference,
        Self::Exclusion,
        Self::Subtract,
        Self::Divide,
        Self::Hue,
        Self::Saturation,
        Self::Color,
        Self::Luminosity,
    ];

    pub const fn info(self) -> BlendModeInfo {
        let (group, label, qa_key) = match self {
            Self::Normal => (BlendModeGroup::Normal, "Normal", "normal"),
            Self::Dissolve => (BlendModeGroup::Normal, "Dissolve", "dissolve"),
            Self::Behind => (BlendModeGroup::Normal, "Behind", "behind"),
            Self::Clear => (BlendModeGroup::Normal, "Clear", "clear"),
            Self::Darken => (BlendModeGroup::Darken, "Darken", "darken"),
            Self::Multiply => (BlendModeGroup::Darken, "Multiply", "multiply"),
            Self::ColorBurn => (BlendModeGroup::Darken, "Color Burn", "color_burn"),
            Self::LinearBurn => (BlendModeGroup::Darken, "Linear Burn", "linear_burn"),
            Self::DarkerColor => (BlendModeGroup::Darken, "Darker Color", "darker_color"),
            Self::Lighten => (BlendModeGroup::Lighten, "Lighten", "lighten"),
            Self::Screen => (BlendModeGroup::Lighten, "Screen", "screen"),
            Self::ColorDodge => (BlendModeGroup::Lighten, "Color Dodge", "color_dodge"),
            Self::LinearDodge => (
                BlendModeGroup::Lighten,
                "Linear Dodge (Add)",
                "linear_dodge",
            ),
            Self::LighterColor => (BlendModeGroup::Lighten, "Lighter Color", "lighter_color"),
            Self::Overlay => (BlendModeGroup::Contrast, "Overlay", "overlay"),
            Self::SoftLight => (BlendModeGroup::Contrast, "Soft Light", "soft_light"),
            Self::HardLight => (BlendModeGroup::Contrast, "Hard Light", "hard_light"),
            Self::VividLight => (BlendModeGroup::Contrast, "Vivid Light", "vivid_light"),
            Self::LinearLight => (BlendModeGroup::Contrast, "Linear Light", "linear_light"),
            Self::PinLight => (BlendModeGroup::Contrast, "Pin Light", "pin_light"),
            Self::HardMix => (BlendModeGroup::Contrast, "Hard Mix", "hard_mix"),
            Self::Difference => (BlendModeGroup::Comparative, "Difference", "difference"),
            Self::Exclusion => (BlendModeGroup::Comparative, "Exclusion", "exclusion"),
            Self::Subtract => (BlendModeGroup::Comparative, "Subtract", "subtract"),
            Self::Divide => (BlendModeGroup::Comparative, "Divide", "divide"),
            Self::Hue => (BlendModeGroup::Hsl, "Hue", "hue"),
            Self::Saturation => (BlendModeGroup::Hsl, "Saturation", "saturation"),
            Self::Color => (BlendModeGroup::Hsl, "Color", "color"),
            Self::Luminosity => (BlendModeGroup::Hsl, "Luminosity", "luminosity"),
        };
        BlendModeInfo {
            mode: self,
            group,
            label,
            qa_key,
        }
    }

    pub const fn group(self) -> BlendModeGroup {
        self.info().group
    }

    pub const fn label(self) -> &'static str {
        self.info().label
    }

    pub const fn qa_key(self) -> &'static str {
        self.info().qa_key
    }

    /// Only these modes differ observably from Normal over an empty backdrop.
    pub const fn can_optimize_empty_backdrop_to_normal(self) -> bool {
        !matches!(self, Self::Dissolve | Self::Clear)
    }
}

#[cfg(test)]
mod tests {
    use super::{BlendMode, BlendModeGroup};

    #[test]
    fn catalog_is_exhaustive_unique_and_group_ordered() {
        assert_eq!(BlendMode::ALL.len(), 29);
        let mut modes = std::collections::HashSet::new();
        let mut keys = std::collections::HashSet::new();
        for mode in BlendMode::ALL {
            assert!(modes.insert(mode));
            assert!(keys.insert(mode.qa_key()));
            assert!(!mode.label().is_empty());
        }
        let groups =
            BlendMode::ALL
                .iter()
                .map(|mode| mode.group())
                .fold(Vec::new(), |mut groups, group| {
                    if groups.last() != Some(&group) {
                        groups.push(group);
                    }
                    groups
                });
        assert_eq!(groups, BlendModeGroup::ALL);
    }

    #[test]
    fn every_mode_roundtrips_without_accepting_removed_add_variant() {
        for mode in BlendMode::ALL {
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(serde_json::from_str::<BlendMode>(&json).unwrap(), mode);
        }
        assert!(serde_json::from_str::<BlendMode>(r#""Add""#).is_err());
        assert_eq!(
            serde_json::to_string(&BlendMode::LinearDodge).unwrap(),
            r#""LinearDodge""#
        );
    }
}
