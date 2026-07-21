use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use super::helpers::{
    container_node_ids, container_output_node_id, container_port_owner, position_after_source,
    terminal_shape_source, validate_candidate,
};
use crate::editor::project_service::ProjectManager;
use crate::error::LibraryError;
use crate::model::project::{
    IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer, NodeGraphBundle, PortAddress,
    PortDataType, PortDirection, PortOwner, Project, ProjectConnection, SHAPE_INPUT_PORT,
};
use crate::model::{Node, NodeContent};
use crate::plugin::{IMAGE_OPACITY_STYLE_COMPONENT_ID, STYLE_APPLY_OPERATION, STYLE_CATEGORY};

/// One independently rasterized Shape branch in the semantic Style stack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticStyleBranch {
    node_id: Uuid,
    shape_source: PortAddress,
    merge_connection_id: Option<Uuid>,
}

impl SemanticStyleBranch {
    pub fn node_id(&self) -> Uuid {
        self.node_id
    }

    pub fn shape_source(&self) -> &PortAddress {
        &self.shape_source
    }

    pub fn merge_connection_id(&self) -> Option<Uuid> {
        self.merge_connection_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticStyleStack {
    owner: NodeContainer,
    node_ids: Vec<Uuid>,
    branches: Vec<SemanticStyleBranch>,
    merge_node_id: Option<Uuid>,
}

impl SemanticStyleStack {
    pub fn owner(&self) -> NodeContainer {
        self.owner
    }

    /// Frontmost-to-backmost, following the relative order of Style wires on
    /// the shared Merge. Non-Style Merge inputs are intentionally omitted.
    pub fn node_ids(&self) -> &[Uuid] {
        &self.node_ids
    }

    pub fn branches(&self) -> &[SemanticStyleBranch] {
        &self.branches
    }

    pub fn merge_node_id(&self) -> Option<Uuid> {
        self.merge_node_id
    }
}

impl ProjectManager {
    pub fn semantic_container_style_stack(
        &self,
        owner: NodeContainer,
    ) -> Result<SemanticStyleStack, LibraryError> {
        let project = self
            .project
            .read()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let state = resolve_style_stack(&project, owner)?;
        let node_ids = state
            .as_ref()
            .map_or_else(Vec::new, |state| state.node_ids.clone());
        let branches = state.as_ref().map_or_else(Vec::new, |state| {
            state
                .node_ids
                .iter()
                .filter_map(|node_id| state.branches.get(node_id))
                .map(|branch| SemanticStyleBranch {
                    node_id: branch.node_id,
                    shape_source: branch.shape_input.from.clone(),
                    merge_connection_id: branch.merge_input.as_ref().map(|wire| wire.id),
                })
                .collect()
        });
        Ok(SemanticStyleStack {
            owner,
            node_ids,
            branches,
            merge_node_id: state.and_then(|state| state.merge.map(|merge| merge.node_id)),
        })
    }

    /// Convenience insertion for the common single-Shape-source case.
    /// Multi-source stacks must use an anchored insertion.
    pub fn append_semantic_container_style(
        &self,
        owner: NodeContainer,
        style_type: &str,
    ) -> Result<Uuid, LibraryError> {
        self.append_semantic_container_style_internal(owner, style_type, None, None)
    }

    /// Inserts a Style after an existing Style branch and copies that branch's
    /// Shape source. `None` is the fail-closed convenience form.
    pub fn append_semantic_container_style_after(
        &self,
        owner: NodeContainer,
        style_type: &str,
        after_style_id: Option<Uuid>,
    ) -> Result<Uuid, LibraryError> {
        self.append_semantic_container_style_internal(owner, style_type, after_style_id, None)
    }

    /// Inserts a first Style from an explicit authoritative Shape output.
    /// This also restores a Style into an existing empty Merge or a downstream
    /// Image trunk whose input became empty when its last Style was removed.
    pub fn append_semantic_container_style_from_shape(
        &self,
        owner: NodeContainer,
        style_type: &str,
        shape_source: PortAddress,
    ) -> Result<Uuid, LibraryError> {
        self.append_semantic_container_style_internal(owner, style_type, None, Some(shape_source))
    }

    fn append_semantic_container_style_internal(
        &self,
        owner: NodeContainer,
        style_type: &str,
        after_style_id: Option<Uuid>,
        explicit_shape_source: Option<PortAddress>,
    ) -> Result<Uuid, LibraryError> {
        let mut style = self
            .plugin_manager
            .create_style_operation_node(style_type)?;
        if !is_shape_style(&style) {
            return Err(LibraryError::Project(format!(
                "Style {style_type:?} is not a Shape -> Image Style"
            )));
        }
        let style_id = style.id;
        let mut project = self
            .project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let mut candidate = project.clone();
        let state = resolve_style_stack(&candidate, owner)?;
        let shape_source = resolve_append_shape_source(
            &candidate,
            owner,
            state.as_ref(),
            after_style_id,
            explicit_shape_source,
        )?;
        position_after_source(&candidate, &mut style, &shape_source, 240.0);
        candidate
            .insert_node_graph(owner, NodeGraphBundle::new(vec![style], Vec::new(), None))
            .map_err(|error| LibraryError::Project(error.to_string()))?;
        candidate
            .connect_ports(shape_source, shape_input(style_id))
            .map_err(|error| LibraryError::Project(error.to_string()))?;

        match state {
            None => attach_first_style(&mut candidate, owner, style_id)?,
            Some(state) => match &state.merge {
                Some(merge) => {
                    let connection_id = candidate
                        .connect_ports(image_output(style_id), merge.target.clone())
                        .map_err(|error| LibraryError::Project(error.to_string()))?;
                    if let Some(anchor_id) = after_style_id {
                        let anchor = merge.inputs.get(&anchor_id).ok_or_else(|| {
                            LibraryError::Project(format!(
                                "Style anchor {anchor_id} has no Merge wire"
                            ))
                        })?;
                        candidate
                            .reorder_connection(connection_id, anchor.order + 1)
                            .map_err(|error| LibraryError::Project(error.to_string()))?;
                    }
                }
                None => synthesize_style_merge(&mut candidate, owner, &state, style_id)?,
            },
        }
        validate_style_candidate(&candidate, owner)?;
        let final_state = resolve_style_stack(&candidate, owner)?.ok_or_else(|| {
            LibraryError::Project("Style insertion produced no semantic Style stack".to_string())
        })?;
        if !final_state.node_ids.contains(&style_id) {
            return Err(LibraryError::Project(format!(
                "Inserted Style {style_id} does not reach the {owner:?} output"
            )));
        }
        *project = candidate;
        Ok(style_id)
    }

    /// Reorders only Style wires in their existing Merge slots. Connection
    /// UUID/blend and every non-Style connection remain byte-for-byte stable.
    pub fn reorder_semantic_container_styles(
        &self,
        owner: NodeContainer,
        requested: &[Uuid],
    ) -> Result<(), LibraryError> {
        let mut project = self
            .project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let state = resolve_style_stack(&project, owner)?.ok_or_else(|| {
            LibraryError::Project(format!("{owner:?} has no semantic Style stack"))
        })?;
        validate_order(&state.node_ids, requested, owner)?;
        if state.node_ids == requested {
            return Ok(());
        }
        let merge = state.merge.ok_or_else(|| {
            LibraryError::Project("Multiple Styles require one semantic Merge".to_string())
        })?;
        let mut slots = merge
            .inputs
            .values()
            .map(|connection| (connection.order, connection.id))
            .collect::<Vec<_>>();
        slots.sort_unstable();
        let slot_orders = slots
            .into_iter()
            .map(|(order, _)| order)
            .collect::<Vec<_>>();
        let mut candidate = project.clone();
        for (node_id, order) in requested.iter().zip(slot_orders) {
            let connection_id = merge
                .inputs
                .get(node_id)
                .ok_or_else(|| {
                    LibraryError::Project(format!("Style {node_id} has no Merge input wire"))
                })?
                .id;
            let connection = candidate
                .connections
                .iter_mut()
                .find(|connection| connection.id == connection_id)
                .ok_or_else(|| {
                    LibraryError::Project(format!("Connection {connection_id} not found"))
                })?;
            connection.order = order;
        }
        validate_style_candidate(&candidate, owner)?;
        let actual = resolve_style_stack(&candidate, owner)?
            .map(|state| state.node_ids)
            .unwrap_or_default();
        if actual != requested {
            return Err(LibraryError::Project(format!(
                "Style reorder did not produce the requested order: actual={}",
                format_ids(&actual)
            )));
        }
        *project = candidate;
        Ok(())
    }

    pub fn remove_semantic_container_style(
        &self,
        owner: NodeContainer,
        style_id: Uuid,
    ) -> Result<(), LibraryError> {
        let mut project = self
            .project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let state = resolve_style_stack(&project, owner)?.ok_or_else(|| {
            LibraryError::Project(format!("{owner:?} has no semantic Style stack"))
        })?;
        if !state.node_ids.contains(&style_id) {
            return Err(LibraryError::Project(format!(
                "Style {style_id} is not in the output-reaching stack for {owner:?}"
            )));
        }
        let mut candidate = project.clone();
        if state.node_ids.len() == 1 && state.direct_is_output {
            candidate
                .set_output_node(owner, None)
                .map_err(|error| LibraryError::Project(error.to_string()))?;
        }
        // Merge and downstream Nodes deliberately survive an empty Style
        // branch. Their disconnected input is NoOutput and preserves topology.
        remove_node(&mut candidate, style_id)?;
        validate_style_candidate(&candidate, owner)?;
        *project = candidate;
        Ok(())
    }
}

#[derive(Clone)]
struct StyleBranchState {
    node_id: Uuid,
    shape_input: ProjectConnection,
    merge_input: Option<ProjectConnection>,
}

#[derive(Clone)]
struct StyleState {
    node_ids: Vec<Uuid>,
    branches: HashMap<Uuid, StyleBranchState>,
    merge: Option<StyleMerge>,
    direct_downstream: Option<ProjectConnection>,
    direct_is_output: bool,
}

#[derive(Clone)]
struct StyleMerge {
    node_id: Uuid,
    target: PortAddress,
    inputs: HashMap<Uuid, ProjectConnection>,
}

fn resolve_style_stack(
    project: &Project,
    owner: NodeContainer,
) -> Result<Option<StyleState>, LibraryError> {
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
        return Ok(None);
    }
    let mut branches = HashMap::new();
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
        branches.insert(
            *style_id,
            StyleBranchState {
                node_id: *style_id,
                shape_input: connection.clone(),
                merge_input: None,
            },
        );
    }
    let output_id = container_output_node_id(project, owner)?
        .ok_or_else(|| stack_error(owner, &styles, "container has no Image output"))?;

