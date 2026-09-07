//! Shared graph assembly for first-party Module factories.

use crate::error::LibraryError;
use crate::model::BlendMode;
use crate::model::authoring::{
    ModuleConnection, ModuleConnectionId, ModuleDefinition, ModulePortAddress, PublishedParameter,
    PublishedParameterId,
};
use crate::model::project::PortDataType;

pub(super) fn connection(
    from_node: uuid::Uuid,
    from_port: &str,
    to_node: uuid::Uuid,
    to_port: &str,
) -> ModuleConnection {
    ModuleConnection {
        id: ModuleConnectionId::new(),
        from: ModulePortAddress {
            node_id: from_node,
            port: from_port.to_string(),
        },
        to: ModulePortAddress {
            node_id: to_node,
            port: to_port.to_string(),
        },
        order: 0,
        blend_mode: BlendMode::Normal,
    }
}

pub(super) fn publish_node_property(
    definition: &mut ModuleDefinition,
    node_id: uuid::Uuid,
    property: &str,
    name: &str,
    data_type: PortDataType,
) -> Result<PublishedParameterId, LibraryError> {
    let default_value = definition
        .graph
        .nodes
        .get(&node_id)
        .and_then(|node| node.properties().get(property))
        .and_then(|property| property.value())
        .cloned()
        .ok_or_else(|| {
            LibraryError::Validation(format!(
                "Module Node {node_id} has no authored '{property}' default"
            ))
        })?;
    let id = PublishedParameterId::new();
    definition.interface.parameters.push(PublishedParameter {
        id,
        name: name.to_string(),
        data_type,
        default_value,
        target: ModulePortAddress {
            node_id,
            port: property.to_string(),
        },
    });
    Ok(id)
}
