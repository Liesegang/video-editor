use eframe::egui;
use egui_snarl::Snarl;
use library::model::project::{PortAddress, PortDirection, PortOwner};
use library::model::Project;

use crate::ui::panels::node_editor::{
    input_definitions, merge_input_slots, output_definitions, ContainerVisual, GraphItem, NodeEdit,
    PortAnchorKind,
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
    if let GraphItem::PortAnchor {
        owner,
        kind: PortAnchorKind::ImageSink,
    } = target_item
    {
        let GraphItem::Node(node_id) = source_item else {
            return None;
        };
        return Some(NodeEdit::SetOutputNode {
            owner,
            node_id: connect.then_some(node_id),
        });
    }
    let output_key = output_definitions(project, source_item)
        .get(output_index)?
        .key
        .clone();
    let merge_connection = match target_item {
        GraphItem::Node(merge_id) => merge_input_slots(project, merge_id)
            .get(input_index)
            .and_then(|slot| match &slot.role {
                crate::ui::panels::node_editor::MergeInputSlotRole::Connected(row) => {
                    Some(row.connection_id)
                }
                crate::ui::panels::node_editor::MergeInputSlotRole::Canonical
                | crate::ui::panels::node_editor::MergeInputSlotRole::VacantImages => None,
            }),
        GraphItem::Container(_) | GraphItem::PortAnchor { .. } => None,
    };
    let input_key = match target_item {
        GraphItem::Node(merge_id)
            if project.get_node(merge_id).is_some_and(|node| {
                matches!(node.content(), library::model::NodeContent::Merge)
            }) =>
        {
            merge_input_slots(project, merge_id)
                .get(input_index)?
                .definition
                .key
                .clone()
        }
        _ => input_definitions(project, target_item)
            .get(input_index)?
            .key
            .clone(),
    };
    let from = PortAddress::new(graph_item_owner(source_item)?, output_key);
    let to = PortAddress::new(graph_item_owner(target_item)?, input_key);

    if !connect && merge_connection.is_some() {
        Some(NodeEdit::DisconnectConnection {
            connection_id: merge_connection?,
        })
    } else if connect {
        Some(NodeEdit::Connect { from, to })
    } else {
        Some(NodeEdit::Disconnect { from, to })
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
