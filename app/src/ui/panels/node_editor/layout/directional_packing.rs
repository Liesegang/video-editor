//! Vertical packing for directional graph-layout ranks.
//!
//! Horizontal graph flow is final before this module runs. Each rank is then
//! treated as one semantic block, so resolving an obstacle cannot invert a
//! variadic input order or move only a suffix of that rank.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

use library::model::{NodeContainer, Project};
use uuid::Uuid;

use crate::ui::panels::node_editor::{
    AUTO_LAYOUT_CLIP_TOP, AUTO_LAYOUT_COMPOSITION_BOTTOM, AUTO_LAYOUT_COMPOSITION_TOP,
    AUTO_LAYOUT_TRACK_BOTTOM, AUTO_LAYOUT_TRACK_TOP,
};

use super::super::{immediate_child_rects, AutoLayoutPlan};
use super::graph::BranchGraph;
use super::{BranchDirection, DirectionalLayoutMode, DirectionalLayoutRequest, NodeLayoutGeometry};

#[derive(Clone, Copy)]
struct LevelBlockEntry {
    node_id: Uuid,
    x: f32,
    offset_y: f32,
    size: [f32; 2],
    preferred_y: f32,
}

#[derive(Clone, Copy)]
struct LayoutObstacle {
    geometry: NodeLayoutGeometry,
    legacy_geometry: NodeLayoutGeometry,
}

#[derive(Clone, Copy, Debug)]
struct OpenInterval {
    start: f32,
    end: f32,
}

#[allow(
    clippy::too_many_arguments,
    reason = "all arguments are immutable parts of one directional layout plan"
)]
pub(super) fn pack_layout_level_blocks(
    project: &Project,
    request: &DirectionalLayoutRequest<'_>,
    graph: &BranchGraph,
    depth: &HashMap<usize, usize>,
    eligible: &[Uuid],
    eligible_set: &HashSet<Uuid>,
    positions: &mut BTreeMap<Uuid, [f32; 2]>,
) -> Result<(), super::DirectionalLayoutError> {
    let owner_nodes = direct_owner_node_ids(project, request.direct_owner);
    let mut fixed_obstacles = owner_nodes
        .iter()
        .copied()
        .filter(|node_id| !eligible_set.contains(node_id))
        .filter_map(|node_id| valid_geometry(request, node_id))
        .map(|geometry| LayoutObstacle {
            geometry,
            legacy_geometry: geometry,
        })
        .collect::<Vec<_>>();
    let child_obstacles = child_obstacles(project, request.direct_owner);
    fixed_obstacles.extend(
        child_obstacles
            .iter()
            .copied()
            .map(|geometry| LayoutObstacle {
                geometry,
                legacy_geometry: geometry,
            }),
    );
    let owner_bounds = valid_owner_bounds(
        project,
        request,
        &owner_nodes,
        &child_obstacles,
        request.direct_owner,
    );

    let can_reflow_vertical = request.mode == DirectionalLayoutMode::Layout;
    let groups = level_groups(project, request, graph, depth, eligible);
    let mut placed = Vec::<LayoutObstacle>::new();
    for group in groups {
        let blocks = if can_reflow_vertical {
            vec![group]
        } else {
            group.into_iter().map(|node_id| vec![node_id]).collect()
        };
        for block in blocks {
            let entries = block_entries(request, positions, block, can_reflow_vertical);
            let Some(last) = entries.last() else {
                continue;
            };
            let block_height = last.offset_y + last.size[1];
            let mut obstacles = fixed_obstacles.clone();
            obstacles.extend(placed.iter().copied());
            let forbidden = merged_forbidden_intervals(request, &entries, &obstacles);
            let top = if can_reflow_vertical {
                choose_block_top(request, &entries, block_height, owner_bounds, &forbidden)
            } else {
                exact_constraint_top(&entries, owner_bounds, &forbidden)
                    .ok_or(super::DirectionalLayoutError::ConstraintCollision)?
            };
            for entry in entries {
                let position = [entry.x, top + entry.offset_y];
                positions.insert(entry.node_id, position);
                placed.push(LayoutObstacle {
                    geometry: NodeLayoutGeometry {
                        position,
                        size: entry.size,
                    },
                    legacy_geometry: request.node_geometry[&entry.node_id],
                });
            }
        }
    }
    Ok(())
}

