use crate::error::LibraryError;
use crate::model::project::connection::{Connection, PinId};
use crate::model::project::graph_node::GraphNode;
use crate::model::project::node::Node;
use crate::model::project::project::Project;
use crate::model::project::property::PropertyMap;
use crate::plugin::PluginManager;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct GraphHandler;

impl GraphHandler {
    /// Add a graph node to a container (track).
    ///
    /// Looks up the `NodeTypeDefinition` from PluginManager to populate default properties.
    /// Returns the new node's ID.
    pub fn add_graph_node(
        project: &Arc<RwLock<Project>>,
        plugin_manager: &PluginManager,
        container_id: Uuid,
        type_id: &str,
    ) -> Result<Uuid, LibraryError> {
        let properties = if let Some(node_type) = plugin_manager.get_node_type(type_id) {
            PropertyMap::from_definitions(&node_type.default_properties)
        } else {
            PropertyMap::new()
        };

        let node = GraphNode::new(type_id, properties);
        let node_id = node.id;

        let mut proj = super::write_project(project)?;

        // Ensure container exists
        if proj.get_track(container_id).is_none() {
            return Err(LibraryError::project(format!(
                "Container track {} not found",
                container_id
            )));
        }

        proj.add_node(Node::Graph(node));

        if let Some(track) = proj.get_track_mut(container_id) {
            track.add_child(node_id);
        }

        Ok(node_id)
    }

    /// Remove a graph node and all its connections.
    pub fn remove_graph_node(
        project: &Arc<RwLock<Project>>,
        node_id: Uuid,
    ) -> Result<(), LibraryError> {
        let mut proj = super::write_project(project)?;

        if proj.get_graph_node(node_id).is_none() {
            return Err(LibraryError::project(format!(
                "Graph node {} not found",
                node_id
            )));
        }

        // Remove from parent track's child_ids
        let parent_track_ids: Vec<Uuid> = proj
            .nodes
            .iter()
            .filter_map(|(tid, n)| match n {
                Node::Track(t) if t.child_ids.contains(&node_id) => Some(*tid),
                _ => None,
            })
            .collect();

        for track_id in parent_track_ids {
            if let Some(track) = proj.get_track_mut(track_id) {
                track.remove_child(node_id);
            }
        }

        // Remove all connections involving this node
        proj.remove_connections_for_node(node_id);

        // Remove the node itself
        proj.remove_node(node_id);

        Ok(())
    }

    /// Add a connection between two pins (with validation).
    pub fn add_connection(
        project: &Arc<RwLock<Project>>,
        from: PinId,
        to: PinId,
    ) -> Result<Connection, LibraryError> {
        let mut proj = super::write_project(project)?;

        let conn = Connection::new(from, to);

        // Validate
        crate::model::project::graph_analysis::validate_connection(&proj, &conn)
            .map_err(LibraryError::validation)?;

        proj.add_connection(conn.clone());

        Ok(conn)
    }

    /// Remove a connection by ID.
    pub fn remove_connection(
        project: &Arc<RwLock<Project>>,
        connection_id: Uuid,
    ) -> Result<(), LibraryError> {
        let mut proj = super::write_project(project)?;

        if proj.remove_connection(connection_id).is_none() {
            return Err(LibraryError::project(format!(
                "Connection {} not found",
                connection_id
            )));
        }

        Ok(())
    }

    /// Update a property on a graph node.
    pub fn update_graph_node_property(
        project: &Arc<RwLock<Project>>,
        node_id: Uuid,
        property_key: &str,
        time: f64,
        value: crate::model::project::property::PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        let mut proj = super::write_project(project)?;

        let node = proj
            .get_graph_node_mut(node_id)
            .ok_or_else(|| LibraryError::project(format!("Graph node {} not found", node_id)))?;

        node.properties
            .update_property_or_keyframe(property_key, time, value, easing);

        Ok(())
    }
}
