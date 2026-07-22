//! Project-aware adapter for the domain-neutral hold-A layout gesture.
//!
//! Pointer updates only replace a sparse Snarl display projection. `Project`
//! is validated and written once, under one write lock, on pointer release.

use std::collections::{BTreeMap, HashMap};

use eframe::egui;
use egui_snarl::Snarl;
use library::model::{NodeContainer, Project};
use node_editor_ui::{EditorOutput, LayoutSwipeAxis, LayoutSwipeIntent, LayoutSwipePhase};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::action::HistoryManager;
use crate::state::context_types::{NodeEditorState, SelectionTarget};
use crate::state::node_editor_layout::{
    DirectionalLayoutGestureDiagnostics, DirectionalLayoutGestureDirection,
    DirectionalLayoutGestureMode, DirectionalLayoutGestureOutcome, FrozenNodeGeometry,
    NodeEditorDirectionalLayoutExecution, NodeEditorDirectionalLayoutGesture,
};
use crate::ui::panels::node_editor::surface::SurfaceOutput;
use crate::ui::panels::node_editor::{GraphItem, AUTO_LAYOUT_COLUMN_GAP, AUTO_LAYOUT_ROW_GAP};

use super::{
    BranchDirection, DirectionalLayoutMode, DirectionalLayoutRequest, LayoutAxis,
    NodeLayoutGeometry,
};

const POSITION_EPSILON: f32 = 0.001;

#[derive(Default)]
pub(in crate::ui::panels::node_editor) struct DirectionalLayoutFrameOutcome {
    pub(in crate::ui::panels::node_editor) commit: Option<DirectionalLayoutCommit>,
    pub(in crate::ui::panels::node_editor) owns_pointer: bool,
    pub(in crate::ui::panels::node_editor) request_repaint: bool,
}

pub(in crate::ui::panels::node_editor) struct DirectionalLayoutCommit {
    gesture: NodeEditorDirectionalLayoutGesture,
    positions: BTreeMap<Uuid, [f32; 2]>,
}

pub(in crate::ui::panels::node_editor) struct DirectionalLayoutCommitResult {
    pub(in crate::ui::panels::node_editor) changed: bool,
    pub(in crate::ui::panels::node_editor) request_repaint: bool,
}

/// Recover a cancellation guard before normal input arbitration.
///
/// A release-only frame must remain guarded until its interactions finish,
/// because Snarl may still expose a latent drag in that frame. A stable frame
/// with no primary state proves the release happened outside the window; a
/// fresh press likewise proves this is a new physical gesture.
pub(in crate::ui::panels::node_editor) fn recover_directional_layout_release_guard(
    state: &mut NodeEditorState,
    primary_pressed: bool,
    primary_down: bool,
    primary_released: bool,
) -> bool {
    let safe_to_recover = primary_pressed || (!primary_down && !primary_released);
    if !state.directional_layout_release_guard || !safe_to_recover {
        return false;
    }
    state.directional_layout_release_guard = false;
    true
}

/// Clear a release-only guard after every competing interaction has observed
/// and suppressed that release frame.
pub(in crate::ui::panels::node_editor) fn finish_directional_layout_release_guard(
    state: &mut NodeEditorState,
    primary_released: bool,
) -> bool {
    if !state.directional_layout_release_guard || !primary_released {
        return false;
    }
    state.directional_layout_release_guard = false;
    true
}

struct PlannedDirectionalLayout {
    positions: BTreeMap<Uuid, [f32; 2]>,
    diagnostics: DirectionalLayoutGestureDiagnostics,
    direction: DirectionalLayoutGestureDirection,
}

