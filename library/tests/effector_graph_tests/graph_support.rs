use anyhow::{Context, Result, ensure};
use library::model::NodeContent;
use library::model::project::{
    NodeGraphBundle, PortAddress, PortDataType, PortDirection, PortOwner, ProjectConnection,
    SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use library::plugin::TRANSFORM_CATEGORY;
use uuid::Uuid;

fn shape_wire(from: Uuid, to: Uuid) -> ProjectConnection {
    ProjectConnection::new(
        PortAddress::new(PortOwner::Node(from), SHAPE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(to), SHAPE_INPUT_PORT),
        0,
    )
}

pub(super) fn insert_effector_chain(
    graph: &mut NodeGraphBundle,
    effector_ids: &[Uuid],
) -> Result<()> {
    let source_id = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.content(),
                NodeContent::Generator(
                    library::model::GeneratorContent::Text
                        | library::model::GeneratorContent::Shape
                )
            )
        })
        .context("graph has no Shape source")?
        .id;
    let mut terminal_id = source_id;
    loop {
        let outgoing = graph
            .connections
            .iter()
            .filter(|connection| {
                connection.from == PortAddress::new(PortOwner::Node(terminal_id), SHAPE_OUTPUT_PORT)
                    && connection.to.port == SHAPE_INPUT_PORT
            })
            .collect::<Vec<_>>();
        let [connection] = outgoing.as_slice() else {
            break;
        };
        let PortOwner::Node(next_id) = connection.to.owner else {
            break;
        };
        let next_produces_shape = graph.nodes.iter().any(|node| {
            node.id == next_id
                && matches!(
                    node.content(),
                    NodeContent::PluginOperation(operation)
                        if operation.declared_ports.iter().any(|port| {
                            port.key == SHAPE_OUTPUT_PORT
                                && port.direction == PortDirection::Output
                                && port.data_type == PortDataType::Shape
                        })
                )
        });
        if !next_produces_shape {
            break;
        }
        terminal_id = next_id;
    }

    let mut targets = Vec::new();
    graph.connections.retain(|connection| {
        let is_shape_fanout = connection.from
            == PortAddress::new(PortOwner::Node(terminal_id), SHAPE_OUTPUT_PORT)
            && connection.to.port == SHAPE_INPUT_PORT;
        if is_shape_fanout {
            targets.push(connection.to.clone());
        }
        !is_shape_fanout
    });
    ensure!(!targets.is_empty(), "factory must expose a Shape consumer");
    let mut upstream = terminal_id;
    for effector_id in effector_ids {
        graph.connections.push(shape_wire(upstream, *effector_id));
        upstream = *effector_id;
    }
    for target in targets {
        graph.connections.push(ProjectConnection::new(
            PortAddress::new(PortOwner::Node(upstream), SHAPE_OUTPUT_PORT),
            target,
            0,
        ));
    }
    Ok(())
}

pub(super) fn root_transform_id(graph: &NodeGraphBundle) -> Result<Uuid> {
    graph
        .nodes
        .iter()
        .find_map(|node| match node.content() {
            NodeContent::PluginOperation(operation) if operation.category == TRANSFORM_CATEGORY => {
                Some(node.id)
            }
            _ => None,
        })
        .context("factory graph has no root Transform")
}
