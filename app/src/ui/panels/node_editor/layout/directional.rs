//! Pure branch-scoped layout planning for pointer and keyboard gestures.
//!
//! This module deliberately reads only authored Node-to-Node connections. A
//! container output may be rendered as a convenient wire, but expanding that
//! output back into structural Merge helpers would make a branch gesture move
//! Nodes the user never traversed.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;

use library::model::{NodeContainer, Project};
use uuid::Uuid;

const POSITION_EPSILON: f32 = 0.001;

#[path = "directional_graph.rs"]
mod graph;
#[path = "directional_packing.rs"]
mod packing;

use graph::{actual_node_edges, semantic_edge_cmp, ActualNodeEdge, BranchGraph, SemanticNodeOrder};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) enum BranchDirection {
    Upstream,
    Downstream,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) enum LayoutAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) enum DirectionalLayoutMode {
    Layout,
    Align,
    Distribute,
    AlignAndDistribute,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::ui::panels::node_editor) struct NodeLayoutGeometry {
    pub(in crate::ui::panels::node_editor) position: [f32; 2],
    pub(in crate::ui::panels::node_editor) size: [f32; 2],
}

impl NodeLayoutGeometry {
    fn is_valid(self) -> bool {
        self.position.into_iter().all(f32::is_finite)
            && self
                .size
                .into_iter()
                .all(|value| value.is_finite() && value > 0.0)
    }

    fn right(self) -> f32 {
        self.position[0] + self.size[0]
    }

    fn bottom(self) -> f32 {
        self.position[1] + self.size[1]
    }

    fn center(self) -> [f32; 2] {
        [
            self.position[0] + self.size[0] * 0.5,
            self.position[1] + self.size[1] * 0.5,
        ]
    }

    fn with_position(self, position: [f32; 2]) -> Self {
        Self { position, ..self }
    }
}

/// Immutable gesture snapshot consumed by the Project-aware adapter.
///
/// An empty `frozen_selected_node_ids` means "the whole reachable branch".
/// Otherwise reachability is computed first and only then intersected with
/// this snapshot. Consequently a selected Node remains reachable through an
/// unselected intermediate Node.
#[derive(Clone, Copy, Debug)]
pub(in crate::ui::panels::node_editor) struct DirectionalLayoutRequest<'a> {
    pub(in crate::ui::panels::node_editor) composition_id: Uuid,
    pub(in crate::ui::panels::node_editor) direct_owner: NodeContainer,
    pub(in crate::ui::panels::node_editor) anchor_node_id: Uuid,
    pub(in crate::ui::panels::node_editor) frozen_selected_node_ids: &'a [Uuid],
    pub(in crate::ui::panels::node_editor) fixed_node_ids: &'a [Uuid],
    pub(in crate::ui::panels::node_editor) direction: BranchDirection,
    pub(in crate::ui::panels::node_editor) axis: LayoutAxis,
    pub(in crate::ui::panels::node_editor) mode: DirectionalLayoutMode,
    /// Rendered or conservatively estimated geometry at gesture start.
    pub(in crate::ui::panels::node_editor) node_geometry: &'a BTreeMap<Uuid, NodeLayoutGeometry>,
    pub(in crate::ui::panels::node_editor) horizontal_gap: f32,
    pub(in crate::ui::panels::node_editor) vertical_gap: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) enum DirectionalLayoutBlockedReason {
    CrossesDirectOwner,
    ExplicitlyFixed,
    MissingGeometry,
    InvalidGeometry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) struct DirectionalLayoutBlockedNode {
    pub(in crate::ui::panels::node_editor) node_id: Uuid,
    pub(in crate::ui::panels::node_editor) reason: DirectionalLayoutBlockedReason,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) struct DirectionalLayoutDiagnostics {
    /// Directionally reachable Nodes, excluding the fixed anchor itself.
    pub(in crate::ui::panels::node_editor) reachable_node_ids: Vec<Uuid>,
    /// Reachable, selected (when constrained), same-owner Nodes that may move.
    pub(in crate::ui::panels::node_editor) eligible_node_ids: Vec<Uuid>,
    pub(in crate::ui::panels::node_editor) moved_node_ids: Vec<Uuid>,
    pub(in crate::ui::panels::node_editor) blocked_nodes: Vec<DirectionalLayoutBlockedNode>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui::panels::node_editor) struct DirectionalLayoutPlan {
    /// Sparse positions: unchanged and fixed Nodes are intentionally absent.
    pub(in crate::ui::panels::node_editor) node_positions: BTreeMap<Uuid, [f32; 2]>,
    pub(in crate::ui::panels::node_editor) diagnostics: DirectionalLayoutDiagnostics,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) enum DirectionalLayoutError {
    CompositionNotFound(Uuid),
    DirectOwnerOutsideComposition(Uuid),
    AnchorNotFound(Uuid),
    AnchorOwnerMismatch,
    AnchorGeometryMissing(Uuid),
    AnchorGeometryInvalid(Uuid),
    InvalidGap,
    ConstraintCollision,
}

