use eframe::egui;
use library::model::project::PortOwner;
use library::model::{NodeContainer, Project};
use std::collections::HashSet;
use uuid::Uuid;

use super::ranking::{
    canonical_edges, estimated_node_size, first_free_y, layout_node_band, node_band_bounds,
    node_rank_columns, rank_nodes_by_scc,
};
use crate::ui::panels::node_editor::{
    container_rect, estimated_node_rect, AutoLayoutPlan, AutoLayoutScope, ContainerLayout,
    AUTO_LAYOUT_CLIP_GAP, AUTO_LAYOUT_CLIP_TOP, AUTO_LAYOUT_COMPOSITION_BOTTOM,
    AUTO_LAYOUT_COMPOSITION_LEFT, AUTO_LAYOUT_COMPOSITION_RIGHT, AUTO_LAYOUT_COMPOSITION_TOP,
    AUTO_LAYOUT_ROW_GAP, AUTO_LAYOUT_TRACK_BOTTOM, AUTO_LAYOUT_TRACK_GAP, AUTO_LAYOUT_TRACK_LEFT,
    AUTO_LAYOUT_TRACK_RIGHT, AUTO_LAYOUT_TRACK_TOP, MIN_CONTAINER_SIZE,
};

pub(in crate::ui::panels::node_editor) fn compute_auto_layout(
    project: &Project,
    composition_id: Uuid,
    scope: AutoLayoutScope,
) -> Option<AutoLayoutPlan> {
    let composition = project.get_composition(composition_id)?;
    match scope {
        AutoLayoutScope::All => compute_full_composition_layout(project, composition_id),
        AutoLayoutScope::Container(PortOwner::Composition(id)) if id == composition_id => {
            compute_full_composition_layout(project, composition_id)
        }
        AutoLayoutScope::Container(PortOwner::Track(id)) if composition.track_ids.contains(&id) => {
            compute_track_scope_layout(project, composition_id, id)
        }
        AutoLayoutScope::Container(PortOwner::Clip(id))
            if composition.track_ids.iter().any(|track_id| {
                project
                    .get_track(*track_id)
                    .is_some_and(|track| track.clip_ids.contains(&id))
            }) =>
        {
            compute_clip_scope_layout(project, composition_id, id)
        }
        AutoLayoutScope::Selection(selected) => {
            compute_selection_scope_layout(project, composition_id, &selected)
        }
        AutoLayoutScope::Container(_) => None,
    }
}

pub(in crate::ui::panels::node_editor) fn composition_graph_node_ids(
    project: &Project,
    composition_id: Uuid,
) -> Vec<Uuid> {
    let Some(composition) = project.get_composition(composition_id) else {
        return Vec::new();
    };
    let mut node_ids = composition.node_ids.clone();
    for track_id in &composition.track_ids {
        node_ids.extend(track_leaf_node_ids(project, *track_id));
    }
    node_ids.sort_unstable();
    node_ids.dedup();
    node_ids
}

pub(in crate::ui::panels::node_editor) fn compute_track_scope_layout(
    project: &Project,
    composition_id: Uuid,
    track_id: Uuid,
) -> Option<AutoLayoutPlan> {
    let composition = project.get_composition(composition_id)?;
    let track = project.get_track(track_id)?;
    let full = compute_full_composition_layout(project, composition_id)?;
    let full_track = *full.track_layouts.get(&track_id)?;
    let occupied = composition
        .track_ids
        .iter()
        .filter(|candidate| **candidate != track_id)
        .filter_map(|candidate| project.get_track(*candidate))
        .map(|candidate| container_rect(candidate.ui_position, candidate.ui_size))
        .collect::<Vec<_>>();
    let y = first_free_y(
        full_track.position[0],
        full_track.size[0],
        full_track.size[1],
        track.ui_position[1].max(composition.ui_position[1] + AUTO_LAYOUT_COMPOSITION_TOP),
        &occupied,
        AUTO_LAYOUT_TRACK_GAP,
    );
    let delta_y = y - full_track.position[1];
    let mut plan = AutoLayoutPlan::default();
    plan.track_layouts.insert(
        track_id,
        ContainerLayout {
            position: [full_track.position[0], y],
            size: full_track.size,
        },
    );
    for clip_id in &track.clip_ids {
        if let Some(layout) = full.clip_layouts.get(clip_id) {
            plan.clip_layouts.insert(
                *clip_id,
                ContainerLayout {
                    position: [layout.position[0], layout.position[1] + delta_y],
                    size: layout.size,
                },
            );
        }
    }
    for node_id in track_leaf_node_ids(project, track_id) {
        if let Some(position) = full.node_positions.get(&node_id) {
            plan.node_positions
                .insert(node_id, [position[0], position[1] + delta_y]);
        }
    }
    let target_rect = egui::Rect::from_min_size(
        egui::pos2(full_track.position[0], y),
        egui::vec2(full_track.size[0], full_track.size[1]),
    );
    plan.composition_size = Some(grown_size_for_rect(
        composition.ui_position,
        composition.ui_size,
        target_rect,
        AUTO_LAYOUT_COMPOSITION_RIGHT,
        AUTO_LAYOUT_COMPOSITION_BOTTOM,
    ));
    Some(plan)
}

