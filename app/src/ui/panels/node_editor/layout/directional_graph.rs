//! Minimal authored-edge and strongly-connected-component support shared by
//! the directional planner's reachability and placement phases.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use library::model::Project;
use uuid::Uuid;

use super::{semantic_node_cmp, semantic_node_cmp_without_geometry};
use super::{BranchDirection, NodeLayoutGeometry};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ActualNodeEdge {
    pub(super) from: Uuid,
    pub(super) to: Uuid,
    pub(super) order: i64,
    from_port: String,
    to_port: String,
}

#[derive(Debug)]
pub(super) struct BranchGraph {
    pub(super) edges: Vec<ActualNodeEdge>,
    pub(super) components: Vec<Vec<Uuid>>,
    pub(super) component_by_node: HashMap<Uuid, usize>,
    outgoing_components: Vec<BTreeSet<usize>>,
    incoming_components: Vec<BTreeSet<usize>>,
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
        .then_with(|| left.from_port.cmp(&right.from_port))
        .then_with(|| semantic_node_cmp_without_geometry(project, left.to, right.to))
        .then_with(|| left.to_port.cmp(&right.to_port))
        .then_with(|| right.order.cmp(&left.order))
        .then_with(|| left.from.cmp(&right.from))
        .then_with(|| left.to.cmp(&right.to))
}

impl BranchGraph {
    pub(super) fn new(
        project: &Project,
        geometry: &BTreeMap<Uuid, NodeLayoutGeometry>,
        context_nodes: &HashSet<Uuid>,
        all_edges: Vec<ActualNodeEdge>,
    ) -> Self {
        let edges = all_edges
            .into_iter()
            .filter(|edge| context_nodes.contains(&edge.from) && context_nodes.contains(&edge.to))
            .collect::<Vec<_>>();
        let mut components =
            strongly_connected_components(project, geometry, context_nodes, &edges);
        components
            .sort_by(|left, right| semantic_node_cmp(project, geometry, left[0], right[0], &edges));
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
) -> Vec<Vec<Uuid>> {
    let mut adjacency = HashMap::<Uuid, Vec<Uuid>>::new();
    for node_id in nodes {
        adjacency.entry(*node_id).or_default();
    }
    for edge in edges {
        adjacency.entry(edge.from).or_default().push(edge.to);
    }
    for successors in adjacency.values_mut() {
        successors
            .sort_by(|left, right| semantic_node_cmp(project, geometry, *left, *right, edges));
        successors.dedup();
    }
    let mut ordered_nodes = nodes.iter().copied().collect::<Vec<_>>();
    ordered_nodes.sort_by(|left, right| semantic_node_cmp(project, geometry, *left, *right, edges));
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
        predecessors
            .sort_by(|left, right| semantic_node_cmp(project, geometry, *left, *right, edges));
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
        component.sort_by(|left, right| semantic_node_cmp(project, geometry, *left, *right, edges));
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
