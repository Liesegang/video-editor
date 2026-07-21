use super::ProjectManager;
use crate::error::LibraryError;
use crate::model::Node;
use crate::model::project::{
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, NodeGraphBundle, PortAddress, PortOwner, ProjectConnection,
};
use crate::model::property::Property;

impl ProjectManager {
    fn create_positioned_image_transform_node(
        &self,
        position: [f64; 2],
        anchor: [f64; 2],
    ) -> Result<Node, LibraryError> {
        let mut node = self
            .plugin_manager
            .create_image_transform_operation_node()?;
        for (key, value) in [
            (
                "position",
                crate::plugin::transforms::vec2_value(position[0], position[1]),
            ),
            (
                "anchor",
                crate::plugin::transforms::vec2_value(anchor[0], anchor[1]),
            ),
        ] {
            node.set_property(key.to_string(), Property::constant(value))
                .map_err(LibraryError::Validation)?;
        }
        Ok(node)
    }

    /// Wraps a spatially neutral Image source in the explicit operation that
    /// owns its absolute placement. Source-specific authored properties stay
    /// on the source Node for exact inspection.
    pub(super) fn create_image_source_graph(
        &self,
        mut source: Node,
        canvas_width: u64,
        canvas_height: u64,
        source_width: u64,
        source_height: u64,
    ) -> Result<NodeGraphBundle, LibraryError> {
        let mut transform = self.create_positioned_image_transform_node(
            [canvas_width as f64 / 2.0, canvas_height as f64 / 2.0],
            [source_width as f64 / 2.0, source_height as f64 / 2.0],
        )?;
        source.ui_position = [0.0, 0.0];
        transform.ui_position = [320.0, 0.0];
        let source_id = source.id;
        let transform_id = transform.id;
        Ok(NodeGraphBundle::new(
            vec![source, transform],
            vec![ProjectConnection::new(
                PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(transform_id), IMAGE_INPUT_PORT),
                0,
            )],
            Some(transform_id),
        ))
    }
}
