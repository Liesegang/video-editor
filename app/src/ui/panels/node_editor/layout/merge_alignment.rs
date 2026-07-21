use library::model::project::{PortAddress, PortOwner};
use library::model::{Node, NodeContent, Project};
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

use super::node_geometry::estimated_node_size;
use super::ranking::{LayoutEdge, NodeRankColumn};
use crate::ui::panels::node_editor::types::{
    AUTO_LAYOUT_CONTAINER_SOURCE_GAP, MERGE_OUTPUT_FIRST_ROW_Y, NODE_OUTPUT_FIRST_ROW_Y,
};
use crate::ui::panels::node_editor::{
    estimated_merge_input_anchor_offset, merge_layer_rows, output_definitions, GraphItem,
    AUTO_LAYOUT_COLUMN_GAP, AUTO_LAYOUT_ROW_GAP, PORT_ROW_HEIGHT,
};

pub(super) fn pack_targeted_column(
    project: &Project,
    group: &[Uuid],
    targets: &HashMap<Uuid, f32>,
    positions: &mut BTreeMap<Uuid, [f32; 2]>,
    origin_y: f32,
) {
    let mut cursor = origin_y;
    for node_id in group {
        let current = positions
            .get(node_id)
            .map_or(origin_y, |position| position[1]);
        let target = targets.get(node_id).copied().unwrap_or(current);
        let y = target.max(cursor);
        if let Some(position) = positions.get_mut(node_id) {
            position[1] = y;
        }
        cursor = y + estimated_node_size(project, *node_id).y + AUTO_LAYOUT_ROW_GAP;
    }
}

pub(super) fn enforce_layout_edge_clearance(
    columns: &mut BTreeMap<usize, NodeRankColumn>,
    ranks: &HashMap<Uuid, usize>,
    edges: &[LayoutEdge],
) {
    let mut constraints = edges
        .iter()
        .filter_map(|edge| {
            let from_rank = *ranks.get(&edge.from)?;
            let to_rank = *ranks.get(&edge.to)?;
            (from_rank < to_rank).then_some((to_rank, from_rank, edge.container_source))
        })
        .collect::<Vec<_>>();
    constraints.sort_unstable();
    for (to_rank, from_rank, container_source) in constraints {
        let (Some(from), Some(to)) = (columns.get(&from_rank), columns.get(&to_rank)) else {
            continue;
        };
        let gap = if container_source {
            AUTO_LAYOUT_CONTAINER_SOURCE_GAP
        } else {
            AUTO_LAYOUT_COLUMN_GAP
        };
        let shift = (from.x + from.width + gap - to.x).max(0.0);
        if shift <= f32::EPSILON {
            continue;
        }
        for (_, column) in columns.range_mut(to_rank..) {
            column.x += shift;
        }
    }
}

pub(super) fn merge_anchor_aligned_top(
    project: &Project,
    merge_id: Uuid,
    positions: &BTreeMap<Uuid, [f32; 2]>,
    container_output_y: &HashMap<PortOwner, f32>,
) -> Option<f32> {
    median(
        merge_layer_rows(project, merge_id)
            .into_iter()
            .filter_map(|row| {
                estimated_source_output_y(project, &row.source, positions, container_output_y).map(
                    |source_y| {
                        source_y - estimated_merge_input_anchor_offset(row.front_to_back_index)
                    },
                )
            })
            .collect(),
    )
}

