//! Typed Point-domain Node contracts shared by Particle and later producers.

use ordered_float::OrderedFloat;

use super::descriptor::{DescriptorIdentity, DescriptorSpec, PortSpec};
use crate::model::frame::point::{
    POINT_GRID_MAX_AXIS, POINT_GRID_MAX_SIZE, POINT_GRID_POSITION_LIMIT,
};
use crate::model::point::PointAttributeElementType;
use crate::model::project::PortDataType;
use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue, Vec3};

pub(crate) const POINT_SOURCE_PORT: &str = "points";
pub(crate) const POINT_ATTRIBUTE_VALUE_PORT: &str = "value";
pub(crate) const POINT_ATTRIBUTE_OUTPUT_PORT: &str = "attribute";
pub(crate) const POINT_POSITION_INPUT_PORT: &str = "position";
pub(crate) const POINT_OFFSET_INPUT_PORT: &str = "offset";
pub(crate) const POINT_SIZE_PORT: &str = "size";
pub(crate) const POINT_SCALE_INPUT_PORT: &str = "scale";
pub(crate) const POINT_SELECTION_INPUT_PORT: &str = "selection";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PointNodeRole {
    Grid,
    Info,
    SetPosition,
    SetSize,
    StoreAttribute(PointAttributeElementType),
}

impl PointNodeRole {
    pub(crate) const fn catalog_id(self) -> &'static str {
        match self {
            Self::Grid => "native.point.grid",
            Self::Info => "native.point.info",
            Self::SetPosition => "native.point.set-position",
            Self::SetSize => "native.point.set-size",
            Self::StoreAttribute(PointAttributeElementType::Number) => {
                "native.point.store-number-attribute"
            }
            Self::StoreAttribute(PointAttributeElementType::Integer) => {
                "native.point.store-integer-attribute"
            }
            Self::StoreAttribute(PointAttributeElementType::Boolean) => {
                "native.point.store-boolean-attribute"
            }
            Self::StoreAttribute(PointAttributeElementType::Vec2) => {
                "native.point.store-vec2-attribute"
            }
            Self::StoreAttribute(PointAttributeElementType::Vec3) => {
                "native.point.store-vec3-attribute"
            }
            Self::StoreAttribute(PointAttributeElementType::Vec4) => {
                "native.point.store-vec4-attribute"
            }
            Self::StoreAttribute(PointAttributeElementType::Color) => {
                "native.point.store-color-attribute"
            }
        }
    }

    pub(crate) fn from_catalog_id(catalog_id: &str) -> Option<Self> {
        [Self::Grid, Self::Info, Self::SetPosition, Self::SetSize]
            .into_iter()
            .chain(STORE_ATTRIBUTE_TYPES.into_iter().map(Self::StoreAttribute))
            .find(|role| role.catalog_id() == catalog_id)
    }

    pub(crate) const fn attribute_type(self) -> Option<PointAttributeElementType> {
        match self {
            Self::StoreAttribute(element_type) => Some(element_type),
            Self::Grid | Self::Info | Self::SetPosition | Self::SetSize => None,
        }
    }
}

const STORE_ATTRIBUTE_TYPES: [PointAttributeElementType; 7] = [
    PointAttributeElementType::Number,
    PointAttributeElementType::Integer,
    PointAttributeElementType::Boolean,
    PointAttributeElementType::Vec2,
    PointAttributeElementType::Vec3,
    PointAttributeElementType::Vec4,
    PointAttributeElementType::Color,
];

const POINT_SOURCE: PortSpec =
    PortSpec::single(POINT_SOURCE_PORT, "Points", PortDataType::PointSource);
