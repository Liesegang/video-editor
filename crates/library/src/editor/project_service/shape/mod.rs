//! Shape-chain Effector/Decorator authoring commands.

use super::lifecycle::ProjectManager;
use crate::error::LibraryError;
use crate::model::Node;
use crate::model::NodeContent;
use crate::model::project::{
    BACKGROUND_SHAPE_INPUT_PORT, NodeGraphBundle, PortAddress, PortDataType, PortDirection,
    PortOwner, ProjectConnection, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use std::collections::HashSet;
use uuid::Uuid;

mod backplate;

impl ProjectManager {
    fn insert_shape_operation_after(
        &self,
        node_id: Uuid,
        mut operation: Node,
    ) -> Result<(), LibraryError> {
        let mut project = self
            .project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let source = PortAddress::new(PortOwner::Node(node_id), SHAPE_OUTPUT_PORT);
        let source_definition = project
            .port_definition(&source, PortDirection::Output)
            .filter(|definition| definition.data_type == PortDataType::Shape)
            .ok_or_else(|| {
                LibraryError::Project(format!("Node {node_id} does not produce Shape"))
            })?;
        debug_assert_eq!(source_definition.direction, PortDirection::Output);
        let container = project.find_node_container(node_id).ok_or_else(|| {
            LibraryError::Project(format!("Node {node_id} has no containing graph"))
        })?;

        // Appending through the public API follows the existing linear
        // Effector/Decorator chain, so repeated additions preserve UI order.
        // A final Shape fan-out is spliced as one atomic graph mutation.
        let mut terminal_id = node_id;
        let mut visited = HashSet::new();
        loop {
            if !visited.insert(terminal_id) {
                return Err(LibraryError::Project(format!(
                    "Shape chain from Node {node_id} contains a cycle"
                )));
            }
            let terminal_output = PortAddress::new(PortOwner::Node(terminal_id), SHAPE_OUTPUT_PORT);
            let outgoing = project
                .connections
                .iter()
                .filter(|connection| connection.from == terminal_output)
                .collect::<Vec<_>>();
            let [connection] = outgoing.as_slice() else {
                break;
            };
            let PortOwner::Node(next_id) = connection.to.owner else {
                break;
            };
            if connection.to.port != SHAPE_INPUT_PORT {
                break;
            }
            if project.find_node_container(next_id) != Some(container) {
                break;
            }
            let next_output = PortAddress::new(PortOwner::Node(next_id), SHAPE_OUTPUT_PORT);
            let next_is_shape_operation = project
                .port_definition(&next_output, PortDirection::Output)
                .is_some_and(|definition| definition.data_type == PortDataType::Shape);
            if !next_is_shape_operation {
                break;
            }
            terminal_id = next_id;
        }

        let terminal_output = PortAddress::new(PortOwner::Node(terminal_id), SHAPE_OUTPUT_PORT);
        let outgoing = project
            .connections
            .iter()
            .filter(|connection| connection.from == terminal_output)
            .cloned()
            .collect::<Vec<_>>();
        let terminal_position = project
            .get_node(terminal_id)
            .map(|node| node.ui_position)
            .ok_or_else(|| LibraryError::Project(format!("Node {terminal_id} not found")))?;
        operation.ui_position = [terminal_position[0] + 240.0, terminal_position[1]];
        let operation_id = operation.id;

        let mut updated = project.clone();
        let removed = outgoing
            .iter()
            .map(|connection| connection.id)
            .collect::<HashSet<_>>();
        updated
            .connections
            .retain(|connection| !removed.contains(&connection.id));
        let mut connections = vec![ProjectConnection::new(
            terminal_output,
            PortAddress::new(PortOwner::Node(operation_id), SHAPE_INPUT_PORT),
            0,
        )];
        connections.extend(outgoing.into_iter().map(|mut connection| {
            connection.from = PortAddress::new(PortOwner::Node(operation_id), SHAPE_OUTPUT_PORT);
            connection
        }));
        updated
            .insert_node_graph(
                container,
                NodeGraphBundle::new(vec![operation], connections, None),
            )
            .map_err(|error| LibraryError::Project(error.to_string()))?;
        *project = updated;
        Ok(())
    }

    pub fn add_effector(&self, node_id: Uuid, effector_type: &str) -> Result<(), LibraryError> {
        let effector = self
            .plugin_manager
            .create_effector_operation_node(effector_type)?;
        self.insert_shape_operation_after(node_id, effector)
    }

    pub fn add_decorator(&self, node_id: Uuid, decorator_type: &str) -> Result<(), LibraryError> {
        let decorator = self
            .plugin_manager
            .create_decorator_operation_node(decorator_type)?;
        if matches!(
            decorator.content(),
            NodeContent::PluginOperation(operation)
                if {
                operation
                    .declared_ports
                    .iter()
                    .any(|port| port.key == BACKGROUND_SHAPE_INPUT_PORT)
                }
        ) {
            return self.insert_backplate_geometry_branch(node_id, decorator);
        }
        self.insert_shape_operation_after(node_id, decorator)
    }
}
