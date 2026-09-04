use library::model::project::{PortAddress, PortOwner};
use library::model::{Node, NodeContent, Project};
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

use super::column_packing::enforce_structural_pair_order;
use super::node_geometry::estimated_node_size;
use super::ranking::{LayoutEdge, NodeRankColumn};
use crate::ui::panels::node_editor::types::{
    AUTO_LAYOUT_CONTAINER_SOURCE_GAP, CONTAINER_RIGHT_PORT_ROW_HEIGHT, CONTAINER_RIGHT_PORT_Y,
    MERGE_OUTPUT_FIRST_ROW_Y, NODE_OUTPUT_FIRST_ROW_Y,
};
use crate::ui::panels::node_editor::{
    AUTO_LAYOUT_COLUMN_GAP, AUTO_LAYOUT_ROW_GAP, GraphItem, PORT_ROW_HEIGHT, PortAnchorKind,
    estimated_merge_input_anchor_offset, merge_layer_rows, output_definitions,
};

pub(super) fn pack_targeted_column(
    project: &Project,
    group: &[Uuid],
    targets: &HashMap<Uuid, f32>,
    positions: &mut BTreeMap<Uuid, [f32; 2]>,
    origin_y: f32,
) {
    // Packing is a constrained projection: keep the ideal top-anchor order,
    // then move each node downward only far enough to clear its predecessor.
    let mut ordered = group.to_vec();
    ordered.sort_by(|left, right| {
        let desired = |node_id: &Uuid| {
            targets.get(node_id).copied().unwrap_or_else(|| {
                positions
                    .get(node_id)
                    .map_or(origin_y, |position| position[1])
            })
        };
        desired(left)
            .total_cmp(&desired(right))
            .then_with(|| left.cmp(right))
    });
    enforce_structural_pair_order(project, group, &mut ordered);

    let mut cursor = origin_y;
    for node_id in ordered {
        let current = positions
            .get(&node_id)
            .map_or(origin_y, |position| position[1]);
        let target = targets.get(&node_id).copied().unwrap_or(current);
        let y = target.max(cursor);
        if let Some(position) = positions.get_mut(&node_id) {
            position[1] = y;
        }
        cursor = y + estimated_node_size(project, node_id).y + AUTO_LAYOUT_ROW_GAP;
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
    // Merge rows are projected front-to-back. Anchoring the front-most source
    // to row zero gives 1, 2, and N inputs the same deterministic vertical
    // rule; a median would align neither row for an even layer count.
    let front = merge_layer_rows(project, merge_id).into_iter().next()?;
    estimated_source_output_y(project, &front.source, positions, container_output_y)
        .map(|source_y| source_y - estimated_merge_input_anchor_offset(front.visual_index))
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
        owner => container_output_y
            .get(&owner)
            .copied()
            .or_else(|| authored_container_output_y(project, source)),
    }
}

fn authored_container_output_y(project: &Project, source: &PortAddress) -> Option<f32> {
    let top = match source.owner {
        PortOwner::Composition(id) => project
            .get_composition(id)
            .map(|composition| composition.ui_position[1]),
        PortOwner::Track(id) => project.get_track(id).map(|track| track.ui_position[1]),
        PortOwner::Clip(id) => project.get_clip(id).map(|clip| clip.ui_position[1]),
        PortOwner::Node(_) => None,
    }?;
    let index = output_definitions(
        project,
        GraphItem::PortAnchor {
            owner: source.owner,
            kind: PortAnchorKind::ExternalOutputs,
        },
    )
    .iter()
    .position(|definition| definition.key == source.port)?;
    Some(top + CONTAINER_RIGHT_PORT_Y + index as f32 * CONTAINER_RIGHT_PORT_ROW_HEIGHT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::project::{IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, PortAddress};

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
            assert!(
                project
                    .connect_ports(
                        PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
                        PortAddress::new(PortOwner::Node(target_id), MERGE_IMAGES_PORT),
                    )
                    .is_ok()
            );
        }
        positions.insert(target_id, [600.0, 0.0]);
        (project, target_id, positions)
    }

    #[test]
    fn merge_top_aligns_the_frontmost_source_to_the_top_row_for_one_two_and_three_inputs() {
        for (tops, expected) in [
            (vec![100.0], 45.0),
            (vec![600.0, 100.0], 45.0),
            (vec![900.0, 500.0, 100.0], 45.0),
        ] {
            let (project, merge_id, positions) = merge_with_sources(&tops);
            assert_eq!(
                merge_anchor_aligned_top(&project, merge_id, &positions, &HashMap::new()),
                Some(expected)
            );
        }
    }

    #[test]
    fn targeted_same_rank_nodes_pack_by_desired_anchor_then_uuid() {
        let (mut project, first, mut positions) = merge_with_sources(&[100.0]);
        let mut second = Node::new_merge("Second target");
        second.id = Uuid::from_u128(1_001);
        let second_id = second.id;
        project.add_node(second);
        positions.insert(second_id, [600.0, 0.0]);
        let targets = HashMap::from([(first, -80.0), (second_id, 20.0)]);
        pack_targeted_column(
            &project,
            &[second_id, first],
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
    fn targeted_same_rank_nodes_break_equal_anchor_ties_by_uuid() {
        let (mut project, first, mut positions) = merge_with_sources(&[100.0]);
        let mut second = Node::new_merge("Second target");
        second.id = Uuid::from_u128(1_001);
        let second_id = second.id;
        project.add_node(second);
        positions.insert(second_id, [600.0, 0.0]);
        let targets = HashMap::from([(first, 40.0), (second_id, 40.0)]);

        pack_targeted_column(
            &project,
            &[second_id, first],
            &targets,
            &mut positions,
            40.0,
        );

        assert_eq!(positions[&first][1], 40.0);
        assert!(positions[&second_id][1] > positions[&first][1]);
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
