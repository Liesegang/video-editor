use eframe::egui;
use library::model::project::{PortOwner, AUDIO_OUTPUT_PORT, IMAGE_OUTPUT_PORT};
#[cfg(test)]
use library::model::Node;
use library::model::Project;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use uuid::Uuid;

use super::column_packing::{enforce_structural_pair_row, pack_column};
use super::merge_alignment::{merge_anchor_aligned_top, pack_targeted_column};
use super::node_geometry::{estimated_node_size, estimated_node_width};
use crate::ui::panels::node_editor::{AUTO_LAYOUT_COLUMN_GAP, AUTO_LAYOUT_ROW_GAP};

#[derive(Clone, Copy, Debug)]
pub(super) struct NodeBandBounds {
    pub(super) min_x: f32,
    pub(super) max_x: f32,
    pub(super) height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct NodeRankColumn {
    pub(super) x: f32,
    pub(super) width: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LayoutEdge {
    pub(super) from: Uuid,
    pub(super) to: Uuid,
    /// Persisted variadic order is back-to-front. Presentation places the
    /// greatest (front-most) order first on the vertical axis.
    pub(super) order: i64,
    pub(super) container_source: bool,
    pub(super) connection_id: Uuid,
}

pub(super) struct NodeBandPlacement<'a> {
    pub(super) container_output_y: &'a HashMap<PortOwner, f32>,
    pub(super) origin_y: f32,
    pub(super) positions: &'a mut BTreeMap<Uuid, [f32; 2]>,
}

pub(super) fn node_rank_columns(
    project: &Project,
    node_ids: &[Uuid],
    ranks: &HashMap<Uuid, usize>,
    column_origin_x: f32,
) -> BTreeMap<usize, NodeRankColumn> {
    let mut widths = BTreeMap::<usize, f32>::new();
    for node_id in node_ids {
        if project.get_node(*node_id).is_none() {
            continue;
        }
        let rank = ranks.get(node_id).copied().unwrap_or_default();
        let width = estimated_node_size(project, *node_id).x;
        widths
            .entry(rank)
            .and_modify(|column_width| *column_width = column_width.max(width))
            .or_insert(width);
    }
    let Some(max_rank) = widths.keys().next_back().copied() else {
        return BTreeMap::new();
    };
    let mut columns = BTreeMap::new();
    let mut x = column_origin_x;
    for rank in 0..=max_rank {
        let width = widths
            .get(&rank)
            .copied()
            .unwrap_or_else(estimated_node_width);
        columns.insert(rank, NodeRankColumn { x, width });
        x += width + AUTO_LAYOUT_COLUMN_GAP;
    }
    columns
}

pub(super) fn node_band_bounds(
    project: &Project,
    node_ids: &[Uuid],
    ranks: &HashMap<Uuid, usize>,
    rank_columns: &BTreeMap<usize, NodeRankColumn>,
) -> Option<NodeBandBounds> {
    let mut column_heights = BTreeMap::<usize, f32>::new();
    for node_id in node_ids {
        if project.get_node(*node_id).is_none() {
            continue;
        }
        let rank = ranks.get(node_id).copied().unwrap_or_default();
        let column_height = column_heights.entry(rank).or_default();
        if *column_height > 0.0 {
            *column_height += AUTO_LAYOUT_ROW_GAP;
        }
        *column_height += estimated_node_size(project, *node_id).y;
    }
    let min_rank = column_heights.keys().next().copied()?;
    let max_rank = column_heights.keys().next_back().copied()?;
    let min_column = rank_columns.get(&min_rank)?;
    let max_column = rank_columns.get(&max_rank)?;
    Some(NodeBandBounds {
        min_x: min_column.x,
        max_x: max_column.x + max_column.width,
        height: column_heights
            .values()
            .copied()
            .max_by(f32::total_cmp)
            .unwrap_or_default(),
    })
}

pub(super) fn layout_node_band(
    project: &Project,
    node_ids: &[Uuid],
    ranks: &HashMap<Uuid, usize>,
    rank_columns: &BTreeMap<usize, NodeRankColumn>,
    edges: &[LayoutEdge],
    placement: NodeBandPlacement<'_>,
) -> Option<NodeBandBounds> {
    let NodeBandPlacement {
        container_output_y,
        origin_y,
        positions,
    } = placement;
    let mut groups = BTreeMap::<usize, Vec<Uuid>>::new();
    for node_id in node_ids {
        if project.get_node(*node_id).is_some() {
            groups
                .entry(ranks.get(node_id).copied().unwrap_or_default())
                .or_default()
                .push(*node_id);
        }
    }
    for group in groups.values_mut() {
        group.sort_by(|left, right| {
            let left_y = project
                .get_node(*left)
                .map_or(0.0, |node| node.ui_position[1]);
            let right_y = project
                .get_node(*right)
                .map_or(0.0, |node| node.ui_position[1]);
            left_y.total_cmp(&right_y).then_with(|| left.cmp(right))
        });
    }

    let mut bounds = node_band_bounds(project, node_ids, ranks, rank_columns)?;
    for (rank, group) in &groups {
        pack_column(
            project,
            group,
            rank_columns.get(rank)?.x,
            origin_y,
            positions,
        );
    }

    // Median sweeps reduce crossings while the persisted connection order
    // provides a deterministic front-to-back tie-break for Merge inputs.
    for _ in 0..2 {
        for group in groups.values_mut() {
            reorder_column(project, group, edges, positions, true);
        }
        for group in groups.values_mut().rev() {
            reorder_column(project, group, edges, positions, false);
        }
    }

    for group in groups.values() {
        enforce_structural_pair_row(project, group, positions, origin_y);
    }

    // Align whole rank blocks after ordering. Forward alignment centers
    // fan-in targets; the reverse pass centers fan-out sources.
    for _ in 0..2 {
        for group in groups.values() {
            align_column(
                project,
                group,
                edges,
                positions,
                container_output_y,
                origin_y,
                true,
            );
        }
        for group in groups.values().rev() {
            align_column(
                project,
                group,
                edges,
                positions,
                container_output_y,
                origin_y,
                false,
            );
        }
    }

    bounds.height = node_ids
        .iter()
        .filter_map(|node_id| {
            positions
                .get(node_id)
                .map(|position| position[1] + estimated_node_size(project, *node_id).y - origin_y)
        })
        .max_by(f32::total_cmp)
        .unwrap_or(bounds.height);
    Some(bounds)
}

fn node_center_y(
    project: &Project,
    node_id: Uuid,
    positions: &BTreeMap<Uuid, [f32; 2]>,
) -> Option<f32> {
    positions
        .get(&node_id)
        .map(|position| position[1] + estimated_node_size(project, node_id).y * 0.5)
}

fn median(mut values: Vec<f32>) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f32::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        Some((values[middle - 1] + values[middle]) * 0.5)
    } else {
        values.get(middle).copied()
    }
}

