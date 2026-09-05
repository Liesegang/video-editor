use ordered_float::OrderedFloat;

use super::super::descriptor::{DescriptorIdentity, DescriptorSpec, PortSpec};
use crate::model::node::{
    TRANSITION_AUDIO_INPUT_NODE_ID, TRANSITION_AUDIO_MIX_NODE_ID, TRANSITION_IMAGE_INPUT_NODE_ID,
    TRANSITION_IMAGE_MIX_NODE_ID, TRANSITION_PROGRESS_INPUT_NODE_ID,
};
use crate::model::project::{
    AUDIO_OUTPUT_PORT, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, NUMBER_RESULT_OUTPUT_PORT,
    PortDataType, SOUND_INPUT_PORT, TRANSITION_FROM_INPUT_PORT, TRANSITION_PROGRESS_INPUT_PORT,
    TRANSITION_PROGRESS_PROPERTY, TRANSITION_TO_INPUT_PORT,
};
use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue};

const IMAGE_INPUT: &[PortSpec] = &[PortSpec::single(
    IMAGE_INPUT_PORT,
    "Image",
    PortDataType::Image,
)];
const IMAGE_OUTPUT: &[PortSpec] = &[PortSpec::single(
    IMAGE_OUTPUT_PORT,
    "Image",
    PortDataType::Image,
)];
const AUDIO_INPUT: &[PortSpec] = &[PortSpec::single(
    SOUND_INPUT_PORT,
    "Audio",
    PortDataType::Audio,
)];
const AUDIO_OUTPUT: &[PortSpec] = &[PortSpec::single(
    AUDIO_OUTPUT_PORT,
    "Audio",
    PortDataType::Audio,
)];
const PROGRESS_INPUT: &[PortSpec] = &[PortSpec::single(
    TRANSITION_PROGRESS_INPUT_PORT,
    "Progress",
    PortDataType::Number,
)];
const PROGRESS_OUTPUT: &[PortSpec] = &[PortSpec::single(
    NUMBER_RESULT_OUTPUT_PORT,
    "Progress",
    PortDataType::Number,
)];
const IMAGE_MIX_INPUTS: &[PortSpec] = &[
    PortSpec::single(TRANSITION_FROM_INPUT_PORT, "A", PortDataType::Image),
    PortSpec::single(TRANSITION_TO_INPUT_PORT, "B", PortDataType::Image),
    PortSpec::single(
        TRANSITION_PROGRESS_INPUT_PORT,
        "Progress",
        PortDataType::Number,
    ),
];
const AUDIO_MIX_INPUTS: &[PortSpec] = &[
    PortSpec::single(TRANSITION_FROM_INPUT_PORT, "A", PortDataType::Audio),
    PortSpec::single(TRANSITION_TO_INPUT_PORT, "B", PortDataType::Audio),
    PortSpec::single(
        TRANSITION_PROGRESS_INPUT_PORT,
        "Progress",
        PortDataType::Number,
    ),
];

const SPECS: &[DescriptorSpec] = &[
    DescriptorSpec::implemented_host_native(
        DescriptorIdentity::new(
            TRANSITION_IMAGE_INPUT_NODE_ID,
            "Transition Image Input",
            "Transition",
            "node_editor.transition.image_input",
            &["transition", "protected", "input", "image"],
        ),
        IMAGE_INPUT,
        IMAGE_OUTPUT,
        no_properties,
    ),
    DescriptorSpec::implemented_host_native(
        DescriptorIdentity::new(
            TRANSITION_AUDIO_INPUT_NODE_ID,
            "Transition Audio Input",
            "Transition",
            "node_editor.transition.audio_input",
            &["transition", "protected", "input", "audio"],
        ),
        AUDIO_INPUT,
        AUDIO_OUTPUT,
        no_properties,
    ),
    DescriptorSpec::implemented_host_native(
        DescriptorIdentity::new(
            TRANSITION_PROGRESS_INPUT_NODE_ID,
            "Transition Progress",
            "Transition",
            "node_editor.transition.progress_input",
            &["transition", "protected", "progress", "normalized"],
        ),
        PROGRESS_INPUT,
        PROGRESS_OUTPUT,
        progress_property,
    ),
    DescriptorSpec::implemented_host_native(
        DescriptorIdentity::new(
            TRANSITION_IMAGE_MIX_NODE_ID,
            "Transition Image Mix",
            "Transition",
            "node_editor.transition.image_mix",
            &["transition", "crossfade", "mix", "image"],
        ),
        IMAGE_MIX_INPUTS,
        IMAGE_OUTPUT,
        progress_property,
    ),
    DescriptorSpec::implemented_host_native(
        DescriptorIdentity::new(
            TRANSITION_AUDIO_MIX_NODE_ID,
            "Transition Audio Mix",
            "Transition",
            "node_editor.transition.audio_mix",
            &["transition", "crossfade", "mix", "audio"],
        ),
        AUDIO_MIX_INPUTS,
        AUDIO_OUTPUT,
        progress_property,
    ),
];

fn no_properties() -> Vec<PropertyDefinition> {
    Vec::new()
}

fn progress_property() -> Vec<PropertyDefinition> {
    vec![PropertyDefinition::new(
        TRANSITION_PROGRESS_PROPERTY,
        PropertyUiType::Float {
            min: 0.0,
            max: 1.0,
            step: 0.01,
            suffix: String::new(),
            min_hard_limit: true,
            max_hard_limit: true,
        },
        "Progress",
        PropertyValue::Number(OrderedFloat(0.0)),
    )]
}

pub(super) const fn specs() -> &'static [DescriptorSpec] {
    SPECS
}
