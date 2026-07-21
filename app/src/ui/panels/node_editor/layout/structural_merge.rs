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
    AUTO_LAYOUT_COMPOSITION_TOP, AUTO_LAYOUT_NODE_PADDING, AUTO_LAYOUT_ROW_GAP,
    AUTO_LAYOUT_TRACK_LEFT, AUTO_LAYOUT_TRACK_TOP,
};

pub(in crate::ui::panels::node_editor) fn ensure_structural_merge_layout(
    project: &mut Project,
    container: NodeContainer,
) -> bool {
    let structural_merge_ids = match container {
        NodeContainer::Composition(id) => project.get_composition(id).map(|composition| {
            [
                composition.structural_merge_node_id,
                composition.structural_sound_merge_node_id,
            ]
        }),
        NodeContainer::Track(id) => project.get_track(id).map(|track| {
            [
                track.structural_merge_node_id,
                track.structural_sound_merge_node_id,
            ]
        }),
        NodeContainer::Clip(_) => None,
    };
    let Some(structural_merge_ids) = structural_merge_ids else {
        return false;
    };
    let Some((container_position, _, direct_node_ids)) = container_geometry(project, container)
    else {
        return false;
    };
    let [image_merge_id, sound_merge_id] = structural_merge_ids;
    let Some(image_merge) = project.get_node(image_merge_id) else {
        return false;
    };
    let Some(sound_merge) = project.get_node(sound_merge_id) else {
        return false;
    };
    let image_size = estimated_node_size(project, image_merge_id);
    let sound_size = estimated_node_size(project, sound_merge_id);
    let current_image = egui::Rect::from_min_size(
        egui::pos2(image_merge.ui_position[0], image_merge.ui_position[1]),
        image_size,
    );
    let current_sound = egui::Rect::from_min_size(
        egui::pos2(sound_merge.ui_position[0], sound_merge.ui_position[1]),
        sound_size,
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
            .filter(|node_id| !structural_merge_ids.contains(node_id))
            .filter_map(|node_id| estimated_node_rect(project, node_id)),
    );

    let aligned_top =
        merge_anchor_aligned_top(project, image_merge_id, &BTreeMap::new(), &HashMap::new())
            .unwrap_or_else(|| current_image.top());
    let image_y = aligned_top.max(content_min.y);
    let sound_y = image_y + image_size.y + AUTO_LAYOUT_ROW_GAP;
    let mut x = current_image
        .left()
        .min(current_sound.left())
        .max(content_min.x);
    let mut image_candidate = egui::Rect::from_min_size(egui::pos2(x, image_y), image_size);
    let mut sound_candidate = egui::Rect::from_min_size(egui::pos2(x, sound_y), sound_size);
    loop {
        let next_x = occupied
            .iter()
            .filter(|other| {
                rects_are_closer_than(image_candidate, **other, AUTO_LAYOUT_NODE_PADDING)
                    || rects_are_closer_than(sound_candidate, **other, AUTO_LAYOUT_NODE_PADDING)
            })
            .map(|other| other.right() + AUTO_LAYOUT_NODE_PADDING + 1.0)
            .max_by(f32::total_cmp);
        let Some(next_x) = next_x else {
            break;
        };
        x = next_x;
        image_candidate = egui::Rect::from_min_size(egui::pos2(x, image_y), image_size);
        sound_candidate = egui::Rect::from_min_size(egui::pos2(x, sound_y), sound_size);
    }

    let mut changed = false;
    for (node_id, current, candidate) in [
        (image_merge_id, current_image, image_candidate),
        (sound_merge_id, current_sound, sound_candidate),
    ] {
        if candidate.min != current.min {
            if let Some(node) = project.get_node_mut(node_id) {
                node.ui_position = [candidate.min.x, candidate.min.y];
                changed = true;
            }
        }
    }
    let pair_bounds = image_candidate.union(sound_candidate);
    if grow_container_to_rect(project, port_owner(container), pair_bounds) {
        changed = true;
    }
    changed
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
    use crate::ui::panels::node_editor::container_rect;
    use crate::ui::panels::node_editor::layout::{
        apply_auto_layout, composition_content_rect, compute_full_composition_layout,
        nested_content_rect, padded_intersection, rect_contains_rect,
    };
    use crate::ui::panels::node_editor::test_fixture::fixture;
    use crate::ui::panels::node_editor::types::{CONTAINER_RIGHT_PORT_Y, MERGE_INPUT_FIRST_ROW_Y};

    fn assert_typed_merge_pair_layout(project: &Project, container: NodeContainer) {
        let (image_merge_id, sound_merge_id, child_rects, direct_node_ids, content) =
            match container {
                NodeContainer::Composition(id) => {
                    let composition = project.get_composition(id).expect("Composition");
                    let rect = container_rect(composition.ui_position, composition.ui_size);
                    (
                        composition.structural_merge_node_id,
                        composition.structural_sound_merge_node_id,
                        composition
                            .track_ids
                            .iter()
                            .filter_map(|track_id| {
                                project
                                    .get_track(*track_id)
                                    .map(|track| container_rect(track.ui_position, track.ui_size))
                            })
                            .collect::<Vec<_>>(),
                        composition.node_ids.clone(),
                        composition_content_rect(rect),
                    )
                }
                NodeContainer::Track(id) => {
                    let track = project.get_track(id).expect("Track");
                    let rect = container_rect(track.ui_position, track.ui_size);
                    (
                        track.structural_merge_node_id,
                        track.structural_sound_merge_node_id,
                        track
                            .clip_ids
                            .iter()
                            .filter_map(|clip_id| {
                                project
                                    .get_clip(*clip_id)
                                    .map(|clip| container_rect(clip.ui_position, clip.ui_size))
                            })
                            .collect::<Vec<_>>(),
                        track.node_ids.clone(),
                        nested_content_rect(rect, AUTO_LAYOUT_TRACK_TOP),
                    )
                }
                NodeContainer::Clip(_) => panic!("Clip has no structural Merge pair"),
            };
        let image_rect = estimated_node_rect(project, image_merge_id).expect("Image Merge rect");
        let sound_rect = estimated_node_rect(project, sound_merge_id).expect("Sound Merge rect");

        assert_eq!(image_rect.left(), sound_rect.left());
        assert!(image_rect.bottom() + AUTO_LAYOUT_ROW_GAP <= sound_rect.top());
        assert!(!padded_intersection(image_rect, sound_rect));
        assert!(rect_contains_rect(content, image_rect));
        assert!(rect_contains_rect(content, sound_rect));

        for other_rect in child_rects.into_iter().chain(
            direct_node_ids
                .into_iter()
                .filter(|node_id| ![image_merge_id, sound_merge_id].contains(node_id))
                .filter_map(|node_id| estimated_node_rect(project, node_id)),
        ) {
            assert!(!padded_intersection(image_rect, other_rect));
            assert!(!padded_intersection(sound_rect, other_rect));
            assert!(
                other_rect.right() + AUTO_LAYOUT_NODE_PADDING <= image_rect.left(),
                "structural outputs must remain to the right of their siblings"
            );
        }
    }

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
        assert_typed_merge_pair_layout(&project, NodeContainer::Track(track_id));
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
        let (mut project, composition_id, track_id, _, _, _) = fixture();
        let first = compute_full_composition_layout(&project, composition_id)
            .expect("fixture must produce a full layout");
        assert!(apply_auto_layout(&mut project, composition_id, &first));
        assert_typed_merge_pair_layout(&project, NodeContainer::Composition(composition_id));
        assert_typed_merge_pair_layout(&project, NodeContainer::Track(track_id));
        let laid_out = project.clone();

        let second = compute_full_composition_layout(&project, composition_id)
            .expect("laid-out fixture must remain layoutable");
        assert!(!apply_auto_layout(&mut project, composition_id, &second));
        assert_eq!(project, laid_out);
    }
}
