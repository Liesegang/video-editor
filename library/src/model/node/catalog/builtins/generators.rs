use super::super::super::GeneratorContent;
use super::super::descriptor::{DescriptorIdentity, DescriptorSpec, NativeNodeFactory, PortSpec};
use crate::model::project::{IMAGE_OUTPUT_PORT, PortDataType, SHAPE_OUTPUT_PORT, TIME_PORT};

const TEXT_INPUTS: &[PortSpec] = &[
    PortSpec::single(TIME_PORT, "Time", PortDataType::Number),
    PortSpec::single("text", "Text", PortDataType::String),
    PortSpec::single("font_family", "Font", PortDataType::String),
    PortSpec::single("size", "Size", PortDataType::Number),
];
const SHAPE_OUTPUT: &[PortSpec] = &[PortSpec::single(
    SHAPE_OUTPUT_PORT,
    "Shape",
    PortDataType::Shape,
)];
const SOLID_INPUTS: &[PortSpec] = &[
    PortSpec::single(TIME_PORT, "Time", PortDataType::Number),
    PortSpec::single("color", "Color", PortDataType::Color),
];
const SHAPE_INPUTS: &[PortSpec] = &[
    PortSpec::single(TIME_PORT, "Time", PortDataType::Number),
    PortSpec::single("path", "Path", PortDataType::Path),
];
const SKSL_INPUTS: &[PortSpec] = &[
    PortSpec::single(TIME_PORT, "Time", PortDataType::Number),
    PortSpec::single("shader", "Shader", PortDataType::String),
    PortSpec::single("width", "Width", PortDataType::Number),
    PortSpec::single("height", "Height", PortDataType::Number),
];
const IMAGE_OUTPUT: &[PortSpec] = &[PortSpec::single(
    IMAGE_OUTPUT_PORT,
    "Image",
    PortDataType::Image,
)];

const SPECS: &[DescriptorSpec] = &[
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.text",
            "Text",
            "Text",
            "node_editor.menu.create.text",
            &["title", "caption", "shape"],
        ),
        NativeNodeFactory::Generator(GeneratorContent::Text),
        TEXT_INPUTS,
        SHAPE_OUTPUT,
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.solid-color",
            "Solid Color",
            "Generators",
            "node_editor.menu.create.solid",
            &["solid", "color", "image"],
        ),
        NativeNodeFactory::Generator(GeneratorContent::Solid),
        SOLID_INPUTS,
        IMAGE_OUTPUT,
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.shape",
            "Shape",
            "Generators",
            "node_editor.menu.create.shape",
            &["shape", "rectangle", "path"],
        ),
        NativeNodeFactory::Generator(GeneratorContent::Shape),
        SHAPE_INPUTS,
        SHAPE_OUTPUT,
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.sksl-shader",
            "SkSL Shader",
            "Generators",
            "node_editor.menu.create.sksl",
            &["sksl", "shader", "procedural", "image"],
        ),
        NativeNodeFactory::Generator(GeneratorContent::SkSL),
        SKSL_INPUTS,
        IMAGE_OUTPUT,
    ),
];

pub(super) const fn specs() -> &'static [DescriptorSpec] {
    SPECS
}
