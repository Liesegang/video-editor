//! Atomic topology operations for Module connections.

use crate::model::BlendMode;
use crate::model::authoring::{
    ModuleConnection, ModuleConnectionId, ModuleDefinition, ModulePortAddress,
};

use super::bump_topology_revision;

pub(super) fn connect_definition_ports(
    definition: &mut ModuleDefinition,
    from: ModulePortAddress,
    to: ModulePortAddress,
    order: i64,
) -> Result<ModuleConnectionId, String> {
    let connection_id = ModuleConnectionId::new();
    definition.graph.connections.push(ModuleConnection {
        id: connection_id,
        from,
        to,
        order,
        blend_mode: BlendMode::Normal,
    });
    bump_topology_revision(definition)?;
    Ok(connection_id)
}

pub(super) fn reconnect_definition_connection(
    definition: &mut ModuleDefinition,
    connection_id: ModuleConnectionId,
    from: ModulePortAddress,
    to: ModulePortAddress,
) -> Result<(), String> {
    let index = definition
        .graph
        .connections
        .iter()
        .position(|connection| connection.id == connection_id)
        .ok_or_else(|| format!("Missing Module connection {connection_id}"))?;
    let previous = definition.graph.connections[index].clone();
    if previous.from == from && previous.to == to {
        return Ok(());
    }
    definition.graph.connections[index].from = from;
    definition.graph.connections[index].to = to;
    if let Err(error) = definition.graph.validate() {
        definition.graph.connections[index] = previous;
        return Err(error);
    }
    // Connection identity, authored input order, and per-edge Blend mode are
    // deliberately untouched by an endpoint gesture.
    if let Err(error) = bump_topology_revision(definition) {
        definition.graph.connections[index] = previous;
        return Err(error);
    }
    Ok(())
}

pub(super) fn set_definition_connection_blend_mode(
    definition: &mut ModuleDefinition,
    connection_id: ModuleConnectionId,
    blend_mode: BlendMode,
) -> Result<(), String> {
    let index = definition
        .graph
        .connections
        .iter()
        .position(|connection| connection.id == connection_id)
        .ok_or_else(|| format!("Missing Module connection {connection_id}"))?;
    if definition.graph.connections[index].blend_mode == blend_mode {
        return Ok(());
    }
    let previous = definition.graph.connections[index].blend_mode;
    definition.graph.connections[index].blend_mode = blend_mode;
    if let Err(error) = definition.graph.validate() {
        definition.graph.connections[index].blend_mode = previous;
        return Err(error);
    }
    bump_topology_revision(definition)
}

pub(super) fn disconnect_definition_connection(
    definition: &mut ModuleDefinition,
    connection_id: ModuleConnectionId,
) -> Result<(), String> {
    let before = definition.graph.connections.len();
    definition
        .graph
        .connections
        .retain(|connection| connection.id != connection_id);
    if before == definition.graph.connections.len() {
        return Err(format!("Missing Module connection {connection_id}"));
    }
    normalize_connection_order(definition);
    bump_topology_revision(definition)
}

