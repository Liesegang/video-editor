use library::model::Project;
use library::model::project::{AUDIO_OUTPUT_PORT, IMAGE_OUTPUT_PORT, PortDataType, PortOwner};
use uuid::Uuid;

pub(in crate::ui::panels::node_editor) const IMAGE_OUTPUT_BINDING_PORT: &str =
    "image_output_binding";
pub(in crate::ui::panels::node_editor) const AUDIO_OUTPUT_BINDING_PORT: &str =
    "audio_output_binding";

pub(in crate::ui::panels::node_editor) fn container_output_type_key(
    data_type: PortDataType,
) -> Option<&'static str> {
    match data_type {
        PortDataType::Image => Some("image"),
        PortDataType::Audio => Some("audio"),
        _ => None,
    }
}

pub(in crate::ui::panels::node_editor) fn container_output_port(
    data_type: PortDataType,
) -> Option<&'static str> {
    match data_type {
        PortDataType::Image => Some(IMAGE_OUTPUT_PORT),
        PortDataType::Audio => Some(AUDIO_OUTPUT_PORT),
        _ => None,
    }
}

pub(in crate::ui::panels::node_editor) fn container_output_binding_port(
    data_type: PortDataType,
) -> Option<&'static str> {
    match data_type {
        PortDataType::Image => Some(IMAGE_OUTPUT_BINDING_PORT),
        PortDataType::Audio => Some(AUDIO_OUTPUT_BINDING_PORT),
        _ => None,
    }
}

pub(in crate::ui::panels::node_editor) fn container_output_binding_type(
    port: &str,
) -> Option<PortDataType> {
    match port {
        IMAGE_OUTPUT_BINDING_PORT => Some(PortDataType::Image),
        AUDIO_OUTPUT_BINDING_PORT => Some(PortDataType::Audio),
        _ => None,
    }
}

pub(in crate::ui::panels::node_editor) fn container_output_node_id(
    project: &Project,
    owner: PortOwner,
    data_type: PortDataType,
) -> Option<Uuid> {
    match (owner, data_type) {
        (PortOwner::Composition(id), PortDataType::Image) => project
            .get_composition(id)
            .and_then(|composition| composition.output_node_id),
        (PortOwner::Composition(id), PortDataType::Audio) => project
            .get_composition(id)
            .and_then(|composition| composition.audio_output_node_id),
        (PortOwner::Track(id), PortDataType::Image) => {
            project.get_track(id).and_then(|track| track.output_node_id)
        }
        (PortOwner::Track(id), PortDataType::Audio) => project
            .get_track(id)
            .and_then(|track| track.audio_output_node_id),
        (PortOwner::Clip(id), PortDataType::Image) => {
            project.get_clip(id).and_then(|clip| clip.output_node_id)
        }
        (PortOwner::Clip(id), PortDataType::Audio) => project
            .get_clip(id)
            .and_then(|clip| clip.audio_output_node_id),
        _ => None,
    }
}