fn neighbor_median(
    project: &Project,
    node_id: Uuid,
    edges: &[LayoutEdge],
    positions: &BTreeMap<Uuid, [f32; 2]>,
    predecessors: bool,
) -> Option<f32> {
    median(
        edges
            .iter()
            .filter_map(|edge| {
                let neighbor = if predecessors && edge.to == node_id {
                    Some(edge.from)
                } else if !predecessors && edge.from == node_id {
                    Some(edge.to)
                } else {
                    None
                }?;
                node_center_y(project, neighbor, positions)
            })
            .collect(),
    )
}

fn reorder_column(
    project: &Project,
    group: &mut [Uuid],
    edges: &[LayoutEdge],
    positions: &mut BTreeMap<Uuid, [f32; 2]>,
    predecessors: bool,
) {
    let Some(first) = group.first() else {
        return;
    };
    let Some(column_x) = positions.get(first).map(|position| position[0]) else {
        return;
    };
    let origin_y = group
        .iter()
        .filter_map(|node_id| positions.get(node_id).map(|position| position[1]))
        .min_by(f32::total_cmp)
        .unwrap_or_default();
    group.sort_by(|left, right| {
        let left_preference = neighbor_median(project, *left, edges, positions, predecessors)
            .or_else(|| node_center_y(project, *left, positions))
            .unwrap_or_default();
        let right_preference = neighbor_median(project, *right, edges, positions, predecessors)
            .or_else(|| node_center_y(project, *right, positions))
            .unwrap_or_default();
        left_preference
            .total_cmp(&right_preference)
            .then_with(|| {
                scoped_neighbor_order(
                    project,
                    *right,
                    right_preference,
                    edges,
                    positions,
                    predecessors,
                )
                .cmp(&scoped_neighbor_order(
                    project,
                    *left,
                    left_preference,
                    edges,
                    positions,
                    predecessors,
                ))
            })
            .then_with(|| {
                let left_y = project
                    .get_node(*left)
                    .map_or(0.0, |node| node.ui_position[1]);
                let right_y = project
                    .get_node(*right)
                    .map_or(0.0, |node| node.ui_position[1]);
                left_y.total_cmp(&right_y)
            })
            .then_with(|| left.cmp(right))
    });
    pack_column(project, group, column_x, origin_y, positions);
}

