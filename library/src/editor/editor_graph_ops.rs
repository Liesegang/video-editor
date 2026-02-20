use super::editor_service::EditorService;
use crate::error::LibraryError;
use crate::model::project::property::PropertyValue;
use uuid::Uuid;

/// Graph node operations.
impl EditorService {
    pub fn add_graph_node(&self, container_id: Uuid, type_id: &str) -> Result<Uuid, LibraryError> {
        self.project_manager.add_graph_node(container_id, type_id)
    }

    pub fn remove_graph_node(&self, node_id: Uuid) -> Result<(), LibraryError> {
        self.project_manager.remove_graph_node(node_id)
    }

    pub fn add_graph_connection(
        &self,
        from: crate::model::project::connection::PinId,
        to: crate::model::project::connection::PinId,
    ) -> Result<crate::model::project::connection::Connection, LibraryError> {
        self.project_manager.add_graph_connection(from, to)
    }

    pub fn remove_graph_connection(&self, connection_id: Uuid) -> Result<(), LibraryError> {
        self.project_manager.remove_graph_connection(connection_id)
    }

    pub fn update_graph_node_property(
        &self,
        node_id: Uuid,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_graph_node_property(node_id, property_key, time, value, easing)
    }
}
