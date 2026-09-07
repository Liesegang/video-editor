use super::super::super::DataContent;
use super::super::descriptor::{DescriptorIdentity, DescriptorSpec, NativeNodeFactory, PortSpec};
use crate::model::project::PortDataType;
use crate::model::project::connection::DATA_VALUE_OUTPUT_PORT;

const COLOR_OUTPUT: &[PortSpec] = &[PortSpec::single(
    DATA_VALUE_OUTPUT_PORT,
    "Value",
    PortDataType::Color,
)];
const PATH_OUTPUT: &[PortSpec] = &[PortSpec::single(
    DATA_VALUE_OUTPUT_PORT,
    "Value",
    PortDataType::Path,
)];
const GRADIENT_OUTPUT: &[PortSpec] = &[PortSpec::single(
    DATA_VALUE_OUTPUT_PORT,
    "Value",
    PortDataType::Gradient,
)];
const IMAGE_COLLECTION_OUTPUT: &[PortSpec] = &[PortSpec::single(
    DATA_VALUE_OUTPUT_PORT,
    "Value",
    PortDataType::ImageCollection,
)];

const SPECS: &[DescriptorSpec] = &[
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.data.image-collection",
            "Image Collection",
            "Data",
            "node_editor.menu.create.data:image_collection",
            &["image", "collection", "sprites", "assets", "data", "value"],
        ),
        NativeNodeFactory::Data(DataContent::ImageCollection),
        &[],
        IMAGE_COLLECTION_OUTPUT,
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.data.gradient",
            "Gradient",
            "Data",
            "node_editor.menu.create.data:gradient",
            &["gradient", "ramp", "stops", "color", "data", "value"],
        ),
        NativeNodeFactory::Data(DataContent::Gradient),
        &[],
        GRADIENT_OUTPUT,
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.data.color",
            "Color",
            "Data",
            "node_editor.menu.create.data:color",
            &["color", "rgba", "hdr", "data", "value"],
        ),
        NativeNodeFactory::Data(DataContent::Color),
        &[],
        COLOR_OUTPUT,
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.data.path",
            "Path",
            "Data",
            "node_editor.menu.create.data:path",
            &["path", "curve", "geometry", "contour", "data", "value"],
        ),
        NativeNodeFactory::Data(DataContent::Path),
        &[],
        PATH_OUTPUT,
    ),
];

pub(super) const fn specs() -> &'static [DescriptorSpec] {
    SPECS
}
