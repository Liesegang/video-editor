//! Track-header gestures project the existing row blocks, then commit one order edit.

use egui::{Pos2, Rect};
use library::editor::TimelineEditorService;
use library::model::authoring::{AuthoringProject, ProjectRevision, TimelineTrackId};

use super::rows::{DisplayRow, RowKind};
use super::viewport::row_top;
use crate::state::authoring::{AuthoringSelection, AuthoringUiState, TimelineTrackGesture};

pub(super) fn begin_gesture(
    ui: &egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    track_id: TimelineTrackId,
    response: &egui::Response,
    source_revision: ProjectRevision,
) {
    if !response.drag_started_by(egui::PointerButton::Primary)
        || ui.input(|input| input.key_pressed(egui::Key::Escape))
        || egui::Popup::is_any_open(ui.ctx())
        || state.timeline.track_gesture.is_some()
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
    // Keep the revision captured atomically with the displayed Project. A
    // different dock panel may already have committed after that snapshot.
    state.timeline.track_gesture = Some(TimelineTrackGesture {
        timeline_id: timeline.id,
        track_id,
        source_revision,
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
    project_revision: ProjectRevision,
) {
    let Some(gesture) = state.timeline.track_gesture.as_ref() else {
        return;
    };
    let valid_origin = gesture.timeline_id == state.active_timeline_id
        && project_revision == gesture.source_revision
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
        || service.revision().ok() != Some(gesture.source_revision)
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
    let displayed_rows = rows
        .iter()
        .enumerate()
        .map(|(index, row)| match &row.kind {
            RowKind::Track { track_id, .. } => serde_json::json!({
                "kind": "track", "track_id": track_id, "display_row_index": index,
            }),
            RowKind::Clip { track_id, item_id } => serde_json::json!({
                "kind": "clip", "track_id": track_id, "item_id": item_id,
                "display_row_index": index,
            }),
            RowKind::Property { item_id, lane } => serde_json::json!({
                "kind": "property", "item_id": item_id,
                "target": crate::ui::automation_lanes::target_metadata(&lane.target),
                "display_row_index": index,
            }),
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
            "rows": displayed_rows,
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
            source_revision: library::model::authoring::ProjectRevision::initial(),
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

    struct HeaderDragFixture {
        service: TimelineEditorService,
        state: AuthoringUiState,
        source: TimelineTrackId,
        frame: usize,
        edit_before_header: bool,
    }

    const CONTENT: Rect = Rect::from_min_max(Pos2::new(200.0, 100.0), Pos2::new(600.0, 450.0));
    const START: Pos2 = Pos2::new(90.0, 116.0);
    const TARGET: Pos2 = Pos2::new(90.0, 170.0);

    fn drag_fixture() -> HeaderDragFixture {
        let (project, back, _) = super::super::tests::fixture();
        let timeline_id = project.root_timeline_id;
        let service = TimelineEditorService::new(project).expect("Track service");
        let (source, _) = service
            .add_track(
                timeline_id,
                "Front Track".to_string(),
                library::model::authoring::TimelineTrackKind::AudioVisual,
            )
            .expect("second Track");
        let mut state = AuthoringUiState::new(timeline_id);
        state.timeline.expanded_tracks.extend([back, source]);
        HeaderDragFixture {
            service,
            state,
            source,
            frame: 0,
            edit_before_header: false,
        }
    }

    fn button(position: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn escape() -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn input_frame(
        context: &egui::Context,
        fixture: &mut HeaderDragFixture,
        events: Vec<egui::Event>,
    ) {
        let (project, revision) = fixture
            .service
            .snapshot_with_revision()
            .expect("Project frame");
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 480.0))),
                time: Some(fixture.frame as f64 / 60.0),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let rows = super::super::rows::display_rows(
                        &project,
                        fixture.state.active_timeline_id,
                        &fixture.state.timeline.expanded_tracks,
                        &fixture.state.timeline.expanded_items,
                        None,
                    );
                    update_projection(ui, &project, &mut fixture.state, &rows, CONTENT, revision);
                    let rows = project_rows(rows, fixture.state.timeline.track_gesture.as_ref());
                    if std::mem::take(&mut fixture.edit_before_header) {
                        fixture
                            .service
                            .set_track_visual_enabled(fixture.source, false)
                            .expect("another panel edited after the frame snapshot");
                    }
                    for (index, row) in rows.iter().enumerate() {
                        let RowKind::Track { track_id, .. } = row.kind else {
                            continue;
                        };
                        let y = row_top(CONTENT, &fixture.state.timeline, index);
                        let rect = Rect::from_min_size(Pos2::new(30.0, y), egui::vec2(110.0, 32.0));
                        let response = ui.interact(
                            rect,
                            ui.id().with(("track", track_id)),
                            egui::Sense::click_and_drag(),
                        );
                        begin_gesture(
                            ui,
                            &project,
                            &mut fixture.state,
                            track_id,
                            &response,
                            revision,
                        );
                    }
                    finish_gesture(ui, &project, &mut fixture.state, &fixture.service);
                });
            },
        ));
        fixture.frame += 1;
    }

    fn press_header(context: &egui::Context, fixture: &mut HeaderDragFixture) {
        input_frame(context, fixture, Vec::new());
        input_frame(
            context,
            fixture,
            vec![egui::Event::PointerMoved(START), button(START, true)],
        );
    }

    #[test]
    fn escape_on_the_first_drag_frame_cannot_recreate_a_track_gesture() {
        let context = egui::Context::default();
        let mut fixture = drag_fixture();
        let before = fixture.service.snapshot_with_revision().expect("origin");
        press_header(&context, &mut fixture);
        input_frame(
            &context,
            &mut fixture,
            vec![egui::Event::PointerMoved(TARGET), escape()],
        );
        assert!(
            fixture.state.timeline.track_gesture.is_none(),
            "Escape must win over drag_started in the same frame"
        );
        input_frame(&context, &mut fixture, vec![button(TARGET, false)]);
        assert_eq!(
            fixture
                .service
                .snapshot_with_revision()
                .expect("after Escape"),
            before
        );
    }

    #[test]
    fn changed_project_revision_cancels_a_held_track_drag_even_if_order_is_unchanged() {
        let context = egui::Context::default();
        let mut fixture = drag_fixture();
        press_header(&context, &mut fixture);
        input_frame(
            &context,
            &mut fixture,
            vec![egui::Event::PointerMoved(TARGET)],
        );
        input_frame(&context, &mut fixture, Vec::new());
        assert_eq!(
            fixture
                .state
                .timeline
                .track_gesture
                .as_ref()
                .expect("held")
                .projected_index,
            0
        );
        fixture
            .service
            .set_track_visual_enabled(fixture.source, false)
            .expect("independent visibility edit");
        let edited = fixture
            .service
            .snapshot_with_revision()
            .expect("edited Project");
        input_frame(&context, &mut fixture, vec![button(TARGET, false)]);
        assert!(fixture.state.timeline.track_gesture.is_none());
        assert_eq!(
            fixture
                .service
                .snapshot_with_revision()
                .expect("cancelled stale drag"),
            edited
        );
    }

    #[test]
    fn drag_origin_keeps_the_displayed_snapshot_revision_after_another_panel_edits() {
        let context = egui::Context::default();
        let mut fixture = drag_fixture();
        press_header(&context, &mut fixture);
        let original_revision = fixture.service.revision().expect("displayed revision");
        fixture.edit_before_header = true;
        input_frame(
            &context,
            &mut fixture,
            vec![egui::Event::PointerMoved(TARGET)],
        );
        assert_eq!(
            fixture
                .state
                .timeline
                .track_gesture
                .as_ref()
                .expect("gesture from displayed Project")
                .source_revision,
            original_revision
        );
        let edited = fixture
            .service
            .snapshot_with_revision()
            .expect("newer service revision");
        assert_ne!(edited.1, original_revision);
        input_frame(&context, &mut fixture, vec![button(TARGET, false)]);
        assert!(fixture.state.timeline.track_gesture.is_none());
        assert_eq!(
            fixture
                .service
                .snapshot_with_revision()
                .expect("stale start cancelled"),
            edited
        );
    }

    #[test]
    fn held_track_projection_stays_stable_and_release_is_one_undoable_edit() {
        let context = egui::Context::default();
        let mut fixture = drag_fixture();
        let before = fixture.service.snapshot_with_revision().expect("origin");
        press_header(&context, &mut fixture);
        input_frame(
            &context,
            &mut fixture,
            vec![egui::Event::PointerMoved(TARGET)],
        );
        for _ in 0..8 {
            input_frame(&context, &mut fixture, Vec::new());
            assert_eq!(
                fixture
                    .state
                    .timeline
                    .track_gesture
                    .as_ref()
                    .expect("held")
                    .projected_index,
                0
            );
            assert_eq!(
                fixture
                    .service
                    .snapshot_with_revision()
                    .expect("held origin"),
                before
            );
        }
        input_frame(&context, &mut fixture, vec![button(TARGET, false)]);
        let after = fixture.service.snapshot_with_revision().expect("committed");
        assert_eq!(after.1.get(), before.1.get() + 1);
        assert_eq!(
            after.0.timelines[&after.0.root_timeline_id].track_order[0],
            fixture.source
        );
        assert_eq!(after.0.items, before.0.items);
        assert!(fixture.state.timeline.track_gesture.is_none());
        fixture.service.undo().expect("single Undo");
        assert_eq!(fixture.service.snapshot().expect("restored"), before.0);
    }

    #[test]
    fn releasing_back_at_the_origin_is_a_noop_even_after_a_held_reorder() {
        let context = egui::Context::default();
        let mut fixture = drag_fixture();
        let before = fixture.service.snapshot_with_revision().expect("origin");
        press_header(&context, &mut fixture);
        input_frame(
            &context,
            &mut fixture,
            vec![egui::Event::PointerMoved(TARGET)],
        );
        input_frame(&context, &mut fixture, Vec::new());
        input_frame(
            &context,
            &mut fixture,
            vec![egui::Event::PointerMoved(START), button(START, false)],
        );
        assert!(fixture.state.timeline.track_gesture.is_none());
        assert_eq!(
            fixture
                .service
                .snapshot_with_revision()
                .expect("no-op release"),
            before
        );
    }

    #[test]
    fn release_rechecks_revision_if_another_edit_lands_after_projection() {
        let context = egui::Context::default();
        let mut fixture = drag_fixture();
        press_header(&context, &mut fixture);
        input_frame(
            &context,
            &mut fixture,
            vec![egui::Event::PointerMoved(TARGET)],
        );
        input_frame(&context, &mut fixture, Vec::new());
        let old_project = fixture
            .service
            .snapshot()
            .expect("project used by this UI frame");
        fixture
            .service
            .set_track_visual_enabled(fixture.source, false)
            .expect("edit after projection");
        let edited = fixture.service.snapshot_with_revision().expect("edited");
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 480.0))),
                time: Some(fixture.frame as f64 / 60.0),
                events: vec![button(TARGET, false)],
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    finish_gesture(ui, &old_project, &mut fixture.state, &fixture.service);
                });
            },
        ));
        assert!(fixture.state.timeline.track_gesture.is_none());
        assert_eq!(
            fixture
                .service
                .snapshot_with_revision()
                .expect("stale release rejected"),
            edited
        );
    }
}
