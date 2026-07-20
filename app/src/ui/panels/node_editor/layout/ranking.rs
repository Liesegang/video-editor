use eframe::egui;
use library::model::project::PortOwner;
use library::model::{GeneratorContent, Node, NodeContent, Project};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use uuid::Uuid;

use crate::ui::panels::node_editor::{
    input_definitions, merge_layer_rows, output_definitions, GraphItem, AUTO_LAYOUT_COLUMN_GAP,
    AUTO_LAYOUT_ROW_GAP, MERGE_BODY_WIDTH, NODE_BODY_WIDTH, NODE_HEADER_WIDTH, PORT_LABEL_WIDTH,
    PORT_ROW_HEIGHT,
};

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

impl NodeBandBounds {
    pub(super) fn width(self) -> f32 {
        self.max_x - self.min_x
    }
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
    origin_y: f32,
    positions: &mut BTreeMap<Uuid, [f32; 2]>,
) -> Option<NodeBandBounds> {
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

    let bounds = node_band_bounds(project, node_ids, ranks, rank_columns)?;
    for (rank, group) in groups {
        let x = rank_columns.get(&rank)?.x;
        let mut y = origin_y;
        for node_id in group {
            let size = estimated_node_size(project, node_id);
            positions.insert(node_id, [x, y]);
            y += size.y + AUTO_LAYOUT_ROW_GAP;
        }
    }
    Some(bounds)
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
    let node_set = nodes.iter().copied().collect::<HashSet<_>>();
    let mut edges = project
        .connections
        .iter()
        .filter_map(|connection| {
            let (PortOwner::Node(from), PortOwner::Node(to)) =
                (connection.from.owner, connection.to.owner)
            else {
                return None;
            };
            (node_set.contains(&from)
                && node_set.contains(&to)
                && project.get_node(from).is_some()
                && project.get_node(to).is_some())
            .then_some((from, to))
        })
        .collect::<Vec<_>>();
    edges.sort_unstable();
    edges.dedup();
    edges
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

pub(in crate::ui::panels::node_editor) fn estimated_node_width() -> f32 {
    (NODE_BODY_WIDTH + PORT_LABEL_WIDTH * 2.0 + 70.0).max(NODE_HEADER_WIDTH + 30.0)
}

pub(in crate::ui::panels::node_editor) fn estimated_merge_node_width() -> f32 {
    (MERGE_BODY_WIDTH + PORT_LABEL_WIDTH * 2.0 + 84.0).max(NODE_HEADER_WIDTH + 30.0)
}

pub(in crate::ui::panels::node_editor) fn estimated_node_size(
    project: &Project,
    node_id: Uuid,
) -> egui::Vec2 {
    let item = GraphItem::Node(node_id);
    let pin_rows = input_definitions(project, item)
        .len()
        .max(output_definitions(project, item).len());
    // These are conservative graph-space bounds for the complete rendered
    // card (header, pin rows and body controls), not just the body widget.
    // The extra pin term keeps plugin Nodes with unusually many ports safe.
    let content = project.get_node(node_id).map(Node::content);
    let base_height = match content {
        Some(NodeContent::Generator(GeneratorContent::Text)) => 330.0,
        Some(NodeContent::Generator(GeneratorContent::Shape))
        | Some(NodeContent::Generator(GeneratorContent::SkSL)) => 300.0,
        Some(NodeContent::Generator(GeneratorContent::Solid)) => 240.0,
        Some(NodeContent::PluginOperation(_)) => 260.0,
        Some(NodeContent::Merge) => {
            let layer_count = merge_layer_rows(project, node_id).len();
            (166.0 + layer_count as f32 * 82.0).max(220.0)
        }
        Some(NodeContent::Media(_) | NodeContent::Reference(_) | NodeContent::Value(_)) => 220.0,
        None => 220.0,
    };
    egui::vec2(
        if matches!(content, Some(NodeContent::Merge)) {
            estimated_merge_node_width()
        } else {
            estimated_node_width()
        },
        base_height + pin_rows.saturating_sub(4) as f32 * PORT_ROW_HEIGHT,
    )
}