impl fmt::Display for DirectionalLayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CompositionNotFound(id) => write!(formatter, "composition {id} does not exist"),
            Self::DirectOwnerOutsideComposition(id) => {
                write!(formatter, "direct owner is outside composition {id}")
            }
            Self::AnchorNotFound(id) => write!(formatter, "anchor Node {id} does not exist"),
            Self::AnchorOwnerMismatch => {
                formatter.write_str("anchor Node is not directly owned by the requested container")
            }
            Self::AnchorGeometryMissing(id) => {
                write!(formatter, "anchor Node {id} has no geometry")
            }
            Self::AnchorGeometryInvalid(id) => {
                write!(formatter, "anchor Node {id} has invalid geometry")
            }
            Self::InvalidGap => formatter.write_str("layout gaps must be finite and non-negative"),
            Self::ConstraintCollision => formatter
                .write_str("directional layout constraints have no collision-free exact placement"),
        }
    }
}

impl std::error::Error for DirectionalLayoutError {}

/// Compute a branch-scoped layout without mutating `Project`.
pub(in crate::ui::panels::node_editor) fn plan_directional_layout(
    project: &Project,
    request: &DirectionalLayoutRequest<'_>,
) -> Result<DirectionalLayoutPlan, DirectionalLayoutError> {
    validate_request(project, request)?;
    let anchor_geometry = request.node_geometry[&request.anchor_node_id];
    let graph_node_ids = composition_node_ids(project, request.composition_id);
    let edges = actual_node_edges(project, &graph_node_ids);
    let semantic_order =
        SemanticNodeOrder::new(project, request.node_geometry, &graph_node_ids, &edges);
    let reachable = reachable_nodes(project, request.anchor_node_id, request.direction, &edges);
    let owner_edges = edges
        .iter()
        .filter(|edge| {
            project.find_node_container(edge.from) == Some(request.direct_owner)
                && project.find_node_container(edge.to) == Some(request.direct_owner)
        })
        .cloned()
        .collect::<Vec<_>>();
    let owner_reachable = reachable_nodes(
        project,
        request.anchor_node_id,
        request.direction,
        &owner_edges,
    )
    .into_iter()
    .collect::<HashSet<_>>();
    let selected = request
        .frozen_selected_node_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let fixed = request
        .fixed_node_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>();

    let mut blocked = Vec::new();
    let mut eligible = Vec::new();
    for node_id in &reachable {
        // A path that exits and later re-enters the direct owner is still a
        // boundary-crossing gesture. Do not move the re-entered tail.
        if !owner_reachable.contains(node_id) {
            blocked.push(blocked_node(
                *node_id,
                DirectionalLayoutBlockedReason::CrossesDirectOwner,
            ));
            continue;
        }
        if !selected.is_empty() && !selected.contains(node_id) {
            continue;
        }
        if fixed.contains(node_id) {
            blocked.push(blocked_node(
                *node_id,
                DirectionalLayoutBlockedReason::ExplicitlyFixed,
            ));
            continue;
        }
        match request.node_geometry.get(node_id).copied() {
            None => blocked.push(blocked_node(
                *node_id,
                DirectionalLayoutBlockedReason::MissingGeometry,
            )),
            Some(geometry) if !geometry.is_valid() => blocked.push(blocked_node(
                *node_id,
                DirectionalLayoutBlockedReason::InvalidGeometry,
            )),
            Some(_) => eligible.push(*node_id),
        }
    }

    sort_nodes_semantically(
        project,
        request.node_geometry,
        &semantic_order,
        &mut eligible,
    );
    sort_nodes_semantically(
        project,
        request.node_geometry,
        &semantic_order,
        &mut blocked,
    );
    let eligible_set = eligible.iter().copied().collect::<HashSet<_>>();
    let context_nodes = reachable
        .iter()
        .copied()
        .chain(std::iter::once(request.anchor_node_id))
        .filter(|node_id| project.find_node_container(*node_id) == Some(request.direct_owner))
        .filter(|node_id| {
            request
                .node_geometry
                .get(node_id)
                .is_some_and(|geometry| geometry.is_valid())
        })
        .collect::<HashSet<_>>();
    let branch_graph = BranchGraph::new(
        project,
        request.node_geometry,
        &context_nodes,
        edges,
        semantic_order,
    );
    let component_depth = branch_graph.component_depths(request.anchor_node_id, request.direction);
    let mut planned = match request.mode {
        DirectionalLayoutMode::Layout => {
            graph_layout_positions(project, request, &branch_graph, &component_depth, &eligible)
        }
        DirectionalLayoutMode::Align => request
            .node_geometry
            .iter()
            .filter(|(node_id, _)| eligible_set.contains(node_id))
            .map(|(node_id, geometry)| (*node_id, geometry.position))
            .collect(),
        DirectionalLayoutMode::Distribute | DirectionalLayoutMode::AlignAndDistribute => {
            distribute_positions(project, request, &branch_graph, &component_depth, &eligible)
        }
    };

    if matches!(
        request.mode,
        DirectionalLayoutMode::Align | DirectionalLayoutMode::AlignAndDistribute
    ) {
        align_positions(request, anchor_geometry, &eligible, &mut planned);
    }
    if request.mode == DirectionalLayoutMode::Layout {
        enforce_left_to_right_flow(request, &branch_graph, &eligible_set, &mut planned);
    }
    packing::pack_layout_level_blocks(
        project,
        request,
        &branch_graph,
        &component_depth,
        &eligible,
        &eligible_set,
        &mut planned,
    )?;

    let mut positions = BTreeMap::new();
    for node_id in &eligible {
        let Some(position) = planned.get(node_id).copied() else {
            continue;
        };
        let current = request.node_geometry[node_id].position;
        if position_changed(current, position) {
            positions.insert(*node_id, position);
        }
    }
    let moved = eligible
        .iter()
        .copied()
        .filter(|node_id| positions.contains_key(node_id))
        .collect();
    let mut reachable_diagnostic = reachable;
    sort_nodes_semantically(
        project,
        request.node_geometry,
        &branch_graph.semantic_order,
        &mut reachable_diagnostic,
    );
    Ok(DirectionalLayoutPlan {
        node_positions: positions,
        diagnostics: DirectionalLayoutDiagnostics {
            reachable_node_ids: reachable_diagnostic,
            eligible_node_ids: eligible,
            moved_node_ids: moved,
            blocked_nodes: blocked,
        },
    })
}

