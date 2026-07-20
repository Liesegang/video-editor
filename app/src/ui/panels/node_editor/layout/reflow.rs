use eframe::egui;
use egui_snarl::Snarl;
use library::model::Project;
use std::collections::HashMap;
use uuid::Uuid;

use super::ranking::{canonical_edges, estimated_node_size, rank_nodes_by_scc};
use crate::ui::panels::node_editor::graph_build::{container_item_position, container_visual};
use crate::ui::panels::node_editor::{
    container_rect, set_container_geometry, translate_container, GraphItem, LayoutEdit,
    AUTO_LAYOUT_CLIP_TOP, AUTO_LAYOUT_COMPOSITION_BOTTOM, AUTO_LAYOUT_COMPOSITION_LEFT,
    AUTO_LAYOUT_COMPOSITION_RIGHT, AUTO_LAYOUT_COMPOSITION_TOP, AUTO_LAYOUT_NODE_PADDING,
    AUTO_LAYOUT_TRACK_BOTTOM, AUTO_LAYOUT_TRACK_LEFT, AUTO_LAYOUT_TRACK_RIGHT,
    AUTO_LAYOUT_TRACK_TOP,
};

pub(in crate::ui::panels::node_editor) fn collect_layout_edits(
    project: &Project,
    snarl: &Snarl<GraphItem>,
) -> Vec<LayoutEdit> {
    let mut edits = Vec::new();
    for (position, item) in snarl.nodes_pos() {
        match *item {
            GraphItem::Node(node_id) => {
                let Some(node) = project.get_node(node_id) else {
                    continue;
                };
                let new_position = [position.x, position.y];
                if node.ui_position != new_position {
                    edits.push(LayoutEdit::MoveNode {
                        node_id,
                        position: new_position,
                    });
                }
            }
            GraphItem::Container(owner) => {
                let Some(visual) = container_visual(project, owner) else {
                    continue;
                };
                let expected = container_item_position(&visual, *item);
                let delta = position - expected;
                if delta.length_sq() > 0.001 {
                    edits.push(LayoutEdit::MoveContainer {
                        owner,
                        delta: [delta.x, delta.y],
                    });
                }
            }
            GraphItem::PortAnchor { .. } => {}
        }
    }
    edits
}

pub(in crate::ui::panels::node_editor) fn layout_needs_reflow(
    project: &Project,
    composition_id: Uuid,
) -> bool {
    let Some(composition) = project.get_composition(composition_id) else {
        return false;
    };
    let composition_rect = container_rect(composition.ui_position, composition.ui_size);
    let composition_content = composition_content_rect(composition_rect);
    let mut node_rects = Vec::<(Uuid, egui::Rect)>::new();

    for node_id in &composition.node_ids {
        let Some(rect) = estimated_node_rect(project, *node_id) else {
            continue;
        };
        if !rect_contains_rect(composition_content, rect) {
            return true;
        }
        node_rects.push((*node_id, rect));
    }

    let mut track_rects = Vec::new();
    for track_id in &composition.track_ids {
        let Some(track) = project.get_track(*track_id) else {
            continue;
        };
        let track_rect = container_rect(track.ui_position, track.ui_size);
        if !rect_contains_rect(composition_content, track_rect) {
            return true;
        }
        if track_rects
            .iter()
            .any(|other: &egui::Rect| padded_intersection(*other, track_rect))
        {
            return true;
        }
        if composition.node_ids.iter().any(|node_id| {
            estimated_node_rect(project, *node_id)
                .is_some_and(|rect| padded_intersection(rect, track_rect))
        }) {
            return true;
        }
        track_rects.push(track_rect);

        let track_content = nested_content_rect(track_rect, AUTO_LAYOUT_TRACK_TOP);
        let mut clip_rects = Vec::new();
        for clip_id in &track.clip_ids {
            let Some(clip) = project.get_clip(*clip_id) else {
                continue;
            };
            let clip_rect = container_rect(clip.ui_position, clip.ui_size);
            if !rect_contains_rect(track_content, clip_rect)
                || clip_rects
                    .iter()
                    .any(|other: &egui::Rect| padded_intersection(*other, clip_rect))
            {
                return true;
            }
            if track.node_ids.iter().any(|node_id| {
                estimated_node_rect(project, *node_id)
                    .is_some_and(|rect| padded_intersection(rect, clip_rect))
            }) {
                return true;
            }
            clip_rects.push(clip_rect);

            let clip_content = nested_content_rect(clip_rect, AUTO_LAYOUT_CLIP_TOP);
            for node_id in &clip.node_ids {
                let Some(rect) = estimated_node_rect(project, *node_id) else {
                    continue;
                };
                if !rect_contains_rect(clip_content, rect) {
                    return true;
                }
                node_rects.push((*node_id, rect));
            }
        }
        for node_id in &track.node_ids {
            let Some(rect) = estimated_node_rect(project, *node_id) else {
                continue;
            };
            if !rect_contains_rect(track_content, rect) {
                return true;
            }
            node_rects.push((*node_id, rect));
        }
    }

    if node_rects.iter().enumerate().any(|(index, (_, left))| {
        node_rects[index + 1..]
            .iter()
            .any(|(_, right)| padded_intersection(*left, *right))
    }) {
        return true;
    }

    let all_nodes = node_rects.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    let ranks = rank_nodes_by_scc(&all_nodes, &canonical_edges(project, &all_nodes));
    let rects = node_rects.into_iter().collect::<HashMap<_, _>>();
    canonical_edges(project, &all_nodes)
        .into_iter()
        .any(|(from, to)| {
            ranks.get(&from) != ranks.get(&to)
                && rects
                    .get(&from)
                    .zip(rects.get(&to))
                    .is_some_and(|(from_rect, to_rect)| {
                        from_rect.right() + AUTO_LAYOUT_NODE_PADDING > to_rect.left()
                    })
        })
}

