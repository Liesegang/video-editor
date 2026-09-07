//! Factory for the reusable Plexus Network Node Clip template.
//!
//! The preset is an ordinary project-owned Module definition. Assets and the
//! Timeline place it through the same reusable Node Clip path as user-created
//! templates, while its published inputs use the shared Inspector surface.

use crate::editor::module_graph_factory::{connection, publish_node_property};
use crate::error::LibraryError;
use crate::model::authoring::{
    ModuleDefinition, ModuleDefinitionSharing, ModuleOutputId, ModuleTemplateOrigin,
    PublishedParameterId,
};
use crate::model::node::{
    CONNECT_POINTS_CATALOG_ID, NODE_LAYOUT_COLUMN_GAP, Node, POINT_COLOR_INPUT_PORT,
    POINT_CONNECTIONS_PORT, POINT_LINE_FADE_INPUT_PORT, POINT_LINE_RENDERER_CATALOG_ID,
    POINT_LINE_WIDTH_INPUT_PORT, POINT_MAX_DISTANCE_INPUT_PORT, POINT_MAX_NEIGHBORS_INPUT_PORT,
    POINT_MIN_DISTANCE_INPUT_PORT, POINT_SOURCE_PORT, PROPERTY_NODE_UI_WIDTH, PointNodeRole,
};
use crate::model::project::{IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, PortDataType};
use crate::model::property::{Property, PropertyValue, Vec3};
use ordered_float::OrderedFloat;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlexusPublishedParameters {
    pub count_x: PublishedParameterId,
    pub count_y: PublishedParameterId,
    pub count_z: PublishedParameterId,
    pub spacing: PublishedParameterId,
    pub center: PublishedParameterId,
    pub min_distance: PublishedParameterId,
    pub max_distance: PublishedParameterId,
    pub max_neighbors: PublishedParameterId,
    pub color: PublishedParameterId,
    pub width: PublishedParameterId,
    pub fade: PublishedParameterId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlexusNodeClipDefinition {
    pub definition: ModuleDefinition,
    pub output_id: ModuleOutputId,
    pub parameters: PlexusPublishedParameters,
}

pub struct PlexusNodeClipFactory;

impl PlexusNodeClipFactory {
    pub fn create(name: impl Into<String>) -> Result<PlexusNodeClipDefinition, LibraryError> {
        let (mut definition, output_id) = ModuleDefinition::new_image(
            name,
            ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
        );
        let output_node_id = definition
            .output(output_id)
            .ok_or_else(|| {
                LibraryError::Validation("Plexus Module lost its Output terminal".to_string())
            })?
            .node_id;

        let mut grid = Node::new_catalog_node(PointNodeRole::Grid.catalog_id())
            .map_err(LibraryError::Validation)?;
        let mut connect =
            Node::new_catalog_node(CONNECT_POINTS_CATALOG_ID).map_err(LibraryError::Validation)?;
        let mut renderer = Node::new_catalog_node(POINT_LINE_RENDERER_CATALOG_ID)
            .map_err(LibraryError::Validation)?;

        set_grid_property(&mut grid, "count_x", PropertyValue::Integer(10))?;
        set_grid_property(&mut grid, "count_y", PropertyValue::Integer(7))?;
        set_grid_property(&mut grid, "count_z", PropertyValue::Integer(1))?;
        set_grid_property(
            &mut grid,
            "spacing",
            PropertyValue::Vec3(Vec3 {
                x: OrderedFloat(40.0),
                y: OrderedFloat(40.0),
                z: OrderedFloat(40.0),
            }),
        )?;
        set_grid_property(
            &mut grid,
            "center",
            PropertyValue::Vec3(Vec3 {
                x: OrderedFloat(0.0),
                y: OrderedFloat(0.0),
                z: OrderedFloat(0.0),
            }),
        )?;

        for (index, node) in [&mut grid, &mut connect, &mut renderer]
            .into_iter()
            .chain(definition.graph.nodes.get_mut(&output_node_id))
            .enumerate()
        {
            if index < 3 {
                node.ui_size[0] = PROPERTY_NODE_UI_WIDTH;
            }
            node.ui_position = [
                index as f32 * (PROPERTY_NODE_UI_WIDTH + NODE_LAYOUT_COLUMN_GAP),
                140.0,
            ];
        }

        let grid_id = grid.id;
        let connect_id = connect.id;
        let renderer_id = renderer.id;
        definition.graph.nodes.extend([
            (grid_id, grid),
            (connect_id, connect),
            (renderer_id, renderer),
        ]);
        definition.graph.connections = vec![
            connection(grid_id, POINT_SOURCE_PORT, connect_id, POINT_SOURCE_PORT),
            connection(
                connect_id,
                POINT_CONNECTIONS_PORT,
                renderer_id,
                POINT_CONNECTIONS_PORT,
            ),
            connection(
                renderer_id,
                IMAGE_OUTPUT_PORT,
                output_node_id,
                IMAGE_INPUT_PORT,
            ),
        ];

        let parameters = PlexusPublishedParameters {
            count_x: publish_node_property(
                &mut definition,
                grid_id,
                "count_x",
                "Count X",
                PortDataType::Integer,
            )?,
            count_y: publish_node_property(
                &mut definition,
                grid_id,
                "count_y",
                "Count Y",
                PortDataType::Integer,
            )?,
            count_z: publish_node_property(
                &mut definition,
                grid_id,
                "count_z",
                "Count Z",
                PortDataType::Integer,
            )?,
            spacing: publish_node_property(
                &mut definition,
                grid_id,
                "spacing",
                "Spacing",
                PortDataType::Vec3,
            )?,
            center: publish_node_property(
                &mut definition,
                grid_id,
                "center",
                "Center",
                PortDataType::Vec3,
            )?,
            min_distance: publish_node_property(
                &mut definition,
                connect_id,
                POINT_MIN_DISTANCE_INPUT_PORT,
                "Min Distance",
                PortDataType::Number,
            )?,
            max_distance: publish_node_property(
                &mut definition,
                connect_id,
                POINT_MAX_DISTANCE_INPUT_PORT,
                "Max Distance",
                PortDataType::Number,
            )?,
            max_neighbors: publish_node_property(
                &mut definition,
                connect_id,
                POINT_MAX_NEIGHBORS_INPUT_PORT,
                "Max Neighbors",
                PortDataType::Integer,
            )?,
            color: publish_node_property(
                &mut definition,
                renderer_id,
                POINT_COLOR_INPUT_PORT,
                "Color",
                PortDataType::Color,
            )?,
            width: publish_node_property(
                &mut definition,
                renderer_id,
                POINT_LINE_WIDTH_INPUT_PORT,
                "Width",
                PortDataType::Number,
            )?,
            fade: publish_node_property(
                &mut definition,
                renderer_id,
                POINT_LINE_FADE_INPUT_PORT,
                "Fade",
                PortDataType::Number,
            )?,
        };
        definition.topology_revision = 2;
        definition.interface_version = 2;
        definition.validate().map_err(LibraryError::Validation)?;

        Ok(PlexusNodeClipDefinition {
            definition,
            output_id,
            parameters,
        })
    }
}

fn set_grid_property(node: &mut Node, key: &str, value: PropertyValue) -> Result<(), LibraryError> {
    node.set_property(key.to_string(), Property::constant(value))
        .map_err(LibraryError::Validation)
}

#[cfg(test)]
#[path = "plexus_node_clip_tests.rs"]
mod tests;
