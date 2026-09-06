//! Typed Point-domain Node contracts shared by Particle and later producers.

use ordered_float::OrderedFloat;

use super::descriptor::{DescriptorIdentity, DescriptorSpec, PortSpec};
use crate::model::frame::point::{
    POINT_GRID_MAX_AXIS, POINT_GRID_MAX_SIZE, POINT_GRID_POSITION_LIMIT,
};
use crate::model::project::PortDataType;
use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue, Vec3};

pub(crate) const POINT_SOURCE_PORT: &str = "points";
pub(crate) const POINT_ATTRIBUTE_VALUE_PORT: &str = "value";
pub(crate) const POINT_ATTRIBUTE_OUTPUT_PORT: &str = "attribute";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PointNodeRole {
    Grid,
    Info,
    StoreNumberAttribute,
}

impl PointNodeRole {
    pub(crate) const fn catalog_id(self) -> &'static str {
        match self {
            Self::Grid => "native.point.grid",
            Self::Info => "native.point.info",
            Self::StoreNumberAttribute => "native.point.store-number-attribute",
        }
    }

    pub(crate) fn from_catalog_id(catalog_id: &str) -> Option<Self> {
        [Self::Grid, Self::Info, Self::StoreNumberAttribute]
            .into_iter()
            .find(|role| role.catalog_id() == catalog_id)
    }
}

const POINT_SOURCE: PortSpec =
    PortSpec::single(POINT_SOURCE_PORT, "Points", PortDataType::PointSource);
const POINT_SOURCE_INPUT: &[PortSpec] = &[POINT_SOURCE];
const POINT_GRID_INPUTS: &[PortSpec] = &[
    PortSpec::single("count_x", "Count X", PortDataType::Integer),
    PortSpec::single("count_y", "Count Y", PortDataType::Integer),
    PortSpec::single("count_z", "Count Z", PortDataType::Integer),
    PortSpec::single("spacing", "Spacing", PortDataType::Vec3),
    PortSpec::single("center", "Center", PortDataType::Vec3),
    PortSpec::single("size", "Size", PortDataType::Number),
    PortSpec::single("seed", "Seed", PortDataType::Integer),
];
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
            PointNodeRole::Grid.catalog_id(),
            "Point Grid",
            "Points",
            "node_editor.menu.create.point_grid",
            &["point", "grid", "lattice", "generator"],
        ),
        POINT_GRID_INPUTS,
        &[POINT_SOURCE],
        point_grid_properties,
    ),
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

fn point_grid_properties() -> Vec<PropertyDefinition> {
    let count = |name, label, default| {
        PropertyDefinition::new(
            name,
            PropertyUiType::Integer {
                min: 1,
                max: i64::from(POINT_GRID_MAX_AXIS),
                suffix: String::new(),
                min_hard_limit: true,
                max_hard_limit: true,
            },
            label,
            PropertyValue::Integer(default),
        )
    };
    let vector = |name, label, default: [f64; 3]| {
        PropertyDefinition::new(
            name,
            PropertyUiType::vec3_with_range(
                -POINT_GRID_POSITION_LIMIT,
                POINT_GRID_POSITION_LIMIT,
                0.1,
                " px",
                true,
                true,
            ),
            label,
            PropertyValue::Vec3(Vec3 {
                x: OrderedFloat(default[0]),
                y: OrderedFloat(default[1]),
                z: OrderedFloat(default[2]),
            }),
        )
    };
    vec![
        count("count_x", "Count X", 8),
        count("count_y", "Count Y", 8),
        count("count_z", "Count Z", 1),
        vector("spacing", "Spacing", [24.0, 24.0, 24.0]),
        vector("center", "Center", [0.0, 0.0, 0.0]),
        PropertyDefinition::new(
            "size",
            PropertyUiType::Float {
                min: 0.000_001,
                max: f64::from(POINT_GRID_MAX_SIZE),
                step: 0.1,
                suffix: " px".to_string(),
                min_hard_limit: true,
                max_hard_limit: true,
            },
            "Size",
            PropertyValue::Number(OrderedFloat(8.0)),
        ),
        PropertyDefinition::new(
            "seed",
            PropertyUiType::Integer {
                min: 0,
                max: i64::from(u32::MAX),
                suffix: String::new(),
                min_hard_limit: true,
                max_hard_limit: true,
            },
            "Seed",
            PropertyValue::Integer(1),
        ),
    ]
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
