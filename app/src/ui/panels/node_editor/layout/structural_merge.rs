use eframe::egui;
use library::model::project::PortOwner;
use library::model::{NodeContainer, Project};
use std::collections::{BTreeMap, HashMap};

use super::merge_alignment::merge_anchor_aligned_top;
use super::{
    estimated_node_rect, estimated_node_size, immediate_child_rects, rects_are_closer_than,
    AutoLayoutPlan,
};
use crate::ui::panels::node_editor::{
    container_geometry, grow_container_to_rect, AUTO_LAYOUT_COMPOSITION_LEFT,
    AUTO_LAYOUT_COMPOSITION_TOP, AUTO_LAYOUT_NODE_PADDING, AUTO_LAYOUT_TRACK_LEFT,
    AUTO_LAYOUT_TRACK_TOP,
};

pub(in crate::ui::panels::node_editor) fn ensure_structural_merge_layout(
    project: &mut Project,
    container: NodeContainer,
) -> bool {
    let structural_merge_id = match container {
        NodeContainer::Composition(id) => project
            .get_composition(id)
            .map(|composition| composition.structural_merge_node_id),
        NodeContainer::Track(id) => project
            .get_track(id)
            .map(|track| track.structural_merge_node_id),
        NodeContainer::Clip(_) => None,
    };
    let Some(structural_merge_id) = structural_merge_id else {
        return false;
    };
    let Some((container_position, _, direct_node_ids)) = container_geometry(project, container)
    else {
        return false;
    };
    let Some(node) = project.get_node(structural_merge_id) else {
        return false;
    };
    let node_size = estimated_node_size(project, structural_merge_id);
    let current = egui::Rect::from_min_size(
        egui::pos2(node.ui_position[0], node.ui_position[1]),
        node_size,
    );
    let (left, top) = match container {
        NodeContainer::Composition(_) => {
            (AUTO_LAYOUT_COMPOSITION_LEFT, AUTO_LAYOUT_COMPOSITION_TOP)
        }
        NodeContainer::Track(_) => (AUTO_LAYOUT_TRACK_LEFT, AUTO_LAYOUT_TRACK_TOP),
        NodeContainer::Clip(_) => return false,
    };
    let content_min = egui::pos2(container_position[0] + left, container_position[1] + top);
    let mut occupied = immediate_child_rects(project, &AutoLayoutPlan::default(), container);
    occupied.extend(
        direct_node_ids
            .iter()
            .copied()
            .filter(|node_id| *node_id != structural_merge_id)
            .filter_map(|node_id| estimated_node_rect(project, node_id)),
    );

    let aligned_top = merge_anchor_aligned_top(
        project,
        structural_merge_id,
        &BTreeMap::new(),
        &HashMap::new(),
    )
    .unwrap_or_else(|| current.top());
    let mut x = current.left().max(content_min.x);
    let y = aligned_top.max(content_min.y);
    let mut candidate = egui::Rect::from_min_size(egui::pos2(x, y), node_size);
    loop {
        let next_x = occupied
            .iter()
            .filter(|other| rects_are_closer_than(candidate, **other, AUTO_LAYOUT_NODE_PADDING))
            .map(|other| other.right() + AUTO_LAYOUT_NODE_PADDING + 1.0)
            .max_by(f32::total_cmp);
        let Some(next_x) = next_x else {
            break;
        };
        x = next_x;
        candidate = egui::Rect::from_min_size(egui::pos2(x, y), node_size);
    }

    let mut changed = false;
    if candidate.min != current.min {
        if let Some(node) = project.get_node_mut(structural_merge_id) {
            node.ui_position = [candidate.min.x, candidate.min.y];
            changed = true;
        }
    }
    changed | grow_container_to_rect(project, port_owner(container), candidate)
}