fn scoped_neighbor_order(
    project: &Project,
    node_id: Uuid,
    preferred_y: f32,
    edges: &[LayoutEdge],
    positions: &BTreeMap<Uuid, [f32; 2]>,
    predecessors: bool,
) -> Option<i64> {
    if predecessors {
        return None;
    }
    edges
        .iter()
        .filter(|edge| edge.from == node_id)
        .filter_map(|edge| {
            node_center_y(project, edge.to, positions)
                .map(|center| ((center - preferred_y).abs(), edge.to, edge.order))
        })
        .min_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| right.2.cmp(&left.2))
        })
        .map(|(_, _, order)| order)
}

fn align_column(
    project: &Project,
    group: &[Uuid],
    edges: &[LayoutEdge],
    positions: &mut BTreeMap<Uuid, [f32; 2]>,
    container_output_y: &HashMap<PortOwner, f32>,
    origin_y: f32,
    predecessors: bool,
) {
    let targeted = group
        .iter()
        .filter_map(|node_id| {
            merge_anchor_aligned_top(project, *node_id, positions, container_output_y)
                .map(|target| (*node_id, target))
        })
        .collect::<HashMap<_, _>>();
    if !targeted.is_empty() {
        if predecessors {
            pack_targeted_column(project, group, &targeted, positions, origin_y);
        }
        return;
    }
    let desired = median(
        group
            .iter()
            .filter_map(|node_id| {
                neighbor_median(project, *node_id, edges, positions, predecessors)
            })
            .collect(),
    );
    let current = median(
        group
            .iter()
            .filter_map(|node_id| node_center_y(project, *node_id, positions))
            .collect(),
    );
    let (Some(desired), Some(current)) = (desired, current) else {
        return;
    };
    let top = group
        .iter()
        .filter_map(|node_id| positions.get(node_id).map(|position| position[1]))
        .min_by(f32::total_cmp)
        .unwrap_or(origin_y);
    let delta = (desired - current).max(origin_y - top);
    for node_id in group {
        if let Some(position) = positions.get_mut(node_id) {
            position[1] += delta;
        }
    }
}

pub(super) fn first_free_y(
    x: f32,
    width: f32,
    height: f32,
    initial_y: f32,
    occupied: &[egui::Rect],
    gap: f32,
) -> f32 {
    let mut y = initial_y;
    loop {
        let candidate = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(width, height));
        let next_y = occupied
            .iter()
            .filter(|other| rects_are_closer_than(candidate, **other, gap))
            .map(|other| other.bottom() + gap)
            .max_by(f32::total_cmp);
        let Some(next_y) = next_y else {
            return y;
        };
        y = next_y;
    }
}

