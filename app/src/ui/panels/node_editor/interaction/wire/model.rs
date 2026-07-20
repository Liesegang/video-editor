use eframe::egui;
use egui_snarl::Snarl;
use library::model::project::{PortAddress, PortDataType, PortDirection, PortOwner};
use library::model::{NodeContainer, Project};

use crate::ui::panels::node_editor::{
    input_definitions, merge_images_target_node_id, merge_input_slots, output_definitions,
    ContainerVisual, GraphItem, NodeEdit,
};

pub(in crate::ui::panels::node_editor) fn edit_for_wire(
    project: &Project,
    snarl: &Snarl<GraphItem>,
    source_snarl_id: egui_snarl::NodeId,
    output_index: usize,
    target_snarl_id: egui_snarl::NodeId,
    input_index: usize,
    connect: bool,
) -> Option<NodeEdit> {
    let source_item = *snarl.get_node(source_snarl_id)?;
    let target_item = *snarl.get_node(target_snarl_id)?;
    let output = output_definitions(project, source_item)
        .get(output_index)?
        .clone();
    let target_merge_id = match target_item {
        GraphItem::Node(node_id) => merge_images_target_node_id(
            project,
            &PortAddress::new(
                PortOwner::Node(node_id),
                library::model::project::MERGE_IMAGES_PORT,
            ),
        ),
        GraphItem::Container(_) | GraphItem::PortAnchor { .. } => None,
    };
    let merge_connection = target_merge_id.and_then(|merge_id| {
        merge_input_slots(project, merge_id)
            .get(input_index)
            .and_then(|slot| match &slot.role {
                crate::ui::panels::node_editor::MergeInputSlotRole::Connected(row) => {
                    Some(row.connection_id)
                }
                crate::ui::panels::node_editor::MergeInputSlotRole::Canonical
                | crate::ui::panels::node_editor::MergeInputSlotRole::VacantImages => None,
            })
    });
    let input = match target_merge_id {
        Some(merge_id) => merge_input_slots(project, merge_id)
            .get(input_index)?
            .definition
            .clone(),
        None => input_definitions(project, target_item)
            .get(input_index)?
            .clone(),
    };
    let from = PortAddress::new(graph_item_owner(source_item)?, output.key);
    let to = PortAddress::new(graph_item_owner(target_item)?, input.key);

    if !connect {
        if let Some(connection_id) = merge_connection {
            return Some(NodeEdit::DisconnectConnection { connection_id });
        }
    }
    edit_for_port_addresses(project, from, to, connect)
}

pub(in crate::ui::panels::node_editor) fn edit_for_port_addresses(
    project: &Project,
    from: PortAddress,
    to: PortAddress,
    connect: bool,
) -> Option<NodeEdit> {
    let (Some(data_type), Some(container)) = (
        crate::ui::panels::node_editor::container_output_binding_type(&to.port),
        output_container(to.owner),
    ) else {
        return if connect {
            Some(NodeEdit::Connect { from, to })
        } else {
            Some(NodeEdit::Disconnect { from, to })
        };
    };
    let PortOwner::Node(node_id) = from.owner else {
        return None;
    };
    let output_port = crate::ui::panels::node_editor::container_output_port(data_type)?;
    let source = project.port_definition(&from, PortDirection::Output)?;
    if from.port != output_port
        || source.data_type != data_type
        || project.find_node_container(node_id) != Some(container)
        || (!connect
            && crate::ui::panels::node_editor::container_output_node_id(
                project, to.owner, data_type,
            ) != Some(node_id))
    {
        return None;
    }
    match data_type {
        PortDataType::Image => Some(NodeEdit::SetOutputNode {
            owner: to.owner,
            node_id: connect.then_some(node_id),
        }),
        PortDataType::Audio => Some(NodeEdit::SetAudioOutputNode {
            owner: to.owner,
            node_id: connect.then_some(node_id),
        }),
        _ => None,
    }
}

fn output_container(owner: PortOwner) -> Option<NodeContainer> {
    match owner {
        PortOwner::Composition(id) => Some(NodeContainer::Composition(id)),
        PortOwner::Track(id) => Some(NodeContainer::Track(id)),
        PortOwner::Clip(id) => Some(NodeContainer::Clip(id)),
        PortOwner::Node(_) => None,
    }
}

pub(in crate::ui::panels::node_editor) fn graph_item_owner(item: GraphItem) -> Option<PortOwner> {
    match item {
        GraphItem::Node(node_id) => Some(PortOwner::Node(node_id)),
        GraphItem::Container(owner) | GraphItem::PortAnchor { owner, .. } => Some(owner),
    }
}

