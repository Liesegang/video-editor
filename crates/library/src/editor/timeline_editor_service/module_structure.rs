//! Shared recognition helpers for structured facades over real Module graphs.

use std::collections::{HashMap, HashSet};

use super::*;
use crate::model::node::PluginOperationContent;
use crate::plugin::PROPERTY_PORT_PREFIX;

pub(super) fn module_item_ids(
    project: &AuthoringProject,
    item_id: TimelineItemId,
) -> Result<Option<(ModuleInstanceId, ModuleOutputId)>, LibraryError> {
    let item = project
        .items
        .get(&item_id)
        .ok_or_else(|| LibraryError::Validation(format!("Missing Timeline item {item_id}")))?;
    Ok(match &item.source {
        SourceRef::Module(invocation) => Some((invocation.instance_id, invocation.output_id)),
        _ => None,
    })
}

pub(super) fn require_module_item_ids(
    project: &AuthoringProject,
    item_id: TimelineItemId,
) -> Result<(ModuleInstanceId, ModuleOutputId), String> {
    module_item_ids(project, item_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Timeline item {item_id} is not a Node Clip"))
}

pub(super) fn operation_parameter_ids(
    definition: &ModuleDefinition,
    node_id: uuid::Uuid,
    content: &PluginOperationContent,
) -> Option<Vec<PublishedParameterId>> {
    let expected = content
        .declared_ports
        .iter()
        .filter_map(|port| {
            (port.direction == crate::model::project::PortDirection::Input)
                .then(|| port.key.strip_prefix(PROPERTY_PORT_PREFIX))
                .flatten()
                .map(|_| port.key.as_str())
        })
        .collect::<HashSet<_>>();
    let parameters = definition
        .interface
        .parameters
        .iter()
        .filter(|parameter| parameter.target.node_id == node_id)
        .collect::<Vec<_>>();
    let actual = parameters
        .iter()
        .map(|parameter| parameter.target.port.as_str())
        .collect::<HashSet<_>>();
    if expected != actual || parameters.len() != expected.len() {
        return None;
    }
    Some(parameters.iter().map(|parameter| parameter.id).collect())
}

pub(super) fn reorder_published_operation_groups(
    definition: &mut ModuleDefinition,
    order: &[uuid::Uuid],
) {
    let operation_ids = order.iter().copied().collect::<HashSet<_>>();
    let slots = definition
        .interface
        .parameters
        .iter()
        .enumerate()
        .filter_map(|(index, parameter)| {
            operation_ids
                .contains(&parameter.target.node_id)
                .then_some(index)
        })
        .collect::<Vec<_>>();
    let by_node = definition
        .interface
        .parameters
        .iter()
        .filter(|parameter| operation_ids.contains(&parameter.target.node_id))
        .cloned()
        .fold(HashMap::<_, Vec<_>>::new(), |mut groups, parameter| {
            groups
                .entry(parameter.target.node_id)
                .or_default()
                .push(parameter);
            groups
        });
    let reordered = order
        .iter()
        .flat_map(|node_id| by_node.get(node_id).into_iter().flatten().cloned())
        .collect::<Vec<_>>();
    for (slot, parameter) in slots.into_iter().zip(reordered) {
        definition.interface.parameters[slot] = parameter;
    }
}
