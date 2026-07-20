use egui_snarl::Snarl;
use library::model::project::{PortDataType, PortOwner};
use library::model::Project;
use std::collections::HashMap;

use crate::ui::panels::node_editor::{
    container_output_binding_port, container_output_port, input_definitions, output_definitions,
    ContainerVisual, GraphItem, PortAnchorKind,
};

pub(super) fn connect_container_output_wires(
    project: &Project,
    containers: &[ContainerVisual],
    snarl_ids: &HashMap<GraphItem, egui_snarl::NodeId>,
    snarl: &mut Snarl<GraphItem>,
) {
    for visual in containers {
        connect_sources(
            project,
            visual.owner,
            PortDataType::Image,
            project
                .container_image_sources(visual.owner)
                .into_iter()
                .map(|source| source.source),
            snarl_ids,
            snarl,
        );
        connect_sources(
            project,
            visual.owner,
            PortDataType::Audio,
            project
                .container_audio_sources(visual.owner)
                .into_iter()
                .map(|source| source.source),
            snarl_ids,
            snarl,
        );
    }
}

fn connect_sources(
    project: &Project,
    owner: PortOwner,
    data_type: PortDataType,
    sources: impl Iterator<Item = PortOwner>,
    snarl_ids: &HashMap<GraphItem, egui_snarl::NodeId>,
    snarl: &mut Snarl<GraphItem>,
) {
    let sink_item = GraphItem::PortAnchor {
        owner,
        kind: PortAnchorKind::OutputSinks,
    };
    let Some(&sink_id) = snarl_ids.get(&sink_item) else {
        return;
    };
    let Some(binding_port) = container_output_binding_port(data_type) else {
        return;
    };
    let Some(input_index) = input_definitions(project, sink_item)
        .iter()
        .position(|definition| definition.key == binding_port)
    else {
        return;
    };
    let Some(output_port) = container_output_port(data_type) else {
        return;
    };

    for source_owner in sources {
        let source_item = match source_owner {
            PortOwner::Node(id) => GraphItem::Node(id),
            child @ (PortOwner::Composition(_) | PortOwner::Track(_) | PortOwner::Clip(_)) => {
                GraphItem::PortAnchor {
                    owner: child,
                    kind: PortAnchorKind::ExternalOutputs,
                }
            }
        };
        let Some(&source_id) = snarl_ids.get(&source_item) else {
            continue;
        };
        let Some(output_index) =
            output_definitions(project, source_item)
                .iter()
                .position(|definition| {
                    definition.key == output_port && definition.data_type == data_type
                })
        else {
            continue;
        };
        snarl.connect(
            egui_snarl::OutPinId {
                node: source_id,
                output: output_index,
            },
            egui_snarl::InPinId {
                node: sink_id,
                input: input_index,
            },
        );
    }
}