pub(in crate::ui::panels::node_editor) fn embedded_pin_center(
    containers: &[ContainerVisual],
    item: Option<GraphItem>,
    _direction: PortDirection,
    index: usize,
) -> Option<egui::Pos2> {
    let GraphItem::PortAnchor { owner, kind } = item? else {
        return None;
    };
    let visual = containers
        .iter()
        .find(|container| container.owner == owner)?;
    Some(visual.embedded_port_center(kind, index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::media_node_for_canvas;
    use crate::ui::panels::node_editor::{
        apply_edit, AUDIO_OUTPUT_BINDING_PORT, IMAGE_OUTPUT_BINDING_PORT,
    };
    use library::editor::project_service::MediaNodeRequest;
    use library::model::asset::{Asset, AssetKind};
    use library::model::project::{AUDIO_OUTPUT_PORT, IMAGE_OUTPUT_PORT};
    use library::model::{Clip, Composition};

    fn video_binding_fixture() -> (Project, uuid::Uuid, uuid::Uuid, uuid::Uuid) {
        let mut project = Project::new("typed Node Editor binding");
        let (composition, track) = Composition::new("Main", 64, 64, 24.0, 2.0);
        let track_id = track.id;
        project.add_track(track);
        project.add_composition(composition);
        let clip = Clip::new("Video", 0.0, 2.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();

        let asset = Asset::new("Video", "/fixture/video.mp4", AssetKind::Video);
        let node = media_node_for_canvas(
            "Video",
            MediaNodeRequest::Video {
                asset_id: asset.id,
                file_path: asset.path.clone(),
                stream_index: None,
                audio_stream_index: None,
            },
            64,
            64,
            64,
            64,
        );
        let node_id = node.id;
        project.assets.push(asset);
        project.add_node(node);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
            .unwrap();
        (project, track_id, clip_id, node_id)
    }

    #[test]
    fn synthetic_output_sinks_author_typed_bindings_and_reject_cross_type() {
        let (mut project, track_id, clip_id, node_id) = video_binding_fixture();
        let node = PortOwner::Node(node_id);
        let clip = PortOwner::Clip(clip_id);

        let image_edit = edit_for_port_addresses(
            &project,
            PortAddress::new(node, IMAGE_OUTPUT_PORT),
            PortAddress::new(clip, IMAGE_OUTPUT_BINDING_PORT),
            true,
        )
        .expect("Image output binds to Image sink");
        assert!(matches!(image_edit, NodeEdit::SetOutputNode { .. }));
        assert!(apply_edit(&mut project, image_edit));
        assert_eq!(
            project.get_clip(clip_id).unwrap().output_node_id,
            Some(node_id)
        );

        let audio_edit = edit_for_port_addresses(
            &project,
            PortAddress::new(node, AUDIO_OUTPUT_PORT),
            PortAddress::new(clip, AUDIO_OUTPUT_BINDING_PORT),
            true,
        )
        .expect("Audio output binds to Audio sink");
        assert!(matches!(audio_edit, NodeEdit::SetAudioOutputNode { .. }));
        assert!(apply_edit(&mut project, audio_edit));
        assert_eq!(
            project.get_clip(clip_id).unwrap().audio_output_node_id,
            Some(node_id)
        );

        for (source, sink) in [
            (IMAGE_OUTPUT_PORT, AUDIO_OUTPUT_BINDING_PORT),
            (AUDIO_OUTPUT_PORT, IMAGE_OUTPUT_BINDING_PORT),
        ] {
            assert!(edit_for_port_addresses(
                &project,
                PortAddress::new(node, source),
                PortAddress::new(clip, sink),
                true,
            )
            .is_none());
        }
        assert!(edit_for_port_addresses(
            &project,
            PortAddress::new(node, AUDIO_OUTPUT_PORT),
            PortAddress::new(PortOwner::Track(track_id), AUDIO_OUTPUT_BINDING_PORT),
            true,
        )
        .is_none());
    }

    #[test]
    fn snarl_projects_image_and_audio_to_distinct_indexed_container_pins() {
        let (mut project, track_id, clip_id, node_id) = video_binding_fixture();
        project
            .set_output_node(NodeContainer::Clip(clip_id), Some(node_id))
            .unwrap();
        project
            .set_audio_output_node(NodeContainer::Clip(clip_id), Some(node_id))
            .unwrap();
        let composition_id = project.compositions[0].id;
        let (snarl, _) = crate::ui::panels::node_editor::build_snarl(&project, composition_id);

        for (owner, source) in [
            (PortOwner::Clip(clip_id), GraphItem::Node(node_id)),
            (
                PortOwner::Track(track_id),
                GraphItem::PortAnchor {
                    owner: PortOwner::Clip(clip_id),
                    kind: crate::ui::panels::node_editor::PortAnchorKind::ExternalOutputs,
                },
            ),
            (
                PortOwner::Composition(composition_id),
                GraphItem::PortAnchor {
                    owner: PortOwner::Track(track_id),
                    kind: crate::ui::panels::node_editor::PortAnchorKind::ExternalOutputs,
                },
            ),
        ] {
            let sink = GraphItem::PortAnchor {
                owner,
                kind: crate::ui::panels::node_editor::PortAnchorKind::OutputSinks,
            };
            let sink_id = snarl
                .nodes_ids_data()
                .find_map(|(id, node)| (node.value == sink).then_some(id))
                .unwrap();
            let source_id = snarl
                .nodes_ids_data()
                .find_map(|(id, node)| (node.value == source).then_some(id))
                .unwrap();

            for (data_type, binding_port, output_port) in [
                (
                    PortDataType::Image,
                    IMAGE_OUTPUT_BINDING_PORT,
                    IMAGE_OUTPUT_PORT,
                ),
                (
                    PortDataType::Audio,
                    AUDIO_OUTPUT_BINDING_PORT,
                    AUDIO_OUTPUT_PORT,
                ),
            ] {
                let input = input_definitions(&project, sink)
                    .iter()
                    .position(|pin| pin.key == binding_port && pin.data_type == data_type)
                    .unwrap();
                let output = output_definitions(&project, source)
                    .iter()
                    .position(|pin| pin.key == output_port && pin.data_type == data_type)
                    .unwrap();
                assert!(snarl.wires().any(|(from, to)| {
                    from.node == source_id
                        && from.output == output
                        && to.node == sink_id
                        && to.input == input
                }));
            }
        }
    }
}