fn port_owner(container: NodeContainer) -> PortOwner {
    match container {
        NodeContainer::Composition(id) => PortOwner::Composition(id),
        NodeContainer::Track(id) => PortOwner::Track(id),
        NodeContainer::Clip(id) => PortOwner::Clip(id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::panels::node_editor::layout::{
        apply_auto_layout, compute_full_composition_layout,
    };
    use crate::ui::panels::node_editor::test_fixture::fixture;
    use crate::ui::panels::node_editor::types::{CONTAINER_RIGHT_PORT_Y, MERGE_INPUT_FIRST_ROW_Y};

    #[test]
    fn normal_repair_uses_the_top_layer_anchor_and_is_idempotent() {
        let (mut project, _, track_id, clip_id, _, _) = fixture();
        let structural_merge_id = project
            .get_track(track_id)
            .expect("fixture Track")
            .structural_merge_node_id;
        project
            .get_track_mut(track_id)
            .expect("fixture Track")
            .ui_size = [1_800.0, 1_000.0];
        project
            .get_clip_mut(clip_id)
            .expect("fixture Clip")
            .ui_position[1] = 400.0;
        project
            .get_node_mut(structural_merge_id)
            .expect("Track structural Merge")
            .ui_position = [1_300.0, 600.0];

        let expected_top = 400.0 + CONTAINER_RIGHT_PORT_Y - MERGE_INPUT_FIRST_ROW_Y;
        assert!(ensure_structural_merge_layout(
            &mut project,
            NodeContainer::Track(track_id),
        ));
        assert_eq!(
            project
                .get_node(structural_merge_id)
                .expect("Track structural Merge")
                .ui_position[1],
            expected_top,
        );
        let repaired = project.clone();
        assert!(!ensure_structural_merge_layout(
            &mut project,
            NodeContainer::Track(track_id),
        ));
        assert_eq!(project, repaired);
    }

    #[test]
    fn top_layer_anchor_clamps_to_container_content() {
        let (mut project, _, track_id, clip_id, _, _) = fixture();
        let track_top = project
            .get_track(track_id)
            .expect("fixture Track")
            .ui_position[1];
        let content_top = track_top + AUTO_LAYOUT_TRACK_TOP;
        let structural_merge_id = project
            .get_track(track_id)
            .expect("fixture Track")
            .structural_merge_node_id;
        project
            .get_track_mut(track_id)
            .expect("fixture Track")
            .ui_size = [1_800.0, 1_000.0];
        project
            .get_clip_mut(clip_id)
            .expect("fixture Clip")
            .ui_position[1] = content_top;
        project
            .get_node_mut(structural_merge_id)
            .expect("Track structural Merge")
            .ui_position = [1_300.0, 600.0];

        assert!(ensure_structural_merge_layout(
            &mut project,
            NodeContainer::Track(track_id),
        ));
        assert_eq!(
            project
                .get_node(structural_merge_id)
                .expect("Track structural Merge")
                .ui_position[1],
            content_top,
        );
    }

    #[test]
    fn collision_moves_right_without_breaking_the_layer_anchor() {
        let (mut project, _, track_id, clip_id, _, _) = fixture();
        let structural_merge_id = project
            .get_track(track_id)
            .expect("fixture Track")
            .structural_merge_node_id;
        project
            .get_track_mut(track_id)
            .expect("fixture Track")
            .ui_size = [1_800.0, 1_000.0];
        project
            .get_clip_mut(clip_id)
            .expect("fixture Clip")
            .ui_position[1] = 400.0;
        project
            .get_node_mut(structural_merge_id)
            .expect("Track structural Merge")
            .ui_position = [800.0, 600.0];

        let clip = project.get_clip(clip_id).expect("fixture Clip");
        let expected_top = clip.ui_position[1] + CONTAINER_RIGHT_PORT_Y - MERGE_INPUT_FIRST_ROW_Y;
        let expected_left = clip.ui_position[0] + clip.ui_size[0] + AUTO_LAYOUT_NODE_PADDING + 1.0;
        assert!(ensure_structural_merge_layout(
            &mut project,
            NodeContainer::Track(track_id),
        ));
        let repaired = project
            .get_node(structural_merge_id)
            .expect("Track structural Merge");
        assert_eq!(repaired.ui_position, [expected_left, expected_top]);
    }

    #[test]
    fn full_fixture_auto_layout_is_idempotent() {
        let (mut project, composition_id, _, _, _, _) = fixture();
        let first = compute_full_composition_layout(&project, composition_id)
            .expect("fixture must produce a full layout");
        assert!(apply_auto_layout(&mut project, composition_id, &first));
        let laid_out = project.clone();

        let second = compute_full_composition_layout(&project, composition_id)
            .expect("laid-out fixture must remain layoutable");
        assert!(!apply_auto_layout(&mut project, composition_id, &second));
        assert_eq!(project, laid_out);
    }
}
