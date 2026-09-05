use super::super::descriptor::{DescriptorIdentity, DescriptorSpec, NativeNodeFactory, PortSpec};
use crate::model::project::{
    APPEARANCE_STYLES_PORT, AUDIO_OUTPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT,
    MERGE_SOUNDS_PORT, PortDataType, SHAPE_INPUT_PORT, TIME_PORT,
};

const IMAGE_OUTPUT: &[PortSpec] = &[PortSpec::single(
    IMAGE_OUTPUT_PORT,
    "Image",
    PortDataType::Image,
)];
const MERGE_INPUTS: &[PortSpec] = &[
    PortSpec::single(TIME_PORT, "Time", PortDataType::Number),
    PortSpec::variadic(MERGE_IMAGES_PORT, "Images", PortDataType::Image),
];
const SOUND_MERGE_INPUTS: &[PortSpec] = &[
    PortSpec::single(TIME_PORT, "Time", PortDataType::Number),
    PortSpec::variadic(MERGE_SOUNDS_PORT, "Sounds", PortDataType::Audio),
];
const AUDIO_OUTPUT: &[PortSpec] = &[PortSpec::single(
    AUDIO_OUTPUT_PORT,
    "Audio",
    PortDataType::Audio,
)];
const APPEARANCE_INPUTS: &[PortSpec] = &[
    PortSpec::single(TIME_PORT, "Time", PortDataType::Number),
    PortSpec::single(SHAPE_INPUT_PORT, "Shape", PortDataType::Shape),
    PortSpec::variadic(APPEARANCE_STYLES_PORT, "Styles", PortDataType::Style),
];

const SPECS: &[DescriptorSpec] = &[
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.merge",
            "Merge",
            "Compositing",
            "node_editor.menu.create.merge",
            &["composite", "blend", "layers"],
        ),
        NativeNodeFactory::Merge,
        MERGE_INPUTS,
        IMAGE_OUTPUT,
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            "native.sound.merge",
            "Audio Mix",
            "Audio",
            "node_editor.menu.create.sound_merge",
            &["sound", "audio", "merge", "mix", "layers"],
        ),
        NativeNodeFactory::SoundMerge,
        SOUND_MERGE_INPUTS,
        AUDIO_OUTPUT,
    ),
    DescriptorSpec::implemented(
        DescriptorIdentity::new(
            super::super::APPEARANCE_STACK_CATALOG_ID,
            "Appearance Stack",
            "Compositing",
            "node_editor.menu.create.appearance_stack",
            &["appearance", "style", "layer style", "shape"],
        ),
        NativeNodeFactory::NativeOperation,
        APPEARANCE_INPUTS,
        IMAGE_OUTPUT,
    ),
];

pub(super) const fn specs() -> &'static [DescriptorSpec] {
    SPECS
}
