//! Atomic Image -> Image Effect-chain authoring for semantic containers.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use super::helpers::{container_node_ids, container_output_node_id, position_after_source};
use super::{container_port_owner, validate_candidate};
use crate::editor::project_service::ProjectManager;
use crate::error::LibraryError;
use crate::model::project::{
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, NodeContainer, NodeGraphBundle, PortAddress, PortDataType,
    PortDirection, PortOwner, Project, ProjectConnection,
};
use crate::model::{Node, NodeContent};
use crate::plugin::{
    EFFECT_APPLY_OPERATION, EFFECT_CATEGORY, IMAGE_OPACITY_STYLE_COMPONENT_ID,
    IMAGE_TRANSFORM_COMPONENT_ID, STYLE_APPLY_OPERATION, STYLE_CATEGORY, TRANSFORM_APPLY_OPERATION,
    TRANSFORM_CATEGORY,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticEffectStack {
    owner: NodeContainer,
    node_ids: Vec<Uuid>,
}

impl SemanticEffectStack {
    pub fn owner(&self) -> NodeContainer {
        self.owner
    }

    /// Ordered upstream -> downstream along the authoritative Image flow.
    pub fn node_ids(&self) -> &[Uuid] {
        &self.node_ids
    }
}

impl ProjectManager {
    pub fn semantic_container_effect_stack(
        &self,
        owner: NodeContainer,
    ) -> Result<SemanticEffectStack, LibraryError> {
        let project = self
            .project
            .read()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let chain = resolve_effect_chain(&project, owner)?;
        Ok(SemanticEffectStack {
            owner,
            node_ids: chain.map_or_else(Vec::new, |chain| chain.node_ids),
        })
    }

    /// Appends one Effect to the output-reaching Effect segment. If no Effect
    /// exists, it is inserted before trailing semantic Image Transform /
    /// Image Opacity operations. With no trailing semantic operations it
    /// wraps the current output and becomes the new output. The complete edit
    /// is one Project transaction.
    pub fn append_semantic_container_effect(
        &self,
        owner: NodeContainer,
        effect_type: &str,
    ) -> Result<Uuid, LibraryError> {
        let mut effect = self
            .plugin_manager
            .create_effect_operation_node(effect_type)?;
        let effect_id = effect.id;
        let mut project = self
            .project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let mut candidate = project.clone();
        let chain = resolve_effect_chain(&candidate, owner)?;
        let insertion = match chain {
            Some(chain) => InsertionPoint::after_chain(&chain)?,
            None => empty_insertion_point(&candidate, owner)?,
        };
        position_after_source(&candidate, &mut effect, insertion.source(), 240.0);
        candidate
            .insert_node_graph(owner, NodeGraphBundle::new(vec![effect], Vec::new(), None))
            .map_err(|error| LibraryError::Project(error.to_string()))?;
        match insertion {
            InsertionPoint::Splice { connection_id, .. } => {
                candidate
                    .splice_connection(
                        connection_id,
                        PortAddress::new(PortOwner::Node(effect_id), IMAGE_INPUT_PORT),
                        PortAddress::new(PortOwner::Node(effect_id), IMAGE_OUTPUT_PORT),
                    )
                    .map_err(|error| LibraryError::Project(error.to_string()))?;
            }
            InsertionPoint::AppendOutput { source, .. } => {
                candidate
                    .connect_ports(
                        source,
                        PortAddress::new(PortOwner::Node(effect_id), IMAGE_INPUT_PORT),
                    )
                    .map_err(|error| LibraryError::Project(error.to_string()))?;
                candidate
                    .set_output_node(owner, Some(effect_id))
                    .map_err(|error| LibraryError::Project(error.to_string()))?;
            }
        }
        validate_effect_candidate(&candidate, owner)?;
        *project = candidate;
        Ok(effect_id)
    }

    /// Reorders only the Effect segment's main-flow connections. Nodes,
    /// properties, property-input wires, and non-output-reaching fan-outs are
    /// left byte-for-byte attached to their existing Effect UUIDs.
    pub fn reorder_semantic_container_effects(
        &self,
        owner: NodeContainer,
        requested: &[Uuid],
    ) -> Result<(), LibraryError> {
        let mut project = self
            .project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let chain = resolve_effect_chain(&project, owner)?.ok_or_else(|| {
            LibraryError::Project(format!("{owner:?} has no output-reaching Effect stack"))
        })?;
        validate_requested_order(&chain.node_ids, requested, owner)?;
        if chain.node_ids == requested {
            return Ok(());
        }

        let mut candidate = project.clone();
        reconnect(
            &mut candidate,
            chain.incoming.id,
            chain.incoming.from.clone(),
            image_input(requested[0]),
        )?;
        for (connection, pair) in chain.internal.iter().zip(requested.windows(2)) {
            reconnect(
                &mut candidate,
                connection.id,
                image_output(pair[0]),
                image_input(pair[1]),
            )?;
        }
        if let Some(downstream) = &chain.downstream {
            reconnect(
                &mut candidate,
                downstream.id,
                image_output(*requested.last().ok_or_else(|| {
                    LibraryError::Project("Effect reorder cannot be empty".to_string())
                })?),
                downstream.to.clone(),
            )?;
        } else {
            candidate
                .set_output_node(owner, requested.last().copied())
                .map_err(|error| LibraryError::Project(error.to_string()))?;
        }
        validate_effect_candidate(&candidate, owner)?;
        *project = candidate;
        Ok(())
    }

    /// Deletes one Effect and bypasses it with the downstream main-flow
    /// connection. The downstream connection keeps its UUID/order/blend.
    pub fn remove_semantic_container_effect(
        &self,
        owner: NodeContainer,
        effect_id: Uuid,
    ) -> Result<(), LibraryError> {
        let mut project = self
            .project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let chain = resolve_effect_chain(&project, owner)?.ok_or_else(|| {
            LibraryError::Project(format!("{owner:?} has no output-reaching Effect stack"))
        })?;
        let index = chain
            .node_ids
            .iter()
            .position(|node_id| *node_id == effect_id)
            .ok_or_else(|| {
                LibraryError::Project(format!(
                    "Effect Node {effect_id} is not in the output-reaching stack for {owner:?}"
                ))
            })?;
        let incoming = if index == 0 {
            &chain.incoming
        } else {
            &chain.internal[index - 1]
        };
        let outgoing = if index < chain.internal.len() {
            Some(&chain.internal[index])
        } else {
            chain.downstream.as_ref()
        };

        let mut candidate = project.clone();
        if let Some(outgoing) = outgoing {
            reconnect(
                &mut candidate,
                outgoing.id,
                incoming.from.clone(),
                outgoing.to.clone(),
            )?;
        } else {
            let PortOwner::Node(predecessor) = incoming.from.owner else {
                return Err(LibraryError::Project(format!(
                    "Cannot remove terminal Effect {effect_id}: predecessor {:?} cannot own a container output",
                    incoming.from.owner
                )));
            };
            candidate
                .set_output_node(owner, Some(predecessor))
                .map_err(|error| LibraryError::Project(error.to_string()))?;
        }
        candidate.disconnect_connection(incoming.id);
        candidate
            .remove_node(effect_id)
            .map_err(|error| LibraryError::Project(error.to_string()))?
            .ok_or_else(|| LibraryError::Project(format!("Effect Node {effect_id} not found")))?;
        validate_effect_candidate(&candidate, owner)?;
        *project = candidate;
        Ok(())
    }
}

#[derive(Clone)]
struct EffectChain {
    node_ids: Vec<Uuid>,
    incoming: ProjectConnection,
    internal: Vec<ProjectConnection>,
    downstream: Option<ProjectConnection>,
}

fn resolve_effect_chain(
    project: &Project,
    owner: NodeContainer,
) -> Result<Option<EffectChain>, LibraryError> {
    let semantics = project.container_graph_semantics(container_port_owner(owner));
    let mut effects = container_node_ids(project, owner)?
        .iter()
        .copied()
        .filter(|node_id| {
            semantics.structurally_reaches_output(PortOwner::Node(*node_id))
                && project.get_node(*node_id).is_some_and(is_effect)
        })
        .collect::<Vec<_>>();
    effects.sort_unstable();
    if effects.is_empty() {
        return Ok(None);
    }
    let effect_set = effects.iter().copied().collect::<HashSet<_>>();
    let mut incoming = HashMap::<Uuid, ProjectConnection>::new();
    for node_id in &effects {
        let target = image_input(*node_id);
        let matches = project
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .cloned()
            .collect::<Vec<_>>();
        let [connection] = matches.as_slice() else {
            return Err(LibraryError::Project(format!(
                "Output-reaching Effect {node_id} has {} Image inputs; expected exactly one",
                matches.len()
            )));
        };
        incoming.insert(*node_id, connection.clone());
    }

    let mut internal_from = HashMap::<Uuid, ProjectConnection>::new();
    let mut internal_to = HashMap::<Uuid, ProjectConnection>::new();
    for connection in &project.connections {
        let (PortOwner::Node(from), PortOwner::Node(to)) =
            (connection.from.owner, connection.to.owner)
        else {
            continue;
        };
        if effect_set.contains(&from)
            && effect_set.contains(&to)
            && connection.from.port == IMAGE_OUTPUT_PORT
            && connection.to.port == IMAGE_INPUT_PORT
            && (internal_from.insert(from, connection.clone()).is_some()
                || internal_to.insert(to, connection.clone()).is_some())
        {
            return Err(ambiguous_chain(
                owner,
                &effects,
                "Effect main flow branches",
            ));
        }
    }
    let heads = effects
        .iter()
        .filter(|node_id| !internal_to.contains_key(node_id))
        .copied()
        .collect::<Vec<_>>();
    let [head] = heads.as_slice() else {
        return Err(ambiguous_chain(
            owner,
            &effects,
            "Effects do not form one contiguous acyclic segment",
        ));
    };
    let mut ordered = Vec::with_capacity(effects.len());
    let mut internal = Vec::with_capacity(effects.len().saturating_sub(1));
    let mut cursor = *head;
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(cursor) {
            return Err(ambiguous_chain(
                owner,
                &effects,
                "Effect segment contains a cycle",
            ));
        }
        ordered.push(cursor);
        let Some(connection) = internal_from.get(&cursor) else {
            break;
        };
        let PortOwner::Node(next) = connection.to.owner else {
            return Err(ambiguous_chain(
                owner,
                &effects,
                "Effect segment has a non-Node internal target",
            ));
        };
        internal.push(connection.clone());
        cursor = next;
    }
    if ordered.len() != effects.len() {
        return Err(ambiguous_chain(
            owner,
            &effects,
            "Effects are split into multiple output-reaching segments",
        ));
    }

    for (index, node_id) in ordered.iter().enumerate() {
        let expected_next = ordered.get(index + 1).copied();
        let main_outgoing = output_reaching_image_connections(project, &semantics, *node_id);
        if let Some(next) = expected_next
            && (main_outgoing.len() != 1
                || main_outgoing[0].to != image_input(next)
                || main_outgoing[0].id != internal[index].id)
        {
            return Err(ambiguous_chain(
                owner,
                &effects,
                "Effect main flow branches before the segment tail",
            ));
        }
    }

    let tail = *ordered.last().ok_or_else(|| {
        LibraryError::Project("Resolved Effect segment unexpectedly became empty".to_string())
    })?;
    let output_id = container_output_node_id(project, owner)?
        .ok_or_else(|| LibraryError::Project(format!("{owner:?} has no Image output Node")))?;
    let downstream = if tail == output_id {
        if !output_reaching_image_connections(project, &semantics, tail).is_empty() {
            return Err(ambiguous_chain(
                owner,
                &effects,
                "Terminal Effect has another output-reaching branch",
            ));
        }
        None
    } else {
        let matches = output_reaching_image_connections(project, &semantics, tail);
        let [connection] = matches.as_slice() else {
            return Err(ambiguous_chain(
                owner,
                &effects,
                "Effect segment tail has no unique downstream Image flow",
            ));
        };
        Some(connection.clone())
    };
    let boundary = incoming.get(head).cloned().ok_or_else(|| {
        LibraryError::Project(format!("Effect segment head {head} has no Image input"))
    })?;
    Ok(Some(EffectChain {
        node_ids: ordered,
        incoming: boundary,
        internal,
        downstream,
    }))
}