fn validate_request(
    project: &Project,
    request: &DirectionalLayoutRequest<'_>,
) -> Result<(), DirectionalLayoutError> {
    if project.get_composition(request.composition_id).is_none() {
        return Err(DirectionalLayoutError::CompositionNotFound(
            request.composition_id,
        ));
    }
    if containing_composition(project, request.direct_owner) != Some(request.composition_id) {
        return Err(DirectionalLayoutError::DirectOwnerOutsideComposition(
            request.composition_id,
        ));
    }
    if project.get_node(request.anchor_node_id).is_none() {
        return Err(DirectionalLayoutError::AnchorNotFound(
            request.anchor_node_id,
        ));
    }
    if project.find_node_container(request.anchor_node_id) != Some(request.direct_owner) {
        return Err(DirectionalLayoutError::AnchorOwnerMismatch);
    }
    let Some(anchor_geometry) = request.node_geometry.get(&request.anchor_node_id).copied() else {
        return Err(DirectionalLayoutError::AnchorGeometryMissing(
            request.anchor_node_id,
        ));
    };
    if !anchor_geometry.is_valid() {
        return Err(DirectionalLayoutError::AnchorGeometryInvalid(
            request.anchor_node_id,
        ));
    }
    if !request.horizontal_gap.is_finite()
        || !request.vertical_gap.is_finite()
        || request.horizontal_gap < 0.0
        || request.vertical_gap < 0.0
    {
        return Err(DirectionalLayoutError::InvalidGap);
    }
    Ok(())
}