    let mut merge_id = None;
    let mut direct_downstream = None;
    let mut direct_is_output = false;
    for style_id in &styles {
        let outgoing = output_reaching_image_connections(project, &semantics, *style_id);
        if *style_id == output_id {
            if !outgoing.is_empty() || styles.len() != 1 {
                return Err(stack_error(
                    owner,
                    &styles,
                    "direct-output Style participates in another output-reaching branch",
                ));
            }
            direct_is_output = true;
            continue;
        }
        let [connection] = outgoing.as_slice() else {
            return Err(stack_error(
                owner,
                &styles,
                "Style output has no unique output-reaching Image connection",
            ));
        };
        if connection.to.port == MERGE_IMAGES_PORT
            && matches!(
                connection.to.owner,
                PortOwner::Node(node_id)
                    if project
                        .get_node(node_id)
                        .is_some_and(|node| matches!(node.content(), NodeContent::Merge))
            )
        {
            let PortOwner::Node(candidate_merge) = connection.to.owner else {
                return Err(stack_error(owner, &styles, "Merge target is not a Node"));
            };
            if merge_id.is_some_and(|existing| existing != candidate_merge) {
                return Err(stack_error(
                    owner,
                    &styles,
                    "Styles feed multiple output-reaching Merge Nodes",
                ));
            }
            merge_id = Some(candidate_merge);
        } else if styles.len() == 1 {
            direct_downstream = Some(connection.clone());
        } else {
            return Err(stack_error(
                owner,
                &styles,
                "multiple Styles are not collected by one Merge",
            ));
        }
    }