pub(in crate::ui::panels::node_editor) fn rects_are_closer_than(
    left: egui::Rect,
    right: egui::Rect,
    gap: f32,
) -> bool {
    left.left() < right.right() + gap
        && left.right() + gap > right.left()
        && left.top() < right.bottom() + gap
        && left.bottom() + gap > right.top()
}

pub(in crate::ui::panels::node_editor) fn canonical_edges(
    project: &Project,
    nodes: &[Uuid],
) -> Vec<(Uuid, Uuid)> {
    let mut edges = collect_layout_edges(project, nodes, false)
        .into_iter()
        .map(|edge| (edge.from, edge.to))
        .collect::<Vec<_>>();
    edges.sort_unstable();
    edges.dedup();
    edges
}

pub(super) fn canonical_layout_edges(project: &Project, nodes: &[Uuid]) -> Vec<LayoutEdge> {
    collect_layout_edges(project, nodes, true)
}

fn collect_layout_edges(
    project: &Project,
    nodes: &[Uuid],
    include_complete_container_bounds: bool,
) -> Vec<LayoutEdge> {
    let node_set = nodes.iter().copied().collect::<HashSet<_>>();
    let mut edges = project
        .connections
        .iter()
        .flat_map(|connection| {
            let PortOwner::Node(to) = connection.to.owner else {
                return Vec::new();
            };
            if !node_set.contains(&to) || project.get_node(to).is_none() {
                return Vec::new();
            }

            let container_source = !matches!(connection.from.owner, PortOwner::Node(_));
            let sources = match connection.from.owner {
                PortOwner::Node(from) => vec![from],
                owner
                    if connection.from.port == IMAGE_OUTPUT_PORT
                        || connection.from.port == AUDIO_OUTPUT_PORT =>
                {
                    if include_complete_container_bounds {
                        container_layout_node_ids(project, owner)
                    } else {
                        container_layout_output_nodes(project, owner, &connection.from.port)
                    }
                }
                _ => Vec::new(),
            };
            sources
                .into_iter()
                .filter(|from| {
                    node_set.contains(from) && project.get_node(*from).is_some() && *from != to
                })
                .map(|from| LayoutEdge {
                    from,
                    to,
                    order: connection.order,
                    container_source,
                    connection_id: connection.id,
                })
                .collect()
        })
        .collect::<Vec<_>>();
    edges.sort_by_key(|edge| {
        (
            edge.from,
            edge.to,
            std::cmp::Reverse(edge.order),
            edge.connection_id,
        )
    });
    edges.dedup_by_key(|edge| (edge.from, edge.to));
    edges
}

fn container_layout_node_ids(project: &Project, owner: PortOwner) -> Vec<Uuid> {
    match owner {
        PortOwner::Node(id) => vec![id],
        PortOwner::Clip(id) => project
            .get_clip(id)
            .map_or_else(Vec::new, |clip| clip.node_ids.clone()),
        PortOwner::Track(id) => project.get_track(id).map_or_else(Vec::new, |track| {
            let mut nodes = track.node_ids.clone();
            for clip_id in &track.clip_ids {
                if let Some(clip) = project.get_clip(*clip_id) {
                    nodes.extend(clip.node_ids.iter().copied());
                }
            }
            nodes
        }),
        PortOwner::Composition(id) => {
            project
                .get_composition(id)
                .map_or_else(Vec::new, |composition| {
                    let mut nodes = composition.node_ids.clone();
                    for track_id in &composition.track_ids {
                        nodes.extend(container_layout_node_ids(
                            project,
                            PortOwner::Track(*track_id),
                        ));
                    }
                    nodes
                })
        }
    }
}

