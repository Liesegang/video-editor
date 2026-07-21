//! Focused query/mutation primitives for an explicit Path Effect main-flow.
//!
//! The stack order is canonical Shape wiring, never Node storage order. A
//! reorder rewrites only the participating Shape connections and retains all
//! Node UUIDs, authored properties, property wires, external targets, and
//! connection UUIDs.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::model::NodeContent;
use crate::plugin::{PATH_EFFECT_APPLY_OPERATION, PATH_EFFECT_CATEGORY};

use super::{
    PortAddress, PortOwner, Project, ProjectGraphError, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};

fn invalid(reason: impl Into<String>) -> ProjectGraphError {
    ProjectGraphError::InvalidPathEffectStack {
        reason: reason.into(),
    }
}

impl Project {
    /// Return the contiguous Path Effect chain immediately upstream of one
    /// Shape input. The result is ordered from source toward `target`.
    pub fn path_effect_chain_to(
        &self,
        target: &PortAddress,
    ) -> Result<Vec<Uuid>, ProjectGraphError> {
        let mut connection = self
            .connections
            .iter()
            .find(|connection| connection.to == *target);
        let mut reversed = Vec::new();
        let mut visited = HashSet::new();
        while let Some(current) = connection {
            let PortOwner::Node(node_id) = current.from.owner else {
                break;
            };
            if current.from.port != SHAPE_OUTPUT_PORT || !self.is_path_effect_node(node_id) {
                break;
            }
            if !visited.insert(node_id) {
                return Err(invalid(format!(
                    "Path Effect chain before {target:?} contains a cycle at Node {node_id}"
                )));
            }
            reversed.push(node_id);
            let input = PortAddress::new(PortOwner::Node(node_id), SHAPE_INPUT_PORT);
            let incoming = self
                .connections
                .iter()
                .filter(|candidate| candidate.to == input)
                .collect::<Vec<_>>();
            connection = match incoming.as_slice() {
                [] => None,
                [incoming] => Some(*incoming),
                _ => {
                    return Err(invalid(format!(
                        "Path Effect Node {node_id} has multiple Shape inputs"
                    )));
                }
            };
        }
        reversed.reverse();
        Ok(reversed)
    }