fn containing_composition(project: &Project, owner: NodeContainer) -> Option<Uuid> {
    match owner {
        NodeContainer::Composition(id) => project.get_composition(id).map(|_| id),
        NodeContainer::Track(id) => project.find_composition_for_track(id),
        NodeContainer::Clip(id) => project
            .find_track_for_clip(id)
            .and_then(|track_id| project.find_composition_for_track(track_id)),
    }
}

fn composition_node_ids(project: &Project, composition_id: Uuid) -> HashSet<Uuid> {
    let Some(composition) = project.get_composition(composition_id) else {
        return HashSet::new();
    };
    let mut result = composition.node_ids.iter().copied().collect::<HashSet<_>>();
    for track_id in &composition.track_ids {
        let Some(track) = project.get_track(*track_id) else {
            continue;
        };
        result.extend(track.node_ids.iter().copied());
        for clip_id in &track.clip_ids {
            if let Some(clip) = project.get_clip(*clip_id) {
                result.extend(clip.node_ids.iter().copied());
            }
        }
    }
    result.retain(|node_id| project.get_node(*node_id).is_some());
    result
}

fn reachable_nodes(
    project: &Project,
    anchor: Uuid,
    direction: BranchDirection,
    edges: &[ActualNodeEdge],
) -> Vec<Uuid> {
    let mut adjacency = HashMap::<Uuid, Vec<&ActualNodeEdge>>::new();
    for edge in edges {
        let key = match direction {
            BranchDirection::Downstream => edge.from,
            BranchDirection::Upstream => edge.to,
        };
        adjacency.entry(key).or_default().push(edge);
    }
    for successors in adjacency.values_mut() {
        successors.sort_by(|left, right| semantic_edge_cmp(project, left, right));
    }
    let mut visited = HashSet::from([anchor]);
    let mut queue = VecDeque::from([anchor]);
    let mut result = Vec::new();
    while let Some(node_id) = queue.pop_front() {
        for edge in adjacency.get(&node_id).into_iter().flatten() {
            let next = match direction {
                BranchDirection::Downstream => edge.to,
                BranchDirection::Upstream => edge.from,
            };
            if visited.insert(next) {
                result.push(next);
                queue.push_back(next);
            }
        }
    }
    result
}

fn graph_layout_positions(
    project: &Project,
    request: &DirectionalLayoutRequest<'_>,
    graph: &BranchGraph,
    depth: &HashMap<usize, usize>,
    eligible: &[Uuid],
) -> BTreeMap<Uuid, [f32; 2]> {
    let anchor = request.node_geometry[&request.anchor_node_id];
    let mut nodes_by_level = BTreeMap::<i32, Vec<Uuid>>::new();
    for node_id in graph.component_by_node.keys().copied() {
        nodes_by_level
            .entry(graph.level_for(node_id, depth, request.direction))
            .or_default()
            .push(node_id);
    }
    for nodes in nodes_by_level.values_mut() {
        sort_nodes_semantically(project, request.node_geometry, &graph.semantic_order, nodes);
    }
    let max_width_by_level = nodes_by_level
        .iter()
        .map(|(level, nodes)| {
            let width = nodes
                .iter()
                .map(|node_id| request.node_geometry[node_id].size[0])
                .max_by(f32::total_cmp)
                .unwrap_or_default();
            (*level, width)
        })
        .collect::<BTreeMap<_, _>>();
    let mut x_by_level = BTreeMap::from([(0_i32, anchor.position[0])]);
    if let Some(max_level) = nodes_by_level.keys().next_back().copied() {
        for level in 1..=max_level.max(0) {
            let previous = level - 1;
            let x = x_by_level[&previous] + max_width_by_level[&previous] + request.horizontal_gap;
            x_by_level.insert(level, x);
        }
    }
    if let Some(min_level) = nodes_by_level.keys().next().copied() {
        for level in (min_level..=-1).rev() {
            let x = x_by_level[&(level + 1)] - max_width_by_level[&level] - request.horizontal_gap;
            x_by_level.insert(level, x);
        }
    }

    let mut planned_geometry = BTreeMap::<Uuid, NodeLayoutGeometry>::new();
    let mut result = BTreeMap::new();
    let mut ordered_eligible = eligible.to_vec();
    ordered_eligible.sort_by(|left, right| {
        let left_level = graph.level_for(*left, depth, request.direction);
        let right_level = graph.level_for(*right, depth, request.direction);
        directional_level_cmp(left_level, right_level, request.direction).then_with(|| {
            semantic_node_cmp(
                project,
                request.node_geometry,
                *left,
                *right,
                &graph.semantic_order,
            )
        })
    });
    for node_id in ordered_eligible {
        let level = graph.level_for(node_id, depth, request.direction);
        let geometry = request.node_geometry[&node_id];
        // A single Node at this rank keeps its authored orthogonal lane when
        // possible. Multi-Node ranks share the anchor lane and are packed in
        // semantic order, so existing reversed Y values cannot invert ordered
        // Merge inputs.
        let preferred_y = if nodes_by_level[&level].len() == 1 {
            geometry.position[1]
        } else {
            anchor.position[1]
        };
        let mut position = [x_by_level[&level], preferred_y];
        position[0] = constrained_x(request, graph, &planned_geometry, node_id, position[0]);
        let placed = geometry.with_position(position);
        planned_geometry.insert(node_id, placed);
        result.insert(node_id, position);
    }
    result
}