pub(in crate::ui::panels::node_editor) fn compute_clip_scope_layout(
    project: &Project,
    composition_id: Uuid,
    clip_id: Uuid,
) -> Option<AutoLayoutPlan> {
    let composition = project.get_composition(composition_id)?;
    let track_id = project.find_track_for_clip(clip_id)?;
    let track = project.get_track(track_id)?;
    let clip = project.get_clip(clip_id)?;
    let full = compute_full_composition_layout(project, composition_id)?;
    let full_clip = *full.clip_layouts.get(&clip_id)?;
    let occupied = track
        .clip_ids
        .iter()
        .filter(|candidate| **candidate != clip_id)
        .filter_map(|candidate| project.get_clip(*candidate))
        .map(|candidate| container_rect(candidate.ui_position, candidate.ui_size))
        .chain(
            track
                .node_ids
                .iter()
                .filter_map(|node_id| estimated_node_rect(project, *node_id)),
        )
        .collect::<Vec<_>>();
    let x = full_clip.position[0].max(track.ui_position[0] + AUTO_LAYOUT_TRACK_LEFT);
    let y = first_free_y(
        x,
        full_clip.size[0],
        full_clip.size[1],
        clip.ui_position[1].max(track.ui_position[1] + AUTO_LAYOUT_TRACK_TOP),
        &occupied,
        AUTO_LAYOUT_ROW_GAP,
    );
    let delta = [x - full_clip.position[0], y - full_clip.position[1]];
    let mut plan = AutoLayoutPlan::default();
    plan.clip_layouts.insert(
        clip_id,
        ContainerLayout {
            position: [x, y],
            size: full_clip.size,
        },
    );
    for node_id in &clip.node_ids {
        if let Some(position) = full.node_positions.get(node_id) {
            plan.node_positions
                .insert(*node_id, [position[0] + delta[0], position[1] + delta[1]]);
        }
    }
    let clip_rect = egui::Rect::from_min_size(
        egui::pos2(x, y),
        egui::vec2(full_clip.size[0], full_clip.size[1]),
    );
    let track_size = grown_size_for_rect(
        track.ui_position,
        track.ui_size,
        clip_rect,
        AUTO_LAYOUT_TRACK_RIGHT,
        AUTO_LAYOUT_TRACK_BOTTOM,
    );
    plan.track_layouts.insert(
        track_id,
        ContainerLayout {
            position: track.ui_position,
            size: track_size,
        },
    );
    let track_rect = egui::Rect::from_min_size(
        egui::pos2(track.ui_position[0], track.ui_position[1]),
        egui::vec2(track_size[0], track_size[1]),
    );
    plan.composition_size = Some(grown_size_for_rect(
        composition.ui_position,
        composition.ui_size,
        track_rect,
        AUTO_LAYOUT_COMPOSITION_RIGHT,
        AUTO_LAYOUT_COMPOSITION_BOTTOM,
    ));
    Some(plan)
}

