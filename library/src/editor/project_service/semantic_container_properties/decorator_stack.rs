use std::collections::HashSet;

use uuid::Uuid;

use super::helpers::{
    container_node_ids, container_port_owner, position_after_source, terminal_shape_source,
    validate_candidate,
};
use crate::editor::project_service::ProjectManager;
use crate::error::LibraryError;
use crate::model::project::{
    IMAGE_OUTPUT_PORT, NodeContainer, NodeGraphBundle, PortAddress, PortDataType, PortDirection,
    PortOwner, Project, ProjectConnection, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use crate::model::{Node, NodeContent};
use crate::plugin::{
    DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY, IMAGE_OPACITY_STYLE_COMPONENT_ID,
    STYLE_APPLY_OPERATION, STYLE_CATEGORY,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticDecoratorStack {
    owner: NodeContainer,
    node_ids: Vec<Uuid>,
}

impl SemanticDecoratorStack {
    pub fn owner(&self) -> NodeContainer {
        self.owner
    }

    pub fn node_ids(&self) -> &[Uuid] {
        &self.node_ids
    }
}

impl ProjectManager {
    pub fn semantic_container_decorator_stack(
        &self,
        owner: NodeContainer,
    ) -> Result<SemanticDecoratorStack, LibraryError> {
        let project = self
            .project
            .read()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let state = resolve_decorator_stack(&project, owner)?;
        Ok(SemanticDecoratorStack {
            owner,
            node_ids: state.node_ids,
        })
    }

    /// Appends a Decorator at the terminal Shape immediately before every
    /// parallel Style branch. Existing Style-input wires keep identity.
    pub fn append_semantic_container_decorator(
        &self,
        owner: NodeContainer,
        decorator_type: &str,
    ) -> Result<Uuid, LibraryError> {
        let mut decorator = self
            .plugin_manager
            .create_decorator_operation_node(decorator_type)?;
        if !is_decorator(&decorator) {
            return Err(LibraryError::Project(format!(
                "Decorator {decorator_type:?} is not a Shape -> Shape operation"
            )));
        }
        let decorator_id = decorator.id;
        let mut project = self
            .project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let mut candidate = project.clone();
        let state = resolve_decorator_stack(&candidate, owner)?;
        position_after_source(&candidate, &mut decorator, &state.terminal_source, 240.0);
        candidate
            .insert_node_graph(
                owner,
                NodeGraphBundle::new(vec![decorator], Vec::new(), None),
            )
            .map_err(|error| LibraryError::Project(error.to_string()))?;
        candidate
            .connect_ports(state.terminal_source, shape_input(decorator_id))
            .map_err(|error| LibraryError::Project(error.to_string()))?;
        for downstream in state.downstream {
            reconnect_from(&mut candidate, downstream.id, shape_output(decorator_id))?;
        }
        validate_decorator_candidate(&candidate, owner)?;
        *project = candidate;
        Ok(decorator_id)
    }

    pub fn reorder_semantic_container_decorators(
        &self,
        owner: NodeContainer,
        requested: &[Uuid],
    ) -> Result<(), LibraryError> {
        let mut project = self
            .project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let state = resolve_decorator_stack(&project, owner)?;
        if state.node_ids.is_empty() {
            return Err(LibraryError::Project(format!(
                "{owner:?} has no semantic Decorator stack"
            )));
        }
        validate_order(&state.node_ids, requested, owner)?;
        if state.node_ids == requested {
            return Ok(());
        }
        let incoming = state.incoming.as_ref().ok_or_else(|| {
            LibraryError::Project("Decorator stack has no upstream connection".to_string())
        })?;
        let mut candidate = project.clone();
        reconnect_to(&mut candidate, incoming.id, shape_input(requested[0]))?;
        for (connection, pair) in state.internal.iter().zip(requested.windows(2)) {
            reconnect_both(
                &mut candidate,
                connection.id,
                shape_output(pair[0]),
                shape_input(pair[1]),
            )?;
        }
        let tail = *requested.last().ok_or_else(|| {
            LibraryError::Project("Decorator reorder cannot be empty".to_string())
        })?;
        for downstream in state.downstream {
            reconnect_from(&mut candidate, downstream.id, shape_output(tail))?;
        }
        validate_decorator_candidate(&candidate, owner)?;
        if resolve_decorator_stack(&candidate, owner)?.node_ids != requested {
            return Err(LibraryError::Project(
                "Decorator reorder did not produce the requested order".to_string(),
            ));
        }
        *project = candidate;
        Ok(())
    }

    pub fn remove_semantic_container_decorator(
        &self,
        owner: NodeContainer,
        decorator_id: Uuid,
    ) -> Result<(), LibraryError> {
        let mut project = self
            .project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let state = resolve_decorator_stack(&project, owner)?;
        let index = state
            .node_ids
            .iter()
            .position(|node_id| *node_id == decorator_id)
            .ok_or_else(|| {
                LibraryError::Project(format!(
                    "Decorator {decorator_id} is not in the semantic stack for {owner:?}"
                ))
            })?;
        let incoming = if index == 0 {
            state.incoming.as_ref()
        } else {
            state.internal.get(index - 1)
        }
        .ok_or_else(|| {
            LibraryError::Project(format!("Decorator {decorator_id} has no main Shape input"))
        })?;
        let mut candidate = project.clone();
        if let Some(outgoing) = state.internal.get(index) {
            reconnect_from(&mut candidate, outgoing.id, incoming.from.clone())?;
        } else {
            for downstream in state.downstream {
                reconnect_from(&mut candidate, downstream.id, incoming.from.clone())?;
            }
        }
        candidate.disconnect_connection(incoming.id);
        remove_node(&mut candidate, decorator_id)?;
        validate_decorator_candidate(&candidate, owner)?;
        *project = candidate;
        Ok(())
    }
}

#[derive(Clone)]
struct DecoratorState {
    node_ids: Vec<Uuid>,
    incoming: Option<ProjectConnection>,
    internal: Vec<ProjectConnection>,
    terminal_source: PortAddress,
    downstream: Vec<ProjectConnection>,
}

fn resolve_decorator_stack(
    project: &Project,
    owner: NodeContainer,
) -> Result<DecoratorState, LibraryError> {
    let (terminal_source, downstream, has_styles) = shape_style_boundary(project, owner)?;
    let mut node_ids_reversed = Vec::new();
    let mut connections_reversed = Vec::new();
    let mut cursor = terminal_source.clone();
    let mut visited = HashSet::new();
    while let PortOwner::Node(node_id) = cursor.owner {
        let Some(node) = project.get_node(node_id) else {
            break;
        };
        if !is_decorator(node) {
            break;
        }
        if !visited.insert(node_id) {
            return Err(stack_error(
                owner,
                &node_ids_reversed,
                "Decorator chain contains a cycle",
            ));
        }
        let incoming = connections_to(project, &shape_input(node_id));
        let [connection] = incoming.as_slice() else {
            return Err(stack_error(
                owner,
                &node_ids_reversed,
                &format!(
                    "Decorator {node_id} has {} primary Shape inputs",
                    incoming.len()
                ),
            ));
        };
        node_ids_reversed.push(node_id);
        connections_reversed.push(connection.clone());
        cursor = connection.from.clone();
    }
    node_ids_reversed.reverse();
    connections_reversed.reverse();

    let semantics = project.container_graph_semantics(container_port_owner(owner));
    let candidates = container_node_ids(project, owner)?
        .iter()
        .copied()
        .filter(|node_id| {
            project.get_node(*node_id).is_some_and(is_decorator)
                && (!has_styles || semantics.structurally_reaches_output(PortOwner::Node(*node_id)))
        })
        .collect::<HashSet<_>>();
    let chain = node_ids_reversed.iter().copied().collect::<HashSet<_>>();
    if candidates != chain {
        let candidates = candidates.iter().copied().collect::<Vec<_>>();
        return Err(stack_error(
            owner,
            &candidates,
            "Decorators are split by another Shape operation or form unrelated branches",
        ));
    }
    let incoming = connections_reversed.first().cloned();
    let internal = connections_reversed.into_iter().skip(1).collect();
    Ok(DecoratorState {
        node_ids: node_ids_reversed,
        incoming,
        internal,
        terminal_source,
        downstream,
    })
}

fn shape_style_boundary(
    project: &Project,
    owner: NodeContainer,
) -> Result<(PortAddress, Vec<ProjectConnection>, bool), LibraryError> {
    let semantics = project.container_graph_semantics(container_port_owner(owner));
    let mut styles = container_node_ids(project, owner)?
        .iter()
        .copied()
        .filter(|node_id| {
            semantics.structurally_reaches_output(PortOwner::Node(*node_id))
                && project.get_node(*node_id).is_some_and(is_shape_style)
        })
        .collect::<Vec<_>>();
    styles.sort_unstable();
    if styles.is_empty() {
        return terminal_shape_source(project, owner).map(|source| (source, Vec::new(), false));
    }
    let mut source = None;
    let mut downstream = Vec::with_capacity(styles.len());
    for style_id in &styles {
        let incoming = connections_to(project, &shape_input(*style_id));
        let [connection] = incoming.as_slice() else {
            return Err(stack_error(
                owner,
                &styles,
                &format!(
                    "Style {style_id} has {} primary Shape inputs",
                    incoming.len()
                ),
            ));
        };
        if source
            .as_ref()
            .is_some_and(|existing| existing != &connection.from)
        {
            return Err(stack_error(
                owner,
                &styles,
                "output-reaching Styles have different Shape sources",
            ));
        }
        source = Some(connection.from.clone());
        downstream.push(connection.clone());
    }
    source
        .map(|source| (source, downstream, true))
        .ok_or_else(|| stack_error(owner, &styles, "Style stack has no Shape source"))
}

fn validate_decorator_candidate(
    project: &Project,
    owner: NodeContainer,
) -> Result<(), LibraryError> {
    validate_candidate(project, owner)?;
    let containment = project.validate_containment();
    if !containment.is_empty() {
        return Err(LibraryError::Validation(format!(
            "Decorator transaction for {owner:?} has invalid containment: {}",
            containment
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }
    resolve_decorator_stack(project, owner).map(|_| ())
}

fn validate_order(
    current: &[Uuid],
    requested: &[Uuid],
    owner: NodeContainer,
) -> Result<(), LibraryError> {
    let current_set = current.iter().copied().collect::<HashSet<_>>();
    let requested_set = requested.iter().copied().collect::<HashSet<_>>();
    if current.len() != requested.len()
        || requested_set.len() != requested.len()
        || current_set != requested_set
    {
        return Err(LibraryError::Project(format!(
            "Decorator reorder for {owner:?} must contain every current Node exactly once; current={}, requested={}",
            format_ids(current),
            format_ids(requested)
        )));
    }
    Ok(())
}

fn reconnect_from(
    project: &mut Project,
    connection_id: Uuid,
    from: PortAddress,
) -> Result<(), LibraryError> {
    let connection = project
        .connections
        .iter_mut()
        .find(|connection| connection.id == connection_id)
        .ok_or_else(|| LibraryError::Project(format!("Connection {connection_id} not found")))?;
    connection.from = from;
    Ok(())
}

fn reconnect_to(
    project: &mut Project,
    connection_id: Uuid,
    to: PortAddress,
) -> Result<(), LibraryError> {
    let connection = project
        .connections
        .iter_mut()
        .find(|connection| connection.id == connection_id)
        .ok_or_else(|| LibraryError::Project(format!("Connection {connection_id} not found")))?;
    connection.to = to;
    Ok(())
}

fn reconnect_both(
    project: &mut Project,
    connection_id: Uuid,
    from: PortAddress,
    to: PortAddress,
) -> Result<(), LibraryError> {
    reconnect_from(project, connection_id, from)?;
    reconnect_to(project, connection_id, to)
}

fn remove_node(project: &mut Project, node_id: Uuid) -> Result<(), LibraryError> {
    project
        .remove_node(node_id)
        .map_err(|error| LibraryError::Project(error.to_string()))?
        .map(|_| ())
        .ok_or_else(|| LibraryError::Project(format!("Node {node_id} not found")))
}

fn connections_to(project: &Project, target: &PortAddress) -> Vec<ProjectConnection> {
    let mut connections = project
        .connections
        .iter()
        .filter(|connection| &connection.to == target)
        .cloned()
        .collect::<Vec<_>>();
    connections.sort_by_key(|connection| (connection.order, connection.id));
    connections
}

fn is_shape_style(node: &Node) -> bool {
    matches!(
        node.content(),
        NodeContent::PluginOperation(operation)
            if operation.category == STYLE_CATEGORY
                && operation.component_id != IMAGE_OPACITY_STYLE_COMPONENT_ID
                && operation.operation == STYLE_APPLY_OPERATION
                && operation.declared_ports.iter().any(|port| {
                    port.key == SHAPE_INPUT_PORT
                        && port.direction == PortDirection::Input
                        && port.data_type == PortDataType::Shape
                })
                && operation.declared_ports.iter().any(|port| {
                    port.key == IMAGE_OUTPUT_PORT
                        && port.direction == PortDirection::Output
                        && port.data_type == PortDataType::Image
                })
    )
}

fn is_decorator(node: &Node) -> bool {
    matches!(
        node.content(),
        NodeContent::PluginOperation(operation)
            if operation.category == DECORATOR_CATEGORY
                && operation.operation == DECORATOR_APPLY_OPERATION
                && operation.declared_ports.iter().any(|port| {
                    port.key == SHAPE_INPUT_PORT
                        && port.direction == PortDirection::Input
                        && port.data_type == PortDataType::Shape
                })
                && operation.declared_ports.iter().any(|port| {
                    port.key == SHAPE_OUTPUT_PORT
                        && port.direction == PortDirection::Output
                        && port.data_type == PortDataType::Shape
                })
    )
}

fn shape_input(node_id: Uuid) -> PortAddress {
    PortAddress::new(PortOwner::Node(node_id), SHAPE_INPUT_PORT)
}

fn shape_output(node_id: Uuid) -> PortAddress {
    PortAddress::new(PortOwner::Node(node_id), SHAPE_OUTPUT_PORT)
}

fn stack_error(owner: NodeContainer, node_ids: &[Uuid], reason: &str) -> LibraryError {
    LibraryError::Project(format!(
        "Cannot edit semantic Decorator stack for {owner:?}: {reason}; Nodes={}",
        format_ids(node_ids)
    ))
}

fn format_ids(ids: &[Uuid]) -> String {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.iter()
        .map(Uuid::to_string)
        .collect::<Vec<_>>()
        .join(",")
}