/// Replace only the transient Snarl positions. The next baseline build uses
/// `Project` again, so cancellation restores the exact display automatically.
pub(in crate::ui::panels::node_editor) fn apply_directional_layout_preview(
    snarl: &mut Snarl<GraphItem>,
    state: &NodeEditorState,
) {
    let Some(gesture) = state.directional_layout_swipe.as_ref() else {
        return;
    };
    for node in snarl.nodes_info_mut() {
        let GraphItem::Node(node_id) = node.value else {
            continue;
        };
        let Some(position) = gesture.preview_positions.get(&node_id) else {
            continue;
        };
        node.pos = egui::pos2(position[0], position[1]);
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the arguments are borrowed pieces of one rendered Node Editor frame"
)]
pub(in crate::ui::panels::node_editor) fn handle_directional_layout_outputs(
    project: &Project,
    composition_id: Uuid,
    selected_targets: &[SelectionTarget],
    rendered_node_rects: &HashMap<Uuid, egui::Rect>,
    outputs: &[SurfaceOutput],
    state: &mut NodeEditorState,
    history: &HistoryManager,
) -> DirectionalLayoutFrameOutcome {
    let mut outcome = DirectionalLayoutFrameOutcome {
        owns_pointer: state.directional_layout_swipe.is_some()
            || state.directional_layout_release_guard,
        ..DirectionalLayoutFrameOutcome::default()
    };
    for intent in outputs.iter().filter_map(|output| {
        let EditorOutput::LayoutSwipe(intent) = output else {
            return None;
        };
        Some(intent)
    }) {
        outcome.owns_pointer = true;
        match intent.phase {
            LayoutSwipePhase::Start => {
                if let Err(reason) = begin_gesture(
                    project,
                    composition_id,
                    selected_targets,
                    rendered_node_rects,
                    intent,
                    state,
                    history,
                ) {
                    reject_without_active(project, composition_id, intent, state, history, reason);
                }
            }
            LayoutSwipePhase::Update => {
                if let Err(reason) = update_gesture(project, intent, state) {
                    finish_cancelled(project, state, history, true, reason);
                }
                outcome.request_repaint = true;
            }
            LayoutSwipePhase::Commit => {
                outcome.commit = prepare_commit(project, intent, state, history);
                outcome.request_repaint = true;
            }
            LayoutSwipePhase::Cancel => {
                finish_cancelled(
                    project,
                    state,
                    history,
                    false,
                    "gesture cancelled before commit".to_string(),
                );
                outcome.request_repaint = true;
            }
        }
    }
    outcome
}

