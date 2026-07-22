//! Minimal authored-edge and strongly-connected-component support shared by
//! the directional planner's reachability and placement phases.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use library::model::project::{PortAddress, PortOwner};
use library::model::Project;
use uuid::Uuid;

use super::{semantic_node_cmp, semantic_node_cmp_without_geometry};
use super::{BranchDirection, NodeLayoutGeometry};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ActualNodeEdge {
    pub(super) from: Uuid,
    pub(super) to: Uuid,
    pub(super) order: i64,
    pub(super) connection_id: Uuid,
    from_port: String,
    pub(super) to_port: String,
}

#[derive(Debug)]
pub(super) struct BranchGraph {
    pub(super) edges: Vec<ActualNodeEdge>,
    pub(super) semantic_order: SemanticNodeOrder,
    pub(super) components: Vec<Vec<Uuid>>,
    pub(super) component_by_node: HashMap<Uuid, usize>,
    outgoing_components: Vec<BTreeSet<usize>>,
    incoming_components: Vec<BTreeSet<usize>>,
}

/// Precomputed semantic constraints used from sort comparators. Building may
/// inspect authored edges, but every subsequent Node-pair lookup is O(1).
#[derive(Debug)]
pub(super) struct SemanticNodeOrder {
    rank_by_node: HashMap<Uuid, usize>,
    #[cfg(test)]
    constraint_edge_count: usize,
}

impl SemanticNodeOrder {
    pub(super) fn new(
        project: &Project,
        geometry: &BTreeMap<Uuid, NodeLayoutGeometry>,
        node_ids: &HashSet<Uuid>,
        edges: &[ActualNodeEdge],
    ) -> Self {
        let mut constraints = native_variadic_constraints(project, edges);
        for node_id in node_ids {
            constraints.entry(*node_id).or_default();
        }
        let components = constraint_sccs(project, geometry, &constraints);
        let rank_by_node = constraint_total_order(project, geometry, &constraints, &components);
        Self {
            rank_by_node,
            #[cfg(test)]
            constraint_edge_count: constraints.values().map(BTreeSet::len).sum(),
        }
    }

    pub(super) fn compare(&self, left: Uuid, right: Uuid) -> Ordering {
        self.rank_by_node[&left].cmp(&self.rank_by_node[&right])
    }

    #[cfg(test)]
    pub(super) fn constraint_count(&self) -> usize {
        self.constraint_edge_count
    }
}

fn stable_node_cmp(
    project: &Project,
    geometry: &BTreeMap<Uuid, NodeLayoutGeometry>,
    left: Uuid,
    right: Uuid,
) -> Ordering {
    geometry
        .get(&left)
        .map_or(0.0, |item| item.position[1])
        .total_cmp(&geometry.get(&right).map_or(0.0, |item| item.position[1]))
        .then_with(|| {
            geometry
                .get(&left)
                .map_or(0.0, |item| item.position[0])
                .total_cmp(&geometry.get(&right).map_or(0.0, |item| item.position[0]))
        })
        .then_with(|| semantic_node_cmp_without_geometry(project, left, right))
        .then_with(|| left.cmp(&right))
}

fn native_variadic_constraints(
    project: &Project,
    edges: &[ActualNodeEdge],
) -> HashMap<Uuid, BTreeSet<Uuid>> {
    let mut groups = HashMap::<(Uuid, String), Vec<&ActualNodeEdge>>::new();
    for edge in edges {
        let target = PortAddress::new(PortOwner::Node(edge.to), edge.to_port.clone());
        if crate::ui::panels::node_editor::native_variadic_merge_target(project, &target).is_some()
        {
            groups
                .entry((edge.to, edge.to_port.clone()))
                .or_default()
                .push(edge);
        }
    }
    let mut constraints = HashMap::<Uuid, BTreeSet<Uuid>>::new();
    for (_, mut target_edges) in groups {
        target_edges.sort_by(|left, right| {
            shared_target_visual_cmp(project, left, right).unwrap_or(Ordering::Equal)
        });
        let mut seen = HashSet::new();
        let sources = target_edges
            .into_iter()
            .filter_map(|edge| seen.insert(edge.from).then_some(edge.from))
            .collect::<Vec<_>>();
        for pair in sources.windows(2) {
            constraints.entry(pair[0]).or_default().insert(pair[1]);
            constraints.entry(pair[1]).or_default();
        }
    }
    constraints
}

