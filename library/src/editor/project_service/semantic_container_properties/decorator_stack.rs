use std::collections::{HashMap, HashSet};

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

/// One contiguous Shape -> Shape Decorator chain serving one or more Style
/// branches. Different Style branches may intentionally have different chains.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticDecoratorChain {
    style_anchor_ids: Vec<Uuid>,
    node_ids: Vec<Uuid>,
    root_source: PortAddress,
    terminal_source: PortAddress,
}

impl SemanticDecoratorChain {
    pub fn style_anchor_ids(&self) -> &[Uuid] {
        &self.style_anchor_ids
    }

    pub fn node_ids(&self) -> &[Uuid] {
        &self.node_ids
    }

    pub fn root_source(&self) -> &PortAddress {
        &self.root_source
    }

    pub fn terminal_source(&self) -> &PortAddress {
        &self.terminal_source
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticDecoratorStack {
    owner: NodeContainer,
    node_ids: Vec<Uuid>,
    chains: Vec<SemanticDecoratorChain>,
}

impl SemanticDecoratorStack {
    pub fn owner(&self) -> NodeContainer {
        self.owner
    }

    /// Stable, de-duplicated root-to-leaf order across all chains.
    pub fn node_ids(&self) -> &[Uuid] {
        &self.node_ids
    }

    pub fn chains(&self) -> &[SemanticDecoratorChain] {
        &self.chains
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
            node_ids: state.node_ids.clone(),
            chains: state
                .chains
                .into_iter()
                .map(|chain| SemanticDecoratorChain {
                    style_anchor_ids: chain.style_anchor_ids,
                    node_ids: chain.node_ids,
                    root_source: chain.root_source,
                    terminal_source: chain.terminal_source,
                })
                .collect(),
        })
    }

    /// Convenience insertion. It appends to a unique chain; when several
    /// chains have one common root, it inserts a shared prefix before the
    /// split. Different roots require an explicit Style anchor.
    pub fn append_semantic_container_decorator(
        &self,
        owner: NodeContainer,
        decorator_type: &str,
    ) -> Result<Uuid, LibraryError> {
        self.append_semantic_container_decorator_internal(owner, decorator_type, None, None)
    }

    /// Inserts immediately before one Style. Only that authoritative Shape
    /// branch is rewired, even when another Style currently shares its source.
    pub fn append_semantic_container_decorator_for_style(
        &self,
        owner: NodeContainer,
        decorator_type: &str,
        style_anchor_id: Uuid,
    ) -> Result<Uuid, LibraryError> {
        self.append_semantic_container_decorator_internal(
            owner,
            decorator_type,
            Some(style_anchor_id),
            None,
        )
    }

    /// Inserts after an exact Decorator Node and preserves every outgoing
    /// primary Shape wire, including a deliberate fan-out.
    pub fn append_semantic_container_decorator_after(
        &self,
        owner: NodeContainer,
        decorator_type: &str,
        decorator_anchor_id: Uuid,
    ) -> Result<Uuid, LibraryError> {
        self.append_semantic_container_decorator_internal(
            owner,
            decorator_type,
            None,
            Some(decorator_anchor_id),
        )
    }

    fn append_semantic_container_decorator_internal(
        &self,
        owner: NodeContainer,
        decorator_type: &str,
        style_anchor_id: Option<Uuid>,
        decorator_anchor_id: Option<Uuid>,
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
        let insertion = resolve_decorator_insertion(
            &candidate,
            owner,
            &state,
            style_anchor_id,
            decorator_anchor_id,
        )?;
        position_after_source(&candidate, &mut decorator, &insertion.source, 240.0);
        candidate
            .insert_node_graph(
                owner,
                NodeGraphBundle::new(vec![decorator], Vec::new(), None),
            )
            .map_err(|error| LibraryError::Project(error.to_string()))?;
        candidate
            .connect_ports(insertion.source, shape_input(decorator_id))
            .map_err(|error| LibraryError::Project(error.to_string()))?;
        for downstream in insertion.downstream {
            reconnect_from(&mut candidate, downstream.id, shape_output(decorator_id))?;
        }
        validate_decorator_candidate(&candidate, owner)?;
        let final_state = resolve_decorator_stack(&candidate, owner)?;
        if !final_state.node_ids.contains(&decorator_id) {
            return Err(LibraryError::Project(format!(
                "Inserted Decorator {decorator_id} is not in an editable chain for {owner:?}"
            )));
        }
        *project = candidate;
        Ok(decorator_id)
    }