fn valid_geometry(
    request: &DirectionalLayoutRequest<'_>,
    node_id: Uuid,
) -> Option<NodeLayoutGeometry> {
    request
        .node_geometry
        .get(&node_id)
        .copied()
        .filter(|geometry| geometry.is_valid())
}

fn child_obstacles(project: &Project, owner: NodeContainer) -> Vec<NodeLayoutGeometry> {
    immediate_child_rects(project, &AutoLayoutPlan::default(), owner)
        .into_iter()
        .map(|rect| NodeLayoutGeometry {
            position: [rect.min.x, rect.min.y],
            size: [rect.width(), rect.height()],
        })
        .collect()
}

fn valid_owner_bounds(
    project: &Project,
    request: &DirectionalLayoutRequest<'_>,
    owner_nodes: &[Uuid],
    child_obstacles: &[NodeLayoutGeometry],
    owner: NodeContainer,
) -> Option<[f32; 2]> {
    direct_owner_vertical_bounds(project, owner).filter(|[top, bottom]| {
        owner_nodes
            .iter()
            .filter_map(|node_id| request.node_geometry.get(node_id))
            .all(|geometry| vertically_contained(*geometry, *top, *bottom))
            && child_obstacles
                .iter()
                .all(|geometry| vertically_contained(*geometry, *top, *bottom))
    })
}

fn vertically_contained(geometry: NodeLayoutGeometry, top: f32, bottom: f32) -> bool {
    geometry.position[1] >= top && geometry.bottom() <= bottom
}

fn level_groups(
    project: &Project,
    request: &DirectionalLayoutRequest<'_>,
    graph: &BranchGraph,
    depth: &HashMap<usize, usize>,
    eligible: &[Uuid],
) -> Vec<Vec<Uuid>> {
    let mut ordered = eligible.to_vec();
    ordered.sort_by(|left, right| {
        let left_level = graph.level_for(*left, depth, request.direction);
        let right_level = graph.level_for(*right, depth, request.direction);
        directional_level_cmp(left_level, right_level, request.direction)
            .then_with(|| graph.semantic_order.compare(*left, *right))
            .then_with(|| {
                let left_name = project
                    .get_node(*left)
                    .map_or("", |node| node.name.as_str());
                let right_name = project
                    .get_node(*right)
                    .map_or("", |node| node.name.as_str());
                left_name.cmp(right_name)
            })
            .then_with(|| left.cmp(right))
    });
    let mut groups = Vec::<Vec<Uuid>>::new();
    let mut previous_level = None;
    for node_id in ordered {
        let level = graph.level_for(node_id, depth, request.direction);
        if previous_level != Some(level) {
            previous_level = Some(level);
            groups.push(Vec::new());
        }
        if let Some(group) = groups.last_mut() {
            group.push(node_id);
        }
    }
    groups
}

fn directional_level_cmp(left: i32, right: i32, direction: BranchDirection) -> Ordering {
    match direction {
        BranchDirection::Downstream => left.cmp(&right),
        BranchDirection::Upstream => right.cmp(&left),
    }
}