    let merge = merge_id
        .map(|merge_id| resolve_style_merge(project, owner, merge_id, &styles, &semantics))
        .transpose()?;
    let node_ids = if let Some(merge) = &merge {
        for (node_id, connection) in &merge.inputs {
            let branch = branches.get_mut(node_id).ok_or_else(|| {
                LibraryError::Project(format!("Style branch {node_id} disappeared"))
            })?;
            branch.merge_input = Some(connection.clone());
        }
        let mut ordered = merge
            .inputs
            .iter()
            .map(|(node_id, connection)| (*node_id, connection.order, connection.id))
            .collect::<Vec<_>>();
        ordered.sort_by_key(|(_, order, connection_id)| (*order, *connection_id));
        ordered.into_iter().map(|(node_id, _, _)| node_id).collect()
    } else {
        if styles.len() != 1 {
            return Err(stack_error(
                owner,
                &styles,
                "parallel Styles require a Merge",
            ));
        }
        styles
    };
    Ok(Some(StyleState {
        node_ids,
        branches,
        merge,
        direct_downstream,
        direct_is_output,
    }))
}

fn resolve_style_merge(
    project: &Project,
    owner: NodeContainer,
    merge_id: Uuid,
    styles: &[Uuid],
    semantics: &crate::model::project::ContainerGraphSemantics,
) -> Result<StyleMerge, LibraryError> {
    let target = PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
    let incoming = connections_to(project, &target);
    let mut inputs = HashMap::new();
    for style_id in styles {
        let matches = incoming
            .iter()
            .filter(|connection| connection.from == image_output(*style_id))
            .cloned()
            .collect::<Vec<_>>();
        let [connection] = matches.as_slice() else {
            return Err(stack_error(
                owner,
                styles,
                &format!(
                    "Style {style_id} has {} wires into the semantic Merge",
                    matches.len()
                ),
            ));
        };
        inputs.insert(*style_id, connection.clone());
    }
    let output_id = container_output_node_id(project, owner)?
        .ok_or_else(|| stack_error(owner, styles, "container has no Image output"))?;
    let is_output = output_id == merge_id;
    if !is_output {
        let outgoing = output_reaching_image_connections(project, semantics, merge_id);
        let [_connection] = outgoing.as_slice() else {
            return Err(stack_error(
                owner,
                styles,
                "Style Merge has no unique downstream Image flow",
            ));
        };
    }
    Ok(StyleMerge {
        node_id: merge_id,
        target,
        inputs,
    })
}

