//! First-party authored data leaf Nodes.
//!
//! Values remain canonical [`PropertyValue`] payloads in the authoritative
//! Project. Rendering or interchange adapters are deliberately not involved
//! in these factories, so authored paths and colors cannot be quantized while
//! passing through the graph.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::model::frame::color::Color;
use crate::model::path::{FillRule, PathValue};
use crate::model::project::connection::DATA_VALUE_PROPERTY;
use crate::model::property::{ColorValue, PropertyDefinition, PropertyUiType, PropertyValue};

static COLOR_PROPERTY_DEFINITIONS: LazyLock<[PropertyDefinition; 1]> = LazyLock::new(|| {
    [PropertyDefinition::new(
        DATA_VALUE_PROPERTY,
        PropertyUiType::ColorValue,
        "Value",
        PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&Color {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        })),
    )]
});

static PATH_PROPERTY_DEFINITIONS: LazyLock<[PropertyDefinition; 1]> = LazyLock::new(|| {
    [PropertyDefinition::new(
        DATA_VALUE_PROPERTY,
        PropertyUiType::Path,
        "Value",
        PropertyValue::Path(PathValue::empty(FillRule::NonZero)),
    )]
});

/// Stable persisted identity for canonical authored data sources.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum DataContent {
    Color,
    Path,
}

impl DataContent {
    pub const ALL: [Self; 2] = [Self::Color, Self::Path];

    pub const fn catalog_id(self) -> &'static str {
        match self {
            Self::Color => "native.data.color",
            Self::Path => "native.data.path",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Color => "Color",
            Self::Path => "Path",
        }
    }

    pub fn property_definitions(self) -> &'static [PropertyDefinition] {
        match self {
            Self::Color => COLOR_PROPERTY_DEFINITIONS.as_slice(),
            Self::Path => PATH_PROPERTY_DEFINITIONS.as_slice(),
        }
    }

    pub const fn accepts_value(self, value: &PropertyValue) -> bool {
        matches!(
            (self, value),
            (Self::Color, PropertyValue::ColorValue(_)) | (Self::Path, PropertyValue::Path(_))
        )
    }
}
