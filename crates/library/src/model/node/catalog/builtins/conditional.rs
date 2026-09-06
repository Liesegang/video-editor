//! First-party scalar comparisons and exact typed conditional selection.

use super::super::descriptor::{DescriptorIdentity, DescriptorSpec, PortSpec};
use crate::model::ComparisonOperation;
use crate::model::frame::color::Color;
use crate::model::project::{
    NUMBER_RESULT_OUTPUT_PORT, NUMERIC_A_INPUT_PORT, NUMERIC_B_INPUT_PORT, PortDataType,
};
use crate::model::property::{
    ColorValue, PropertyDefinition, PropertyUiType, PropertyValue, Vec2, Vec3, Vec4,
};

pub(crate) const CONDITION_INPUT_PORT: &str = "condition";
pub(crate) const SELECT_TRUE_INPUT_PORT: &str = "if_true";
pub(crate) const SELECT_FALSE_INPUT_PORT: &str = "if_false";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ConditionalNodeRole {
    Compare(ComparisonOperation),
    Select(PortDataType),
}

impl ConditionalNodeRole {
    #[expect(
        clippy::panic,
        reason = "only descriptor constants and roles returned by from_catalog_id call this; arbitrary PortDataType values never cross the catalog boundary"
    )]
    pub(crate) const fn catalog_id(self) -> &'static str {
        match self {
            Self::Compare(ComparisonOperation::Less) => "native.logic.less",
            Self::Compare(ComparisonOperation::LessEqual) => "native.logic.less-equal",
            Self::Compare(ComparisonOperation::Greater) => "native.logic.greater",
            Self::Compare(ComparisonOperation::GreaterEqual) => "native.logic.greater-equal",
            Self::Compare(ComparisonOperation::Equal) => "native.logic.equal",
            Self::Compare(ComparisonOperation::NotEqual) => "native.logic.not-equal",
            Self::Select(PortDataType::Number) => "native.logic.select-number",
            Self::Select(PortDataType::Integer) => "native.logic.select-integer",
            Self::Select(PortDataType::Vec2) => "native.logic.select-vec2",
            Self::Select(PortDataType::Vec3) => "native.logic.select-vec3",
            Self::Select(PortDataType::Vec4) => "native.logic.select-vec4",
            Self::Select(PortDataType::Color) => "native.logic.select-color",
            Self::Select(PortDataType::Boolean) => "native.logic.select-boolean",
            Self::Select(_) => panic!("Select requires one exact PropertyValue port type"),
        }
    }

    pub(crate) fn from_catalog_id(catalog_id: &str) -> Option<Self> {
        ComparisonOperation::ALL
            .into_iter()
            .map(Self::Compare)
            .chain(SELECT_TYPES.into_iter().map(Self::Select))
            .find(|role| role.catalog_id() == catalog_id)
    }
}

const SELECT_TYPES: [PortDataType; 7] = [
    PortDataType::Number,
    PortDataType::Integer,
    PortDataType::Vec2,
    PortDataType::Vec3,
    PortDataType::Vec4,
    PortDataType::Color,
    PortDataType::Boolean,
];

const COMPARE_INPUTS: &[PortSpec] = &[
    PortSpec::single(NUMERIC_A_INPUT_PORT, "A", PortDataType::Number),
    PortSpec::single(NUMERIC_B_INPUT_PORT, "B", PortDataType::Number),
];
const BOOLEAN_RESULT: &[PortSpec] = &[PortSpec::single(
    NUMBER_RESULT_OUTPUT_PORT,
    "Result",
    PortDataType::Boolean,
)];

const fn select_inputs(data_type: PortDataType) -> [PortSpec; 3] {
    [
        PortSpec::single(CONDITION_INPUT_PORT, "Condition", PortDataType::Boolean),
        PortSpec::single(SELECT_TRUE_INPUT_PORT, "True", data_type),
        PortSpec::single(SELECT_FALSE_INPUT_PORT, "False", data_type),
    ]
}

const fn select_output(data_type: PortDataType) -> [PortSpec; 1] {
    [PortSpec::single(
        NUMBER_RESULT_OUTPUT_PORT,
        "Result",
        data_type,
    )]
}

static SELECT_INPUTS: [[PortSpec; 3]; 7] = [
    select_inputs(PortDataType::Number),
    select_inputs(PortDataType::Integer),
    select_inputs(PortDataType::Vec2),
    select_inputs(PortDataType::Vec3),
    select_inputs(PortDataType::Vec4),
    select_inputs(PortDataType::Color),
    select_inputs(PortDataType::Boolean),
];
static SELECT_OUTPUTS: [[PortSpec; 1]; 7] = [
    select_output(PortDataType::Number),
    select_output(PortDataType::Integer),
    select_output(PortDataType::Vec2),
    select_output(PortDataType::Vec3),
    select_output(PortDataType::Vec4),
    select_output(PortDataType::Color),
    select_output(PortDataType::Boolean),
];

macro_rules! compare_spec {
    ($operation:ident, $label:literal, $qa:literal, $keyword:literal) => {
        DescriptorSpec::implemented_native(
            DescriptorIdentity::new(
                ConditionalNodeRole::Compare(ComparisonOperation::$operation).catalog_id(),
                $label,
                "Logic",
                $qa,
                &["compare", "number", "logic", $keyword],
            ),
            COMPARE_INPUTS,
            BOOLEAN_RESULT,
            comparison_properties,
        )
    };
}