fn distribute_positions(
    project: &Project,
    request: &DirectionalLayoutRequest<'_>,
    graph: &BranchGraph,
    depth: &HashMap<usize, usize>,
    eligible: &[Uuid],
) -> BTreeMap<Uuid, [f32; 2]> {
    let mut ordered = eligible.to_vec();
    ordered.sort_by(|left, right| {
        let left_level = graph.level_for(*left, depth, request.direction);
        let right_level = graph.level_for(*right, depth, request.direction);
        directional_level_cmp(left_level, right_level, request.direction).then_with(|| {
            semantic_node_cmp(
                project,
                request.node_geometry,
                *left,
                *right,
                &graph.semantic_order,
            )
        })
    });
    let anchor = request.node_geometry[&request.anchor_node_id];
    let (coordinate, gap) = match request.axis {
        LayoutAxis::Horizontal => (0, request.horizontal_gap),
        LayoutAxis::Vertical => (1, request.vertical_gap),
    };
    let mut cursor = match request.direction {
        BranchDirection::Downstream => anchor.position[coordinate] + anchor.size[coordinate] + gap,
        BranchDirection::Upstream => anchor.position[coordinate] - gap,
    };
    let mut result = BTreeMap::new();
    for node_id in ordered {
        let geometry = request.node_geometry[&node_id];
        let mut position = geometry.position;
        match request.direction {
            BranchDirection::Downstream => {
                position[coordinate] = cursor;
                cursor += geometry.size[coordinate] + gap;
            }
            BranchDirection::Upstream => {
                position[coordinate] = cursor - geometry.size[coordinate];
                cursor = position[coordinate] - gap;
            }
        }
        result.insert(node_id, position);
    }
    result
}

fn align_positions(
    request: &DirectionalLayoutRequest<'_>,
    anchor: NodeLayoutGeometry,
    eligible: &[Uuid],
    planned: &mut BTreeMap<Uuid, [f32; 2]>,
) {
    let aligned_coordinate = match request.axis {
        LayoutAxis::Horizontal => 1,
        LayoutAxis::Vertical => 0,
    };
    let anchor_center = anchor.center()[aligned_coordinate];
    for node_id in eligible {
        let geometry = request.node_geometry[node_id];
        let position = planned.entry(*node_id).or_insert(geometry.position);
        position[aligned_coordinate] = anchor_center - geometry.size[aligned_coordinate] * 0.5;
    }
}

fn constrained_x(
    request: &DirectionalLayoutRequest<'_>,
    graph: &BranchGraph,
    planned: &BTreeMap<Uuid, NodeLayoutGeometry>,
    node_id: Uuid,
    initial_x: f32,
) -> f32 {
    let node_component = graph.component_by_node[&node_id];
    let width = request.node_geometry[&node_id].size[0];
    match request.direction {
        BranchDirection::Downstream => graph
            .edges
            .iter()
            .filter(|edge| {
                edge.to == node_id && graph.component_by_node[&edge.from] != node_component
            })
            .filter_map(|edge| effective_geometry(request, planned, edge.from))
            .fold(initial_x, |x, predecessor| {
                x.max(predecessor.right() + request.horizontal_gap)
            }),
        BranchDirection::Upstream => graph
            .edges
            .iter()
            .filter(|edge| {
                edge.from == node_id && graph.component_by_node[&edge.to] != node_component
            })
            .filter_map(|edge| effective_geometry(request, planned, edge.to))
            .fold(initial_x, |x, successor| {
                x.min(successor.position[0] - request.horizontal_gap - width)
            }),
    }
}