fn constraint_sccs(
    project: &Project,
    geometry: &BTreeMap<Uuid, NodeLayoutGeometry>,
    constraints: &HashMap<Uuid, BTreeSet<Uuid>>,
) -> Vec<Vec<Uuid>> {
    let mut nodes = constraints.keys().copied().collect::<Vec<_>>();
    nodes.sort_by(|left, right| stable_node_cmp(project, geometry, *left, *right));
    let mut adjacency = constraints
        .iter()
        .map(|(node, successors)| (*node, successors.iter().copied().collect()))
        .collect::<HashMap<_, Vec<_>>>();
    for successors in adjacency.values_mut() {
        successors.sort_by(|left, right| stable_node_cmp(project, geometry, *left, *right));
    }
    let finishing = iterative_finishing_order(&nodes, &adjacency);
    let mut reverse = nodes
        .iter()
        .copied()
        .map(|node| (node, Vec::new()))
        .collect::<HashMap<_, Vec<_>>>();
    for (from, successors) in constraints {
        for successor in successors {
            reverse.entry(*successor).or_default().push(*from);
        }
    }
    for predecessors in reverse.values_mut() {
        predecessors.sort_by(|left, right| stable_node_cmp(project, geometry, *left, *right));
    }
    let mut assigned = HashSet::new();
    let mut components = Vec::new();
    for start in finishing.into_iter().rev() {
        if !assigned.insert(start) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            component.push(node);
            for predecessor in reverse[&node].iter().rev() {
                if assigned.insert(*predecessor) {
                    stack.push(*predecessor);
                }
            }
        }
        component.sort_by(|left, right| stable_node_cmp(project, geometry, *left, *right));
        components.push(component);
    }
    components.sort_by(|left, right| stable_node_cmp(project, geometry, left[0], right[0]));
    components
}

fn constraint_total_order(
    project: &Project,
    geometry: &BTreeMap<Uuid, NodeLayoutGeometry>,
    constraints: &HashMap<Uuid, BTreeSet<Uuid>>,
    components: &[Vec<Uuid>],
) -> HashMap<Uuid, usize> {
    let component_by_node = components
        .iter()
        .enumerate()
        .flat_map(|(index, nodes)| nodes.iter().map(move |node| (*node, index)))
        .collect::<HashMap<_, _>>();
    let mut outgoing = vec![BTreeSet::new(); components.len()];
    let mut indegree = vec![0_usize; components.len()];
    for (from, successors) in constraints {
        let from_component = component_by_node[from];
        for successor in successors {
            let to_component = component_by_node[successor];
            if from_component != to_component && outgoing[from_component].insert(to_component) {
                indegree[to_component] += 1;
            }
        }
    }
    let component_key = |index: usize| {
        let node = components[index][0];
        (
            ordered_float::OrderedFloat(geometry.get(&node).map_or(0.0, |item| item.position[1])),
            ordered_float::OrderedFloat(geometry.get(&node).map_or(0.0, |item| item.position[0])),
            project
                .get_node(node)
                .map_or_else(String::new, |item| item.name.clone()),
            node,
            index,
        )
    };
    let mut ready = indegree
        .iter()
        .enumerate()
        .filter(|(_, degree)| **degree == 0)
        .map(|(index, _)| component_key(index))
        .collect::<BTreeSet<_>>();
    let mut rank_by_node = HashMap::new();
    let mut rank = 0;
    while let Some((_, _, _, _, component)) = ready.pop_first() {
        for node in &components[component] {
            rank_by_node.insert(*node, rank);
            rank += 1;
        }
        for successor in &outgoing[component] {
            indegree[*successor] -= 1;
            if indegree[*successor] == 0 {
                ready.insert(component_key(*successor));
            }
        }
    }

    rank_by_node
}