fn block_entries(
    request: &DirectionalLayoutRequest<'_>,
    positions: &BTreeMap<Uuid, [f32; 2]>,
    group: Vec<Uuid>,
    stack: bool,
) -> Vec<LevelBlockEntry> {
    let mut offset_y = 0.0;
    group
        .into_iter()
        .map(|node_id| {
            let geometry = request.node_geometry[&node_id];
            let preferred = positions
                .get(&node_id)
                .copied()
                .unwrap_or(geometry.position);
            let entry = LevelBlockEntry {
                node_id,
                x: preferred[0],
                offset_y,
                size: geometry.size,
                preferred_y: preferred[1],
            };
            if stack {
                offset_y += geometry.size[1] + request.vertical_gap;
            }
            entry
        })
        .collect()
}

/// Convert every horizontally relevant obstacle into a forbidden interval for
/// the block top once. Candidate evaluation then performs only binary searches
/// over the merged intervals instead of rescanning entries and obstacles.
fn merged_forbidden_intervals(
    request: &DirectionalLayoutRequest<'_>,
    entries: &[LevelBlockEntry],
    obstacles: &[LayoutObstacle],
) -> Vec<OpenInterval> {
    let mut intervals = Vec::new();
    for entry in entries {
        for obstacle in obstacles {
            if !intervals_overlap_with_gap(
                entry.x,
                entry.x + entry.size[0],
                obstacle.geometry.position[0],
                obstacle.geometry.right(),
                request.horizontal_gap,
            ) {
                continue;
            }
            let baseline = request.node_geometry[&entry.node_id];
            let baseline_horizontal =
                horizontal_penetration(baseline, obstacle.legacy_geometry, request.horizontal_gap);
            let baseline_vertical =
                vertical_penetration(baseline, obstacle.legacy_geometry, request.vertical_gap);
            let final_horizontal = horizontal_penetration(
                NodeLayoutGeometry {
                    position: [entry.x, 0.0],
                    size: entry.size,
                },
                NodeLayoutGeometry {
                    position: [obstacle.geometry.position[0], 0.0],
                    size: obstacle.geometry.size,
                },
                request.horizontal_gap,
            );
            let allowed_penetration = if request.mode != DirectionalLayoutMode::Layout
                && baseline_horizontal > 0.0
                && baseline_vertical > 0.0
                && final_horizontal <= baseline_horizontal + super::POSITION_EPSILON
            {
                baseline_vertical
            } else {
                0.0
            };
            intervals.push(OpenInterval {
                start: obstacle.geometry.position[1] - request.vertical_gap + allowed_penetration
                    - entry.size[1]
                    - entry.offset_y,
                end: obstacle.geometry.bottom() + request.vertical_gap
                    - allowed_penetration
                    - entry.offset_y,
            });
        }
    }
    intervals.retain(|interval| {
        interval.start.is_finite() && interval.end.is_finite() && interval.start < interval.end
    });
    intervals.sort_by(|left, right| {
        left.start
            .total_cmp(&right.start)
            .then_with(|| left.end.total_cmp(&right.end))
    });
    let mut merged = Vec::<OpenInterval>::new();
    for interval in intervals {
        match merged.last_mut() {
            Some(previous) if interval.start < previous.end => {
                previous.end = previous.end.max(interval.end);
            }
            _ => merged.push(interval),
        }
    }
    merged
}

fn exact_constraint_top(
    entries: &[LevelBlockEntry],
    owner_bounds: Option<[f32; 2]>,
    forbidden: &[OpenInterval],
) -> Option<f32> {
    let entry = entries.first()?;
    let top = entry.preferred_y;
    let bottom = top + entry.size[1];
    let inside = owner_bounds
        .is_none_or(|[owner_top, owner_bottom]| top >= owner_top && bottom <= owner_bottom);
    (inside && !is_forbidden(forbidden, top)).then_some(top)
}

