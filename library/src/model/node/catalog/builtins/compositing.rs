use super::super::descriptor::{DescriptorIdentity, DescriptorSpec, NativeNodeFactory, PortSpec};
use crate::model::project::{
    AUDIO_OUTPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, MERGE_SOUNDS_PORT, PortDataType,
    TIME_PORT,
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
            "Sound Merge",
            "Sound",
            "node_editor.menu.create.sound_merge",
            &["sound", "audio", "merge", "mix", "layers"],
        ),
        NativeNodeFactory::SoundMerge,
        SOUND_MERGE_INPUTS,
        AUDIO_OUTPUT,
    ),
];

pub(super) const fn specs() -> &'static [DescriptorSpec] {
    SPECS
}