/// Extract only persisted Node-to-Node wires. Container and port-anchor
/// convenience edges are intentionally absent.
pub(super) fn actual_node_edges(
    project: &Project,
    node_ids: &HashSet<Uuid>,
) -> Vec<ActualNodeEdge> {
    let mut result = project
        .connections
        .iter()
        .filter_map(|connection| {
            let (
                library::model::project::PortOwner::Node(from),
                library::model::project::PortOwner::Node(to),
            ) = (connection.from.owner, connection.to.owner)
            else {
                return None;
            };
            (from != to && node_ids.contains(&from) && node_ids.contains(&to)).then(|| {
                ActualNodeEdge {
                    from,
                    to,
                    order: connection.order,
                    connection_id: connection.id,
                    from_port: connection.from.port.clone(),
                    to_port: connection.to.port.clone(),
                }
            })
        })
        .collect::<Vec<_>>();
    result.sort_by(|left, right| semantic_edge_cmp(project, left, right));
    result.dedup();
    result
}

pub(super) fn semantic_edge_cmp(
    project: &Project,
    left: &ActualNodeEdge,
    right: &ActualNodeEdge,
) -> Ordering {
    semantic_node_cmp_without_geometry(project, left.from, right.from)
        .then_with(|| left.from.cmp(&right.from))
        .then_with(|| left.from_port.cmp(&right.from_port))
        .then_with(|| semantic_node_cmp_without_geometry(project, left.to, right.to))
        .then_with(|| left.to.cmp(&right.to))
        .then_with(|| left.to_port.cmp(&right.to_port))
        .then_with(|| left.order.cmp(&right.order))
        .then_with(|| left.connection_id.cmp(&right.connection_id))
}

pub(super) fn shared_target_visual_cmp(
    project: &Project,
    left: &ActualNodeEdge,
    right: &ActualNodeEdge,
) -> Option<Ordering> {
    (left.to == right.to && left.to_port == right.to_port)
        .then(|| PortAddress::new(PortOwner::Node(left.to), left.to_port.clone()))
        .and_then(|target| {
            crate::ui::panels::node_editor::native_variadic_connection_visual_cmp(
                project,
                &target,
                (left.order, left.connection_id),
                (right.order, right.connection_id),
            )
        })
}

impl BranchGraph {
    pub(super) fn new(
        project: &Project,
        geometry: &BTreeMap<Uuid, NodeLayoutGeometry>,
        context_nodes: &HashSet<Uuid>,
        all_edges: Vec<ActualNodeEdge>,
        semantic_order: SemanticNodeOrder,
    ) -> Self {
        let edges = all_edges
            .into_iter()
            .filter(|edge| context_nodes.contains(&edge.from) && context_nodes.contains(&edge.to))
            .collect::<Vec<_>>();
        let mut components = strongly_connected_components(
            project,
            geometry,
            context_nodes,
            &edges,
            &semantic_order,
        );
        components.sort_by(|left, right| {
            semantic_node_cmp(project, geometry, left[0], right[0], &semantic_order)
        });
        let mut component_by_node = HashMap::new();
        for (component_index, component) in components.iter().enumerate() {
            for node_id in component {
                component_by_node.insert(*node_id, component_index);
            }
        }
        let mut outgoing_components = vec![BTreeSet::new(); components.len()];
        let mut incoming_components = vec![BTreeSet::new(); components.len()];
        for edge in &edges {
            let from_component = component_by_node[&edge.from];
            let to_component = component_by_node[&edge.to];
            if from_component != to_component {
                outgoing_components[from_component].insert(to_component);
                incoming_components[to_component].insert(from_component);
            }
        }
        Self {
            edges,
            semantic_order,
            components,
            component_by_node,
            outgoing_components,
            incoming_components,
        }
    }

    pub(super) fn component_depths(
        &self,
        anchor: Uuid,
        direction: BranchDirection,
    ) -> HashMap<usize, usize> {
        let Some(&anchor_component) = self.component_by_node.get(&anchor) else {
            return HashMap::new();
        };
        let mut depth = HashMap::from([(anchor_component, 0_usize)]);
        let mut queue = VecDeque::from([anchor_component]);
        while let Some(component) = queue.pop_front() {
            let next_components = match direction {
                BranchDirection::Downstream => &self.outgoing_components[component],
                BranchDirection::Upstream => &self.incoming_components[component],
            };
            let candidate_depth = depth[&component] + 1;
            for next in next_components {
                let entry = depth.entry(*next).or_default();
                if candidate_depth > *entry {
                    *entry = candidate_depth;
                    queue.push_back(*next);
                }
            }
        }
        depth
    }

