use anyhow::Result;
use library::editor::project_service::GeneratorNodeRequest;
use library::model::project::{
    NodeGraphBundle, PortAddress, PortOwner, ProjectConnection, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::model::property::{PropertyValue, Vec2};
use library::plugin::PluginManager;
use ordered_float::OrderedFloat;

use super::{generator_node_for_canvas, set_declared_property};

pub(super) fn text_overlay_graph(plugin_manager: &PluginManager) -> Result<NodeGraphBundle> {
    let mut text = generator_node_for_canvas(
        "text",
        GeneratorNodeRequest::Text {
            text: "E2E".to_string(),
            font: "Arial".to_string(),
        },
        12,
        8,
        12,
        8,
    );
    set_declared_property(&mut text, "size", PropertyValue::Number(OrderedFloat(5.0)))?;
    let mut transform = plugin_manager.create_shape_transform_operation_node()?;
    set_declared_property(
        &mut transform,
        "position",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(1.0),
            y: OrderedFloat(5.0),
        }),
    )?;
    set_declared_property(
        &mut transform,
        "anchor",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(6.0),
            y: OrderedFloat(4.0),
        }),
    )?;
    let fill = plugin_manager.create_style_operation_node("fill")?;
    let text_id = text.id;
    let transform_id = transform.id;
    let fill_id = fill.id;
    Ok(NodeGraphBundle::new(
        vec![text, transform, fill],
        vec![
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(text_id), SHAPE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(transform_id), SHAPE_INPUT_PORT),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(transform_id), SHAPE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(fill_id), SHAPE_INPUT_PORT),
                0,
            ),
        ],
        Some(fill_id),
    ))
}