fn choose_block_top(
    request: &DirectionalLayoutRequest<'_>,
    entries: &[LevelBlockEntry],
    block_height: f32,
    owner_bounds: Option<[f32; 2]>,
    forbidden: &[OpenInterval],
) -> f32 {
    let mut targets = entries
        .iter()
        .map(|entry| entry.preferred_y - entry.offset_y)
        .filter(|target| target.is_finite())
        .collect::<Vec<_>>();
    targets.sort_by(f32::total_cmp);
    let minimum_target = targets.first().copied().unwrap_or_default();
    let maximum_target = targets.last().copied().unwrap_or_default();
    let median_target = targets[targets.len() / 2];
    let mut candidates = targets;
    candidates.push(request.node_geometry[&request.anchor_node_id].position[1]);
    for interval in forbidden {
        candidates.push(interval.start);
        candidates.push(interval.end);
    }
    let bounded_range = owner_bounds.and_then(|[top, bottom]| {
        let maximum = bottom - block_height;
        (top <= maximum).then_some([top, maximum])
    });
    if let Some([minimum, maximum]) = bounded_range {
        candidates.push(minimum);
        candidates.push(maximum);
    }
    candidates.retain(|candidate| candidate.is_finite());
    candidates.sort_by(f32::total_cmp);
    candidates.dedup_by(|left, right| left.total_cmp(right) == Ordering::Equal);

    let within_bounds = |candidate: f32| {
        bounded_range.is_none_or(|[minimum, maximum]| candidate >= minimum && candidate <= maximum)
    };
    let mut valid = candidates
        .into_iter()
        .filter(|candidate| within_bounds(*candidate) && !is_forbidden(forbidden, *candidate))
        .collect::<Vec<_>>();
    if valid.is_empty() && bounded_range.is_some() {
        // A pre-v1 Project can contain a block taller than its authored owner.
        // Keep the edit usable while the commit layer grows only that owner.
        valid = forbidden
            .iter()
            .flat_map(|interval| [interval.start, interval.end])
            .chain([minimum_target, maximum_target])
            .filter(|candidate| !is_forbidden(forbidden, *candidate))
            .collect();
    }
    if owner_bounds.is_none() {
        // Invalid legacy bounds have no trustworthy upper edge. Preserve the
        // established downward collision fallback instead of unexpectedly
        // moving authored Nodes above their lane.
        if let Some(candidate) = valid
            .iter()
            .copied()
            .filter(|candidate| *candidate >= maximum_target)
            .min_by(f32::total_cmp)
        {
            return candidate;
        }
    }
    valid
        .into_iter()
        .min_by(|left, right| {
            candidate_score(*left, median_target, minimum_target, maximum_target)
                .partial_cmp(&candidate_score(
                    *right,
                    median_target,
                    minimum_target,
                    maximum_target,
                ))
                .unwrap_or(Ordering::Equal)
                .then_with(|| right.total_cmp(left))
        })
        .unwrap_or(maximum_target)
}

fn candidate_score(
    top: f32,
    median_target: f32,
    minimum_target: f32,
    maximum_target: f32,
) -> (
    ordered_float::OrderedFloat<f32>,
    ordered_float::OrderedFloat<f32>,
) {
    (
        ordered_float::OrderedFloat((top - median_target).abs()),
        ordered_float::OrderedFloat(
            (top - minimum_target)
                .abs()
                .max((top - maximum_target).abs()),
        ),
    )
}

fn is_forbidden(intervals: &[OpenInterval], candidate: f32) -> bool {
    let index = intervals.partition_point(|interval| interval.end <= candidate);
    intervals
        .get(index)
        .is_some_and(|interval| interval.start < candidate && candidate < interval.end)
}