    pub(super) fn level_for(
        &self,
        node_id: Uuid,
        depth: &HashMap<usize, usize>,
        direction: BranchDirection,
    ) -> i32 {
        let component = self.component_by_node[&node_id];
        let magnitude = depth.get(&component).copied().unwrap_or_default() as i32;
        match direction {
            BranchDirection::Downstream => magnitude,
            BranchDirection::Upstream => -magnitude,
        }
    }
}

fn strongly_connected_components(
    project: &Project,
    geometry: &BTreeMap<Uuid, NodeLayoutGeometry>,
    nodes: &HashSet<Uuid>,
    edges: &[ActualNodeEdge],
    semantic_order: &SemanticNodeOrder,
) -> Vec<Vec<Uuid>> {
    let mut adjacency = HashMap::<Uuid, Vec<Uuid>>::new();
    for node_id in nodes {
        adjacency.entry(*node_id).or_default();
    }
    for edge in edges {
        adjacency.entry(edge.from).or_default().push(edge.to);
    }
    for successors in adjacency.values_mut() {
        successors.sort_by(|left, right| {
            semantic_node_cmp(project, geometry, *left, *right, semantic_order)
        });
        successors.dedup();
    }
    let mut ordered_nodes = nodes.iter().copied().collect::<Vec<_>>();
    ordered_nodes
        .sort_by(|left, right| semantic_node_cmp(project, geometry, *left, *right, semantic_order));
    let finishing_order = iterative_finishing_order(&ordered_nodes, &adjacency);
    let mut reverse_adjacency = nodes
        .iter()
        .copied()
        .map(|node_id| (node_id, Vec::new()))
        .collect::<HashMap<_, Vec<Uuid>>>();
    for (from, successors) in &adjacency {
        for successor in successors {
            reverse_adjacency.entry(*successor).or_default().push(*from);
        }
    }
    for predecessors in reverse_adjacency.values_mut() {
        predecessors.sort_by(|left, right| {
            semantic_node_cmp(project, geometry, *left, *right, semantic_order)
        });
        predecessors.dedup();
    }

    let mut assigned = HashSet::new();
    let mut components = Vec::new();
    for start in finishing_order.into_iter().rev() {
        if !assigned.insert(start) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![start];
        while let Some(node_id) = stack.pop() {
            component.push(node_id);
            let predecessors = &reverse_adjacency[&node_id];
            for predecessor in predecessors.iter().rev() {
                if assigned.insert(*predecessor) {
                    stack.push(*predecessor);
                }
            }
        }
        components.push(component);
    }
    for component in &mut components {
        component.sort_by(|left, right| {
            semantic_node_cmp(project, geometry, *left, *right, semantic_order)
        });
    }
    components
}

/// Iterative DFS avoids a call-stack limit on large procedural graphs.
fn iterative_finishing_order(
    ordered_nodes: &[Uuid],
    adjacency: &HashMap<Uuid, Vec<Uuid>>,
) -> Vec<Uuid> {
    let mut visited = HashSet::new();
    let mut finishing_order = Vec::with_capacity(ordered_nodes.len());
    for start in ordered_nodes {
        if !visited.insert(*start) {
            continue;
        }
        let mut stack = vec![(*start, 0_usize)];
        while let Some((node_id, next_index)) = stack.last_mut() {
            let successors = &adjacency[node_id];
            if let Some(successor) = successors.get(*next_index).copied() {
                *next_index += 1;
                if visited.insert(successor) {
                    stack.push((successor, 0));
                }
            } else {
                finishing_order.push(*node_id);
                stack.pop();
            }
        }
    }
    finishing_order
}

#[cfg(test)]
pub(super) fn actual_edge_pairs(project: &Project, composition_id: Uuid) -> Vec<(Uuid, Uuid)> {
    let nodes = super::composition_node_ids(project, composition_id);
    actual_node_edges(project, &nodes)
        .into_iter()
        .map(|edge| (edge.from, edge.to))
        .collect()
}