pub(in crate::ui::panels::node_editor) fn compute_selection_scope_layout(
    project: &Project,
    composition_id: Uuid,
    selected: &[Uuid],
) -> Option<AutoLayoutPlan> {
    let composition = project.get_composition(composition_id)?;
    let graph_nodes = composition_graph_node_ids(project, composition_id);
    let graph_node_set = graph_nodes.iter().copied().collect::<HashSet<_>>();
    let mut selected = selected
        .iter()
        .copied()
        .filter(|node_id| graph_node_set.contains(node_id))
        .collect::<Vec<_>>();
    selected.sort_unstable();
    selected.dedup();
    if selected.is_empty() {
        return None;
    }

    let full = compute_full_composition_layout(project, composition_id)?;
    selected.sort_by(|left, right| {
        full.node_positions[left][0]
            .total_cmp(&full.node_positions[right][0])
            .then_with(|| left.cmp(right))
    });
    let selected_set = selected.iter().copied().collect::<HashSet<_>>();
    let mut occupied = graph_nodes
        .iter()
        .filter(|node_id| !selected_set.contains(node_id))
        .filter_map(|node_id| estimated_node_rect(project, *node_id))
        .collect::<Vec<_>>();
    let mut plan = AutoLayoutPlan {
        composition_size: Some(composition.ui_size),
        ..AutoLayoutPlan::default()
    };

    for node_id in selected {
        let container = project.find_node_container(node_id)?;
        let desired = *full.node_positions.get(&node_id)?;
        let size = estimated_node_size(project, node_id);
        let (owner_position, _) = planned_container_geometry(project, &plan, container)?;
        let left_inset = match container {
            NodeContainer::Composition(_) => AUTO_LAYOUT_COMPOSITION_LEFT,
            NodeContainer::Track(_) | NodeContainer::Clip(_) => AUTO_LAYOUT_TRACK_LEFT,
        };
        let top_inset = match container {
            NodeContainer::Composition(_) => AUTO_LAYOUT_COMPOSITION_TOP,
            NodeContainer::Track(_) => AUTO_LAYOUT_TRACK_TOP,
            NodeContainer::Clip(_) => AUTO_LAYOUT_CLIP_TOP,
        };
        let x = desired[0].max(owner_position[0] + left_inset);
        let child_obstacles = immediate_child_rects(project, &plan, container);
        let mut local_occupied = occupied.clone();
        local_occupied.extend(child_obstacles);
        let y = first_free_y(
            x,
            size.x,
            size.y,
            owner_position[1] + top_inset,
            &local_occupied,
            AUTO_LAYOUT_ROW_GAP,
        );
        let node_rect = egui::Rect::from_min_size(egui::pos2(x, y), size);
        plan.node_positions.insert(node_id, [x, y]);
        occupied.push(node_rect);
        grow_planned_hierarchy(project, composition_id, container, node_rect, &mut plan);
    }
    Some(plan)
}

pub(in crate::ui::panels::node_editor) fn planned_container_geometry(
    project: &Project,
    plan: &AutoLayoutPlan,
    container: NodeContainer,
) -> Option<([f32; 2], [f32; 2])> {
    match container {
        NodeContainer::Composition(id) => project.get_composition(id).map(|item| {
            (
                item.ui_position,
                plan.composition_size.unwrap_or(item.ui_size),
            )
        }),
        NodeContainer::Track(id) => project.get_track(id).map(|item| {
            plan.track_layouts
                .get(&id)
                .map_or((item.ui_position, item.ui_size), |layout| {
                    (layout.position, layout.size)
                })
        }),
        NodeContainer::Clip(id) => project.get_clip(id).map(|item| {
            plan.clip_layouts
                .get(&id)
                .map_or((item.ui_position, item.ui_size), |layout| {
                    (layout.position, layout.size)
                })
        }),
    }
}

pub(in crate::ui::panels::node_editor) fn immediate_child_rects(
    project: &Project,
    plan: &AutoLayoutPlan,
    container: NodeContainer,
) -> Vec<egui::Rect> {
    match container {
        NodeContainer::Composition(id) => project
            .get_composition(id)
            .into_iter()
            .flat_map(|composition| composition.track_ids.iter())
            .filter_map(|track_id| {
                planned_container_geometry(project, plan, NodeContainer::Track(*track_id))
            })
            .map(|(position, size)| container_rect(position, size))
            .collect(),
        NodeContainer::Track(id) => project
            .get_track(id)
            .into_iter()
            .flat_map(|track| track.clip_ids.iter())
            .filter_map(|clip_id| {
                planned_container_geometry(project, plan, NodeContainer::Clip(*clip_id))
            })
            .map(|(position, size)| container_rect(position, size))
            .collect(),
        NodeContainer::Clip(_) => Vec::new(),
    }
}

