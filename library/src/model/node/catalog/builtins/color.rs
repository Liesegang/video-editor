use super::super::super::{
    COLOR_ALPHA_PORT, COLOR_BLUE_PORT, COLOR_GREEN_PORT, COLOR_MIX_FACTOR_PORT,
    COLOR_MIX_LEFT_PORT, COLOR_MIX_RIGHT_PORT, COLOR_RED_PORT, COLOR_SPACE_PORT,
    COLOR_TARGET_SPACE_PORT, COLOR_VALUE_PORT, ColorContent,
};
use super::super::descriptor::{DescriptorIdentity, DescriptorSpec, NativeNodeFactory, PortSpec};
use crate::model::project::{PortDataType, TIME_PORT};

const COMPOSE_INPUTS: &[PortSpec] = &[
    PortSpec::single(TIME_PORT, "Time", PortDataType::Number),
    PortSpec::single(COLOR_SPACE_PORT, "Color Space", PortDataType::String),
    PortSpec::single(COLOR_RED_PORT, "Red", PortDataType::Number),
    PortSpec::single(COLOR_GREEN_PORT, "Green", PortDataType::Number),
    PortSpec::single(COLOR_BLUE_PORT, "Blue", PortDataType::Number),
    PortSpec::single(COLOR_ALPHA_PORT, "Alpha", PortDataType::Number),
];
const COLOR_OUTPUT: &[PortSpec] = &[PortSpec::single(
    COLOR_VALUE_PORT,
    "Color",
    PortDataType::Color,
)];
const SPLIT_INPUTS: &[PortSpec] = &[
    PortSpec::single(TIME_PORT, "Time", PortDataType::Number),
    PortSpec::single(COLOR_VALUE_PORT, "Color", PortDataType::Color),
];
const SPLIT_OUTPUTS: &[PortSpec] = &[
    PortSpec::single(COLOR_SPACE_PORT, "Color Space", PortDataType::String),
    PortSpec::single(COLOR_RED_PORT, "Red", PortDataType::Number),
    PortSpec::single(COLOR_GREEN_PORT, "Green", PortDataType::Number),
    PortSpec::single(COLOR_BLUE_PORT, "Blue", PortDataType::Number),
    PortSpec::single(COLOR_ALPHA_PORT, "Alpha", PortDataType::Number),
];
const MIX_INPUTS: &[PortSpec] = &[
    PortSpec::single(TIME_PORT, "Time", PortDataType::Number),
    PortSpec::single(COLOR_MIX_LEFT_PORT, "A", PortDataType::Color),
    PortSpec::single(COLOR_MIX_RIGHT_PORT, "B", PortDataType::Color),
    PortSpec::single(COLOR_MIX_FACTOR_PORT, "Factor", PortDataType::Number),
];
const CONVERT_SPACE_INPUTS: &[PortSpec] = &[
    PortSpec::single(TIME_PORT, "Time", PortDataType::Number),
    PortSpec::single(COLOR_VALUE_PORT, "Color", PortDataType::Color),
    PortSpec::single(
        COLOR_TARGET_SPACE_PORT,
        "Target Space",
        PortDataType::String,
    ),
];

const SPECS: &[DescriptorSpec] = &[
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.color.compose",
            "Compose Color",
            "Color",
            "node_editor.menu.create.color:compose",
            &["color", "rgba", "compose", "combine", "hdr", "data"],
        ),
        NativeNodeFactory::Color(ColorContent::Compose),
        COMPOSE_INPUTS,
        COLOR_OUTPUT,
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.color.split",
            "Split Color",
            "Color",
            "node_editor.menu.create.color:split",
            &["color", "rgba", "split", "separate", "channels", "data"],
        ),
        NativeNodeFactory::Color(ColorContent::Split),
        SPLIT_INPUTS,
        SPLIT_OUTPUTS,
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.color.mix",
            "Mix Color",
            "Color",
            "node_editor.menu.create.color:mix",
            &["color", "mix", "lerp", "interpolate", "hdr", "data"],
        ),
        NativeNodeFactory::Color(ColorContent::Mix),
        MIX_INPUTS,
        COLOR_OUTPUT,
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.color.convert_space",
            "Convert Color Space",
            "Color",
            "node_editor.menu.create.color:convert_space",
            &[
                "color",
                "space",
                "convert",
                "transform",
                "linear",
                "srgb",
                "transfer",
            ],
        ),
        NativeNodeFactory::Color(ColorContent::ConvertSpace),
        CONVERT_SPACE_INPUTS,
        COLOR_OUTPUT,
    ),
];

pub(super) const fn specs() -> &'static [DescriptorSpec] {
    SPECS
}