fn begin_gesture(
    project: &Project,
    composition_id: Uuid,
    selected_targets: &[SelectionTarget],
    rendered_node_rects: &HashMap<Uuid, egui::Rect>,
    intent: &LayoutSwipeIntent<Uuid>,
    state: &mut NodeEditorState,
    history: &HistoryManager,
) -> Result<(), String> {
    if state.directional_layout_swipe.is_some() {
        return Err("a directional layout gesture is already active".to_string());
    }
    let direct_owner = project
        .find_node_container(intent.anchor)
        .ok_or_else(|| format!("anchor Node {} has no direct owner", intent.anchor))?;
    if containing_composition(project, direct_owner) != Some(composition_id) {
        return Err("anchor Node is outside the active Composition".to_string());
    }

    let mut baseline_positions = BTreeMap::new();
    let mut frozen_geometry = BTreeMap::new();
    let mut composition_node_ids = project
        .nodes
        .keys()
        .copied()
        .filter(|node_id| {
            project
                .find_node_container(*node_id)
                .and_then(|owner| containing_composition(project, owner))
                == Some(composition_id)
        })
        .collect::<Vec<_>>();
    composition_node_ids.sort_unstable();
    for node_id in composition_node_ids {
        let Some(node) = project.get_node(node_id) else {
            continue;
        };
        let position = node.ui_position;
        let (rect, render_offset, measured) = rendered_node_rects.get(&node_id).map_or_else(
            || {
                let size = super::estimated_node_size(project, node_id);
                (
                    egui::Rect::from_min_size(egui::pos2(position[0], position[1]), size),
                    egui::Vec2::ZERO,
                    false,
                )
            },
            |rect| (*rect, rect.min - egui::pos2(position[0], position[1]), true),
        );
        baseline_positions.insert(node_id, position);
        frozen_geometry.insert(
            node_id,
            FrozenNodeGeometry {
                rect,
                render_offset,
                measured,
            },
        );
    }
    if !frozen_geometry.contains_key(&intent.anchor) {
        return Err(format!(
            "anchor Node {} has no rendered geometry",
            intent.anchor
        ));
    }

    let mut frozen_selected_node_ids = selected_targets
        .iter()
        .filter_map(|target| target.node_id())
        .filter(|node_id| project.find_node_container(*node_id) == Some(direct_owner))
        .collect::<Vec<_>>();
    frozen_selected_node_ids.sort_unstable();
    frozen_selected_node_ids.dedup();

    state.directional_layout_swipe_serial = state.directional_layout_swipe_serial.saturating_add(1);
    state.last_directional_layout_swipe = None;
    state.directional_layout_release_guard = false;
    state.directional_layout_swipe = Some(NodeEditorDirectionalLayoutGesture {
        gesture_id: state.directional_layout_swipe_serial,
        composition_id,
        direct_owner,
        anchor_node_id: intent.anchor,
        frozen_selected_node_ids,
        baseline_positions,
        frozen_geometry,
        preview_positions: BTreeMap::new(),
        start: intent.start,
        current: intent.current,
        axis: intent.axis,
        direction: None,
        mode: DirectionalLayoutGestureMode::from_modifiers(intent.modifiers),
        modifiers: intent.modifiers,
        canvas_transform: intent.transform,
        project_revision: project_revision(project)?,
        history_undo_depth: history.undo_depth(),
        history_redo_depth: history.redo_depth(),
        diagnostics: DirectionalLayoutGestureDiagnostics::default(),
    });
    Ok(())
}

fn update_gesture(
    project: &Project,
    intent: &LayoutSwipeIntent<Uuid>,
    state: &mut NodeEditorState,
) -> Result<(), String> {
    let gesture = matching_gesture_mut(intent, state)?;
    gesture.current = intent.current;
    gesture.axis = intent.axis;
    let plan = plan_gesture(project, gesture)?;
    gesture.preview_positions = plan.positions;
    gesture.diagnostics = plan.diagnostics;
    gesture.direction = Some(plan.direction);
    Ok(())
}

fn prepare_commit(
    project: &Project,
    intent: &LayoutSwipeIntent<Uuid>,
    state: &mut NodeEditorState,
    history: &HistoryManager,
) -> Option<DirectionalLayoutCommit> {
    let mut gesture = match state.directional_layout_swipe.take() {
        Some(gesture) => gesture,
        None => {
            reject_without_active(
                project,
                Uuid::nil(),
                intent,
                state,
                history,
                "commit has no active directional layout gesture".to_string(),
            );
            return None;
        }
    };
    if let Err(reason) = validate_intent(&gesture, intent) {
        record_execution(
            project,
            state,
            history,
            gesture,
            DirectionalLayoutGestureOutcome::Rejected,
            Some(reason),
            Vec::new(),
        );
        return None;
    }
    gesture.current = intent.current;
    gesture.axis = intent.axis;
    match plan_gesture(project, &gesture) {
        Ok(plan) => {
            gesture.preview_positions = plan.positions.clone();
            gesture.diagnostics = plan.diagnostics;
            gesture.direction = Some(plan.direction);
            Some(DirectionalLayoutCommit {
                gesture,
                positions: plan.positions,
            })
        }
        Err(reason) => {
            record_execution(
                project,
                state,
                history,
                gesture,
                DirectionalLayoutGestureOutcome::Rejected,
                Some(reason),
                Vec::new(),
            );
            None
        }
    }
}