fn resolve_append_shape_source(
    project: &Project,
    owner: NodeContainer,
    state: Option<&StyleState>,
    after_style_id: Option<Uuid>,
    explicit: Option<PortAddress>,
) -> Result<PortAddress, LibraryError> {
    if after_style_id.is_some() && explicit.is_some() {
        return Err(LibraryError::Project(
            "Style insertion cannot use both a Style anchor and an explicit Shape source"
                .to_string(),
        ));
    }
    if let Some(source) = explicit {
        validate_shape_source(project, owner, &source)?;
        return Ok(source);
    }
    if let Some(anchor_id) = after_style_id {
        let state = state.ok_or_else(|| {
            LibraryError::Project(format!(
                "Style anchor {anchor_id} does not exist in the empty stack for {owner:?}"
            ))
        })?;
        return state
            .branches
            .get(&anchor_id)
            .map(|branch| branch.shape_input.from.clone())
            .ok_or_else(|| {
                LibraryError::Project(format!(
                    "Style anchor {anchor_id} is not in the semantic stack for {owner:?}"
                ))
            });
    }
    let Some(state) = state else {
        return terminal_shape_source(project, owner);
    };
    let sources = state
        .branches
        .values()
        .map(|branch| branch.shape_input.from.clone())
        .collect::<HashSet<_>>();
    let sources = sources.into_iter().collect::<Vec<_>>();
    let [source] = sources.as_slice() else {
        return Err(LibraryError::Project(format!(
            "Cannot choose a Shape source for {owner:?}: {} distinct Style branches; pass after_style_id or an explicit Shape source",
            sources.len()
        )));
    };
    Ok(source.clone())
}

fn validate_shape_source(
    project: &Project,
    owner: NodeContainer,
    source: &PortAddress,
) -> Result<(), LibraryError> {
    let PortOwner::Node(node_id) = source.owner else {
        return Err(LibraryError::Project(
            "Semantic Style Shape source must be a contained Node output".to_string(),
        ));
    };
    if !container_node_ids(project, owner)?.contains(&node_id) {
        return Err(LibraryError::Project(format!(
            "Shape source {source:?} is not contained by {owner:?}"
        )));
    }
    let definition = project
        .port_definition(source, PortDirection::Output)
        .ok_or_else(|| LibraryError::Project(format!("Shape source port {source:?} not found")))?;
    if definition.data_type != PortDataType::Shape {
        return Err(LibraryError::Project(format!(
            "Style source {source:?} is {:?}, not Shape",
            definition.data_type
        )));
    }
    Ok(())
}

fn attach_first_style(
    project: &mut Project,
    owner: NodeContainer,
    style_id: Uuid,
) -> Result<(), LibraryError> {
    let Some(output_id) = container_output_node_id(project, owner)? else {
        return project
            .set_output_node(owner, Some(style_id))
            .map_err(|error| LibraryError::Project(error.to_string()));
    };
    if project
        .get_node(output_id)
        .is_some_and(|node| matches!(node.content(), NodeContent::Merge))
    {
        project
            .connect_ports(
                image_output(style_id),
                PortAddress::new(PortOwner::Node(output_id), MERGE_IMAGES_PORT),
            )
            .map_err(|error| LibraryError::Project(error.to_string()))?;
        return Ok(());
    }
    let target = unique_empty_image_input_on_output_trunk(project, owner)?;
    project
        .connect_ports(image_output(style_id), target)
        .map_err(|error| LibraryError::Project(error.to_string()))?;
    Ok(())
}

