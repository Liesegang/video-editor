//! Trait definitions for decoupling the node editor from domain-specific types.

use uuid::Uuid;

use crate::types::{ConnectionView, NodeDisplay, NodeTypeInfo};

/// Read-only data source for the node editor.
pub trait NodeEditorDataSource {
    /// Get direct child node IDs of a container.
    fn get_container_children(&self, container_id: Uuid) -> Vec<Uuid>;

    /// Get display name of a container.
    fn get_container_name(&self, id: Uuid) -> Option<String>;

    /// Find the parent container of a node.
    fn find_parent_container(&self, node_id: Uuid) -> Option<Uuid>;

    /// Get display information for a node.
    fn get_node_display(&self, id: Uuid) -> Option<NodeDisplay>;

    /// Get all connections relevant to the current view.
    fn get_connections(&self) -> Vec<ConnectionView>;

    /// Get the type_id hint for a node (for coloring connections).
    fn get_node_type_id(&self, id: Uuid) -> Option<String>;

    /// Returns whether the node is currently active (within time range).
    /// Inactive nodes are rendered dimmed/grayed out.
    fn is_node_active(&self, id: Uuid) -> bool {
        let _ = id;
        true
    }
}

/// Mutation interface for the node editor.
pub trait NodeEditorMutator {
    /// Add a new graph node to a container.
    fn add_node(&mut self, container_id: Uuid, type_id: &str) -> Result<Uuid, String>;

    /// Remove a graph node.
    fn remove_node(&mut self, node_id: Uuid) -> Result<(), String>;

    /// Add a connection between two pins.
    fn add_connection(
        &mut self,
        from_node: Uuid,
        from_pin: &str,
        to_node: Uuid,
        to_pin: &str,
    ) -> Result<(), String>;

    /// Remove a connection by ID.
    fn remove_connection(&mut self, connection_id: Uuid) -> Result<(), String>;

    /// Get all available node types for the context menu.
    fn get_available_node_types(&self) -> Vec<NodeTypeInfo>;
}
