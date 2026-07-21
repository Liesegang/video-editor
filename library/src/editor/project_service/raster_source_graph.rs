use super::ProjectManager;
use crate::editor::handlers::clip_handler::ClipBundle;
use crate::error::LibraryError;
use crate::model::project::{
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, NodeGraphBundle, PortAddress, PortOwner, ProjectConnection,
};
use crate::model::property::Property;
use crate::model::{Clip, CompositionInstanceContent, Node};

impl ProjectManager {
    pub fn create_composition_instance_clip(
        &self,
        composition_id: uuid::Uuid,
        start_time: f64,
        duration: f64,
    ) -> Result<ClipBundle, LibraryError> {
        let source = Node::new_composition_instance(
            "Composition Instance",
            CompositionInstanceContent { composition_id },
        );
        self.wrap_positioned_av_clip(
            Clip::new("Composition Instance Clip", start_time, duration),
            source,
            [0, 0],
            [0, 0],
        )
    }

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

    pub(super) fn wrap_positioned_image_clip(
        &self,
        clip: Clip,
        source: Node,
        canvas: [u64; 2],
        source_size: [u64; 2],
    ) -> Result<ClipBundle, LibraryError> {
        Ok(ClipBundle {
            clip,
            graph: self.create_image_source_graph(
                source,
                canvas[0],
                canvas[1],
                source_size[0],
                source_size[1],
            )?,
        })
    }

    pub(super) fn wrap_positioned_av_clip(
        &self,
        mut clip: Clip,
        source: Node,
        canvas: [u64; 2],
        source_size: [u64; 2],
    ) -> Result<ClipBundle, LibraryError> {
        clip.audio_output_node_id = Some(source.id);
        self.wrap_positioned_image_clip(clip, source, canvas, source_size)
    }
}
