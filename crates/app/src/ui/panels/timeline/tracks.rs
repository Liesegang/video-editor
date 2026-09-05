//! Track-header gestures project the existing row blocks, then commit one order edit.

use egui::{Pos2, Rect};
use library::editor::TimelineEditorService;
use library::model::authoring::{AuthoringProject, TimelineTrackId};

use super::rows::{DisplayRow, RowKind};
use super::viewport::row_top;
use crate::state::authoring::{AuthoringSelection, AuthoringUiState, TimelineTrackGesture};

pub(super) fn begin_gesture(
    ui: &egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    track_id: TimelineTrackId,
    response: &egui::Response,
) {
    if !response.drag_started_by(egui::PointerButton::Primary)
        || egui::Popup::is_any_open(ui.ctx())
        || state.timeline.item_gesture.is_some()
        || state.timeline.keyframe_gesture.is_some()
    {
        return;
    }
    let Some(timeline) = project.timelines.get(&state.active_timeline_id) else {
        return;
    };
    let Some(index) = timeline.track_order.iter().position(|id| *id == track_id) else {
        return;
    };
    state.selection.replace(AuthoringSelection::Track(track_id));
    state.timeline.track_gesture = Some(TimelineTrackGesture {
        timeline_id: timeline.id,
        track_id,
        original_order: timeline.track_order.clone(),
        projected_index: index,
    });
    ui.ctx().request_repaint();
}

pub(super) fn update_projection(
    ui: &egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    canonical_rows: &[DisplayRow],
    content_rect: Rect,
) {
    let Some(gesture) = state.timeline.track_gesture.as_ref() else {
        return;
    };
    let valid_origin = gesture.timeline_id == state.active_timeline_id
        && project
            .timelines
            .get(&gesture.timeline_id)
            .is_some_and(|timeline| timeline.track_order == gesture.original_order);
    let active = ui.input(|input| input.pointer.primary_down() || input.pointer.primary_released());
    if !valid_origin
        || !active
        || ui.input(|input| input.key_pressed(egui::Key::Escape))
        || egui::Popup::is_any_open(ui.ctx())
    {
        state.timeline.track_gesture = None;
        return;
    }
    let Some(pointer) = ui.ctx().pointer_latest_pos() else {
        return;
    };
    let header_positions = canonical_rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| match row.kind {
            RowKind::Track { track_id, .. } => {
                Some((track_id, row_top(content_rect, &state.timeline, index)))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let Some(target) = track_at_y(&header_positions, pointer) else {
        return;
    };
    if let Some(gesture) = state.timeline.track_gesture.as_mut() {
        if let Some(index) = gesture.original_order.iter().position(|id| *id == target) {
            gesture.projected_index = index;
        }
    }
    ui.ctx().request_repaint();
}

fn track_at_y(headers: &[(TimelineTrackId, f32)], pointer: Pos2) -> Option<TimelineTrackId> {
    headers
        .iter()
        .rev()
        .find(|(_, top)| pointer.y >= *top)
        .or_else(|| headers.first())
        .map(|(id, _)| *id)
}

pub(super) fn project_rows(
    rows: Vec<DisplayRow>,
    gesture: Option<&TimelineTrackGesture>,
) -> Vec<DisplayRow> {
    let Some(gesture) = gesture else { return rows };
    let mut blocks: Vec<Vec<DisplayRow>> = Vec::new();
    for row in rows {
        if matches!(row.kind, RowKind::Track { .. }) {
            blocks.push(vec![row]);
        } else if let Some(block) = blocks.last_mut() {
            block.push(row);
        }
    }
    if let Some(old_index) = blocks.iter().position(|block| {
        matches!(block.first().map(|row| &row.kind),
            Some(RowKind::Track { track_id, .. }) if *track_id == gesture.track_id)
    }) {
        let target = blocks
            .len()
            .saturating_sub(1)
            .saturating_sub(gesture.projected_index);
        let moved = blocks.remove(old_index);
        blocks.insert(target.min(blocks.len()), moved);
    }
    blocks.into_iter().flatten().collect()
}

pub(super) fn finish_gesture(
    ui: &egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
) {
    if !ui.input(|input| input.pointer.primary_released()) {
        return;
    }
    let Some(gesture) = state.timeline.track_gesture.take() else {
        return;
    };
    let Some(timeline) = project.timelines.get(&gesture.timeline_id) else {
        return;
    };
    if timeline.track_order != gesture.original_order
        || gesture.original_order.get(gesture.projected_index) == Some(&gesture.track_id)
    {
        return;
    }
    match service.reorder_track(
        gesture.timeline_id,
        gesture.track_id,
        gesture.projected_index,
    ) {
        Ok(_) => state.status = "Reordered Track".to_string(),
        Err(error) => state.error = Some(error.to_string()),
    }
}

pub(super) fn register_projection_qa(
    rows: &[DisplayRow],
    gesture: Option<&TimelineTrackGesture>,
    sidebar: Rect,
) {
    if !crate::qa::is_enabled() {
        return;
    }
    let Some(gesture) = gesture else { return };
    let displayed_order = rows
        .iter()
        .filter_map(|row| match row.kind {
            RowKind::Track { track_id, .. } => Some(track_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    crate::qa::register_component_with_metadata(
        "timeline.track_reorder_preview",
        "track_reorder_preview",
        sidebar,
        true,
        Some(serde_json::json!({
            "track_id": gesture.track_id,
            "original_order": gesture.original_order,
            "displayed_order": displayed_order,
            "projected_index": gesture.projected_index,
            "committed": false,
        })),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::authoring::{TimelineId, TimelineItemId};

    #[test]
    fn track_projection_moves_the_whole_expanded_block_without_mutating_origin() {
        let back = TimelineTrackId::new();
        let front = TimelineTrackId::new();
        let child = TimelineItemId::new();
        let rows = vec![
            DisplayRow {
                kind: RowKind::Track {
                    track_id: front,
                    expanded: true,
                },
            },
            DisplayRow {
                kind: RowKind::Clip {
                    track_id: front,
                    item_id: child,
                },
            },
            DisplayRow {
                kind: RowKind::Track {
                    track_id: back,
                    expanded: false,
                },
            },
        ];
        let gesture = TimelineTrackGesture {
            timeline_id: TimelineId::new(),
            track_id: back,
            original_order: vec![back, front],
            projected_index: 1,
        };
        let projected = project_rows(rows, Some(&gesture));
        assert!(matches!(projected[0].kind, RowKind::Track { track_id, .. } if track_id == back));
        assert!(matches!(projected[1].kind, RowKind::Track { track_id, .. } if track_id == front));
        assert!(matches!(projected[2].kind, RowKind::Clip { item_id, .. } if item_id == child));
        assert_eq!(gesture.original_order, vec![back, front]);
    }

    #[test]
    fn target_uses_original_blocks_so_reordering_cannot_oscillate_under_pointer() {
        let a = TimelineTrackId::new();
        let b = TimelineTrackId::new();
        let headers = [(a, 100.0), (b, 220.0)];
        assert_eq!(track_at_y(&headers, Pos2::new(0.0, 70.0)), Some(a));
        assert_eq!(track_at_y(&headers, Pos2::new(0.0, 180.0)), Some(a));
        assert_eq!(track_at_y(&headers, Pos2::new(0.0, 240.0)), Some(b));
    }
}
