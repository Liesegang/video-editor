use std::collections::HashSet;

use uuid::Uuid;

use crate::model::BlendMode;

use super::super::{Project, ProjectGraphError};
use super::ports::{is_graph_connectable_output, variadic_target_allows_duplicate_sources};
use super::{PortAddress, PortDirection, PortExposure, PortMultiplicity, ProjectConnection};

impl Project {
    pub fn connect_ports(
        &mut self,
        from: PortAddress,
        to: PortAddress,
    ) -> Result<Uuid, ProjectGraphError> {
        let source = self
            .port_definition(&from, PortDirection::Output)
            .ok_or_else(|| ProjectGraphError::PortNotFound(from.clone()))?;
        let target = self
            .port_definition(&to, PortDirection::Input)
            .ok_or_else(|| ProjectGraphError::PortNotFound(to.clone()))?;
        if !target.data_type.accepts(source.data_type) {
            return Err(ProjectGraphError::IncompatiblePortTypes {
                source_type: source.data_type,
                target_type: target.data_type,
            });
        }
        if source.exposure == PortExposure::Internal
            && !self.is_internal_port_target(from.owner, to.owner)
        {
            return Err(ProjectGraphError::InternalPortEscapesContainer {
                source_owner: from.owner,
                target_owner: to.owner,
            });
        }
        if !is_graph_connectable_output(self, &from, source.data_type) {
            return Err(ProjectGraphError::UnsupportedConnectionType {
                connection_id: Uuid::nil(),
                data_type: source.data_type,
            });
        }
        if (target.multiplicity == PortMultiplicity::Single
            || !variadic_target_allows_duplicate_sources(self, &to))
            && let Some(existing) = self
                .connections
                .iter()
                .find(|item| item.from == from && item.to == to)
        {
            return Ok(existing.id);
        }

        let order = match target.multiplicity {
            PortMultiplicity::Single => 0,
            PortMultiplicity::Variadic => {
                self.connections
                    .iter()
                    .filter(|item| item.to == to)
                    .map(|item| item.order)
                    .max()
                    .unwrap_or(-1)
                    + 1
            }
        };
        let connection = ProjectConnection::new(from, to, order);
        let mut prospective = self.connections.clone();
        if target.multiplicity == PortMultiplicity::Single {
            prospective.retain(|item| item.to != connection.to);
        }
        prospective.push(connection.clone());
        if source.exposure != PortExposure::Internal
            && is_graph_connectable_output(self, &connection.from, source.data_type)
            && self.connection_is_cyclic(&connection, &prospective)
        {
            return Err(ProjectGraphError::ConnectionCycle {
                from: connection.from.owner,
                to: connection.to.owner,
            });
        }
        let baseline = self.validate_connections();
        let mut candidate = self.clone();
        if target.multiplicity == PortMultiplicity::Single {
            candidate
                .connections
                .retain(|item| item.to != connection.to);
        }
        let id = connection.id;
        let structural_container = candidate.container_for_structural_target(&connection.to);
        candidate.connections.push(connection);
        if let Some(container) = structural_container {
            candidate.reorder_structural_children(container, Some(id));
        }
        if let Some(error) = super::super::first_new_project_validation_error(
            &baseline,
            candidate.validate_connections(),
        ) {
            return Err(error);
        }
        *self = candidate;
        Ok(id)
    }

    pub fn reorder_connection(
        &mut self,
        connection_id: Uuid,
        new_order: i64,
    ) -> Result<(), ProjectGraphError> {
        let baseline = self.validate_connections();
        let index = self
            .connections
            .iter()
            .position(|item| item.id == connection_id)
            .ok_or(ProjectGraphError::ConnectionNotFound(connection_id))?;
        let target = self.connections[index].to.clone();
        let definition = self
            .port_definition(&target, PortDirection::Input)
            .ok_or_else(|| ProjectGraphError::PortNotFound(target.clone()))?;
        let mut candidate = self.clone();
        if definition.multiplicity != PortMultiplicity::Variadic {
            candidate.connections[index].order = 0;
            if let Some(error) = super::super::first_new_project_validation_error(
                &baseline,
                candidate.validate_connections(),
            ) {
                return Err(error);
            }
            *self = candidate;
            return Ok(());
        }
        let mut ids = candidate
            .connections
            .iter()
            .filter(|item| item.to == target)
            .map(|item| (item.order, item.id))
            .collect::<Vec<_>>();
        ids.sort_by_key(|(order, id)| (*order, *id));
        let mut ids = ids.into_iter().map(|item| item.1).collect::<Vec<_>>();
        ids.retain(|id| *id != connection_id);
        let insert_at = new_order.max(0) as usize;
        let insert_at = insert_at.min(ids.len());
        if let Some(error) = self.structural_custom_insertion_error(
            &target,
            &self.connections[index].from,
            insert_at,
        ) {
            return Err(error);
        }
        ids.insert(insert_at, connection_id);
        for (order, id) in ids.into_iter().enumerate() {
            let connection = candidate
                .connections
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or(ProjectGraphError::ConnectionNotFound(id))?;
            connection.order = order as i64;
        }
        candidate.sync_child_order_from_structural_target(&target);
        if let Some(error) = super::super::first_new_project_validation_error(
            &baseline,
            candidate.validate_connections(),
        ) {
            return Err(error);
        }
        *self = candidate;
        Ok(())
    }

