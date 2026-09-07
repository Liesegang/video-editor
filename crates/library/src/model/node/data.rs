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
use crate::model::property::{
    ColorValue, GradientValue, ImageCollectionValue, PropertyDefinition, PropertyUiType,
    PropertyValue,
};

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

static GRADIENT_PROPERTY_DEFINITIONS: LazyLock<[PropertyDefinition; 1]> = LazyLock::new(|| {
    [PropertyDefinition::new(
        DATA_VALUE_PROPERTY,
        PropertyUiType::Gradient,
        "Value",
        PropertyValue::Gradient(GradientValue::default()),
    )]
});

static IMAGE_COLLECTION_PROPERTY_DEFINITIONS: LazyLock<[PropertyDefinition; 1]> =
    LazyLock::new(|| {
        [PropertyDefinition::new(
            DATA_VALUE_PROPERTY,
            PropertyUiType::ImageCollection,
            "Value",
            PropertyValue::ImageCollection(ImageCollectionValue::default()),
        )]
    });

/// Stable persisted identity for canonical authored data sources.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum DataContent {
    Color,
    Gradient,
    ImageCollection,
    Path,
}

impl DataContent {
    pub const ALL: [Self; 4] = [
        Self::Color,
        Self::Gradient,
        Self::ImageCollection,
        Self::Path,
    ];

    pub const fn catalog_id(self) -> &'static str {
        match self {
            Self::Color => "native.data.color",
            Self::Gradient => "native.data.gradient",
            Self::ImageCollection => "native.data.image-collection",
            Self::Path => "native.data.path",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Color => "Color",
            Self::Gradient => "Gradient",
            Self::ImageCollection => "Image Collection",
            Self::Path => "Path",
        }
    }

    pub fn property_definitions(self) -> &'static [PropertyDefinition] {
        match self {
            Self::Color => COLOR_PROPERTY_DEFINITIONS.as_slice(),
            Self::Gradient => GRADIENT_PROPERTY_DEFINITIONS.as_slice(),
            Self::ImageCollection => IMAGE_COLLECTION_PROPERTY_DEFINITIONS.as_slice(),
            Self::Path => PATH_PROPERTY_DEFINITIONS.as_slice(),
        }
    }

    pub const fn accepts_value(self, value: &PropertyValue) -> bool {
        matches!(
            (self, value),
            (Self::Color, PropertyValue::ColorValue(_))
                | (Self::Gradient, PropertyValue::Gradient(_))
                | (Self::ImageCollection, PropertyValue::ImageCollection(_))
                | (Self::Path, PropertyValue::Path(_))
        )
    }
}