fn container_layout_output_nodes(project: &Project, owner: PortOwner, port: &str) -> Vec<Uuid> {
    match owner {
        PortOwner::Node(id) => vec![id],
        owner if port == IMAGE_OUTPUT_PORT => project
            .container_image_sources(owner)
            .into_iter()
            .flat_map(|source| container_layout_output_nodes(project, source.source, port))
            .collect(),
        owner if port == AUDIO_OUTPUT_PORT => project
            .container_audio_sources(owner)
            .into_iter()
            .flat_map(|source| container_layout_output_nodes(project, source.source, port))
            .collect(),
        _ => Vec::new(),
    }
}

pub(in crate::ui::panels::node_editor) fn rank_nodes_by_scc(
    nodes: &[Uuid],
    edges: &[(Uuid, Uuid)],
) -> HashMap<Uuid, usize> {
    let node_set = nodes.iter().copied().collect::<HashSet<_>>();
    let mut adjacency = HashMap::<Uuid, Vec<Uuid>>::new();
    for node_id in nodes {
        adjacency.entry(*node_id).or_default();
    }
    for (from, to) in edges {
        if node_set.contains(from) && node_set.contains(to) {
            adjacency.entry(*from).or_default().push(*to);
        }
    }
    for successors in adjacency.values_mut() {
        successors.sort_unstable();
        successors.dedup();
    }

    let mut sorted_nodes = nodes.to_vec();
    sorted_nodes.sort_unstable();
    sorted_nodes.dedup();
    let mut next_index = 0;
    let mut indices = HashMap::<Uuid, usize>::new();
    let mut lowlinks = HashMap::<Uuid, usize>::new();
    let mut stack = Vec::new();
    let mut on_stack = HashSet::new();
    let mut components = Vec::<Vec<Uuid>>::new();
    for node_id in sorted_nodes {
        if !indices.contains_key(&node_id) {
            visit_scc(
                node_id,
                &adjacency,
                &mut next_index,
                &mut indices,
                &mut lowlinks,
                &mut stack,
                &mut on_stack,
                &mut components,
            );
        }
    }
    for component in &mut components {
        component.sort_unstable();
    }
    components.sort_by_key(|component| component.first().copied());

    let mut component_by_node = HashMap::new();
    for (component_index, component) in components.iter().enumerate() {
        for node_id in component {
            component_by_node.insert(*node_id, component_index);
        }
    }
    let mut outgoing = vec![BTreeSet::<usize>::new(); components.len()];
    let mut indegree = vec![0_usize; components.len()];
    for (from, to) in edges {
        let (Some(&from_component), Some(&to_component)) =
            (component_by_node.get(from), component_by_node.get(to))
        else {
            continue;
        };
        if from_component != to_component && outgoing[from_component].insert(to_component) {
            indegree[to_component] += 1;
        }
    }

    let mut ready = BTreeSet::<(Uuid, usize)>::new();
    for (index, component) in components.iter().enumerate() {
        if indegree[index] == 0 {
            if let Some(first) = component.first() {
                ready.insert((*first, index));
            }
        }
    }
    let mut component_rank = vec![0_usize; components.len()];
    while let Some(entry) = ready.iter().next().copied() {
        ready.remove(&entry);
        let component = entry.1;
        for successor in outgoing[component].iter().copied() {
            component_rank[successor] =
                component_rank[successor].max(component_rank[component] + 1);
            indegree[successor] -= 1;
            if indegree[successor] == 0 {
                ready.insert((components[successor][0], successor));
            }
        }
    }

    component_by_node
        .into_iter()
        .map(|(node_id, component)| (node_id, component_rank[component]))
        .collect()
}

