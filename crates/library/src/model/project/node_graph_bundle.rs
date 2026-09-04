//! Atomic insertion of detached Node graphs into Project containers.

use std::collections::HashSet;

use uuid::Uuid;

use crate::model::Node;

use super::transaction::first_new_project_validation_error;
use super::{NodeContainer, PortOwner, Project, ProjectConnection, ProjectGraphError};

/// A detached set of Nodes and canonical connections that can be inserted
/// into one Composition, Track, or Clip as a single Project transaction.
///
/// `output_node_id` is optional because helper-only graphs (for example a
/// detached style operation) need not replace a container's current image
/// output. When present, it must identify one of `nodes` and declare an Image
/// output port.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeGraphBundle {
    pub nodes: Vec<Node>,
    pub connections: Vec<ProjectConnection>,
    pub output_node_id: Option<Uuid>,
}

impl NodeGraphBundle {
    pub fn new(
        nodes: Vec<Node>,
        connections: Vec<ProjectConnection>,
        output_node_id: Option<Uuid>,
    ) -> Self {
        Self {
            nodes,
            connections,
            output_node_id,
        }
    }

    pub fn with_output_node(node: Node) -> Self {
        let output_node_id = Some(node.id);
        Self::new(vec![node], Vec::new(), output_node_id)
    }

    pub fn output_node(&self) -> Option<&Node> {
        let output_node_id = self.output_node_id?;
        self.nodes.iter().find(|node| node.id == output_node_id)
    }

    pub fn output_node_mut(&mut self) -> Option<&mut Node> {
        let output_node_id = self.output_node_id?;
        self.nodes.iter_mut().find(|node| node.id == output_node_id)
    }
}

impl Project {
    /// Insert a detached Node graph into one container as a single Project
    /// transaction. The receiver is unchanged if identity, containment, port,
    /// connection, cycle, or output validation fails.
    pub fn insert_node_graph(
        &mut self,
        container: NodeContainer,
        graph: NodeGraphBundle,
    ) -> Result<(), ProjectGraphError> {
        self.insert_node_graph_at(container, graph, None)
    }

    /// Variant of [`Project::insert_node_graph`] that inserts the bundled
    /// Nodes at a stable position while preserving their bundle order.
    pub fn insert_node_graph_at(
        &mut self,
        container: NodeContainer,
        graph: NodeGraphBundle,
        insert_index: Option<usize>,
    ) -> Result<(), ProjectGraphError> {
        if graph.nodes.is_empty() {
            return Err(ProjectGraphError::EmptyNodeGraph);
        }
        match container {
            NodeContainer::Composition(id) if self.get_composition(id).is_none() => {
                return Err(ProjectGraphError::CompositionNotFound(id));
            }
            NodeContainer::Track(id) if self.get_track(id).is_none() => {
                return Err(ProjectGraphError::TrackNotFound(id));
            }
            NodeContainer::Clip(id) if self.get_clip(id).is_none() => {
                return Err(ProjectGraphError::ClipNotFound(id));
            }
            _ => {}
        }

        let mut node_ids = HashSet::new();
        for node in &graph.nodes {
            if !node_ids.insert(node.id) {
                return Err(ProjectGraphError::DuplicateNodeGraphNodeId(node.id));
            }
            if self.nodes.contains_key(&node.id) {
                return Err(ProjectGraphError::NodeGraphNodeAlreadyExists(node.id));
            }
        }
        if let Some(output_node_id) = graph.output_node_id
            && !node_ids.contains(&output_node_id)
        {
            return Err(ProjectGraphError::NodeGraphOutputNotBundled(output_node_id));
        }

        let existing_connection_ids = self
            .connections
            .iter()
            .map(|connection| connection.id)
            .collect::<HashSet<_>>();
        let affected_connection_targets = graph
            .connections
            .iter()
            .map(|connection| connection.to.clone())
            .collect::<HashSet<_>>();
        let mut connection_ids = HashSet::new();
        for connection in &graph.connections {
            if !connection_ids.insert(connection.id) {
                return Err(ProjectGraphError::DuplicateNodeGraphConnectionId(
                    connection.id,
                ));
            }
            if existing_connection_ids.contains(&connection.id) {
                return Err(ProjectGraphError::NodeGraphConnectionAlreadyExists(
                    connection.id,
                ));
            }
            let touches_bundled_node = [connection.from.owner, connection.to.owner]
                .into_iter()
                .any(|owner| {
                    matches!(owner, PortOwner::Node(node_id) if node_ids.contains(&node_id))
                });
            if !touches_bundled_node {
                return Err(ProjectGraphError::NodeGraphConnectionOutsideBundle(
                    connection.id,
                ));
            }
        }

        let validation_baseline = self.validate_connections();
        let mut candidate = self.clone();
        let bundled_node_ids = graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
        for node in graph.nodes {
            candidate.nodes.insert(node.id, node);
        }
        let container_node_ids =
            candidate
                .container_node_ids_mut(container)
                .ok_or(match container {
                    NodeContainer::Composition(id) => ProjectGraphError::CompositionNotFound(id),
                    NodeContainer::Track(id) => ProjectGraphError::TrackNotFound(id),
                    NodeContainer::Clip(id) => ProjectGraphError::ClipNotFound(id),
                })?;
        let insert_index = insert_index
            .unwrap_or(container_node_ids.len())
            .min(container_node_ids.len());
        container_node_ids.splice(insert_index..insert_index, bundled_node_ids);
        candidate.connections.extend(graph.connections);
        candidate.normalize_connection_orders_for_targets(&affected_connection_targets);
        if let Some(output_node_id) = graph.output_node_id {
            candidate.set_output_node(container, Some(output_node_id))?;
        }

        if let Some(error) = first_new_project_validation_error(
            &validation_baseline,
            candidate.validate_connections(),
        ) {
            return Err(error);
        }
        *self = candidate;
        Ok(())
    }
}