    /// Reorder one connected linear Path Effect segment transactionally.
    /// Only main-flow Shape endpoints change; connection identities and every
    /// Node remain intact.
    pub fn reorder_path_effect_chain(
        &mut self,
        ordered_node_ids: &[Uuid],
    ) -> Result<(), ProjectGraphError> {
        if ordered_node_ids.is_empty() {
            return Ok(());
        }
        let requested = ordered_node_ids.iter().copied().collect::<HashSet<_>>();
        if requested.len() != ordered_node_ids.len() {
            return Err(invalid(
                "Path Effect reorder contains a duplicate Node UUID",
            ));
        }
        for node_id in ordered_node_ids {
            if !self.is_path_effect_node(*node_id) {
                return Err(invalid(format!(
                    "Node {node_id} is not a Path Effect operation"
                )));
            }
        }

        let mut incoming = HashMap::new();
        let mut successor = HashMap::new();
        let mut external_outgoing = HashMap::<Uuid, Vec<Uuid>>::new();
        for node_id in &requested {
            let input = PortAddress::new(PortOwner::Node(*node_id), SHAPE_INPUT_PORT);
            let matches = self
                .connections
                .iter()
                .filter(|connection| connection.to == input)
                .collect::<Vec<_>>();
            let [connection] = matches.as_slice() else {
                return Err(invalid(format!(
                    "Path Effect Node {node_id} must have exactly one Shape input"
                )));
            };
            incoming.insert(*node_id, connection.id);

            let output = PortAddress::new(PortOwner::Node(*node_id), SHAPE_OUTPUT_PORT);
            for connection in self
                .connections
                .iter()
                .filter(|connection| connection.from == output)
            {
                if let PortOwner::Node(next_id) = connection.to.owner
                    && requested.contains(&next_id)
                    && connection.to.port == SHAPE_INPUT_PORT
                {
                    if successor
                        .insert(*node_id, (next_id, connection.id))
                        .is_some()
                    {
                        return Err(invalid(format!(
                            "Path Effect Node {node_id} branches inside the requested stack"
                        )));
                    }
                } else {
                    external_outgoing
                        .entry(*node_id)
                        .or_default()
                        .push(connection.id);
                }
            }
        }

        let roots = requested
            .iter()
            .copied()
            .filter(|node_id| {
                let connection_id = incoming[node_id];
                let connection = self
                    .connections
                    .iter()
                    .find(|connection| connection.id == connection_id)
                    .expect("captured connection must still exist");
                !matches!(connection.from.owner, PortOwner::Node(id) if requested.contains(&id))
            })
            .collect::<Vec<_>>();
        let [root] = roots.as_slice() else {
            return Err(invalid(
                "requested Path Effect Nodes do not form one externally rooted linear stack",
            ));
        };
        let mut current_order = Vec::with_capacity(requested.len());
        let mut cursor = *root;
        loop {
            current_order.push(cursor);
            let Some((next, _)) = successor.get(&cursor) else {
                break;
            };
            cursor = *next;
            if current_order.contains(&cursor) {
                return Err(invalid("requested Path Effect stack contains a cycle"));
            }
        }
        if current_order.len() != requested.len() {
            return Err(invalid(
                "requested Path Effect Nodes do not form one connected linear stack",
            ));
        }
        for node_id in current_order.iter().take(current_order.len() - 1) {
            if external_outgoing
                .get(node_id)
                .is_some_and(|connections| !connections.is_empty())
            {
                return Err(invalid(format!(
                    "Path Effect Node {node_id} fans out before the end of the stack"
                )));
            }
        }
        if current_order == ordered_node_ids {
            return Ok(());
        }

        let upstream_connection_id = incoming[&current_order[0]];
        let internal_connection_ids = current_order
            .iter()
            .take(current_order.len() - 1)
            .map(|node_id| successor[node_id].1)
            .collect::<Vec<_>>();
        let downstream_connection_ids = external_outgoing
            .remove(current_order.last().expect("non-empty stack"))
            .unwrap_or_default();

        let baseline = self.validate_connections();
        let mut candidate = self.clone();
        candidate.connection_mut(upstream_connection_id)?.to =
            PortAddress::new(PortOwner::Node(ordered_node_ids[0]), SHAPE_INPUT_PORT);
        for (connection_id, pair) in internal_connection_ids
            .iter()
            .zip(ordered_node_ids.windows(2))
        {
            let connection = candidate.connection_mut(*connection_id)?;
            connection.from = PortAddress::new(PortOwner::Node(pair[0]), SHAPE_OUTPUT_PORT);
            connection.to = PortAddress::new(PortOwner::Node(pair[1]), SHAPE_INPUT_PORT);
        }
        let terminal = *ordered_node_ids.last().expect("non-empty stack");
        for connection_id in downstream_connection_ids {
            candidate.connection_mut(connection_id)?.from =
                PortAddress::new(PortOwner::Node(terminal), SHAPE_OUTPUT_PORT);
        }
        if let Some(error) =
            super::first_new_project_validation_error(&baseline, candidate.validate_connections())
        {
            return Err(error);
        }
        *self = candidate;
        Ok(())
    }

    fn is_path_effect_node(&self, node_id: Uuid) -> bool {
        matches!(
            self.get_node(node_id).map(crate::model::Node::content),
            Some(NodeContent::PluginOperation(operation))
                if operation.category == PATH_EFFECT_CATEGORY
                    && operation.operation == PATH_EFFECT_APPLY_OPERATION
        )
    }

    fn connection_mut(
        &mut self,
        connection_id: Uuid,
    ) -> Result<&mut super::ProjectConnection, ProjectGraphError> {
        self.connections
            .iter_mut()
            .find(|connection| connection.id == connection_id)
            .ok_or(ProjectGraphError::ConnectionNotFound(connection_id))
    }
}