fn enforce_left_to_right_flow(
    request: &DirectionalLayoutRequest<'_>,
    graph: &BranchGraph,
    eligible: &HashSet<Uuid>,
    planned: &mut BTreeMap<Uuid, [f32; 2]>,
) {
    for _ in 0..graph.components.len().max(1) {
        let mut changed = false;
        for edge in &graph.edges {
            if graph.component_by_node[&edge.from] == graph.component_by_node[&edge.to] {
                continue;
            }
            let Some(from) = effective_position_geometry(request, planned, edge.from) else {
                continue;
            };
            let Some(to) = effective_position_geometry(request, planned, edge.to) else {
                continue;
            };
            let minimum_to_x = from.right() + request.horizontal_gap;
            if eligible.contains(&edge.to) && to.position[0] < minimum_to_x {
                if let Some(position) = planned.get_mut(&edge.to) {
                    position[0] = minimum_to_x;
                    changed = true;
                }
            } else if eligible.contains(&edge.from) && !eligible.contains(&edge.to) {
                let maximum_from_x = to.position[0] - request.horizontal_gap - from.size[0];
                if from.position[0] > maximum_from_x {
                    if let Some(position) = planned.get_mut(&edge.from) {
                        position[0] = maximum_from_x;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
}

fn effective_geometry(
    request: &DirectionalLayoutRequest<'_>,
    planned: &BTreeMap<Uuid, NodeLayoutGeometry>,
    node_id: Uuid,
) -> Option<NodeLayoutGeometry> {
    planned
        .get(&node_id)
        .copied()
        .or_else(|| request.node_geometry.get(&node_id).copied())
}

fn effective_position_geometry(
    request: &DirectionalLayoutRequest<'_>,
    planned: &BTreeMap<Uuid, [f32; 2]>,
    node_id: Uuid,
) -> Option<NodeLayoutGeometry> {
    let geometry = request.node_geometry.get(&node_id).copied()?;
    Some(geometry.with_position(planned.get(&node_id).copied().unwrap_or(geometry.position)))
}

fn directional_level_cmp(left: i32, right: i32, direction: BranchDirection) -> Ordering {
    match direction {
        BranchDirection::Downstream => left.cmp(&right),
        BranchDirection::Upstream => right.cmp(&left),
    }
}

fn sort_nodes_semantically<T: SemanticNodeIdentity>(
    project: &Project,
    geometry: &BTreeMap<Uuid, NodeLayoutGeometry>,
    semantic_order: &SemanticNodeOrder,
    nodes: &mut [T],
) {
    nodes.sort_by(|left, right| {
        semantic_node_cmp(
            project,
            geometry,
            left.node_id(),
            right.node_id(),
            semantic_order,
        )
    });
}

trait SemanticNodeIdentity {
    fn node_id(&self) -> Uuid;
}

impl SemanticNodeIdentity for Uuid {
    fn node_id(&self) -> Uuid {
        *self
    }
}

impl SemanticNodeIdentity for DirectionalLayoutBlockedNode {
    fn node_id(&self) -> Uuid {
        self.node_id
    }
}

fn semantic_node_cmp(
    _project: &Project,
    _geometry: &BTreeMap<Uuid, NodeLayoutGeometry>,
    left: Uuid,
    right: Uuid,
    semantic_order: &SemanticNodeOrder,
) -> Ordering {
    semantic_order.compare(left, right)
}

fn semantic_node_cmp_without_geometry(project: &Project, left: Uuid, right: Uuid) -> Ordering {
    let left_name = project.get_node(left).map_or("", |node| node.name.as_str());
    let right_name = project
        .get_node(right)
        .map_or("", |node| node.name.as_str());
    left_name.cmp(right_name)
}

fn blocked_node(
    node_id: Uuid,
    reason: DirectionalLayoutBlockedReason,
) -> DirectionalLayoutBlockedNode {
    DirectionalLayoutBlockedNode { node_id, reason }
}

fn position_changed(left: [f32; 2], right: [f32; 2]) -> bool {
    (left[0] - right[0]).abs() > POSITION_EPSILON || (left[1] - right[1]).abs() > POSITION_EPSILON
}

#[cfg(test)]
#[path = "directional_tests.rs"]
mod tests;