pub(in crate::ui::panels::node_editor) fn grow_planned_hierarchy(
    project: &Project,
    composition_id: Uuid,
    container: NodeContainer,
    rect: egui::Rect,
    plan: &mut AutoLayoutPlan,
) {
    let mut ancestor_rect = rect;
    if let NodeContainer::Clip(clip_id) = container {
        if let Some(clip) = project.get_clip(clip_id) {
            let current = plan
                .clip_layouts
                .get(&clip_id)
                .copied()
                .unwrap_or(ContainerLayout {
                    position: clip.ui_position,
                    size: clip.ui_size,
                });
            let size = grown_size_for_rect(
                current.position,
                current.size,
                rect,
                AUTO_LAYOUT_TRACK_RIGHT,
                AUTO_LAYOUT_TRACK_BOTTOM,
            );
            plan.clip_layouts.insert(
                clip_id,
                ContainerLayout {
                    position: current.position,
                    size,
                },
            );
            ancestor_rect = container_rect(current.position, size);
        }
    }

    let track_id = match container {
        NodeContainer::Track(id) => Some(id),
        NodeContainer::Clip(id) => project.find_track_for_clip(id),
        NodeContainer::Composition(_) => None,
    };
    if let Some(track_id) = track_id {
        if let Some(track) = project.get_track(track_id) {
            let current = plan
                .track_layouts
                .get(&track_id)
                .copied()
                .unwrap_or(ContainerLayout {
                    position: track.ui_position,
                    size: track.ui_size,
                });
            let size = grown_size_for_rect(
                current.position,
                current.size,
                ancestor_rect,
                AUTO_LAYOUT_TRACK_RIGHT,
                AUTO_LAYOUT_TRACK_BOTTOM,
            );
            plan.track_layouts.insert(
                track_id,
                ContainerLayout {
                    position: current.position,
                    size,
                },
            );
            ancestor_rect = container_rect(current.position, size);
        }
    }

    if let Some(composition) = project.get_composition(composition_id) {
        plan.composition_size = Some(grown_size_for_rect(
            composition.ui_position,
            plan.composition_size.unwrap_or(composition.ui_size),
            ancestor_rect,
            AUTO_LAYOUT_COMPOSITION_RIGHT,
            AUTO_LAYOUT_COMPOSITION_BOTTOM,
        ));
    }
}

pub(in crate::ui::panels::node_editor) fn grown_size_for_rect(
    position: [f32; 2],
    current_size: [f32; 2],
    rect: egui::Rect,
    right_margin: f32,
    bottom_margin: f32,
) -> [f32; 2] {
    [
        current_size[0].max(rect.right() - position[0] + right_margin),
        current_size[1].max(rect.bottom() - position[1] + bottom_margin),
    ]
}

pub(in crate::ui::panels::node_editor) fn track_leaf_node_ids(
    project: &Project,
    track_id: Uuid,
) -> Vec<Uuid> {
    let Some(track) = project.get_track(track_id) else {
        return Vec::new();
    };
    let mut node_ids = track.node_ids.clone();
    for clip_id in &track.clip_ids {
        if let Some(clip) = project.get_clip(*clip_id) {
            node_ids.extend(clip.node_ids.iter().copied());
        }
    }
    node_ids
}

