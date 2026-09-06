use super::super::super::ValueContent;
use super::super::descriptor::{DescriptorIdentity, DescriptorSpec, NativeNodeFactory, PortSpec};
use crate::model::project::{
    FMOD_DIVISOR_INPUT_PORT, FMOD_X_INPUT_PORT, NUMBER_RESULT_OUTPUT_PORT, NUMERIC_A_INPUT_PORT,
    NUMERIC_B_INPUT_PORT, PortDataType,
};
use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue, Vec3};

pub(crate) const NUMERIC_LENGTH_CATALOG_ID: &str = "native.numeric.length";
pub(crate) const NUMERIC_LENGTH_INPUT_PORT: &str = "value";

const FMOD_INPUTS: &[PortSpec] = &[
    PortSpec::single(FMOD_X_INPUT_PORT, "X", PortDataType::Numeric),
    PortSpec::single(FMOD_DIVISOR_INPUT_PORT, "Divisor", PortDataType::Numeric),
];
const NUMERIC_INPUTS: &[PortSpec] = &[
    PortSpec::single(NUMERIC_A_INPUT_PORT, "A", PortDataType::Numeric),
    PortSpec::single(NUMERIC_B_INPUT_PORT, "B", PortDataType::Numeric),
];
const NUMERIC_OUTPUT: &[PortSpec] = &[PortSpec::single(
    NUMBER_RESULT_OUTPUT_PORT,
    "Result",
    PortDataType::Numeric,
)];
const LENGTH_INPUT: &[PortSpec] = &[PortSpec::single(
    NUMERIC_LENGTH_INPUT_PORT,
    "Value",
    PortDataType::Numeric,
)];
const LENGTH_OUTPUT: &[PortSpec] = &[PortSpec::single(
    NUMBER_RESULT_OUTPUT_PORT,
    "Result",
    PortDataType::Number,
)];

fn length_property_definitions() -> Vec<PropertyDefinition> {
    vec![PropertyDefinition::new(
        NUMERIC_LENGTH_INPUT_PORT,
        PropertyUiType::vec3(""),
        "Value",
        PropertyValue::Vec3(Vec3 {
            x: 0.0.into(),
            y: 0.0.into(),
            z: 0.0.into(),
        }),
    )]
}

const SPECS: &[DescriptorSpec] = &[
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.math.fmod",
            "Fmod",
            "Math",
            "node_editor.menu.create.value:fmod",
            &["modulo", "remainder", "loop", "number", "value"],
        ),
        NativeNodeFactory::Value(ValueContent::Fmod),
        FMOD_INPUTS,
        NUMERIC_OUTPUT,
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.math.add",
            "Add",
            "Math",
            "node_editor.menu.create.value:add",
            &["plus", "sum", "number", "value"],
        ),
        NativeNodeFactory::Value(ValueContent::Add),
        NUMERIC_INPUTS,
        NUMERIC_OUTPUT,
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.math.subtract",
            "Subtract",
            "Math",
            "node_editor.menu.create.value:subtract",
            &["minus", "difference", "number", "value"],
        ),
        NativeNodeFactory::Value(ValueContent::Subtract),
        NUMERIC_INPUTS,
        NUMERIC_OUTPUT,
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.math.multiply",
            "Multiply",
            "Math",
            "node_editor.menu.create.value:multiply",
            &["times", "product", "number", "value"],
        ),
        NativeNodeFactory::Value(ValueContent::Multiply),
        NUMERIC_INPUTS,
        NUMERIC_OUTPUT,
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.math.divide",
            "Divide",
            "Math",
            "node_editor.menu.create.value:divide",
            &["quotient", "ratio", "number", "value"],
        ),
        NativeNodeFactory::Value(ValueContent::Divide),
        NUMERIC_INPUTS,
        NUMERIC_OUTPUT,
    ),
    DescriptorSpec::implemented_native(
        DescriptorIdentity::new(
            NUMERIC_LENGTH_CATALOG_ID,
            "Length",
            "Math",
            "node_editor.menu.create.numeric:length",
            &["magnitude", "norm", "distance", "vector", "number", "value"],
        ),
        LENGTH_INPUT,
        LENGTH_OUTPUT,
        length_property_definitions,
    ),
];

pub(super) const fn specs() -> &'static [DescriptorSpec] {
    SPECS
}