fn normalize_connection_order(definition: &mut ModuleDefinition) {
    let mut targets = definition
        .graph
        .connections
        .iter()
        .map(|connection| connection.to.clone())
        .collect::<Vec<_>>();
    targets.sort_by(|left, right| {
        left.node_id
            .cmp(&right.node_id)
            .then_with(|| left.port.cmp(&right.port))
    });
    targets.dedup();
    for target in targets {
        let mut connections = definition
            .graph
            .connections
            .iter_mut()
            .filter(|connection| connection.to == target)
            .collect::<Vec<_>>();
        connections.sort_by_key(|connection| connection.order);
        for (order, connection) in connections.into_iter().enumerate() {
            connection.order = order as i64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Node;
    use crate::model::authoring::ModuleDefinitionSharing;
    use crate::model::project::{
        IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NUMBER_RESULT_OUTPUT_PORT, NUMERIC_A_INPUT_PORT,
    };
    use crate::plugin::PluginManager;

    #[test]
    fn reconnect_preserves_edge_identity_order_and_blend_atomically() {
        let (mut definition, _) =
            ModuleDefinition::new_image("Reconnect", ModuleDefinitionSharing::Private);
        let first = Node::new_add("First");
        let second = Node::new_add("Second");
        let target = Node::new_add("Target");
        let (first_id, second_id, target_id) = (first.id, second.id, target.id);
        definition.graph.nodes.extend([
            (first_id, first),
            (second_id, second),
            (target_id, target),
        ]);
        let connection_id = ModuleConnectionId::new();
        definition.graph.connections.push(ModuleConnection {
            id: connection_id,
            from: ModulePortAddress {
                node_id: first_id,
                port: NUMBER_RESULT_OUTPUT_PORT.to_string(),
            },
            to: ModulePortAddress {
                node_id: target_id,
                port: NUMERIC_A_INPUT_PORT.to_string(),
            },
            order: 0,
            blend_mode: BlendMode::Normal,
        });
        let before_revision = definition.topology_revision;
        let to = definition.graph.connections[0].to.clone();
        reconnect_definition_connection(
            &mut definition,
            connection_id,
            ModulePortAddress {
                node_id: second_id,
                port: NUMBER_RESULT_OUTPUT_PORT.to_string(),
            },
            to,
        )
        .unwrap();

        let changed = &definition.graph.connections[0];
        assert_eq!(changed.id, connection_id);
        assert_eq!(changed.from.node_id, second_id);
        assert_eq!(changed.order, 0);
        assert_eq!(changed.blend_mode, BlendMode::Normal);
        assert_eq!(definition.topology_revision, before_revision + 1);
        definition.validate().unwrap();
    }

    #[test]
    fn invalid_reconnect_restores_the_complete_original_edge() {
        let (mut definition, _) =
            ModuleDefinition::new_image("Reconnect", ModuleDefinitionSharing::Private);
        let source = Node::new_add("Source");
        let target = Node::new_add("Target");
        let (source_id, target_id) = (source.id, target.id);
        definition
            .graph
            .nodes
            .extend([(source_id, source), (target_id, target)]);
        let connection_id = ModuleConnectionId::new();
        definition.graph.connections.push(ModuleConnection {
            id: connection_id,
            from: ModulePortAddress {
                node_id: source_id,
                port: NUMBER_RESULT_OUTPUT_PORT.to_string(),
            },
            to: ModulePortAddress {
                node_id: target_id,
                port: NUMERIC_A_INPUT_PORT.to_string(),
            },
            order: 0,
            blend_mode: BlendMode::Normal,
        });
        let original = definition.graph.connections[0].clone();
        let revision = definition.topology_revision;
        assert!(
            reconnect_definition_connection(
                &mut definition,
                connection_id,
                ModulePortAddress {
                    node_id: target_id,
                    port: NUMBER_RESULT_OUTPUT_PORT.to_string(),
                },
                ModulePortAddress {
                    node_id: target_id,
                    port: NUMERIC_A_INPUT_PORT.to_string(),
                },
            )
            .is_err()
        );
        assert_eq!(definition.graph.connections[0], original);
        assert_eq!(definition.topology_revision, revision);
    }

    #[test]
    fn reconnect_target_keeps_variadic_order_and_non_default_blend() {
        let (mut definition, _) =
            ModuleDefinition::new_image("Reconnect", ModuleDefinitionSharing::Private);
        let plugins = PluginManager::default();
        let source_zero = plugins.create_image_transform_operation_node().unwrap();
        let source_one = plugins.create_image_transform_operation_node().unwrap();
        let first_merge = Node::new_merge("First Merge");
        let second_merge = Node::new_merge("Second Merge");
        let (source_zero_id, source_one_id, first_merge_id, second_merge_id) = (
            source_zero.id,
            source_one.id,
            first_merge.id,
            second_merge.id,
        );
        definition.graph.nodes.extend([
            (source_zero_id, source_zero),
            (source_one_id, source_one),
            (first_merge_id, first_merge),
            (second_merge_id, second_merge),
        ]);
        for merge_id in [first_merge_id, second_merge_id] {
            definition.graph.connections.push(ModuleConnection {
                id: ModuleConnectionId::new(),
                from: ModulePortAddress {
                    node_id: source_zero_id,
                    port: IMAGE_OUTPUT_PORT.to_string(),
                },
                to: ModulePortAddress {
                    node_id: merge_id,
                    port: MERGE_IMAGES_PORT.to_string(),
                },
                order: 0,
                blend_mode: BlendMode::Normal,
            });
        }
        let connection_id = ModuleConnectionId::new();
        definition.graph.connections.push(ModuleConnection {
            id: connection_id,
            from: ModulePortAddress {
                node_id: source_one_id,
                port: IMAGE_OUTPUT_PORT.to_string(),
            },
            to: ModulePortAddress {
                node_id: first_merge_id,
                port: MERGE_IMAGES_PORT.to_string(),
            },
            order: 1,
            blend_mode: BlendMode::Screen,
        });

        let from = definition.graph.connections[2].from.clone();
        reconnect_definition_connection(
            &mut definition,
            connection_id,
            from,
            ModulePortAddress {
                node_id: second_merge_id,
                port: MERGE_IMAGES_PORT.to_string(),
            },
        )
        .unwrap();

        let changed = definition
            .graph
            .connections
            .iter()
            .find(|connection| connection.id == connection_id)
            .unwrap();
        assert_eq!(changed.to.node_id, second_merge_id);
        assert_eq!(changed.order, 1);
        assert_eq!(changed.blend_mode, BlendMode::Screen);
        definition.validate().unwrap();
    }
}
