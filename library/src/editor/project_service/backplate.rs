//! Timeline/Inspector semantic authoring for geometry-only Backplate.
//!
//! A user adding Backplate from Timeline should not need to understand its
//! graph expansion. This writes the same authoritative Project Nodes and wires
//! an advanced user can edit directly: target + template -> Backplate -> Fill,
//! merged behind the existing image output.

use std::collections::HashSet;

use crate::editor::project_service::ProjectManager;
use crate::error::LibraryError;
use crate::model::Node;
use crate::model::frame::color::Color;
use crate::model::project::{
    BACKGROUND_SHAPE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer,
    NodeGraphBundle, PortAddress, PortDataType, PortDirection, PortOwner, ProjectConnection,
    SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use crate::model::property::{Property, PropertyValue};
use uuid::Uuid;

const DEFAULT_BACKPLATE_TEMPLATE: &str = "M 0 0 H 1 V 1 H 0 Z";

impl ProjectManager {
    pub(super) fn insert_backplate_geometry_branch(
        &self,
        target_node_id: Uuid,
        mut backplate: Node,
    ) -> Result<(), LibraryError> {
        let mut template = self.create_shape_node(DEFAULT_BACKPLATE_TEMPLATE, 1, 1, 1, 1)?;
        template.name = "Backplate Shape".to_string();
        let mut style = self.plugin_manager.create_style_operation_node("fill")?;
        style.name = "Backplate Fill".to_string();
        style
            .set_property(
                "color".to_string(),
                Property::constant(PropertyValue::Color(Color::black())),
            )
            .map_err(LibraryError::Validation)?;
        let mut merge = Node::new_merge("Backplate Merge");

        let mut project = self
            .project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let source = PortAddress::new(PortOwner::Node(target_node_id), SHAPE_OUTPUT_PORT);
        project
            .port_definition(&source, PortDirection::Output)
            .filter(|definition| definition.data_type == PortDataType::Shape)
            .ok_or_else(|| {
                LibraryError::Project(format!(
                    "Node {target_node_id} does not produce Backplate target Shape"
                ))
            })?;
        let container = project.find_node_container(target_node_id).ok_or_else(|| {
            LibraryError::Project(format!("Node {target_node_id} has no containing graph"))
        })?;
        let terminal_id = terminal_shape_node(&project, container, target_node_id)?;
        let current_output_id = current_image_output(&project, container).ok_or_else(|| {
            LibraryError::Project(format!(
                "Backplate requires an existing Image output in {container:?}"
            ))
        })?;
        let current_output_blend = project
            .get_node(current_output_id)
            .map(|node| node.blend_mode)
            .ok_or_else(|| LibraryError::Project(format!("Node {current_output_id} not found")))?;

        let terminal_position = project
            .get_node(terminal_id)
            .map(|node| node.ui_position)
            .ok_or_else(|| LibraryError::Project(format!("Node {terminal_id} not found")))?;
        backplate.ui_position = [terminal_position[0] + 240.0, terminal_position[1] + 120.0];
        template.ui_position = [terminal_position[0], terminal_position[1] + 260.0];
        style.ui_position = [terminal_position[0] + 480.0, terminal_position[1] + 120.0];
        merge.ui_position = project
            .get_node(current_output_id)
            .map(|node| [node.ui_position[0] + 300.0, node.ui_position[1]])
            .unwrap_or([terminal_position[0] + 720.0, terminal_position[1]]);

        let backplate_id = backplate.id;
        let template_id = template.id;
        let style_id = style.id;
        let merge_id = merge.id;
        let mut foreground_connection = ProjectConnection::new(
            PortAddress::new(PortOwner::Node(current_output_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
            1,
        );
        foreground_connection.blend_mode = current_output_blend;
        let connections = vec![
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(terminal_id), SHAPE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(backplate_id), SHAPE_INPUT_PORT),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(template_id), SHAPE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(backplate_id), BACKGROUND_SHAPE_INPUT_PORT),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(backplate_id), SHAPE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(style_id), SHAPE_INPUT_PORT),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(style_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
                0,
            ),
            foreground_connection,
        ];

        let mut updated = project.clone();
        updated
            .insert_node_graph(
                container,
                NodeGraphBundle::new(
                    vec![template, backplate, style, merge],
                    connections,
                    Some(merge_id),
                ),
            )
            .map_err(|error| LibraryError::Project(error.to_string()))?;
        *project = updated;
        Ok(())
    }
}

fn terminal_shape_node(
    project: &crate::model::Project,
    container: NodeContainer,
    start: Uuid,
) -> Result<Uuid, LibraryError> {
    let mut terminal = start;
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(terminal) {
            return Err(LibraryError::Project(format!(
                "Shape chain from Node {start} contains a cycle"
            )));
        }
        let output = PortAddress::new(PortOwner::Node(terminal), SHAPE_OUTPUT_PORT);
        let outgoing = project
            .connections
            .iter()
            .filter(|connection| connection.from == output)
            .collect::<Vec<_>>();
        let [connection] = outgoing.as_slice() else {
            break;
        };
        let PortOwner::Node(next) = connection.to.owner else {
            break;
        };
        if connection.to.port != SHAPE_INPUT_PORT
            || project.find_node_container(next) != Some(container)
        {
            break;
        }
        let next_output = PortAddress::new(PortOwner::Node(next), SHAPE_OUTPUT_PORT);
        if !project
            .port_definition(&next_output, PortDirection::Output)
            .is_some_and(|definition| definition.data_type == PortDataType::Shape)
        {
            break;
        }
        terminal = next;
    }
    Ok(terminal)
}

fn current_image_output(project: &crate::model::Project, container: NodeContainer) -> Option<Uuid> {
    match container {
        NodeContainer::Composition(id) => project.get_composition(id)?.output_node_id,
        NodeContainer::Track(id) => project.get_track(id)?.output_node_id,
        NodeContainer::Clip(id) => project.get_clip(id)?.output_node_id,
    }
}