fn output_reaching_image_connections(
    project: &Project,
    semantics: &crate::model::project::ContainerGraphSemantics,
    node_id: Uuid,
) -> Vec<ProjectConnection> {
    let source = image_output(node_id);
    let mut result = project
        .connections
        .iter()
        .filter(|connection| {
            connection.from == source
                && semantics.structurally_reaches_output(connection.to.owner)
                && project
                    .port_definition(&connection.to, PortDirection::Input)
                    .is_some_and(|port| port.data_type == PortDataType::Image)
        })
        .cloned()
        .collect::<Vec<_>>();
    result.sort_by_key(|connection| (connection.order, connection.id));
    result
}

fn empty_insertion_point(
    project: &Project,
    owner: NodeContainer,
) -> Result<InsertionPoint, LibraryError> {
    let output_id = container_output_node_id(project, owner)?
        .ok_or_else(|| LibraryError::Project(format!("{owner:?} has no Image output Node")))?;
    let mut cursor = output_id;
    let mut earliest = None;
    let mut visited = HashSet::new();
    while visited.insert(cursor)
        && project
            .get_node(cursor)
            .is_some_and(is_trailing_semantic_image_operation)
    {
        let target = image_input(cursor);
        let incoming = project
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .cloned()
            .collect::<Vec<_>>();
        let [connection] = incoming.as_slice() else {
            return Err(LibraryError::Project(format!(
                "Trailing semantic Image Node {cursor} has {} Image inputs; cannot choose an Effect insertion point",
                incoming.len()
            )));
        };
        earliest = Some(connection.clone());
        let PortOwner::Node(upstream) = connection.from.owner else {
            break;
        };
        cursor = upstream;
    }
    Ok(earliest.map_or_else(
        || InsertionPoint::AppendOutput {
            source: image_output(output_id),
        },
        |connection| InsertionPoint::Splice {
            connection_id: connection.id,
            source: connection.from,
        },
    ))
}