pub(in crate::ui::panels::node_editor) fn container_hierarchy_needs_reflow(
    project: &Project,
    composition_id: Uuid,
) -> bool {
    let Some(composition) = project.get_composition(composition_id) else {
        return false;
    };
    let composition_content =
        composition_content_rect(container_rect(composition.ui_position, composition.ui_size));
    if composition.node_ids.iter().any(|node_id| {
        estimated_node_rect(project, *node_id)
            .is_some_and(|rect| !rect_contains_rect(composition_content, rect))
    }) {
        return true;
    }
    let mut track_rects = Vec::new();
    for track_id in &composition.track_ids {
        let Some(track) = project.get_track(*track_id) else {
            continue;
        };
        let track_rect = container_rect(track.ui_position, track.ui_size);
        if !rect_contains_rect(composition_content, track_rect)
            || track_rects
                .iter()
                .any(|other| padded_intersection(*other, track_rect))
            || composition.node_ids.iter().any(|node_id| {
                estimated_node_rect(project, *node_id)
                    .is_some_and(|rect| padded_intersection(rect, track_rect))
            })
        {
            return true;
        }
        track_rects.push(track_rect);
        let track_content = nested_content_rect(track_rect, AUTO_LAYOUT_TRACK_TOP);
        if track.node_ids.iter().any(|node_id| {
            estimated_node_rect(project, *node_id)
                .is_some_and(|rect| !rect_contains_rect(track_content, rect))
        }) {
            return true;
        }
        let mut clip_rects = Vec::new();
        for clip_id in &track.clip_ids {
            let Some(clip) = project.get_clip(*clip_id) else {
                continue;
            };
            let clip_rect = container_rect(clip.ui_position, clip.ui_size);
            if !rect_contains_rect(track_content, clip_rect)
                || clip_rects
                    .iter()
                    .any(|other| padded_intersection(*other, clip_rect))
                || track.node_ids.iter().any(|node_id| {
                    estimated_node_rect(project, *node_id)
                        .is_some_and(|rect| padded_intersection(rect, clip_rect))
                })
            {
                return true;
            }
            let clip_content = nested_content_rect(clip_rect, AUTO_LAYOUT_CLIP_TOP);
            if clip.node_ids.iter().any(|node_id| {
                estimated_node_rect(project, *node_id)
                    .is_some_and(|rect| !rect_contains_rect(clip_content, rect))
            }) {
                return true;
            }
            clip_rects.push(clip_rect);
        }
    }
    false
}

pub(in crate::ui::panels::node_editor) fn estimated_node_rect(
    project: &Project,
    node_id: Uuid,
) -> Option<egui::Rect> {
    let node = project.get_node(node_id)?;
    Some(egui::Rect::from_min_size(
        egui::pos2(node.ui_position[0], node.ui_position[1]),
        estimated_node_size(project, node_id),
    ))
}

pub(in crate::ui::panels::node_editor) fn composition_content_rect(rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        rect.min + egui::vec2(AUTO_LAYOUT_COMPOSITION_LEFT, AUTO_LAYOUT_COMPOSITION_TOP),
        rect.max
            - egui::vec2(
                AUTO_LAYOUT_COMPOSITION_RIGHT,
                AUTO_LAYOUT_COMPOSITION_BOTTOM,
            ),
    )
}

pub(in crate::ui::panels::node_editor) fn nested_content_rect(
    rect: egui::Rect,
    top: f32,
) -> egui::Rect {
    egui::Rect::from_min_max(
        rect.min + egui::vec2(AUTO_LAYOUT_TRACK_LEFT, top),
        rect.max - egui::vec2(AUTO_LAYOUT_TRACK_RIGHT, AUTO_LAYOUT_TRACK_BOTTOM),
    )
}

pub(in crate::ui::panels::node_editor) fn rect_contains_rect(
    outer: egui::Rect,
    inner: egui::Rect,
) -> bool {
    outer.min.x <= inner.min.x + f32::EPSILON
        && outer.min.y <= inner.min.y + f32::EPSILON
        && outer.max.x + f32::EPSILON >= inner.max.x
        && outer.max.y + f32::EPSILON >= inner.max.y
}

pub(in crate::ui::panels::node_editor) fn padded_intersection(
    left: egui::Rect,
    right: egui::Rect,
) -> bool {
    left.expand(AUTO_LAYOUT_NODE_PADDING * 0.5)
        .intersects(right.expand(AUTO_LAYOUT_NODE_PADDING * 0.5))
}

pub(in crate::ui::panels::node_editor) fn apply_layout_edit(
    project: &mut Project,
    edit: LayoutEdit,
) -> bool {
    match edit {
        LayoutEdit::MoveNode { node_id, position } => {
            let Some(node) = project.get_node_mut(node_id) else {
                return false;
            };
            if node.ui_position == position {
                return false;
            }
            node.ui_position = position;
            true
        }
        LayoutEdit::MoveContainer { owner, delta } => translate_container(project, owner, delta),
        LayoutEdit::ResizeContainer {
            owner,
            position,
            size,
        } => set_container_geometry(project, owner, position, size),
    }
}