    /// Set the compositing mode for one canonical Merge input without
    /// changing its persistent identity or order. Non-normal modes belong to
    /// Image -> Merge `images` wires; every other connection remains Normal.
    pub fn set_connection_blend_mode(
        &mut self,
        connection_id: Uuid,
        blend_mode: BlendMode,
    ) -> Result<(), ProjectGraphError> {
        let index = self
            .connections
            .iter()
            .position(|connection| connection.id == connection_id)
            .ok_or(ProjectGraphError::ConnectionNotFound(connection_id))?;
        let mut candidate = self.connections[index].clone();
        candidate.blend_mode = blend_mode;
        if let Some(error) = self
            .validate_connection_blend_mode(&candidate)
            .into_iter()
            .next()
        {
            return Err(error);
        }
        if self.connections[index].blend_mode == blend_mode {
            return Ok(());
        }
        self.connections[index].blend_mode = blend_mode;
        Ok(())
    }

    pub fn disconnect_connection(&mut self, id: Uuid) -> bool {
        self.disconnect_connections([id]) == 1
    }

    /// Remove a set of canonical connections and normalize every affected
    /// variadic target once. This is the atomic model primitive used by a
    /// multi-wire knife gesture.
    pub fn disconnect_connections(&mut self, ids: impl IntoIterator<Item = Uuid>) -> usize {
        let ids = ids.into_iter().collect::<HashSet<_>>();
        let baseline = self.validate_connections();
        let mut candidate = self.clone();
        let removed = candidate.disconnect_connections_unchecked(ids);
        if removed == 0 {
            return 0;
        }
        if super::super::first_new_project_validation_error(
            &baseline,
            candidate.validate_connections(),
        )
        .is_some()
        {
            return 0;
        }
        *self = candidate;
        removed
    }

    pub(in crate::model::project) fn disconnect_connections_unchecked(
        &mut self,
        ids: impl IntoIterator<Item = Uuid>,
    ) -> usize {
        let ids = ids.into_iter().collect::<HashSet<_>>();
        let affected_targets = self
            .connections
            .iter()
            .filter(|item| ids.contains(&item.id))
            .map(|item| item.to.clone())
            .collect::<HashSet<_>>();
        let old_len = self.connections.len();
        self.connections.retain(|item| !ids.contains(&item.id));
        let removed = old_len - self.connections.len();
        if removed != 0 {
            self.normalize_connection_orders_for_targets(&affected_targets);
        }
        removed
    }