#[allow(
    clippy::too_many_arguments,
    reason = "Tarjan traversal state is intentionally explicit and stack-local"
)]
fn visit_scc(
    node_id: Uuid,
    adjacency: &HashMap<Uuid, Vec<Uuid>>,
    next_index: &mut usize,
    indices: &mut HashMap<Uuid, usize>,
    lowlinks: &mut HashMap<Uuid, usize>,
    stack: &mut Vec<Uuid>,
    on_stack: &mut HashSet<Uuid>,
    components: &mut Vec<Vec<Uuid>>,
) {
    let index = *next_index;
    *next_index += 1;
    indices.insert(node_id, index);
    lowlinks.insert(node_id, index);
    stack.push(node_id);
    on_stack.insert(node_id);

    for successor in adjacency.get(&node_id).into_iter().flatten().copied() {
        if !indices.contains_key(&successor) {
            visit_scc(
                successor, adjacency, next_index, indices, lowlinks, stack, on_stack, components,
            );
            let successor_lowlink = lowlinks[&successor];
            lowlinks
                .entry(node_id)
                .and_modify(|lowlink| *lowlink = (*lowlink).min(successor_lowlink));
        } else if on_stack.contains(&successor) {
            let successor_index = indices[&successor];
            lowlinks
                .entry(node_id)
                .and_modify(|lowlink| *lowlink = (*lowlink).min(successor_index));
        }
    }

    if lowlinks[&node_id] == indices[&node_id] {
        let mut component = Vec::new();
        while let Some(member) = stack.pop() {
            on_stack.remove(&member);
            component.push(member);
            if member == node_id {
                break;
            }
        }
        components.push(component);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::project::NodeContainer;
    use library::model::{Clip, Composition};

    fn layered_positions(
        node_y: &[(Uuid, f32)],
        edge_specs: &[(Uuid, Uuid, i64)],
    ) -> (Project, BTreeMap<Uuid, [f32; 2]>) {
        let mut project = Project::new("layered positions");
        for (node_id, y) in node_y {
            let mut node = Node::new_merge("Node");
            node.id = *node_id;
            node.ui_position = [0.0, *y];
            project.add_node(node);
        }
        let edges = edge_specs
            .iter()
            .enumerate()
            .map(|(index, (from, to, order))| LayoutEdge {
                from: *from,
                to: *to,
                order: *order,
                container_source: false,
                connection_id: Uuid::from_u128(10_000 + index as u128),
            })
            .collect::<Vec<_>>();
        let edge_pairs = edges
            .iter()
            .map(|edge| (edge.from, edge.to))
            .collect::<Vec<_>>();
        let node_ids = node_y
            .iter()
            .map(|(node_id, _)| *node_id)
            .collect::<Vec<_>>();
        let ranks = rank_nodes_by_scc(&node_ids, &edge_pairs);
        let columns = node_rank_columns(&project, &node_ids, &ranks, 20.0);
        let mut positions = BTreeMap::new();
        assert!(layout_node_band(
            &project,
            &node_ids,
            &ranks,
            &columns,
            &edges,
            NodeBandPlacement {
                container_output_y: &HashMap::new(),
                origin_y: 40.0,
                positions: &mut positions,
            },
        )
        .is_some());
        (project, positions)
    }

    fn center_y(project: &Project, positions: &BTreeMap<Uuid, [f32; 2]>, node_id: Uuid) -> f32 {
        positions[&node_id][1] + estimated_node_size(project, node_id).y * 0.5
    }

    #[test]
    fn layered_layout_aligns_chain_fan_in_and_fan_out_medians() {
        let ids = (1..=7).map(Uuid::from_u128).collect::<Vec<_>>();
        let (chain_project, chain) = layered_positions(
            &[(ids[0], 300.0), (ids[1], 20.0), (ids[2], 170.0)],
            &[(ids[0], ids[1], 0), (ids[1], ids[2], 0)],
        );
        assert_eq!(
            center_y(&chain_project, &chain, ids[0]),
            center_y(&chain_project, &chain, ids[1])
        );
        assert_eq!(
            center_y(&chain_project, &chain, ids[1]),
            center_y(&chain_project, &chain, ids[2])
        );

        let (fan_in_project, fan_in) = layered_positions(
            &[(ids[0], 200.0), (ids[1], 10.0), (ids[2], 0.0)],
            &[(ids[0], ids[2], 0), (ids[1], ids[2], 1)],
        );
        let source_median = (center_y(&fan_in_project, &fan_in, ids[0])
            + center_y(&fan_in_project, &fan_in, ids[1]))
            * 0.5;
        assert_eq!(center_y(&fan_in_project, &fan_in, ids[2]), source_median);
        assert!(fan_in[&ids[1]][1] < fan_in[&ids[0]][1]);

        let (fan_out_project, fan_out) = layered_positions(
            &[(ids[3], 300.0), (ids[4], 10.0), (ids[5], 200.0)],
            &[(ids[3], ids[4], 0), (ids[3], ids[5], 0)],
        );
        let target_median = (center_y(&fan_out_project, &fan_out, ids[4])
            + center_y(&fan_out_project, &fan_out, ids[5]))
            * 0.5;
        assert_eq!(center_y(&fan_out_project, &fan_out, ids[3]), target_median);
    }

    #[test]
    fn layered_layout_is_deterministic_and_columns_do_not_overlap() {
        let a = Uuid::from_u128(20);
        let b = Uuid::from_u128(21);
        let c = Uuid::from_u128(22);
        let d = Uuid::from_u128(23);
        let nodes = [(a, 400.0), (b, 20.0), (c, 200.0), (d, 60.0)];
        let edges = [(a, c, 0), (b, c, 1), (c, d, 0)];
        let (project, forward) = layered_positions(&nodes, &edges);
        let mut reversed_nodes = nodes;
        reversed_nodes.reverse();
        let mut reversed_edges = edges;
        reversed_edges.reverse();
        let (_, reversed) = layered_positions(&reversed_nodes, &reversed_edges);
        assert_eq!(forward, reversed);

        let mut rects = forward
            .iter()
            .map(|(node_id, position)| {
                egui::Rect::from_min_size(
                    egui::pos2(position[0], position[1]),
                    estimated_node_size(&project, *node_id),
                )
            })
            .collect::<Vec<_>>();
        rects.sort_by(|left, right| {
            left.left()
                .total_cmp(&right.left())
                .then_with(|| left.top().total_cmp(&right.top()))
        });
        assert!(rects
            .iter()
            .enumerate()
            .all(|(index, left)| rects[index + 1..]
                .iter()
                .all(|right| !left.intersects(*right))));
    }

    #[test]
    fn dag_ranking_is_stable_and_every_edge_moves_left_to_right() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let c = Uuid::from_u128(3);
        let d = Uuid::from_u128(4);
        let edges = vec![(a, b), (a, c), (b, d), (c, d)];
        let forward = rank_nodes_by_scc(&[d, b, a, c], &edges);
        let reverse = rank_nodes_by_scc(
            &[a, c, b, d],
            &edges.iter().rev().copied().collect::<Vec<_>>(),
        );

        assert_eq!(forward, reverse);
        assert!(edges.iter().all(|(from, to)| forward[from] < forward[to]));
    }

    #[test]
    fn structural_container_edges_rank_merges_after_complete_child_bounds() {
        let mut project = Project::new("structural layout");
        let (composition, track) = Composition::new("Main", 1_920, 1_080, 30.0, 5.0);
        let composition_id = composition.id;
        let composition_merge_id = composition.structural_merge_node_id;
        let track_id = track.id;
        let track_merge_id = track.structural_merge_node_id;
        assert!(project.add_track(track).is_ok());
        assert!(project.add_composition(composition).is_ok());

        let clip = Clip::new("Clip", 0.0, 5.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        assert!(project.attach_clip_to_track(track_id, clip_id).is_ok());

        let output = Node::new_merge("Clip output");
        let output_id = output.id;
        project.add_node(output);
        assert!(project
            .attach_node_to_container(NodeContainer::Clip(clip_id), output_id)
            .is_ok());
        let disconnected = Node::new_merge("Disconnected but inside bounds");
        let disconnected_id = disconnected.id;
        project.add_node(disconnected);
        assert!(project
            .attach_node_to_container(NodeContainer::Clip(clip_id), disconnected_id)
            .is_ok());
        assert!(project
            .set_output_node(NodeContainer::Clip(clip_id), Some(output_id))
            .is_ok());
        let authored_connections = project.connections.clone();

        let nodes = vec![
            composition_merge_id,
            disconnected_id,
            output_id,
            track_merge_id,
        ];
        let edges = canonical_edges(&project, &nodes);
        assert!(edges.contains(&(output_id, track_merge_id)));
        assert!(!edges.contains(&(disconnected_id, track_merge_id)));
        assert!(edges.contains(&(track_merge_id, composition_merge_id)));

        let ranks = rank_nodes_by_scc(&nodes, &edges);
        assert!(
            edges.iter().all(|(from, to)| ranks[from] < ranks[to]),
            "acyclic structural edges must never turn back: {edges:?} / {ranks:?}"
        );

        let columns = node_rank_columns(&project, &nodes, &ranks, 0.0);
        for (left_rank, right_rank) in [(0, 1), (1, 2)] {
            let left = columns[&left_rank];
            let right = columns[&right_rank];
            assert_eq!(right.x - (left.x + left.width), AUTO_LAYOUT_COLUMN_GAP);
        }
        let container_right_padding = crate::ui::panels::node_editor::AUTO_LAYOUT_TRACK_RIGHT;
        assert!(
            columns[&0].x + columns[&0].width + container_right_padding < columns[&1].x,
            "Clip output chrome must stay left of the Track Merge"
        );
        assert!(
            columns[&1].x + columns[&1].width + container_right_padding < columns[&2].x,
            "Track output chrome must stay left of the Composition Merge"
        );

        let plan = crate::ui::panels::node_editor::compute_full_composition_layout(
            &project,
            composition_id,
        );
        assert!(plan.is_some());
        let Some(plan) = plan else {
            return;
        };
        assert_eq!(project.connections, authored_connections);
        assert!(crate::ui::panels::node_editor::apply_auto_layout(
            &mut project,
            composition_id,
            &plan,
        ));
        let Some(clip) = project.get_clip(clip_id) else {
            return;
        };
        let clip_node_bottom = clip
            .node_ids
            .iter()
            .filter_map(|node_id| project.get_node(*node_id))
            .map(|node| node.ui_position[1] + estimated_node_size(&project, node.id).y)
            .max_by(f32::total_cmp)
            .unwrap_or(clip.ui_position[1]);
        assert!(clip
            .node_ids
            .iter()
            .filter_map(|node_id| project.get_node(*node_id))
            .all(|node| node.ui_position[1]
                >= clip.ui_position[1] + crate::ui::panels::node_editor::AUTO_LAYOUT_CLIP_TOP));
        assert_eq!(
            clip.ui_position[1] + clip.ui_size[1] - clip_node_bottom,
            crate::ui::panels::node_editor::AUTO_LAYOUT_TRACK_BOTTOM
        );
        let Some(track_merge) = project.get_node(track_merge_id) else {
            return;
        };
        assert!(
            clip.ui_position[0]
                + clip.ui_size[0]
                + crate::ui::panels::node_editor::AUTO_LAYOUT_NODE_PADDING
                < track_merge.ui_position[0]
        );
        let Some(track) = project.get_track(track_id) else {
            return;
        };
        let Some(composition_merge) = project.get_node(composition_merge_id) else {
            return;
        };
        assert!(
            track.ui_position[0]
                + track.ui_size[0]
                + crate::ui::panels::node_editor::AUTO_LAYOUT_NODE_PADDING
                < composition_merge.ui_position[0]
        );
        let Some(composition) = project.get_composition(composition_id) else {
            return;
        };
        assert_eq!(
            composition_merge.ui_position[1],
            composition.ui_position[1]
                + crate::ui::panels::node_editor::AUTO_LAYOUT_COMPOSITION_TOP,
            "row-anchor optimum is clamped to the owning container, not bottom-biased"
        );
        assert_eq!(project.connections, authored_connections);
    }
}