const POINT_SOURCE_INPUT: &[PortSpec] = &[POINT_SOURCE];
const POINT_GRID_INPUTS: &[PortSpec] = &[
    PortSpec::single("count_x", "Count X", PortDataType::Integer),
    PortSpec::single("count_y", "Count Y", PortDataType::Integer),
    PortSpec::single("count_z", "Count Z", PortDataType::Integer),
    PortSpec::single("spacing", "Spacing", PortDataType::Vec3),
    PortSpec::single("center", "Center", PortDataType::Vec3),
    PortSpec::single(POINT_SIZE_PORT, "Size", PortDataType::Number),
    PortSpec::single("seed", "Seed", PortDataType::Integer),
];
const POINT_INFO_OUTPUTS: &[PortSpec] = &[
    PortSpec::single("position", "Position", PortDataType::Vec3),
    PortSpec::single(POINT_SIZE_PORT, "Size", PortDataType::Number),
    PortSpec::single("age", "Age", PortDataType::Number),
    PortSpec::single("normalized_age", "Normalized Age", PortDataType::Number),
    PortSpec::single("random", "Random", PortDataType::Number),
];
const POINT_SET_POSITION_INPUTS: &[PortSpec] = &[
    POINT_SOURCE,
    PortSpec::single(POINT_POSITION_INPUT_PORT, "Position", PortDataType::Vec3),
    PortSpec::single(POINT_OFFSET_INPUT_PORT, "Offset", PortDataType::Vec3),
    PortSpec::single(
        POINT_SELECTION_INPUT_PORT,
        "Selection",
        PortDataType::Boolean,
    ),
];
const POINT_SET_SIZE_INPUTS: &[PortSpec] = &[
    POINT_SOURCE,
    PortSpec::single(POINT_SIZE_PORT, "Size", PortDataType::Number),
    PortSpec::single(POINT_SCALE_INPUT_PORT, "Scale", PortDataType::Number),
    PortSpec::single(
        POINT_SELECTION_INPUT_PORT,
        "Selection",
        PortDataType::Boolean,
    ),
];
const fn store_attribute_inputs(data_type: PortDataType) -> [PortSpec; 2] {
    [
        POINT_SOURCE,
        PortSpec::single(POINT_ATTRIBUTE_VALUE_PORT, "Value", data_type),
    ]
}

const fn store_attribute_outputs(data_type: PortDataType) -> [PortSpec; 2] {
    [
        POINT_SOURCE,
        PortSpec::single(POINT_ATTRIBUTE_OUTPUT_PORT, "Attribute", data_type),
    ]
}

static STORE_ATTRIBUTE_INPUTS: [[PortSpec; 2]; 7] = [
    store_attribute_inputs(PortDataType::Number),
    store_attribute_inputs(PortDataType::Integer),
    store_attribute_inputs(PortDataType::Boolean),
    store_attribute_inputs(PortDataType::Vec2),
    store_attribute_inputs(PortDataType::Vec3),
    store_attribute_inputs(PortDataType::Vec4),
    store_attribute_inputs(PortDataType::Color),
];
static STORE_ATTRIBUTE_OUTPUTS: [[PortSpec; 2]; 7] = [
    store_attribute_outputs(PortDataType::Number),
    store_attribute_outputs(PortDataType::Integer),
    store_attribute_outputs(PortDataType::Boolean),
    store_attribute_outputs(PortDataType::Vec2),
    store_attribute_outputs(PortDataType::Vec3),
    store_attribute_outputs(PortDataType::Vec4),
    store_attribute_outputs(PortDataType::Color),
];