    /// Convenience reorder for a single unambiguous Decorator chain.
    pub fn reorder_semantic_container_decorators(
        &self,
        owner: NodeContainer,
        requested: &[Uuid],
    ) -> Result<(), LibraryError> {
        self.reorder_semantic_container_decorators_internal(owner, None, requested)
    }

    pub fn reorder_semantic_container_decorators_for_style(
        &self,
        owner: NodeContainer,
        style_anchor_id: Uuid,
        requested: &[Uuid],
    ) -> Result<(), LibraryError> {
        self.reorder_semantic_container_decorators_internal(owner, Some(style_anchor_id), requested)
    }

    fn reorder_semantic_container_decorators_internal(
        &self,
        owner: NodeContainer,
        style_anchor_id: Option<Uuid>,
        requested: &[Uuid],
    ) -> Result<(), LibraryError> {
        let mut project = self
            .project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let state = resolve_decorator_stack(&project, owner)?;
        let chain = select_decorator_chain(&state, owner, style_anchor_id)?.clone();
        if chain.node_ids.is_empty() {
            return Err(LibraryError::Project(format!(
                "Selected chain for {owner:?} has no Decorators"
            )));
        }
        validate_order(&chain.node_ids, requested, owner)?;
        if chain.node_ids == requested {
            return Ok(());
        }
        reject_cross_branch_reorder(&state, &chain)?;
        let incoming = chain.incoming.as_ref().ok_or_else(|| {
            LibraryError::Project("Decorator chain has no upstream connection".to_string())
        })?;
        let mut candidate = project.clone();
        reconnect_to(&mut candidate, incoming.id, shape_input(requested[0]))?;
        for (connection, pair) in chain.internal.iter().zip(requested.windows(2)) {
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
        for downstream in chain.downstream {
            reconnect_from(&mut candidate, downstream.id, shape_output(tail))?;
        }
        validate_decorator_candidate(&candidate, owner)?;
        let actual = resolve_decorator_stack(&candidate, owner)?;
        let actual_chain = select_decorator_chain(&actual, owner, style_anchor_id)?;
        if actual_chain.node_ids != requested {
            return Err(LibraryError::Project(
                "Decorator reorder did not produce the requested anchored order".to_string(),
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
        self.remove_semantic_container_decorator_internal(owner, decorator_id, None)
    }

    pub fn remove_semantic_container_decorator_for_style(
        &self,
        owner: NodeContainer,
        style_anchor_id: Uuid,
        decorator_id: Uuid,
    ) -> Result<(), LibraryError> {
        self.remove_semantic_container_decorator_internal(
            owner,
            decorator_id,
            Some(style_anchor_id),
        )
    }

    fn remove_semantic_container_decorator_internal(
        &self,
        owner: NodeContainer,
        decorator_id: Uuid,
        style_anchor_id: Option<Uuid>,
    ) -> Result<(), LibraryError> {
        let mut project = self
            .project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let state = resolve_decorator_stack(&project, owner)?;
        if !state.node_ids.contains(&decorator_id) {
            return Err(LibraryError::Project(format!(
                "Decorator {decorator_id} is not in an editable chain for {owner:?}"
            )));
        }
        if let Some(style_id) = style_anchor_id {
            let chain = select_decorator_chain(&state, owner, Some(style_id))?;
            if !chain.node_ids.contains(&decorator_id) {
                return Err(LibraryError::Project(format!(
                    "Decorator {decorator_id} is not on Style anchor {style_id}"
                )));
            }
            let membership = state
                .chains
                .iter()
                .filter(|chain| chain.node_ids.contains(&decorator_id))
                .count();
            if membership > 1 {
                return Err(LibraryError::Project(format!(
                    "Decorator {decorator_id} is shared by {membership} chains; remove it globally instead of through one Style anchor"
                )));
            }
        }
        let incoming = connections_to(&project, &shape_input(decorator_id));
        let [incoming] = incoming.as_slice() else {
            return Err(LibraryError::Project(format!(
                "Decorator {decorator_id} has {} primary Shape inputs",
                incoming.len()
            )));
        };
        let source = incoming.from.clone();
        let downstream = primary_shape_connections_from(&project, decorator_id, owner);
        let mut candidate = project.clone();
        for connection in downstream {
            reconnect_from(&mut candidate, connection.id, source.clone())?;
        }
        candidate.disconnect_connection(incoming.id);
        remove_node(&mut candidate, decorator_id)?;
        validate_decorator_candidate(&candidate, owner)?;
        *project = candidate;
        Ok(())
    }
}

#[derive(Clone)]
struct DecoratorInsertion {
    source: PortAddress,
    downstream: Vec<ProjectConnection>,
}

#[derive(Clone)]
struct DecoratorState {
    node_ids: Vec<Uuid>,
    chains: Vec<DecoratorChainState>,
}

#[derive(Clone)]
struct DecoratorChainState {
    style_anchor_ids: Vec<Uuid>,
    node_ids: Vec<Uuid>,
    incoming: Option<ProjectConnection>,
    internal: Vec<ProjectConnection>,
    root_source: PortAddress,
    terminal_source: PortAddress,
    downstream: Vec<ProjectConnection>,
}

fn resolve_decorator_stack(
    project: &Project,
    owner: NodeContainer,
) -> Result<DecoratorState, LibraryError> {
    let boundaries = shape_style_boundaries(project, owner)?;
    let mut chains = boundaries
        .into_iter()
        .map(|boundary| resolve_decorator_chain(project, owner, boundary))
        .collect::<Result<Vec<_>, _>>()?;
    chains.sort_by(|left, right| {
        left.style_anchor_ids
            .cmp(&right.style_anchor_ids)
            .then_with(|| {
                format!("{:?}", left.terminal_source).cmp(&format!("{:?}", right.terminal_source))
            })
    });

    let semantics = project.container_graph_semantics(container_port_owner(owner));
    let has_styles = chains
        .iter()
        .any(|chain| !chain.style_anchor_ids.is_empty());
    let candidates = container_node_ids(project, owner)?
        .iter()
        .copied()
        .filter(|node_id| {
            project.get_node(*node_id).is_some_and(is_decorator)
                && (!has_styles || semantics.structurally_reaches_output(PortOwner::Node(*node_id)))
        })
        .collect::<HashSet<_>>();
    let discovered = chains
        .iter()
        .flat_map(|chain| chain.node_ids.iter().copied())
        .collect::<HashSet<_>>();
    if candidates != discovered {
        let candidates = candidates.iter().copied().collect::<Vec<_>>();
        return Err(stack_error(
            owner,
            &candidates,
            "Decorator is separated from every Style anchor by another Shape operation",
        ));
    }
    let mut seen = HashSet::new();
    let node_ids = chains
        .iter()
        .flat_map(|chain| chain.node_ids.iter().copied())
        .filter(|node_id| seen.insert(*node_id))
        .collect();
    Ok(DecoratorState { node_ids, chains })
}

fn resolve_decorator_chain(
    project: &Project,
    owner: NodeContainer,
    boundary: ShapeBoundary,
) -> Result<DecoratorChainState, LibraryError> {
    let mut node_ids_reversed = Vec::new();
    let mut connections_reversed = Vec::new();
    let mut cursor = boundary.terminal_source.clone();
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
    Ok(DecoratorChainState {
        style_anchor_ids: boundary.style_anchor_ids,
        node_ids: node_ids_reversed,
        incoming: connections_reversed.first().cloned(),
        internal: connections_reversed.into_iter().skip(1).collect(),
        root_source: cursor,
        terminal_source: boundary.terminal_source,
        downstream: boundary.downstream,
    })
}

#[derive(Clone)]
struct ShapeBoundary {
    style_anchor_ids: Vec<Uuid>,
    terminal_source: PortAddress,
    downstream: Vec<ProjectConnection>,
}

fn shape_style_boundaries(
    project: &Project,
    owner: NodeContainer,
) -> Result<Vec<ShapeBoundary>, LibraryError> {
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
        return terminal_shape_source(project, owner).map(|source| {
            vec![ShapeBoundary {
                style_anchor_ids: Vec::new(),
                terminal_source: source,
                downstream: Vec::new(),
            }]
        });
    }
    let mut grouped: HashMap<PortAddress, (Vec<Uuid>, Vec<ProjectConnection>)> = HashMap::new();
    for style_id in styles {
        let incoming = connections_to(project, &shape_input(style_id));
        let [connection] = incoming.as_slice() else {
            return Err(stack_error(
                owner,
                &[style_id],
                &format!(
                    "Style {style_id} has {} primary Shape inputs",
                    incoming.len()
                ),
            ));
        };
        let entry = grouped.entry(connection.from.clone()).or_default();
        entry.0.push(style_id);
        entry.1.push(connection.clone());
    }
    Ok(grouped
        .into_iter()
        .map(
            |(terminal_source, (mut style_anchor_ids, mut downstream))| {
                style_anchor_ids.sort_unstable();
                downstream.sort_by_key(|connection| (connection.order, connection.id));
                ShapeBoundary {
                    style_anchor_ids,
                    terminal_source,
                    downstream,
                }
            },
        )
        .collect())
}

fn resolve_decorator_insertion(
    project: &Project,
    owner: NodeContainer,
    state: &DecoratorState,
    style_anchor_id: Option<Uuid>,
    decorator_anchor_id: Option<Uuid>,
) -> Result<DecoratorInsertion, LibraryError> {
    if style_anchor_id.is_some() && decorator_anchor_id.is_some() {
        return Err(LibraryError::Project(
            "Decorator insertion accepts only one anchor".to_string(),
        ));
    }
    if let Some(style_id) = style_anchor_id {
        let chain = select_decorator_chain(state, owner, Some(style_id))?;
        let downstream = chain
            .downstream
            .iter()
            .filter(|connection| connection.to == shape_input(style_id))
            .cloned()
            .collect::<Vec<_>>();
        let [connection] = downstream.as_slice() else {
            return Err(LibraryError::Project(format!(
                "Style anchor {style_id} has {} main Shape wires",
                downstream.len()
            )));
        };
        return Ok(DecoratorInsertion {
            source: connection.from.clone(),
            downstream,
        });
    }
    if let Some(decorator_id) = decorator_anchor_id {
        if !state.node_ids.contains(&decorator_id) {
            return Err(LibraryError::Project(format!(
                "Decorator anchor {decorator_id} is not in {owner:?}"
            )));
        }
        let downstream = primary_shape_connections_from(project, decorator_id, owner);
        if downstream.is_empty() {
            return Err(LibraryError::Project(format!(
                "Decorator anchor {decorator_id} has no primary Shape downstream"
            )));
        }
        return Ok(DecoratorInsertion {
            source: shape_output(decorator_id),
            downstream,
        });
    }
    if state.chains.len() == 1 {
        let chain = &state.chains[0];
        return Ok(DecoratorInsertion {
            source: chain.terminal_source.clone(),
            downstream: chain.downstream.clone(),
        });
    }
    let roots = state
        .chains
        .iter()
        .map(|chain| chain.root_source.clone())
        .collect::<HashSet<_>>();
    let roots = roots.into_iter().collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(LibraryError::Project(format!(
            "Cannot choose a Decorator chain for {owner:?}: pass a Style or Decorator anchor"
        )));
    };
    let mut downstream = state
        .chains
        .iter()
        .flat_map(|chain| {
            chain
                .incoming
                .iter()
                .chain(chain.downstream.iter())
                .filter(|wire| wire.from == *root)
        })
        .cloned()
        .collect::<Vec<_>>();
    downstream.sort_by_key(|connection| connection.id);
    downstream.dedup_by_key(|connection| connection.id);
    if downstream.is_empty() {
        return Err(LibraryError::Project(format!(
            "Common Decorator root {root:?} has no editable downstream"
        )));
    }
    Ok(DecoratorInsertion {
        source: root.clone(),
        downstream,
    })
}

fn select_decorator_chain(
    state: &DecoratorState,
    owner: NodeContainer,
    style_anchor_id: Option<Uuid>,
) -> Result<&DecoratorChainState, LibraryError> {
    if let Some(style_id) = style_anchor_id {
        return state
            .chains
            .iter()
            .find(|chain| chain.style_anchor_ids.contains(&style_id))
            .ok_or_else(|| {
                LibraryError::Project(format!(
                    "Style anchor {style_id} has no Decorator chain in {owner:?}"
                ))
            });
    }
    let [chain] = state.chains.as_slice() else {
        return Err(LibraryError::Project(format!(
            "{owner:?} has {} Decorator chains; pass a Style anchor",
            state.chains.len()
        )));
    };
    Ok(chain)
}

fn reject_cross_branch_reorder(
    state: &DecoratorState,
    selected: &DecoratorChainState,
) -> Result<(), LibraryError> {
    for node_id in &selected.node_ids {
        if state
            .chains
            .iter()
            .any(|chain| chain.node_ids.contains(node_id) && chain.node_ids != selected.node_ids)
        {
            return Err(LibraryError::Project(format!(
                "Decorator {node_id} is shared across different chains; exact branch reorder would change another Style"
            )));
        }
    }
    Ok(())
}

fn primary_shape_connections_from(
    project: &Project,
    decorator_id: Uuid,
    owner: NodeContainer,
) -> Vec<ProjectConnection> {
    let source = shape_output(decorator_id);
    let mut downstream = project
        .connections
        .iter()
        .filter(|connection| connection.from == source && connection.to.port == SHAPE_INPUT_PORT)
        .filter(|connection| match connection.to.owner {
            PortOwner::Node(node_id) => project.find_node_container(node_id) == Some(owner),
            _ => false,
        })
        .cloned()
        .collect::<Vec<_>>();
    downstream.sort_by_key(|connection| (connection.order, connection.id));
    downstream
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