enum InsertionPoint {
    Splice {
        connection_id: Uuid,
        source: PortAddress,
    },
    AppendOutput {
        source: PortAddress,
    },
}

impl InsertionPoint {
    fn after_chain(chain: &EffectChain) -> Result<Self, LibraryError> {
        let tail = *chain.node_ids.last().ok_or_else(|| {
            LibraryError::Project("Resolved Effect chain unexpectedly became empty".to_string())
        })?;
        Ok(chain.downstream.as_ref().map_or_else(
            || Self::AppendOutput {
                source: image_output(tail),
            },
            |connection| Self::Splice {
                connection_id: connection.id,
                source: connection.from.clone(),
            },
        ))
    }

    fn source(&self) -> &PortAddress {
        match self {
            Self::Splice { source, .. } | Self::AppendOutput { source } => source,
        }
    }
}

fn validate_requested_order(
    current: &[Uuid],
    requested: &[Uuid],
    owner: NodeContainer,
) -> Result<(), LibraryError> {
    let requested_set = requested.iter().copied().collect::<HashSet<_>>();
    let current_set = current.iter().copied().collect::<HashSet<_>>();
    if requested.len() != current.len()
        || requested_set.len() != requested.len()
        || requested_set != current_set
    {
        return Err(LibraryError::Project(format!(
            "Effect reorder for {owner:?} must contain each current Effect exactly once; current={}, requested={}",
            format_ids(current),
            format_ids(requested)
        )));
    }
    Ok(())
}

