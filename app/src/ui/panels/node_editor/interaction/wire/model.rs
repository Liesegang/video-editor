use eframe::egui;
use egui_snarl::Snarl;
use library::model::project::{PortAddress, PortDirection, PortOwner};
use library::model::Project;

use crate::ui::panels::node_editor::{
    input_definitions, output_definitions, ContainerVisual, GraphItem, NodeEdit, PortAnchorKind,
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
    let input_key = input_definitions(project, target_item)
        .get(input_index)?
        .key
        .clone();
    let from = PortAddress::new(graph_item_owner(source_item)?, output_key);
    let to = PortAddress::new(graph_item_owner(target_item)?, input_key);

    if connect {
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
