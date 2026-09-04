//! Stable first-party Node identities used by Transition Module boundaries.
//!
//! These Nodes are created by the Transition Module factory. They are not a
//! second Transition model: the Timeline still owns placement, participants,
//! duration, and progress time mapping.

pub const TRANSITION_IMAGE_INPUT_NODE_ID: &str = "native.transition.image_input";
pub const TRANSITION_AUDIO_INPUT_NODE_ID: &str = "native.transition.audio_input";
pub const TRANSITION_PROGRESS_INPUT_NODE_ID: &str = "native.transition.progress_input";
pub const TRANSITION_IMAGE_MIX_NODE_ID: &str = "native.transition.image_mix";
pub const TRANSITION_AUDIO_MIX_NODE_ID: &str = "native.transition.audio_mix";

pub const fn transition_input_node_id(audio: bool) -> &'static str {
    if audio {
        TRANSITION_AUDIO_INPUT_NODE_ID
    } else {
        TRANSITION_IMAGE_INPUT_NODE_ID
    }
}

pub const fn transition_mix_node_id(audio: bool) -> &'static str {
    if audio {
        TRANSITION_AUDIO_MIX_NODE_ID
    } else {
        TRANSITION_IMAGE_MIX_NODE_ID
    }
}