macro_rules! store_attribute_spec {
    ($index:expr, $type:ident, $label:literal, $qa_id:literal, $keyword:literal, $properties:ident) => {
        DescriptorSpec::implemented_native(
            DescriptorIdentity::new(
                PointNodeRole::StoreAttribute(PointAttributeElementType::$type).catalog_id(),
                $label,
                "Points",
                $qa_id,
                &["point", "store", "set", $keyword, "attribute"],
            ),
            &STORE_ATTRIBUTE_INPUTS[$index],
            &STORE_ATTRIBUTE_OUTPUTS[$index],
            $properties,
        )
    };
}

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
            &[
                "point",
                "attribute",
                "position",
                "size",
                "age",
                "normalized age",
                "random",
            ],
        ),
        POINT_SOURCE_INPUT,
        POINT_INFO_OUTPUTS,
        no_properties,
    ),
    DescriptorSpec::implemented_native(
        DescriptorIdentity::new(
            PointNodeRole::SetPosition.catalog_id(),
            "Set Point Position",
            "Points",
            "node_editor.menu.create.point_set_position",
            &[
                "point",
                "set",
                "position",
                "offset",
                "transform",
                "geometry",
            ],
        ),
        POINT_SET_POSITION_INPUTS,
        &[POINT_SOURCE],
        set_position_properties,
    ),
    DescriptorSpec::implemented_native(
        DescriptorIdentity::new(
            PointNodeRole::SetSize.catalog_id(),
            "Set Point Size",
            "Points",
            "node_editor.menu.create.point_set_size",
            &["point", "set", "size", "scale", "geometry"],
        ),
        POINT_SET_SIZE_INPUTS,
        &[POINT_SOURCE],
        set_size_properties,
    ),
    store_attribute_spec!(
        0,
        Number,
        "Store Number Attribute",
        "node_editor.menu.create.point_store_number_attribute",
        "number",
        store_number_properties
    ),
    store_attribute_spec!(
        1,
        Integer,
        "Store Integer Attribute",
        "node_editor.menu.create.point_store_integer_attribute",
        "integer",
        store_integer_properties
    ),
    store_attribute_spec!(
        2,
        Boolean,
        "Store Boolean Attribute",
        "node_editor.menu.create.point_store_boolean_attribute",
        "boolean",
        store_boolean_properties
    ),
    store_attribute_spec!(
        3,
        Vec2,
        "Store Vec2 Attribute",
        "node_editor.menu.create.point_store_vec2_attribute",
        "vec2",
        store_vec2_properties
    ),
    store_attribute_spec!(
        4,
        Vec3,
        "Store Vec3 Attribute",
        "node_editor.menu.create.point_store_vec3_attribute",
        "vec3",
        store_vec3_properties
    ),
    store_attribute_spec!(
        5,
        Vec4,
        "Store Vec4 Attribute",
        "node_editor.menu.create.point_store_vec4_attribute",
        "vec4",
        store_vec4_properties
    ),
    store_attribute_spec!(
        6,
        Color,
        "Store Color Attribute",
        "node_editor.menu.create.point_store_color_attribute",
        "color",
        store_color_properties
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
            POINT_SIZE_PORT,
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

fn store_attribute_properties(element_type: PointAttributeElementType) -> Vec<PropertyDefinition> {
    let ui_type = match element_type {
        PointAttributeElementType::Number => PropertyUiType::Float {
            min: -1_000_000.0,
            max: 1_000_000.0,
            step: 0.1,
            suffix: String::new(),
            min_hard_limit: true,
            max_hard_limit: true,
        },
        PointAttributeElementType::Integer => PropertyUiType::Integer {
            min: i64::from(i32::MIN),
            max: i64::from(i32::MAX),
            suffix: String::new(),
            min_hard_limit: true,
            max_hard_limit: true,
        },
        PointAttributeElementType::Boolean => PropertyUiType::Bool,
        PointAttributeElementType::Vec2 => {
            PropertyUiType::vec2_with_range(-1_000_000.0, 1_000_000.0, 0.1, "", true, true)
        }
        PointAttributeElementType::Vec3 => {
            PropertyUiType::vec3_with_range(-1_000_000.0, 1_000_000.0, 0.1, "", true, true)
        }
        PointAttributeElementType::Vec4 => {
            PropertyUiType::vec4_with_range(-1_000_000.0, 1_000_000.0, 0.1, "", true, true)
        }
        PointAttributeElementType::Color => PropertyUiType::ColorValue,
    };
    vec![PropertyDefinition::new(
        POINT_ATTRIBUTE_VALUE_PORT,
        ui_type,
        "Value",
        element_type.default_value(),
    )]
}

fn set_position_properties() -> Vec<PropertyDefinition> {
    vec![
        PropertyDefinition::new(
            POINT_OFFSET_INPUT_PORT,
            PropertyUiType::vec3_with_range(-1_000_000.0, 1_000_000.0, 0.1, " px", true, true),
            "Offset",
            PropertyValue::Vec3(Vec3 {
                x: OrderedFloat(0.0),
                y: OrderedFloat(0.0),
                z: OrderedFloat(0.0),
            }),
        ),
        selection_property(),
    ]
}

fn set_size_properties() -> Vec<PropertyDefinition> {
    vec![
        PropertyDefinition::new(
            POINT_SCALE_INPUT_PORT,
            PropertyUiType::Float {
                min: 0.0,
                max: 1_000_000.0,
                step: 0.01,
                suffix: String::new(),
                min_hard_limit: true,
                max_hard_limit: true,
            },
            "Scale",
            PropertyValue::Number(OrderedFloat(1.0)),
        ),
        selection_property(),
    ]
}

fn selection_property() -> PropertyDefinition {
    PropertyDefinition::new(
        POINT_SELECTION_INPUT_PORT,
        PropertyUiType::Bool,
        "Selection",
        PropertyValue::Boolean(true),
    )
}

macro_rules! store_property_factory {
    ($name:ident, $type:ident) => {
        fn $name() -> Vec<PropertyDefinition> {
            store_attribute_properties(PointAttributeElementType::$type)
        }
    };
}

store_property_factory!(store_number_properties, Number);
store_property_factory!(store_integer_properties, Integer);
store_property_factory!(store_boolean_properties, Boolean);
store_property_factory!(store_vec2_properties, Vec2);
store_property_factory!(store_vec3_properties, Vec3);
store_property_factory!(store_vec4_properties, Vec4);
store_property_factory!(store_color_properties, Color);