fn reconnect(
    project: &mut Project,
    connection_id: Uuid,
    from: PortAddress,
    to: PortAddress,
) -> Result<(), LibraryError> {
    let connection = project
        .connections
        .iter_mut()
        .find(|connection| connection.id == connection_id)
        .ok_or_else(|| {
            LibraryError::Project(format!("Main-flow connection {connection_id} not found"))
        })?;
    connection.from = from;
    connection.to = to;
    Ok(())
}

fn validate_effect_candidate(project: &Project, owner: NodeContainer) -> Result<(), LibraryError> {
    validate_candidate(project, owner)?;
    let containment = project.validate_containment();
    if containment.is_empty() {
        resolve_effect_chain(project, owner).map(|_| ())
    } else {
        Err(LibraryError::Validation(format!(
            "Effect stack transaction for {owner:?} has invalid containment: {}",
            containment
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        )))
    }
}

fn is_effect(node: &Node) -> bool {
    matches!(
        node.content(),
        NodeContent::PluginOperation(operation)
            if operation.category == EFFECT_CATEGORY
                && operation.operation == EFFECT_APPLY_OPERATION
    )
}

fn is_trailing_semantic_image_operation(node: &Node) -> bool {
    matches!(
        node.content(),
        NodeContent::PluginOperation(operation)
            if (operation.category == TRANSFORM_CATEGORY
                && operation.component_id == IMAGE_TRANSFORM_COMPONENT_ID
                && operation.operation == TRANSFORM_APPLY_OPERATION)
                || (operation.category == STYLE_CATEGORY
                    && operation.component_id == IMAGE_OPACITY_STYLE_COMPONENT_ID
                    && operation.operation == STYLE_APPLY_OPERATION)
    )
}

fn image_input(node_id: Uuid) -> PortAddress {
    PortAddress::new(PortOwner::Node(node_id), IMAGE_INPUT_PORT)
}

fn image_output(node_id: Uuid) -> PortAddress {
    PortAddress::new(PortOwner::Node(node_id), IMAGE_OUTPUT_PORT)
}

fn ambiguous_chain(owner: NodeContainer, effects: &[Uuid], reason: &str) -> LibraryError {
    LibraryError::Project(format!(
        "Cannot edit Effect stack for {owner:?}: {reason}; Effects={}",
        format_ids(effects)
    ))
}

fn format_ids(ids: &[Uuid]) -> String {
    ids.iter()
        .map(Uuid::to_string)
        .collect::<Vec<_>>()
        .join(",")
}
