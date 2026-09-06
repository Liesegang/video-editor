//! Typed Point-domain Node contracts shared by Particle and later producers.

use ordered_float::OrderedFloat;

use super::descriptor::{DescriptorIdentity, DescriptorSpec, PortSpec};
use crate::model::project::PortDataType;
use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue};

pub(crate) const POINT_SOURCE_PORT: &str = "points";
pub(crate) const POINT_ATTRIBUTE_VALUE_PORT: &str = "value";
pub(crate) const POINT_ATTRIBUTE_OUTPUT_PORT: &str = "attribute";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PointNodeRole {
    Info,
    StoreNumberAttribute,
}

impl PointNodeRole {
    pub(crate) const fn catalog_id(self) -> &'static str {
        match self {
            Self::Info => "native.point.info",
            Self::StoreNumberAttribute => "native.point.store-number-attribute",
        }
    }

    pub(crate) fn from_catalog_id(catalog_id: &str) -> Option<Self> {
        [Self::Info, Self::StoreNumberAttribute]
            .into_iter()
            .find(|role| role.catalog_id() == catalog_id)
    }
}

const POINT_SOURCE: PortSpec =
    PortSpec::single(POINT_SOURCE_PORT, "Points", PortDataType::PointSource);
const POINT_SOURCE_INPUT: &[PortSpec] = &[POINT_SOURCE];
const POINT_INFO_OUTPUTS: &[PortSpec] = &[
    PortSpec::single("age", "Age", PortDataType::Number),
    PortSpec::single("normalized_age", "Normalized Age", PortDataType::Number),
    PortSpec::single("random", "Random", PortDataType::Number),
];
const STORE_NUMBER_INPUTS: &[PortSpec] = &[
    POINT_SOURCE,
    PortSpec::single(POINT_ATTRIBUTE_VALUE_PORT, "Value", PortDataType::Number),
];
const STORE_NUMBER_OUTPUTS: &[PortSpec] = &[
    POINT_SOURCE,
    PortSpec::single(
        POINT_ATTRIBUTE_OUTPUT_PORT,
        "Attribute",
        PortDataType::Number,
    ),
];

const SPECS: &[DescriptorSpec] = &[
    DescriptorSpec::implemented_native(
        DescriptorIdentity::new(
            PointNodeRole::Info.catalog_id(),
            "Point Info",
            "Points",
            "node_editor.menu.create.point_info",
            &["point", "attribute", "age", "normalized age", "random"],
        ),
        POINT_SOURCE_INPUT,
        POINT_INFO_OUTPUTS,
        no_properties,
    ),
    DescriptorSpec::implemented_native(
        DescriptorIdentity::new(
            PointNodeRole::StoreNumberAttribute.catalog_id(),
            "Store Number Attribute",
            "Points",
            "node_editor.menu.create.point_store_number_attribute",
            &["point", "store", "set", "number", "attribute"],
        ),
        STORE_NUMBER_INPUTS,
        STORE_NUMBER_OUTPUTS,
        store_number_properties,
    ),
];

pub(super) const fn specs() -> &'static [DescriptorSpec] {
    SPECS
}

fn no_properties() -> Vec<PropertyDefinition> {
    Vec::new()
}

fn store_number_properties() -> Vec<PropertyDefinition> {
    vec![PropertyDefinition::new(
        POINT_ATTRIBUTE_VALUE_PORT,
        PropertyUiType::Float {
            min: -1_000_000.0,
            max: 1_000_000.0,
            step: 0.1,
            suffix: String::new(),
            min_hard_limit: true,
            max_hard_limit: true,
        },
        "Value",
        PropertyValue::Number(OrderedFloat(0.0)),
    )]
}
