//! First-party parameterized 2D primitive Shape sources.

use ordered_float::OrderedFloat;

use super::super::descriptor::{DescriptorIdentity, DescriptorSpec, PortSpec};
use super::super::{ELLIPSE_SHAPE_CATALOG_ID, RECTANGLE_SHAPE_CATALOG_ID};
use crate::model::project::{PortDataType, SHAPE_OUTPUT_PORT};
use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue};

const SIZE_INPUTS: &[PortSpec] = &[
    PortSpec::single("width", "Width", PortDataType::Number),
    PortSpec::single("height", "Height", PortDataType::Number),
];
const SHAPE_OUTPUT: &[PortSpec] = &[PortSpec::single(
    SHAPE_OUTPUT_PORT,
    "Shape",
    PortDataType::Shape,
)];

const SPECS: &[DescriptorSpec] = &[
    DescriptorSpec::implemented_native(
        DescriptorIdentity::new(
            RECTANGLE_SHAPE_CATALOG_ID,
            "Rectangle",
            "Generators",
            "node_editor.menu.create.rectangle",
            &["shape", "rectangle", "box", "primitive"],
        ),
        SIZE_INPUTS,
        SHAPE_OUTPUT,
        size_properties,
    ),
    DescriptorSpec::implemented_native(
        DescriptorIdentity::new(
            ELLIPSE_SHAPE_CATALOG_ID,
            "Ellipse",
            "Generators",
            "node_editor.menu.create.ellipse",
            &["shape", "ellipse", "circle", "oval", "primitive"],
        ),
        SIZE_INPUTS,
        SHAPE_OUTPUT,
        size_properties,
    ),
];

fn size_properties() -> Vec<PropertyDefinition> {
    ["width", "height"]
        .into_iter()
        .map(|key| {
            let label = if key == "width" { "Width" } else { "Height" };
            PropertyDefinition::new(
                key,
                PropertyUiType::Float {
                    min: f64::EPSILON,
                    max: 100_000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: true,
                    max_hard_limit: false,
                },
                label,
                PropertyValue::Number(OrderedFloat(100.0)),
            )
        })
        .collect()
}

pub(super) const fn specs() -> &'static [DescriptorSpec] {
    SPECS
}