fn estimated_source_output_y(
    project: &Project,
    source: &PortAddress,
    positions: &BTreeMap<Uuid, [f32; 2]>,
    container_output_y: &HashMap<PortOwner, f32>,
) -> Option<f32> {
    match source.owner {
        PortOwner::Node(node_id) => {
            let top = positions
                .get(&node_id)
                .copied()
                .or_else(|| project.get_node(node_id).map(|node| node.ui_position))?[1];
            let index = output_definitions(project, GraphItem::Node(node_id))
                .iter()
                .position(|definition| definition.key == source.port)?;
            let first = if matches!(
                project.get_node(node_id).map(Node::content),
                Some(NodeContent::Merge)
            ) {
                MERGE_OUTPUT_FIRST_ROW_Y
            } else {
                NODE_OUTPUT_FIRST_ROW_Y
            };
            Some(top + first + index as f32 * PORT_ROW_HEIGHT)
        }
        owner => container_output_y.get(&owner).copied(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::project::{PortAddress, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT};

    fn merge_with_sources(tops_back_to_front: &[f32]) -> (Project, Uuid, BTreeMap<Uuid, [f32; 2]>) {
        let mut project = Project::new("Merge alignment");
        let mut target = Node::new_merge("Target");
        target.id = Uuid::from_u128(1_000);
        let target_id = target.id;
        project.add_node(target);
        let mut positions = BTreeMap::new();
        for (index, top) in tops_back_to_front.iter().copied().enumerate() {
            let mut source = Node::new_merge("Source");
            source.id = Uuid::from_u128(2_000 + index as u128);
            let source_id = source.id;
            project.add_node(source);
            positions.insert(source_id, [0.0, top]);
            assert!(project
                .connect_ports(
                    PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
                    PortAddress::new(PortOwner::Node(target_id), MERGE_IMAGES_PORT),
                )
                .is_ok());
        }
        positions.insert(target_id, [600.0, 0.0]);
        (project, target_id, positions)
    }

    #[test]
    fn merge_top_uses_physical_row_offsets_for_one_two_and_three_inputs() {
        for (tops, expected) in [
            (vec![100.0], 45.0),
            (vec![600.0, 100.0], 267.5),
            (vec![900.0, 500.0, 100.0], 390.0),
        ] {
            let (project, merge_id, positions) = merge_with_sources(&tops);
            assert_eq!(
                merge_anchor_aligned_top(&project, merge_id, &positions, &HashMap::new()),
                Some(expected)
            );
        }
    }

    #[test]
    fn targeted_same_rank_nodes_are_clamped_and_packed_without_overlap() {
        let (mut project, first, mut positions) = merge_with_sources(&[100.0]);
        let mut second = Node::new_merge("Second target");
        second.id = Uuid::from_u128(1_001);
        let second_id = second.id;
        project.add_node(second);
        positions.insert(second_id, [600.0, 0.0]);
        let targets = HashMap::from([(first, -80.0), (second_id, 20.0)]);
        pack_targeted_column(
            &project,
            &[first, second_id],
            &targets,
            &mut positions,
            40.0,
        );
        assert_eq!(positions[&first][1], 40.0);
        assert!(
            positions[&second_id][1]
                >= positions[&first][1]
                    + estimated_node_size(&project, first).y
                    + AUTO_LAYOUT_ROW_GAP
        );
    }

    #[test]
    fn container_source_clearance_preserves_compact_node_gap_and_actual_anchor_ltr() {
        let source = Uuid::from_u128(10);
        let target = Uuid::from_u128(11);
        let ranks = HashMap::from([(source, 0), (target, 1)]);
        let mut columns = BTreeMap::from([
            (
                0,
                NodeRankColumn {
                    x: 0.0,
                    width: 100.0,
                },
            ),
            (
                1,
                NodeRankColumn {
                    x: 136.0,
                    width: 100.0,
                },
            ),
        ]);
        enforce_layout_edge_clearance(
            &mut columns,
            &ranks,
            &[LayoutEdge {
                from: source,
                to: target,
                order: 0,
                container_source: true,
                connection_id: Uuid::nil(),
            }],
        );
        assert_eq!(
            columns[&1].x - columns[&0].width,
            crate::ui::panels::node_editor::types::AUTO_LAYOUT_CONTAINER_SOURCE_GAP
        );
        let container_anchor_x = columns[&0].x
            + columns[&0].width
            + crate::ui::panels::node_editor::AUTO_LAYOUT_TRACK_RIGHT;
        let conservative_input_x =
            columns[&1].x - crate::ui::panels::node_editor::AUTO_LAYOUT_NODE_PADDING;
        assert!(container_anchor_x <= conservative_input_x);
    }
}