    /// Move either end of one canonical connection without changing its
    /// persistent identity. The mutation is transactional and retains the
    /// original variadic order whenever the target is unchanged.
    pub fn reconnect_connection(
        &mut self,
        id: Uuid,
        from: PortAddress,
        to: PortAddress,
    ) -> Result<(), ProjectGraphError> {
        let original_index = self
            .connections
            .iter()
            .position(|connection| connection.id == id)
            .ok_or(ProjectGraphError::ConnectionNotFound(id))?;
        let original = self.connections[original_index].clone();
        if original.from == from && original.to == to {
            return Ok(());
        }

        let target = self
            .port_definition(&to, PortDirection::Input)
            .ok_or_else(|| ProjectGraphError::PortNotFound(to.clone()))?;
        let baseline = self.validate_connections();
        let mut candidate = self.clone();
        let mut moved = candidate.connections.remove(original_index);
        moved.from = from;
        moved.to = to.clone();

        if original.to != to {
            candidate.normalize_connection_orders();
            moved.order = match target.multiplicity {
                PortMultiplicity::Single => {
                    candidate
                        .connections
                        .retain(|connection| connection.to != to);
                    0
                }
                PortMultiplicity::Variadic => {
                    let count = candidate
                        .connections
                        .iter()
                        .filter(|connection| connection.to == to)
                        .count() as i64;
                    let insertion_order = original.order.clamp(0, count);
                    for connection in candidate
                        .connections
                        .iter_mut()
                        .filter(|connection| connection.to == to)
                    {
                        if connection.order >= insertion_order {
                            connection.order += 1;
                        }
                    }
                    insertion_order
                }
            };
        }

        if let Some(error) = candidate.structural_custom_insertion_error(
            &to,
            &moved.from,
            moved.order.max(0) as usize,
        ) {
            return Err(error);
        }

        candidate
            .connections
            .insert(original_index.min(candidate.connections.len()), moved);
        if let Some(container) = candidate.container_for_structural_target(&to) {
            candidate.reorder_structural_children(container, Some(id));
        }
        if let Some(error) = super::super::first_new_project_validation_error(
            &baseline,
            candidate.validate_connections(),
        ) {
            return Err(error);
        }
        *self = candidate;
        Ok(())
    }

    /// Insert an already-contained Node (or another compatible owner) into an
    /// existing connection. The original connection becomes the downstream
    /// edge and therefore keeps its UUID, target and variadic order; only one
    /// new upstream connection is allocated.
    pub fn splice_connection(
        &mut self,
        connection_id: Uuid,
        via_input: PortAddress,
        via_output: PortAddress,
    ) -> Result<Uuid, ProjectGraphError> {
        let original = self
            .connections
            .iter()
            .find(|connection| connection.id == connection_id)
            .cloned()
            .ok_or(ProjectGraphError::ConnectionNotFound(connection_id))?;
        let via_input_definition = self
            .port_definition(&via_input, PortDirection::Input)
            .ok_or_else(|| ProjectGraphError::PortNotFound(via_input.clone()))?;
        if via_input_definition.multiplicity == PortMultiplicity::Single
            && self
                .connections
                .iter()
                .any(|connection| connection.to == via_input)
        {
            return Err(ProjectGraphError::SpliceInputOccupied { target: via_input });
        }
        let baseline = self.validate_connections();
        let mut candidate = self.clone();
        let upstream_id = candidate.connect_ports(original.from, via_input)?;
        let downstream = candidate
            .connections
            .iter_mut()
            .find(|connection| connection.id == connection_id)
            .ok_or(ProjectGraphError::ConnectionNotFound(connection_id))?;
        downstream.from = via_output;

        if let Some(error) = super::super::first_new_project_validation_error(
            &baseline,
            candidate.validate_connections(),
        ) {
            return Err(error);
        }
        *self = candidate;
        Ok(upstream_id)
    }

    pub fn disconnect_ports(&mut self, from: &PortAddress, to: &PortAddress) -> bool {
        let baseline = self.validate_connections();
        let mut candidate = self.clone();
        let old_len = candidate.connections.len();
        candidate
            .connections
            .retain(|item| &item.from != from || &item.to != to);
        if old_len == candidate.connections.len() {
            return false;
        }
        candidate.normalize_connection_orders();
        if super::super::first_new_project_validation_error(
            &baseline,
            candidate.validate_connections(),
        )
        .is_some()
        {
            return false;
        }
        *self = candidate;
        true
    }

    fn normalize_connection_orders(&mut self) {
        let targets = self
            .connections
            .iter()
            .map(|item| item.to.clone())
            .collect::<HashSet<_>>();
        self.normalize_connection_orders_for_targets(&targets);
    }

    pub(in crate::model::project) fn normalize_connection_orders_for_targets(
        &mut self,
        targets: &HashSet<PortAddress>,
    ) {
        for target in targets {
            let mut ids = self
                .connections
                .iter()
                .filter(|item| item.to == *target)
                .map(|item| (item.order, item.id))
                .collect::<Vec<_>>();
            ids.sort_by_key(|(order, id)| (*order, *id));
            for (order, (_, id)) in ids.into_iter().enumerate() {
                if let Some(connection) = self.connections.iter_mut().find(|item| item.id == id) {
                    connection.order = order as i64;
                }
            }
        }
    }
}