pub(in crate::ui::panels::node_editor) fn compute_full_composition_layout(
    project: &Project,
    composition_id: Uuid,
) -> Option<AutoLayoutPlan> {
    let composition = project.get_composition(composition_id)?;
    let mut nodes = composition.node_ids.clone();
    for track_id in &composition.track_ids {
        nodes.extend(track_leaf_node_ids(project, *track_id));
    }
    nodes.sort_unstable();
    nodes.dedup();
    let edges = canonical_edges(project, &nodes);
    let ranks = rank_nodes_by_scc(&nodes, &edges);
    let mut plan = AutoLayoutPlan::default();
    let track_x = composition.ui_position[0] + AUTO_LAYOUT_COMPOSITION_LEFT;
    // Every leaf Node shares this composition-space rank axis. Container
    // membership may move a Node vertically, but can never reverse an edge by
    // giving two sibling Clips or Tracks unrelated local x origins.
    let column_origin_x = track_x + AUTO_LAYOUT_TRACK_LEFT * 2.0;
    let rank_columns = node_rank_columns(project, &nodes, &ranks, column_origin_x);
    let mut track_y = composition.ui_position[1] + AUTO_LAYOUT_COMPOSITION_TOP;
    let mut composition_right = track_x;
    let mut composition_bottom = track_y;

    for track_id in &composition.track_ids {
        let Some(track) = project.get_track(*track_id) else {
            continue;
        };
        let content_y = track_y + AUTO_LAYOUT_TRACK_TOP;
        let default_clip_x = track_x + AUTO_LAYOUT_TRACK_LEFT;
        let mut required_right = default_clip_x;
        let mut required_bottom = content_y;
        let mut occupied = Vec::<egui::Rect>::new();

        let mut clip_ids = track.clip_ids.clone();
        clip_ids.sort_by_key(|clip_id| {
            project
                .get_clip(*clip_id)
                .map_or((usize::MAX, *clip_id), |clip| {
                    let minimum_rank = clip
                        .node_ids
                        .iter()
                        .filter_map(|node_id| ranks.get(node_id))
                        .copied()
                        .min()
                        .unwrap_or_default();
                    (minimum_rank, *clip_id)
                })
        });
        for clip_id in clip_ids {
            let Some(clip) = project.get_clip(clip_id) else {
                continue;
            };
            let band = node_band_bounds(project, &clip.node_ids, &ranks, &rank_columns);
            let clip_x = band.map_or(default_clip_x, |bounds| {
                bounds.min_x - AUTO_LAYOUT_TRACK_LEFT
            });
            let clip_width = band.map_or(MIN_CONTAINER_SIZE.x, |bounds| {
                (bounds.max_x + AUTO_LAYOUT_TRACK_RIGHT - clip_x).max(MIN_CONTAINER_SIZE.x)
            });
            let clip_height = band.map_or(MIN_CONTAINER_SIZE.y, |bounds| {
                (AUTO_LAYOUT_CLIP_TOP + bounds.height + AUTO_LAYOUT_TRACK_BOTTOM)
                    .max(MIN_CONTAINER_SIZE.y)
            });
            let clip_y = first_free_y(
                clip_x,
                clip_width,
                clip_height,
                content_y,
                &occupied,
                AUTO_LAYOUT_CLIP_GAP,
            );
            let clip_rect = egui::Rect::from_min_size(
                egui::pos2(clip_x, clip_y),
                egui::vec2(clip_width, clip_height),
            );
            let _ = layout_node_band(
                project,
                &clip.node_ids,
                &ranks,
                &rank_columns,
                clip_y + AUTO_LAYOUT_CLIP_TOP,
                &mut plan.node_positions,
            );
            plan.clip_layouts.insert(
                clip_id,
                ContainerLayout {
                    position: [clip_x, clip_y],
                    size: [clip_width, clip_height],
                },
            );
            required_right = required_right.max(clip_rect.right());
            required_bottom = required_bottom.max(clip_rect.bottom());
            occupied.push(clip_rect);
        }

        if let Some(bounds) = node_band_bounds(project, &track.node_ids, &ranks, &rank_columns) {
            let direct_y = first_free_y(
                bounds.min_x,
                bounds.width(),
                bounds.height,
                content_y,
                &occupied,
                AUTO_LAYOUT_ROW_GAP,
            );
            let _ = layout_node_band(
                project,
                &track.node_ids,
                &ranks,
                &rank_columns,
                direct_y,
                &mut plan.node_positions,
            );
            required_right = required_right.max(bounds.max_x);
            required_bottom = required_bottom.max(direct_y + bounds.height);
        }

        let track_size = [
            (required_right - track_x + AUTO_LAYOUT_TRACK_RIGHT).max(MIN_CONTAINER_SIZE.x),
            (required_bottom - track_y + AUTO_LAYOUT_TRACK_BOTTOM).max(MIN_CONTAINER_SIZE.y),
        ];
        plan.track_layouts.insert(
            *track_id,
            ContainerLayout {
                position: [track_x, track_y],
                size: track_size,
            },
        );
        composition_right = composition_right.max(track_x + track_size[0]);
        composition_bottom = composition_bottom.max(track_y + track_size[1]);
        track_y += track_size[1] + AUTO_LAYOUT_TRACK_GAP;
    }

    if let Some(bounds) = node_band_bounds(project, &composition.node_ids, &ranks, &rank_columns) {
        let direct_y = if composition.track_ids.is_empty() {
            composition.ui_position[1] + AUTO_LAYOUT_COMPOSITION_TOP
        } else {
            composition_bottom + AUTO_LAYOUT_TRACK_GAP
        };
        let _ = layout_node_band(
            project,
            &composition.node_ids,
            &ranks,
            &rank_columns,
            direct_y,
            &mut plan.node_positions,
        );
        composition_right = composition_right.max(bounds.max_x);
        composition_bottom = composition_bottom.max(direct_y + bounds.height);
    }

    plan.composition_size = Some([
        (composition_right - composition.ui_position[0] + AUTO_LAYOUT_COMPOSITION_RIGHT)
            .max(MIN_CONTAINER_SIZE.x),
        (composition_bottom - composition.ui_position[1] + AUTO_LAYOUT_COMPOSITION_BOTTOM)
            .max(MIN_CONTAINER_SIZE.y),
    ]);
    Some(plan)
}
