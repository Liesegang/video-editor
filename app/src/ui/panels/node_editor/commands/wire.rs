use crate::state::context_types::NodeEditorEditableWire;
use eframe::egui;
use library::model::project::{
    PortAddress, PortDataType, PortDirection, PortMultiplicity, PortOwner,
};
use library::model::{Node, NodeContainer, NodeContent, Project};
use std::collections::HashSet;
use uuid::Uuid;

use crate::ui::panels::node_editor::{
    attach_node_at_position, container_output_node_id, editable_wire_sort_key,
    ensure_container_hierarchy_contains, estimated_node_rect, place_node_in_free_slot,
};

pub(in crate::ui::panels::node_editor) fn node_can_splice_connection(
    project: &Project,
    connection_id: Uuid,
    node_id: Uuid,
) -> bool {
    splice_ports_for_node(project, connection_id, node_id).is_some()
}

pub(super) fn splice_ports_for_node(
    project: &Project,
    connection_id: Uuid,
    node_id: Uuid,
) -> Option<(PortAddress, PortAddress)> {
    let node = project.get_node(node_id)?;
    if !matches!(
        node.content(),
        NodeContent::PluginOperation(_) | NodeContent::Merge
    ) {
        return None;
    }
    let connection = project
        .connections
        .iter()
        .find(|connection| connection.id == connection_id)?;
    if [connection.from.owner, connection.to.owner].contains(&PortOwner::Node(node_id)) {
        return None;
    }
    let source = project.port_definition(&connection.from, PortDirection::Output)?;
    let target = project.port_definition(&connection.to, PortDirection::Input)?;
    let definitions = project.port_definitions(PortOwner::Node(node_id));

    let mut inputs = definitions
        .iter()
        .filter(|definition| {
            if definition.direction != PortDirection::Input
                || !definition.data_type.accepts(source.data_type)
            {
                return false;
            }
            let address = PortAddress::new(PortOwner::Node(node_id), definition.key.clone());
            definition.multiplicity == PortMultiplicity::Variadic
                || !project
                    .connections
                    .iter()
                    .any(|connection| connection.to == address)
        })
        .collect::<Vec<_>>();
    inputs.sort_by_key(|definition| {
        (
            definition.data_type != source.data_type,
            !matches!(definition.key.as_str(), "image" | "shape" | "input"),
            definition.key.clone(),
        )
    });

    let mut outputs = definitions
        .iter()
        .filter(|definition| {
            definition.direction == PortDirection::Output
                && target.data_type.accepts(definition.data_type)
        })
        .collect::<Vec<_>>();
    outputs.sort_by_key(|definition| {
        (
            definition.data_type != target.data_type,
            !matches!(definition.key.as_str(), "image" | "shape" | "output"),
            definition.key.clone(),
        )
    });

    Some((
        PortAddress::new(PortOwner::Node(node_id), inputs.first()?.key.clone()),
        PortAddress::new(PortOwner::Node(node_id), outputs.first()?.key.clone()),
    ))
}

pub(in crate::ui::panels::node_editor) fn splice_existing_node_on_connection(
    project: &mut Project,
    connection_id: Uuid,
    node_id: Uuid,
) -> bool {
    let Some((via_input, via_output)) = splice_ports_for_node(project, connection_id, node_id)
    else {
        return false;
    };
    match project.splice_connection(connection_id, via_input, via_output) {
        Ok(_) => true,
        Err(error) => {
            log::warn!("Cannot splice Node {node_id} into wire {connection_id}: {error}");
            false
        }
    }
}

pub(in crate::ui::panels::node_editor) fn insert_node_on_connection(
    project: &mut Project,
    connection_id: Uuid,
    mut node: Node,
    position: egui::Pos2,
    composition_id: Uuid,
) -> bool {
    if !project
        .connections
        .iter()
        .any(|connection| connection.id == connection_id)
    {
        return false;
    }
    let mut candidate = project.clone();
    node.ui_position = [position.x, position.y];
    let node_id = node.id;
    candidate.add_node(node);
    let Some(container) =
        attach_node_at_position(&mut candidate, node_id, composition_id, position)
    else {
        return false;
    };
    place_node_in_free_slot(&mut candidate, node_id, container, position, &[]);
    if !splice_existing_node_on_connection(&mut candidate, connection_id, node_id) {
        return false;
    }
    if let Some(rect) = estimated_node_rect(&candidate, node_id) {
        ensure_container_hierarchy_contains(&mut candidate, container, rect);
    }
    *project = candidate;
    true
}

pub(super) fn container_for_output_owner(owner: PortOwner) -> Option<NodeContainer> {
    match owner {
        PortOwner::Composition(id) => Some(NodeContainer::Composition(id)),
        PortOwner::Track(id) => Some(NodeContainer::Track(id)),
        PortOwner::Clip(id) => Some(NodeContainer::Clip(id)),
        PortOwner::Node(_) => None,
    }
}

pub(super) fn disconnect_editable_wires(
    project: &mut Project,
    wires: Vec<NodeEditorEditableWire>,
) -> bool {
    let mut wires = wires
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    wires.sort_by_key(|target| editable_wire_sort_key(*target));
    let mut candidate = project.clone();
    let mut changed = false;
    for target in wires {
        match target {
            NodeEditorEditableWire::ProjectConnection { connection_id } => {
                changed |= candidate.disconnect_connection(connection_id);
            }
            NodeEditorEditableWire::OutputBinding {
                owner,
                node_id,
                data_type,
            } => {
                if container_output_node_id(&candidate, owner, data_type) != Some(node_id) {
                    continue;
                }
                let Some(container) = container_for_output_owner(owner) else {
                    return false;
                };
                let result = match data_type {
                    PortDataType::Image => candidate.set_output_node(container, None),
                    PortDataType::Audio => candidate.set_audio_output_node(container, None),
                    _ => return false,
                };
                if let Err(error) = result {
                    log::warn!("Cannot clear {data_type:?} container output binding: {error}");
                    return false;
                }
                changed = true;
            }
        }
    }
    if changed {
        *project = candidate;
    }
    changed
}