fn direct_owner_vertical_bounds(project: &Project, owner: NodeContainer) -> Option<[f32; 2]> {
    let (position, size, top_margin, bottom_margin) = match owner {
        NodeContainer::Composition(id) => {
            let item = project.get_composition(id)?;
            (
                item.ui_position,
                item.ui_size,
                AUTO_LAYOUT_COMPOSITION_TOP,
                AUTO_LAYOUT_COMPOSITION_BOTTOM,
            )
        }
        NodeContainer::Track(id) => {
            let item = project.get_track(id)?;
            (
                item.ui_position,
                item.ui_size,
                AUTO_LAYOUT_TRACK_TOP,
                AUTO_LAYOUT_TRACK_BOTTOM,
            )
        }
        NodeContainer::Clip(id) => {
            let item = project.get_clip(id)?;
            (
                item.ui_position,
                item.ui_size,
                AUTO_LAYOUT_CLIP_TOP,
                AUTO_LAYOUT_TRACK_BOTTOM,
            )
        }
    };
    let bounds = [
        position[1] + top_margin,
        position[1] + size[1] - bottom_margin,
    ];
    (bounds.into_iter().all(f32::is_finite) && bounds[0] <= bounds[1]).then_some(bounds)
}

fn intervals_overlap_with_gap(
    left_start: f32,
    left_end: f32,
    right_start: f32,
    right_end: f32,
    gap: f32,
) -> bool {
    left_start < right_end + gap && left_end + gap > right_start
}

fn horizontal_penetration(
    node: NodeLayoutGeometry,
    obstacle: NodeLayoutGeometry,
    horizontal_gap: f32,
) -> f32 {
    (node.right() + horizontal_gap - obstacle.position[0])
        .min(obstacle.right() + horizontal_gap - node.position[0])
        .max(0.0)
}

fn vertical_penetration(
    node: NodeLayoutGeometry,
    obstacle: NodeLayoutGeometry,
    vertical_gap: f32,
) -> f32 {
    (node.bottom() + vertical_gap - obstacle.position[1])
        .min(obstacle.bottom() + vertical_gap - node.position[1])
        .max(0.0)
}

fn direct_owner_node_ids(project: &Project, owner: NodeContainer) -> Vec<Uuid> {
    match owner {
        NodeContainer::Composition(id) => project
            .get_composition(id)
            .map_or_else(Vec::new, |composition| composition.node_ids.clone()),
        NodeContainer::Track(id) => project
            .get_track(id)
            .map_or_else(Vec::new, |track| track.node_ids.clone()),
        NodeContainer::Clip(id) => project
            .get_clip(id)
            .map_or_else(Vec::new, |clip| clip.node_ids.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_interval_boundaries_are_valid() {
        let intervals = [OpenInterval {
            start: 10.0,
            end: 20.0,
        }];
        assert!(!is_forbidden(&intervals, 10.0));
        assert!(is_forbidden(&intervals, 15.0));
        assert!(!is_forbidden(&intervals, 20.0));
    }

    #[test]
    fn penetration_is_symmetric_for_unequal_rectangles() {
        let small = NodeLayoutGeometry {
            position: [0.0, 20.0],
            size: [10.0, 30.0],
        };
        let large = NodeLayoutGeometry {
            position: [0.0, 0.0],
            size: [100.0, 100.0],
        };
        assert_eq!(
            horizontal_penetration(small, large, 30.0),
            horizontal_penetration(large, small, 30.0)
        );
        assert_eq!(
            vertical_penetration(small, large, 20.0),
            vertical_penetration(large, small, 20.0)
        );
    }

    #[test]
    fn equal_legacy_penetration_produces_no_forbidden_interior() {
        let geometry = NodeLayoutGeometry {
            position: [0.0, 200.0],
            size: [100.0, 100.0],
        };
        let gap = 20.0;
        let allowed = vertical_penetration(geometry, geometry, gap);
        let interval = OpenInterval {
            start: geometry.position[1] - gap + allowed - geometry.size[1],
            end: geometry.bottom() + gap - allowed,
        };
        assert!(interval.start >= interval.end);

        let shallower_allowed = allowed - 20.0;
        let shallower = OpenInterval {
            start: geometry.position[1] - gap + shallower_allowed - geometry.size[1],
            end: geometry.bottom() + gap - shallower_allowed,
        };
        assert!(shallower.start < geometry.position[1]);
        assert!(geometry.position[1] < shallower.end);
    }
}
