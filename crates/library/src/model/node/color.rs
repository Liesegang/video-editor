//! Lossless graph operations for color-space-tagged straight RGBA values.
//!
//! These operations stay in the metadata graph. They never cross the current
//! RGBA8 renderer boundary, so negative and HDR RGB components remain exact.
//! Color-space conversion is intentionally not inferred: Mix requires both
//! inputs to carry the same explicit [`ColorSpaceRef`].

use std::sync::LazyLock;

use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

use crate::model::frame::color::Color;
use crate::model::property::{
    ColorSpaceRef, ColorValue, PropertyDefinition, PropertyUiType, PropertyValue,
};

pub const COLOR_SPACE_PORT: &str = "space";
pub const COLOR_RED_PORT: &str = "r";
pub const COLOR_GREEN_PORT: &str = "g";
pub const COLOR_BLUE_PORT: &str = "b";
pub const COLOR_ALPHA_PORT: &str = "a";
pub const COLOR_VALUE_PORT: &str = "color";
pub const COLOR_MIX_LEFT_PORT: &str = "a";
pub const COLOR_MIX_RIGHT_PORT: &str = "b";
pub const COLOR_MIX_FACTOR_PORT: &str = "factor";
pub const COLOR_TARGET_SPACE_PORT: &str = "target_space";

fn number_definition(name: &str, label: &str, default: f64) -> PropertyDefinition {
    PropertyDefinition::new(
        name,
        PropertyUiType::Float {
            min: -16.0,
            max: 16.0,
            step: 0.01,
            suffix: String::new(),
            min_hard_limit: false,
            max_hard_limit: false,
        },
        label,
        PropertyValue::Number(OrderedFloat(default)),
    )
}

fn alpha_definition() -> PropertyDefinition {
    PropertyDefinition::new(
        COLOR_ALPHA_PORT,
        PropertyUiType::Float {
            min: 0.0,
            max: 1.0,
            step: 0.01,
            suffix: String::new(),
            min_hard_limit: true,
            max_hard_limit: true,
        },
        "Alpha",
        PropertyValue::Number(OrderedFloat(1.0)),
    )
}

fn default_color(color: Color) -> PropertyValue {
    PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&color))
}

static COMPOSE_PROPERTY_DEFINITIONS: LazyLock<[PropertyDefinition; 5]> = LazyLock::new(|| {
    [
        PropertyDefinition::new(
            COLOR_SPACE_PORT,
            PropertyUiType::Text,
            "Color Space",
            PropertyValue::String(ColorSpaceRef::srgb().to_string()),
        ),
        number_definition(COLOR_RED_PORT, "Red", 1.0),
        number_definition(COLOR_GREEN_PORT, "Green", 1.0),
        number_definition(COLOR_BLUE_PORT, "Blue", 1.0),
        alpha_definition(),
    ]
});

static SPLIT_PROPERTY_DEFINITIONS: LazyLock<[PropertyDefinition; 1]> = LazyLock::new(|| {
    [PropertyDefinition::new(
        COLOR_VALUE_PORT,
        PropertyUiType::ColorValue,
        "Color",
        default_color(Color {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        }),
    )]
});

static MIX_PROPERTY_DEFINITIONS: LazyLock<[PropertyDefinition; 3]> = LazyLock::new(|| {
    [
        PropertyDefinition::new(
            COLOR_MIX_LEFT_PORT,
            PropertyUiType::ColorValue,
            "A",
            default_color(Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            }),
        ),
        PropertyDefinition::new(
            COLOR_MIX_RIGHT_PORT,
            PropertyUiType::ColorValue,
            "B",
            default_color(Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            }),
        ),
        PropertyDefinition::new(
            COLOR_MIX_FACTOR_PORT,
            PropertyUiType::Float {
                min: 0.0,
                max: 1.0,
                step: 0.01,
                suffix: String::new(),
                min_hard_limit: true,
                max_hard_limit: true,
            },
            "Factor",
            PropertyValue::Number(OrderedFloat(0.5)),
        ),
    ]
});

static CONVERT_SPACE_PROPERTY_DEFINITIONS: LazyLock<[PropertyDefinition; 2]> =
    LazyLock::new(|| {
        [
            PropertyDefinition::new(
                COLOR_VALUE_PORT,
                PropertyUiType::ColorValue,
                "Color",
                default_color(Color {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                }),
            ),
            PropertyDefinition::new(
                COLOR_TARGET_SPACE_PORT,
                PropertyUiType::Text,
                "Target Space",
                PropertyValue::String(
                    crate::color_management::working_linear_srgb_space_id().to_string(),
                ),
            ),
        ]
    });

/// Stable persisted identity for lossless first-party Color operations.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorContent {
    Compose,
    Split,
    Mix,
    ConvertSpace,
}

impl ColorContent {
    pub const ALL: [Self; 4] = [Self::Compose, Self::Split, Self::Mix, Self::ConvertSpace];

    pub const fn catalog_id(self) -> &'static str {
        match self {
            Self::Compose => "native.color.compose",
            Self::Split => "native.color.split",
            Self::Mix => "native.color.mix",
            Self::ConvertSpace => "native.color.convert_space",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Compose => "Compose Color",
            Self::Split => "Split Color",
            Self::Mix => "Mix Color",
            Self::ConvertSpace => "Convert Color Space",
        }
    }

    pub fn property_definitions(self) -> &'static [PropertyDefinition] {
        match self {
            Self::Compose => COMPOSE_PROPERTY_DEFINITIONS.as_slice(),
            Self::Split => SPLIT_PROPERTY_DEFINITIONS.as_slice(),
            Self::Mix => MIX_PROPERTY_DEFINITIONS.as_slice(),
            Self::ConvertSpace => CONVERT_SPACE_PROPERTY_DEFINITIONS.as_slice(),
        }
    }

    pub fn accepts_property(self, key: &str, value: &PropertyValue) -> bool {
        let Some(definition) = self
            .property_definitions()
            .iter()
            .find(|definition| definition.name() == key)
        else {
            return false;
        };
        if definition.validate_value(value).is_ok() {
            return true;
        }
        // Legacy straight sRGBA8 is a lossless source for a canonical Color
        // input even though new authoring always stores ColorValue.
        matches!(
            (definition.ui_type(), value),
            (
                PropertyUiType::ColorValue,
                PropertyValue::Color(Color { .. })
            )
        )
    }
}
