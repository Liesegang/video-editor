use library::model::property::{Property, PropertyValue, Vec2};
use library::model::Node;
use library::plugin::PluginManager;
use ordered_float::OrderedFloat;
use uuid::Uuid;

pub(super) fn root_transform_node(
    plugin_manager: &PluginManager,
    id: Uuid,
    name: &str,
    position: [f64; 2],
    anchor: [f64; 2],
    ui_position: [f32; 2],
) -> Result<Node, String> {
    let mut node = operation_node(
        plugin_manager.create_shape_transform_operation_node(),
        id,
        name,
        ui_position,
    )?;
    for (key, value) in [
        (
            "position",
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(position[0]),
                y: OrderedFloat(position[1]),
            }),
        ),
        (
            "anchor",
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(anchor[0]),
                y: OrderedFloat(anchor[1]),
            }),
        ),
    ] {
        node.set_property(key.to_string(), Property::constant(value))?;
    }
    Ok(node)
}

pub(super) fn operation_node<E: std::fmt::Display>(
    result: Result<Node, E>,
    id: Uuid,
    name: &str,
    ui_position: [f32; 2],
) -> Result<Node, String> {
    let mut node = result.map_err(|error| format!("cannot create QA {name}: {error}"))?;
    node.id = id;
    node.name = name.to_string();
    node.ui_position = ui_position;
    Ok(node)
}