fn matching_gesture_mut<'a>(
    intent: &LayoutSwipeIntent<Uuid>,
    state: &'a mut NodeEditorState,
) -> Result<&'a mut NodeEditorDirectionalLayoutGesture, String> {
    let gesture = state
        .directional_layout_swipe
        .as_mut()
        .ok_or_else(|| "directional layout update has no active gesture".to_string())?;
    validate_intent(gesture, intent)?;
    Ok(gesture)
}

fn validate_intent(
    gesture: &NodeEditorDirectionalLayoutGesture,
    intent: &LayoutSwipeIntent<Uuid>,
) -> Result<(), String> {
    if gesture.anchor_node_id != intent.anchor
        || gesture.start != intent.start
        || gesture.modifiers != intent.modifiers
        || gesture.canvas_transform != intent.transform
    {
        return Err("directional layout intent does not match the frozen gesture".to_string());
    }
    Ok(())
}

fn plan_gesture(
    project: &Project,
    gesture: &NodeEditorDirectionalLayoutGesture,
) -> Result<PlannedDirectionalLayout, String> {
    let axis = gesture
        .axis
        .ok_or_else(|| "directional layout did not cross its activation threshold".to_string())?;
    let direction = gesture_direction(axis, gesture.current - gesture.start)?;
    let geometry = gesture
        .frozen_geometry
        .iter()
        .map(|(node_id, geometry)| {
            (
                *node_id,
                NodeLayoutGeometry {
                    position: [geometry.rect.min.x, geometry.rect.min.y],
                    size: [geometry.rect.width(), geometry.rect.height()],
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    // Selecting only the anchor is equivalent to an unconstrained branch.
    // Additional selected Nodes constrain movement after reachability.
    let selected = gesture
        .frozen_selected_node_ids
        .iter()
        .copied()
        .filter(|node_id| *node_id != gesture.anchor_node_id)
        .collect::<Vec<_>>();
    let request = DirectionalLayoutRequest {
        composition_id: gesture.composition_id,
        direct_owner: gesture.direct_owner,
        anchor_node_id: gesture.anchor_node_id,
        frozen_selected_node_ids: &selected,
        fixed_node_ids: &[],
        direction: match direction {
            DirectionalLayoutGestureDirection::Upstream => BranchDirection::Upstream,
            DirectionalLayoutGestureDirection::Downstream => BranchDirection::Downstream,
        },
        axis: match axis {
            LayoutSwipeAxis::Horizontal => LayoutAxis::Horizontal,
            LayoutSwipeAxis::Vertical => LayoutAxis::Vertical,
        },
        mode: match gesture.mode {
            DirectionalLayoutGestureMode::Layout => DirectionalLayoutMode::Layout,
            DirectionalLayoutGestureMode::Align => DirectionalLayoutMode::Align,
            DirectionalLayoutGestureMode::Distribute => DirectionalLayoutMode::Distribute,
            DirectionalLayoutGestureMode::AlignAndDistribute => {
                DirectionalLayoutMode::AlignAndDistribute
            }
        },
        node_geometry: &geometry,
        horizontal_gap: AUTO_LAYOUT_COLUMN_GAP,
        vertical_gap: AUTO_LAYOUT_ROW_GAP,
    };
    let plan = super::plan_directional_layout(project, &request)
        .map_err(|error| format!("cannot plan directional layout: {error}"))?;
    let mut positions = BTreeMap::new();
    for (node_id, rect_position) in &plan.node_positions {
        let geometry = gesture
            .frozen_geometry
            .get(node_id)
            .ok_or_else(|| format!("Node {node_id} lost its frozen geometry"))?;
        let position = [
            rect_position[0] - geometry.render_offset.x,
            rect_position[1] - geometry.render_offset.y,
        ];
        if !position.into_iter().all(f32::is_finite) {
            return Err(format!("Node {node_id} planned a non-finite position"));
        }
        positions.insert(*node_id, position);
    }
    let diagnostics = DirectionalLayoutGestureDiagnostics {
        reachable_node_ids: plan.diagnostics.reachable_node_ids,
        eligible_node_ids: plan.diagnostics.eligible_node_ids,
        moved_node_ids: plan.diagnostics.moved_node_ids,
        blocked_node_ids: plan
            .diagnostics
            .blocked_nodes
            .into_iter()
            .map(|blocked| blocked.node_id)
            .collect(),
    };
    Ok(PlannedDirectionalLayout {
        positions,
        diagnostics,
        direction,
    })
}

fn gesture_direction(
    axis: LayoutSwipeAxis,
    displacement: egui::Vec2,
) -> Result<DirectionalLayoutGestureDirection, String> {
    let signed = match axis {
        LayoutSwipeAxis::Horizontal => displacement.x,
        LayoutSwipeAxis::Vertical => displacement.y,
    };
    if !signed.is_finite() || signed.abs() <= f32::EPSILON {
        return Err("directional layout displacement is invalid".to_string());
    }
    Ok(if signed < 0.0 {
        DirectionalLayoutGestureDirection::Upstream
    } else {
        DirectionalLayoutGestureDirection::Downstream
    })
}

pub(in crate::ui::panels::node_editor) fn apply_directional_layout_commit(
    project: &mut Project,
    state: &mut NodeEditorState,
    history: &mut HistoryManager,
    commit: DirectionalLayoutCommit,
) -> DirectionalLayoutCommitResult {
    let gesture = commit.gesture;
    let rejection = validate_commit_project(project, &gesture, &commit.positions).err();
    if let Some(reason) = rejection {
        record_execution(
            project,
            state,
            history,
            gesture,
            DirectionalLayoutGestureOutcome::Rejected,
            Some(reason),
            Vec::new(),
        );
        return DirectionalLayoutCommitResult {
            changed: false,
            request_repaint: true,
        };
    }

    let mut moved_node_ids = Vec::new();
    for (node_id, position) in &commit.positions {
        let Some(node) = project.get_node_mut(*node_id) else {
            // Every target was validated under this same write lock.
            continue;
        };
        if positions_differ(node.ui_position, *position) {
            node.ui_position = *position;
            moved_node_ids.push(*node_id);
        }
    }
    moved_node_ids.sort_unstable();
    let changed = !moved_node_ids.is_empty();
    if changed {
        history.push_project_state(project.clone());
    }
    record_execution(
        project,
        state,
        history,
        gesture,
        DirectionalLayoutGestureOutcome::Committed,
        None,
        moved_node_ids,
    );
    DirectionalLayoutCommitResult {
        changed,
        request_repaint: true,
    }
}

fn validate_commit_project(
    project: &Project,
    gesture: &NodeEditorDirectionalLayoutGesture,
    positions: &BTreeMap<Uuid, [f32; 2]>,
) -> Result<(), String> {
    if project_revision(project)? != gesture.project_revision {
        return Err("authoritative Project changed during directional layout".to_string());
    }
    if containing_composition(project, gesture.direct_owner) != Some(gesture.composition_id) {
        return Err("direct owner left the frozen Composition".to_string());
    }
    if project.find_node_container(gesture.anchor_node_id) != Some(gesture.direct_owner) {
        return Err("anchor Node left its frozen direct owner".to_string());
    }
    for (node_id, position) in positions {
        let Some(node) = project.get_node(*node_id) else {
            return Err(format!("planned Node {node_id} was deleted"));
        };
        if project.find_node_container(*node_id) != Some(gesture.direct_owner) {
            return Err(format!("planned Node {node_id} changed direct owner"));
        }
        if gesture.baseline_positions.get(node_id) != Some(&node.ui_position) {
            return Err(format!(
                "planned Node {node_id} changed position concurrently"
            ));
        }
        if !position.iter().all(|coordinate| coordinate.is_finite()) {
            return Err(format!("planned Node {node_id} has a non-finite position"));
        }
    }
    Ok(())
}

fn finish_cancelled(
    project: &Project,
    state: &mut NodeEditorState,
    history: &HistoryManager,
    rejected: bool,
    reason: String,
) {
    let Some(gesture) = state.directional_layout_swipe.take() else {
        return;
    };
    state.directional_layout_release_guard = true;
    record_execution(
        project,
        state,
        history,
        gesture,
        if rejected {
            DirectionalLayoutGestureOutcome::Rejected
        } else {
            DirectionalLayoutGestureOutcome::Cancelled
        },
        Some(reason),
        Vec::new(),
    );
}

fn reject_without_active(
    project: &Project,
    composition_id: Uuid,
    intent: &LayoutSwipeIntent<Uuid>,
    state: &mut NodeEditorState,
    history: &HistoryManager,
    reason: String,
) {
    state.directional_layout_release_guard = true;
    state.directional_layout_swipe_serial = state.directional_layout_swipe_serial.saturating_add(1);
    let revision = project_revision(project).unwrap_or_else(|error| format!("unavailable:{error}"));
    state.last_directional_layout_swipe = Some(NodeEditorDirectionalLayoutExecution {
        gesture_id: state.directional_layout_swipe_serial,
        outcome: DirectionalLayoutGestureOutcome::Rejected,
        reason: Some(reason),
        composition_id,
        direct_owner: NodeContainer::Composition(composition_id),
        anchor_node_id: intent.anchor,
        axis: intent.axis,
        direction: None,
        mode: DirectionalLayoutGestureMode::from_modifiers(intent.modifiers),
        moved_node_ids: Vec::new(),
        project_revision_before: revision.clone(),
        project_revision_after: revision,
        history_undo_before: history.undo_depth(),
        history_undo_after: history.undo_depth(),
        history_redo_before: history.redo_depth(),
        history_redo_after: history.redo_depth(),
    });
}

fn record_execution(
    project: &Project,
    state: &mut NodeEditorState,
    history: &HistoryManager,
    gesture: NodeEditorDirectionalLayoutGesture,
    outcome: DirectionalLayoutGestureOutcome,
    reason: Option<String>,
    moved_node_ids: Vec<Uuid>,
) {
    let after = project_revision(project).unwrap_or_else(|error| format!("unavailable:{error}"));
    state.last_directional_layout_swipe = Some(NodeEditorDirectionalLayoutExecution {
        gesture_id: gesture.gesture_id,
        outcome,
        reason,
        composition_id: gesture.composition_id,
        direct_owner: gesture.direct_owner,
        anchor_node_id: gesture.anchor_node_id,
        axis: gesture.axis,
        direction: gesture.direction,
        mode: gesture.mode,
        moved_node_ids,
        project_revision_before: gesture.project_revision,
        project_revision_after: after,
        history_undo_before: gesture.history_undo_depth,
        history_undo_after: history.undo_depth(),
        history_redo_before: gesture.history_redo_depth,
        history_redo_after: history.redo_depth(),
    });
}

fn project_revision(project: &Project) -> Result<String, String> {
    let bytes = serde_json::to_vec(project)
        .map_err(|error| format!("cannot fingerprint authoritative Project: {error}"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn containing_composition(project: &Project, owner: NodeContainer) -> Option<Uuid> {
    match owner {
        NodeContainer::Composition(id) => project.get_composition(id).map(|_| id),
        NodeContainer::Track(id) => project.find_composition_for_track(id),
        NodeContainer::Clip(id) => project
            .find_track_for_clip(id)
            .and_then(|track_id| project.find_composition_for_track(track_id)),
    }
}

fn positions_differ(left: [f32; 2], right: [f32; 2]) -> bool {
    (left[0] - right[0]).abs() > POSITION_EPSILON || (left[1] - right[1]).abs() > POSITION_EPSILON
}

#[cfg(test)]
#[path = "swipe_tests.rs"]
mod tests;
