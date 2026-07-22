use eframe::egui;
use egui_snarl::Snarl;
use library::model::project::PortOwner;
use library::model::{NodeContainer, Project};
use std::collections::BTreeMap;
use uuid::Uuid;

use super::node_geometry::estimated_node_size;
use crate::state::context_types::SelectionTarget;
use crate::ui::panels::node_editor::graph_build::{container_item_position, container_visual};
use crate::ui::panels::node_editor::{
    container_rect, set_container_geometry, translate_container, GraphItem, LayoutEdit,
    AUTO_LAYOUT_CLIP_TOP, AUTO_LAYOUT_COMPOSITION_BOTTOM, AUTO_LAYOUT_COMPOSITION_LEFT,
    AUTO_LAYOUT_COMPOSITION_RIGHT, AUTO_LAYOUT_COMPOSITION_TOP, AUTO_LAYOUT_NODE_PADDING,
    AUTO_LAYOUT_TRACK_BOTTOM, AUTO_LAYOUT_TRACK_LEFT, AUTO_LAYOUT_TRACK_RIGHT,
    AUTO_LAYOUT_TRACK_TOP,
};

const HIERARCHY_SEVERITY_EPSILON: f32 = 0.001;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum HierarchyItem {
    Composition(Uuid),
    Track(Uuid),
    Clip(Uuid),
    Node(Uuid),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ContainmentSide {
    Left,
    Top,
    Right,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum OverlapAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum HierarchyViolationId {
    Outside {
        parent: HierarchyItem,
        child: HierarchyItem,
        side: ContainmentSide,
    },
    Overlap {
        parent: HierarchyItem,
        first: HierarchyItem,
        second: HierarchyItem,
        axis: OverlapAxis,
    },
}

#[derive(Clone, Copy, Debug, Default)]
struct HierarchyViolationSeverity {
    correction: f32,
}

impl HierarchyViolationSeverity {
    fn is_worse_than(self, baseline: Self) -> bool {
        self.correction > baseline.correction + HIERARCHY_SEVERITY_EPSILON
    }
}

type HierarchyViolations = BTreeMap<HierarchyViolationId, HierarchyViolationSeverity>;

#[cfg(test)]
pub(in crate::ui::panels::node_editor) fn collect_layout_edits(
    project: &Project,
    snarl: &Snarl<GraphItem>,
) -> Vec<LayoutEdit> {
    observed_layout_deltas(project, snarl)
        .into_iter()
        .filter_map(|(target, delta)| layout_edit_for_delta(project, target, delta))
        .collect()
}

/// Expand the one transient Snarl frame that actually captured the pointer to
/// the authoritative typed selection. Port anchors are drag handles for their
/// owning container, never independently movable graph objects.
pub(in crate::ui::panels::node_editor) fn collect_layout_edits_for_selection(
    project: &Project,
    snarl: &Snarl<GraphItem>,
    drag_driver: Option<SelectionTarget>,
    selected: &[SelectionTarget],
) -> Vec<LayoutEdit> {
    let observed = observed_layout_deltas(project, snarl);
    let Some(driver) = drag_driver else {
        return observed
            .into_iter()
            .filter_map(|(target, delta)| layout_edit_for_delta(project, target, delta))
            .collect();
    };
    let Some(delta) = observed.get(&driver).copied() else {
        return Vec::new();
    };

    let candidates = if selected.contains(&driver) {
        selected.to_vec()
    } else {
        vec![driver]
    };
    selection_move_roots(project, &candidates)
        .into_iter()
        .filter_map(|target| layout_edit_for_delta(project, target, delta))
        .collect()
}

fn observed_layout_deltas(
    project: &Project,
    snarl: &Snarl<GraphItem>,
) -> BTreeMap<SelectionTarget, egui::Vec2> {
    let mut deltas = BTreeMap::new();
    for (position, item) in snarl.nodes_pos() {
        match *item {
            GraphItem::Node(node_id) => {
                let Some(node) = project.get_node(node_id) else {
                    continue;
                };
                let expected = egui::pos2(node.ui_position[0], node.ui_position[1]);
                insert_largest_delta(
                    &mut deltas,
                    SelectionTarget::Node(node_id),
                    position - expected,
                );
            }
            GraphItem::Container(owner) | GraphItem::PortAnchor { owner, .. } => {
                let Some(visual) = container_visual(project, owner) else {
                    continue;
                };
                let expected = container_item_position(&visual, *item);
                insert_largest_delta(
                    &mut deltas,
                    selection_target_for_owner(owner),
                    position - expected,
                );
            }
        }
    }
    deltas.retain(|_, delta| delta.length_sq() > 0.001);
    deltas
}

fn insert_largest_delta(
    deltas: &mut BTreeMap<SelectionTarget, egui::Vec2>,
    target: SelectionTarget,
    delta: egui::Vec2,
) {
    match deltas.get_mut(&target) {
        Some(existing) if delta.length_sq() > existing.length_sq() => *existing = delta,
        Some(_) => {}
        None => {
            deltas.insert(target, delta);
        }
    }
}

fn selection_target_for_owner(owner: PortOwner) -> SelectionTarget {
    match owner {
        PortOwner::Node(id) => SelectionTarget::Node(id),
        PortOwner::Clip(id) => SelectionTarget::Clip(id),
        PortOwner::Track(id) => SelectionTarget::Track(id),
        PortOwner::Composition(id) => SelectionTarget::Composition(id),
    }
}

fn owner_for_selection_target(target: SelectionTarget) -> PortOwner {
    match target {
        SelectionTarget::Node(id) => PortOwner::Node(id),
        SelectionTarget::Clip(id) => PortOwner::Clip(id),
        SelectionTarget::Track(id) => PortOwner::Track(id),
        SelectionTarget::Composition(id) => PortOwner::Composition(id),
    }
}

fn parent_owner(project: &Project, owner: PortOwner) -> Option<PortOwner> {
    match owner {
        PortOwner::Node(id) => project
            .find_node_container(id)
            .map(|container| match container {
                NodeContainer::Composition(id) => PortOwner::Composition(id),
                NodeContainer::Track(id) => PortOwner::Track(id),
                NodeContainer::Clip(id) => PortOwner::Clip(id),
            }),
        PortOwner::Clip(id) => project.find_track_for_clip(id).map(PortOwner::Track),
        PortOwner::Track(id) => project
            .find_composition_for_track(id)
            .map(PortOwner::Composition),
        PortOwner::Composition(_) => None,
    }
}

fn target_is_ancestor(
    project: &Project,
    ancestor: SelectionTarget,
    descendant: SelectionTarget,
) -> bool {
    let ancestor = owner_for_selection_target(ancestor);
    let mut current = parent_owner(project, owner_for_selection_target(descendant));
    while let Some(owner) = current {
        if owner == ancestor {
            return true;
        }
        current = parent_owner(project, owner);
    }
    false
}

fn selection_move_roots(project: &Project, selected: &[SelectionTarget]) -> Vec<SelectionTarget> {
    let mut roots = Vec::new();
    for target in selected.iter().copied() {
        if roots.contains(&target)
            || selected.iter().copied().any(|candidate| {
                candidate != target && target_is_ancestor(project, candidate, target)
            })
        {
            continue;
        }
        roots.push(target);
    }
    roots
}

fn layout_edit_for_delta(
    project: &Project,
    target: SelectionTarget,
    delta: egui::Vec2,
) -> Option<LayoutEdit> {
    match target {
        SelectionTarget::Node(node_id) => {
            let node = project.get_node(node_id)?;
            Some(LayoutEdit::MoveNode {
                node_id,
                position: [node.ui_position[0] + delta.x, node.ui_position[1] + delta.y],
            })
        }
        SelectionTarget::Clip(id) => Some(LayoutEdit::MoveContainer {
            owner: PortOwner::Clip(id),
            delta: [delta.x, delta.y],
        }),
        SelectionTarget::Track(id) => Some(LayoutEdit::MoveContainer {
            owner: PortOwner::Track(id),
            delta: [delta.x, delta.y],
        }),
        SelectionTarget::Composition(id) => Some(LayoutEdit::MoveContainer {
            owner: PortOwner::Composition(id),
            delta: [delta.x, delta.y],
        }),
    }
}

pub(in crate::ui::panels::node_editor) fn layout_needs_reflow(
    project: &Project,
    composition_id: Uuid,
) -> bool {
    let Some(composition) = project.get_composition(composition_id) else {
        return false;
    };
    if !collect_container_hierarchy_violations(project, composition_id).is_empty() {
        return true;
    }

    let mut node_ids = composition.node_ids.clone();
    for track_id in &composition.track_ids {
        let Some(track) = project.get_track(*track_id) else {
            continue;
        };
        node_ids.extend(track.node_ids.iter().copied());
        for clip_id in &track.clip_ids {
            let Some(clip) = project.get_clip(*clip_id) else {
                continue;
            };
            node_ids.extend(clip.node_ids.iter().copied());
        }
    }
    let node_rects = node_ids
        .into_iter()
        .filter_map(|node_id| estimated_node_rect(project, node_id).map(|rect| (node_id, rect)))
        .collect::<Vec<_>>();

    // Startup repair is deliberately limited to invalid geometry. A wire that
    // travels right-to-left can be a valid piece of manual graph authoring;
    // left-to-right flow is an output guarantee of an explicit auto-layout
    // command, not a persistence invariant that may rewrite authored positions.
    node_rects.iter().enumerate().any(|(index, (_, left))| {
        node_rects[index + 1..]
            .iter()
            .any(|(_, right)| padded_intersection(*left, *right))
    })
}

pub(in crate::ui::panels::node_editor) fn container_hierarchy_needs_reflow(
    project: &Project,
    composition_id: Uuid,
) -> bool {
    !collect_container_hierarchy_violations(project, composition_id).is_empty()
}

/// Compare hierarchy validity by stable violation identity and repair
/// magnitude. This allows an edit beside an unrelated legacy defect, while a
/// new defect or a worsened existing defect still rejects atomically.
pub(in crate::ui::panels::node_editor) fn container_hierarchy_regresses(
    baseline: &Project,
    candidate: &Project,
    composition_id: Uuid,
) -> bool {
    let baseline = collect_container_hierarchy_violations(baseline, composition_id);
    let candidate = collect_container_hierarchy_violations(candidate, composition_id);
    hierarchy_violations_regress(&baseline, &candidate)
}

fn hierarchy_violations_regress(
    baseline: &HierarchyViolations,
    candidate: &HierarchyViolations,
) -> bool {
    candidate.iter().any(|(id, severity)| {
        baseline
            .get(id)
            .is_none_or(|baseline| severity.is_worse_than(*baseline))
    })
}

fn collect_container_hierarchy_violations(
    project: &Project,
    composition_id: Uuid,
) -> HierarchyViolations {
    let mut violations = HierarchyViolations::new();
    let Some(composition) = project.get_composition(composition_id) else {
        return violations;
    };
    let composition_item = HierarchyItem::Composition(composition_id);
    let composition_content =
        composition_content_rect(container_rect(composition.ui_position, composition.ui_size));
    let composition_nodes = composition
        .node_ids
        .iter()
        .filter_map(|node_id| estimated_node_rect(project, *node_id).map(|rect| (*node_id, rect)))
        .collect::<Vec<_>>();
    for (node_id, rect) in &composition_nodes {
        record_containment_violation(
            &mut violations,
            composition_item,
            HierarchyItem::Node(*node_id),
            composition_content,
            *rect,
        );
    }

    let mut track_rects = Vec::<(Uuid, egui::Rect)>::new();
    for track_id in &composition.track_ids {
        let Some(track) = project.get_track(*track_id) else {
            continue;
        };
        let track_item = HierarchyItem::Track(*track_id);
        let track_rect = container_rect(track.ui_position, track.ui_size);
        record_containment_violation(
            &mut violations,
            composition_item,
            track_item,
            composition_content,
            track_rect,
        );
        for (other_id, other_rect) in &track_rects {
            record_overlap_violation(
                &mut violations,
                composition_item,
                HierarchyItem::Track(*other_id),
                track_item,
                *other_rect,
                track_rect,
            );
        }
        for (node_id, node_rect) in &composition_nodes {
            record_overlap_violation(
                &mut violations,
                composition_item,
                HierarchyItem::Node(*node_id),
                track_item,
                *node_rect,
                track_rect,
            );
        }
        track_rects.push((*track_id, track_rect));

        let track_content = nested_content_rect(track_rect, AUTO_LAYOUT_TRACK_TOP);
        let track_nodes = track
            .node_ids
            .iter()
            .filter_map(|node_id| {
                estimated_node_rect(project, *node_id).map(|rect| (*node_id, rect))
            })
            .collect::<Vec<_>>();
        for (node_id, rect) in &track_nodes {
            record_containment_violation(
                &mut violations,
                track_item,
                HierarchyItem::Node(*node_id),
                track_content,
                *rect,
            );
        }

        let mut clip_rects = Vec::<(Uuid, egui::Rect)>::new();
        for clip_id in &track.clip_ids {
            let Some(clip) = project.get_clip(*clip_id) else {
                continue;
            };
            let clip_item = HierarchyItem::Clip(*clip_id);
            let clip_rect = container_rect(clip.ui_position, clip.ui_size);
            record_containment_violation(
                &mut violations,
                track_item,
                clip_item,
                track_content,
                clip_rect,
            );
            for (other_id, other_rect) in &clip_rects {
                record_overlap_violation(
                    &mut violations,
                    track_item,
                    HierarchyItem::Clip(*other_id),
                    clip_item,
                    *other_rect,
                    clip_rect,
                );
            }
            for (node_id, node_rect) in &track_nodes {
                record_overlap_violation(
                    &mut violations,
                    track_item,
                    HierarchyItem::Node(*node_id),
                    clip_item,
                    *node_rect,
                    clip_rect,
                );
            }

            let clip_content = nested_content_rect(clip_rect, AUTO_LAYOUT_CLIP_TOP);
            for node_id in &clip.node_ids {
                if let Some(rect) = estimated_node_rect(project, *node_id) {
                    record_containment_violation(
                        &mut violations,
                        clip_item,
                        HierarchyItem::Node(*node_id),
                        clip_content,
                        rect,
                    );
                }
            }
            clip_rects.push((*clip_id, clip_rect));
        }
    }
    violations
}

fn record_containment_violation(
    violations: &mut HierarchyViolations,
    parent: HierarchyItem,
    child: HierarchyItem,
    outer: egui::Rect,
    inner: egui::Rect,
) {
    if rect_contains_rect(outer, inner) {
        return;
    }
    let overflows = [
        (ContainmentSide::Left, outer.left() - inner.left()),
        (ContainmentSide::Top, outer.top() - inner.top()),
        (ContainmentSide::Right, inner.right() - outer.right()),
        (ContainmentSide::Bottom, inner.bottom() - outer.bottom()),
    ];
    for (side, correction) in overflows {
        if correction <= f32::EPSILON {
            continue;
        }
        violations.insert(
            HierarchyViolationId::Outside {
                parent,
                child,
                side,
            },
            HierarchyViolationSeverity { correction },
        );
    }
}

fn record_overlap_violation(
    violations: &mut HierarchyViolations,
    parent: HierarchyItem,
    left_item: HierarchyItem,
    right_item: HierarchyItem,
    left: egui::Rect,
    right: egui::Rect,
) {
    if !padded_intersection(left, right) {
        return;
    }
    let left = left.expand(AUTO_LAYOUT_NODE_PADDING * 0.5);
    let right = right.expand(AUTO_LAYOUT_NODE_PADDING * 0.5);
    let width = (left.right().min(right.right()) - left.left().max(right.left())).max(0.0);
    let height = (left.bottom().min(right.bottom()) - left.top().max(right.top())).max(0.0);
    let (first, second) = if left_item <= right_item {
        (left_item, right_item)
    } else {
        (right_item, left_item)
    };
    // Track both axes independently. Improving one penetration cannot hide a
    // deeper penetration on the other axis; any newly harder separation is a
    // worsened violation and rejects the candidate.
    for (axis, correction) in [
        (OverlapAxis::Horizontal, width),
        (OverlapAxis::Vertical, height),
    ] {
        violations.insert(
            HierarchyViolationId::Overlap {
                parent,
                first,
                second,
                axis,
            },
            HierarchyViolationSeverity { correction },
        );
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::panels::node_editor::test_fixture::fixture;
    use crate::ui::panels::node_editor::PortAnchorKind;

    #[test]
    fn hierarchy_regression_compares_each_side_and_overlap_axis_independently() {
        let parent = HierarchyItem::Composition(Uuid::from_u128(1));
        let child = HierarchyItem::Track(Uuid::from_u128(2));
        let sibling = HierarchyItem::Track(Uuid::from_u128(3));
        let outside = |side| HierarchyViolationId::Outside {
            parent,
            child,
            side,
        };
        let overlap = |axis| HierarchyViolationId::Overlap {
            parent,
            first: child,
            second: sibling,
            axis,
        };
        let severity = |correction| HierarchyViolationSeverity { correction };

        let baseline = BTreeMap::from([
            (outside(ContainmentSide::Left), severity(10.0)),
            (overlap(OverlapAxis::Horizontal), severity(10.0)),
            (overlap(OverlapAxis::Vertical), severity(100.0)),
        ]);
        let containment_tradeoff = BTreeMap::from([
            (outside(ContainmentSide::Left), severity(5.0)),
            (outside(ContainmentSide::Right), severity(9.0)),
        ]);
        assert!(hierarchy_violations_regress(
            &baseline,
            &containment_tradeoff
        ));

        let overlap_tradeoff = BTreeMap::from([
            (overlap(OverlapAxis::Horizontal), severity(5.0)),
            (overlap(OverlapAxis::Vertical), severity(101.0)),
        ]);
        assert!(hierarchy_violations_regress(&baseline, &overlap_tradeoff));
    }

    #[test]
    fn submillipixel_escape_remains_a_hierarchy_violation() {
        let parent = HierarchyItem::Composition(Uuid::from_u128(1));
        let child = HierarchyItem::Node(Uuid::from_u128(2));
        let mut violations = HierarchyViolations::new();
        record_containment_violation(
            &mut violations,
            parent,
            child,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0)),
            egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(100.0005, 90.0)),
        );
        assert!(violations.contains_key(&HierarchyViolationId::Outside {
            parent,
            child,
            side: ContainmentSide::Right,
        }));
    }

    #[test]
    fn dragging_any_port_anchor_moves_its_container_tree_exactly_once() {
        let (mut project, _composition_id, track_id, _clip_id, solid_id, _) = fixture();
        let driver = SelectionTarget::Track(track_id);
        let delta = egui::vec2(37.0, -19.0);
        let item = GraphItem::PortAnchor {
            owner: PortOwner::Track(track_id),
            kind: PortAnchorKind::InternalMetadata,
        };
        let visual = container_visual(&project, PortOwner::Track(track_id)).unwrap();
        let mut snarl = Snarl::new();
        snarl.insert_node(container_item_position(&visual, item) + delta, item);

        let edits = collect_layout_edits_for_selection(&project, &snarl, Some(driver), &[driver]);
        assert_eq!(edits.len(), 1, "one logical container edit: {edits:?}");
        assert!(matches!(
            edits.as_slice(),
            [LayoutEdit::MoveContainer { owner, delta: observed }]
                if *owner == PortOwner::Track(track_id)
                    && *observed == [delta.x, delta.y]
        ));

        let track_before = project.get_track(track_id).unwrap().ui_position;
        let node_before = project.get_node(solid_id).unwrap().ui_position;
        assert!(apply_layout_edit(&mut project, edits[0]));
        assert_eq!(
            project.get_track(track_id).unwrap().ui_position,
            [track_before[0] + delta.x, track_before[1] + delta.y]
        );
        assert_eq!(
            project.get_node(solid_id).unwrap().ui_position,
            [node_before[0] + delta.x, node_before[1] + delta.y]
        );
    }
}