macro_rules! select_spec {
    ($index:expr, $type:ident, $label:literal, $qa:literal, $keyword:literal, $properties:ident) => {
        DescriptorSpec::implemented_native(
            DescriptorIdentity::new(
                ConditionalNodeRole::Select(PortDataType::$type).catalog_id(),
                $label,
                "Logic",
                $qa,
                &["select", "condition", "branch", "logic", $keyword],
            ),
            &SELECT_INPUTS[$index],
            &SELECT_OUTPUTS[$index],
            $properties,
        )
    };
}

const SPECS: &[DescriptorSpec] = &[
    compare_spec!(
        Less,
        "Less Than",
        "node_editor.menu.create.logic:less",
        "less"
    ),
    compare_spec!(
        LessEqual,
        "Less Than or Equal",
        "node_editor.menu.create.logic:less-equal",
        "less equal"
    ),
    compare_spec!(
        Greater,
        "Greater Than",
        "node_editor.menu.create.logic:greater",
        "greater"
    ),
    compare_spec!(
        GreaterEqual,
        "Greater Than or Equal",
        "node_editor.menu.create.logic:greater-equal",
        "greater equal"
    ),
    compare_spec!(
        Equal,
        "Equal",
        "node_editor.menu.create.logic:equal",
        "equal"
    ),
    compare_spec!(
        NotEqual,
        "Not Equal",
        "node_editor.menu.create.logic:not-equal",
        "not equal"
    ),
    select_spec!(
        0,
        Number,
        "Select Number",
        "node_editor.menu.create.logic:select-number",
        "number",
        select_number_properties
    ),
    select_spec!(
        1,
        Integer,
        "Select Integer",
        "node_editor.menu.create.logic:select-integer",
        "integer",
        select_integer_properties
    ),
    select_spec!(
        2,
        Vec2,
        "Select Vec2",
        "node_editor.menu.create.logic:select-vec2",
        "vec2",
        select_vec2_properties
    ),
    select_spec!(
        3,
        Vec3,
        "Select Vec3",
        "node_editor.menu.create.logic:select-vec3",
        "vec3",
        select_vec3_properties
    ),
    select_spec!(
        4,
        Vec4,
        "Select Vec4",
        "node_editor.menu.create.logic:select-vec4",
        "vec4",
        select_vec4_properties
    ),
    select_spec!(
        5,
        Color,
        "Select Color",
        "node_editor.menu.create.logic:select-color",
        "color",
        select_color_properties
    ),
    select_spec!(
        6,
        Boolean,
        "Select Boolean",
        "node_editor.menu.create.logic:select-boolean",
        "boolean",
        select_boolean_properties
    ),
];

pub(super) const fn specs() -> &'static [DescriptorSpec] {
    SPECS
}

fn comparison_properties() -> Vec<PropertyDefinition> {
    vec![
        number_property(NUMERIC_A_INPUT_PORT, "A"),
        number_property(NUMERIC_B_INPUT_PORT, "B"),
    ]
}

fn select_properties(ui_type: PropertyUiType, default: PropertyValue) -> Vec<PropertyDefinition> {
    vec![
        PropertyDefinition::new(
            CONDITION_INPUT_PORT,
            PropertyUiType::Bool,
            "Condition",
            PropertyValue::Boolean(false),
        ),
        PropertyDefinition::new(
            SELECT_TRUE_INPUT_PORT,
            ui_type.clone(),
            "True",
            default.clone(),
        ),
        PropertyDefinition::new(SELECT_FALSE_INPUT_PORT, ui_type, "False", default),
    ]
}

fn number_property(key: &str, label: &str) -> PropertyDefinition {
    PropertyDefinition::new(
        key,
        number_ui_type(),
        label,
        PropertyValue::Number(0.0.into()),
    )
}

fn number_ui_type() -> PropertyUiType {
    PropertyUiType::Float {
        min: -1_000_000.0,
        max: 1_000_000.0,
        step: 0.1,
        suffix: String::new(),
        min_hard_limit: false,
        max_hard_limit: false,
    }
}

fn select_number_properties() -> Vec<PropertyDefinition> {
    select_properties(number_ui_type(), PropertyValue::Number(0.0.into()))
}

fn select_integer_properties() -> Vec<PropertyDefinition> {
    select_properties(
        PropertyUiType::Integer {
            min: i64::MIN,
            max: i64::MAX,
            suffix: String::new(),
            min_hard_limit: false,
            max_hard_limit: false,
        },
        PropertyValue::Integer(0),
    )
}

fn select_vec2_properties() -> Vec<PropertyDefinition> {
    select_properties(
        PropertyUiType::vec2(""),
        PropertyValue::Vec2(Vec2 {
            x: 0.0.into(),
            y: 0.0.into(),
        }),
    )
}

fn select_vec3_properties() -> Vec<PropertyDefinition> {
    select_properties(
        PropertyUiType::vec3(""),
        PropertyValue::Vec3(Vec3 {
            x: 0.0.into(),
            y: 0.0.into(),
            z: 0.0.into(),
        }),
    )
}

fn select_vec4_properties() -> Vec<PropertyDefinition> {
    select_properties(
        PropertyUiType::vec4(""),
        PropertyValue::Vec4(Vec4 {
            x: 0.0.into(),
            y: 0.0.into(),
            z: 0.0.into(),
            w: 0.0.into(),
        }),
    )
}

fn select_color_properties() -> Vec<PropertyDefinition> {
    select_properties(
        PropertyUiType::ColorValue,
        PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&Color::white())),
    )
}

fn select_boolean_properties() -> Vec<PropertyDefinition> {
    select_properties(PropertyUiType::Bool, PropertyValue::Boolean(false))
}