fn unique_empty_image_input_on_output_trunk(
    project: &Project,
    owner: NodeContainer,
) -> Result<PortAddress, LibraryError> {
    let semantics = project.container_graph_semantics(container_port_owner(owner));
    let mut candidates = container_node_ids(project, owner)?
        .iter()
        .copied()
        .flat_map(|node_id| {
            project
                .port_definitions(PortOwner::Node(node_id))
                .into_iter()
                .filter(move |port| {
                    port.direction == PortDirection::Input && port.data_type == PortDataType::Image
                })
                .map(move |port| PortAddress::new(PortOwner::Node(node_id), port.key))
        })
        .filter(|target| semantics.structurally_reaches_output(target.owner))
        .filter(|target| {
            !project
                .connections
                .iter()
                .any(|connection| connection.to == *target)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| format!("{left:?}").cmp(&format!("{right:?}")));
    let [target] = candidates.as_slice() else {
        return Err(LibraryError::Project(format!(
            "Cannot attach the first Style to {owner:?}: expected one empty Image input on the output trunk, found {}",
            candidates.len()
        )));
    };
    Ok(target.clone())
}

fn synthesize_style_merge(
    project: &mut Project,
    owner: NodeContainer,
    state: &StyleState,
    new_style_id: Uuid,
) -> Result<(), LibraryError> {
    let [existing_style_id] = state.node_ids.as_slice() else {
        return Err(LibraryError::Project(
            "A direct Style stack must contain exactly one Style".to_string(),
        ));
    };
    let mut merge = Node::new_merge("Style Merge");
    let existing_position = project
        .get_node(*existing_style_id)
        .map(|node| node.ui_position)
        .ok_or_else(|| LibraryError::Project(format!("Style {existing_style_id} not found")))?;
    merge.ui_position = [existing_position[0] + 320.0, existing_position[1]];
    let merge_id = merge.id;
    project
        .insert_node_graph(owner, NodeGraphBundle::new(vec![merge], Vec::new(), None))
        .map_err(|error| LibraryError::Project(error.to_string()))?;
    let target = PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT);
    project
        .connect_ports(image_output(*existing_style_id), target.clone())
        .map_err(|error| LibraryError::Project(error.to_string()))?;
    project
        .connect_ports(image_output(new_style_id), target)
        .map_err(|error| LibraryError::Project(error.to_string()))?;
    if let Some(downstream) = &state.direct_downstream {
        reconnect_from(project, downstream.id, image_output(merge_id))?;
    } else if state.direct_is_output {
        project
            .set_output_node(owner, Some(merge_id))
            .map_err(|error| LibraryError::Project(error.to_string()))?;
    } else {
        return Err(LibraryError::Project(
            "Direct Style has neither an output binding nor downstream connection".to_string(),
        ));
    }
    Ok(())
}

fn output_reaching_image_connections(
    project: &Project,
    semantics: &crate::model::project::ContainerGraphSemantics,
    node_id: Uuid,
) -> Vec<ProjectConnection> {
    let mut connections = project
        .connections
        .iter()
        .filter(|connection| {
            connection.from == image_output(node_id)
                && semantics.structurally_reaches_output(connection.to.owner)
                && project
                    .port_definition(&connection.to, PortDirection::Input)
                    .is_some_and(|port| port.data_type == PortDataType::Image)
        })
        .cloned()
        .collect::<Vec<_>>();
    connections.sort_by_key(|connection| (connection.order, connection.id));
    connections
}

fn validate_style_candidate(project: &Project, owner: NodeContainer) -> Result<(), LibraryError> {
    validate_candidate(project, owner)?;
    let containment = project.validate_containment();
    if !containment.is_empty() {
        return Err(LibraryError::Validation(format!(
            "Style transaction for {owner:?} has invalid containment: {}",
            containment
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }
    resolve_style_stack(project, owner).map(|_| ())
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
            "Style reorder for {owner:?} must contain every current Style exactly once; current={}, requested={}",
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

fn shape_input(node_id: Uuid) -> PortAddress {
    PortAddress::new(PortOwner::Node(node_id), SHAPE_INPUT_PORT)
}

fn image_output(node_id: Uuid) -> PortAddress {
    PortAddress::new(PortOwner::Node(node_id), IMAGE_OUTPUT_PORT)
}

fn stack_error(owner: NodeContainer, node_ids: &[Uuid], reason: &str) -> LibraryError {
    LibraryError::Project(format!(
        "Cannot edit semantic Style stack for {owner:?}: {reason}; Nodes={}",
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
