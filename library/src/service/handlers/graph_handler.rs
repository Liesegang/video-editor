use crate::error::LibraryError;
use crate::plugin::PluginManager;
use crate::project::connection::{Connection, PinId};
use crate::project::graph_node::GraphNode;
use crate::project::node::Node;
use crate::project::project::Project;
use crate::project::property::PropertyMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct GraphHandler;

impl GraphHandler {
    /// Add a graph node to a container (Track or Layer).
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

        // Ensure container exists (Track or Layer)
        if proj.get_container_child_ids(container_id).is_none() {
            return Err(LibraryError::project(format!(
                "Container {} not found (not a Track or Layer)",
                container_id
            )));
        }

        proj.add_node(Node::Graph(node));

        if let Some(children) = proj.get_container_child_ids_mut(container_id) {
            children.push(node_id);
        }

        Ok(node_id)
    }

    /// Remove a graph node, reconnecting chains where possible.
    ///
    /// For each pin type (image, shape), if the node has both an input and output connection,
    /// the upstream and downstream nodes are bridged together to preserve chain continuity.
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

        // Before removing connections, bridge chains that pass through this node.
        // For each pin type pair (e.g., image_in/image_out, shape_in/shape_out),
        // if this node has an incoming and outgoing connection, reconnect them.
        let pin_pairs = [("image_in", "image_out"), ("shape_in", "shape_out")];
        let mut bridge_connections = Vec::new();

        for (in_pin, out_pin) in &pin_pairs {
            let incoming = proj
                .connections
                .iter()
                .find(|c| c.to.node_id == node_id && c.to.pin_name == *in_pin)
                .map(|c| c.from.clone());

            let outgoing = proj
                .connections
                .iter()
                .find(|c| c.from.node_id == node_id && c.from.pin_name == *out_pin)
                .map(|c| c.to.clone());

            if let (Some(upstream_pin), Some(downstream_pin)) = (incoming, outgoing) {
                bridge_connections.push((upstream_pin, downstream_pin));
            }
        }

        // Remove from parent container's child_ids (Track or Layer)
        let parent_ids: Vec<Uuid> = proj
            .nodes
            .iter()
            .filter_map(|(id, n)| match n {
                Node::Track(t) if t.child_ids.contains(&node_id) => Some(*id),
                Node::Layer(l) if l.child_ids.contains(&node_id) => Some(*id),
                _ => None,
            })
            .collect();

        for pid in parent_ids {
            if let Some(children) = proj.get_container_child_ids_mut(pid) {
                children.retain(|id| *id != node_id);
            }
        }

        // Remove all connections involving this node
        proj.remove_connections_for_node(node_id);

        // Remove the node itself
        proj.remove_node(node_id);

        // Create bridge connections
        for (from_pin, to_pin) in bridge_connections {
            proj.add_connection(Connection::new(from_pin, to_pin));
        }

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
        crate::project::graph_analysis::validate_connection(&proj, &conn)
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

    /// Reorder the effect chain for a source.
    ///
    /// Rewires the image chain connections to match the new order.
    /// Chain: source.image_out → effect[0].image_in → … → effect[N].image_out → terminal.image_in
    pub fn reorder_effect_chain(
        project: &Arc<RwLock<Project>>,
        source_id: Uuid,
        new_order: &[Uuid],
    ) -> Result<(), LibraryError> {
        let mut proj = super::write_project(project)?;

        // Resolve source context to find the current chain, source and terminal
        let ctx = crate::project::graph_analysis::resolve_source_context(&proj, source_id);
        let old_chain = &ctx.effect_chain;

        // Validate new_order contains exactly the same effect IDs
        if new_order.len() != old_chain.len() {
            return Err(LibraryError::project(
                "New effect order has different length than current chain".to_string(),
            ));
        }
        for id in new_order {
            if !old_chain.contains(id) {
                return Err(LibraryError::project(format!(
                    "Effect {} not in current chain",
                    id
                )));
            }
        }

        // Find feed node (what feeds into the first effect)
        // It's either the source itself, or the last style node
        let feed_id = if !ctx.style_chain.is_empty() {
            *ctx.style_chain.last().unwrap()
        } else {
            source_id
        };

        // Find terminal: what the last effect connects to (usually transform)
        let terminal_pin = if let Some(last_effect) = old_chain.last() {
            proj.connections
                .iter()
                .find(|c| {
                    c.from == PinId::new(*last_effect, "image_out") && c.to.pin_name == "image_in"
                })
                .map(|c| c.to.clone())
        } else {
            None
        };

        // Remove all connections in the effect chain (source→effects→terminal)
        let chain_node_ids: std::collections::HashSet<Uuid> = old_chain.iter().copied().collect();
        let connections_to_remove: Vec<Uuid> = proj
            .connections
            .iter()
            .filter(|c| {
                // feed → first effect
                (c.from.node_id == feed_id && c.from.pin_name == "image_out"
                    && chain_node_ids.contains(&c.to.node_id) && c.to.pin_name == "image_in")
                // effect → effect
                || (chain_node_ids.contains(&c.from.node_id) && c.from.pin_name == "image_out"
                    && chain_node_ids.contains(&c.to.node_id) && c.to.pin_name == "image_in")
                // last effect → terminal
                || (chain_node_ids.contains(&c.from.node_id) && c.from.pin_name == "image_out"
                    && terminal_pin.as_ref().map_or(false, |tp| c.to == *tp))
            })
            .map(|c| c.id)
            .collect();

        for conn_id in connections_to_remove {
            proj.remove_connection(conn_id);
        }

        // Re-create connections in new order
        if !new_order.is_empty() {
            // feed → first effect
            proj.add_connection(Connection::new(
                PinId::new(feed_id, "image_out"),
                PinId::new(new_order[0], "image_in"),
            ));

            // effect[i] → effect[i+1]
            for i in 0..new_order.len() - 1 {
                proj.add_connection(Connection::new(
                    PinId::new(new_order[i], "image_out"),
                    PinId::new(new_order[i + 1], "image_in"),
                ));
            }

            // last effect → terminal
            if let Some(tp) = terminal_pin {
                proj.add_connection(Connection::new(
                    PinId::new(*new_order.last().unwrap(), "image_out"),
                    tp,
                ));
            }
        }

        Ok(())
    }

    /// Update a property on a graph node.
    pub fn update_graph_node_property(
        project: &Arc<RwLock<Project>>,
        node_id: Uuid,
        property_key: &str,
        time: f64,
        value: crate::project::property::PropertyValue,
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
