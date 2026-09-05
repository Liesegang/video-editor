use std::collections::HashSet;

use egui::{Color32, Pos2, Rect, Stroke, StrokeKind};
use library::model::authoring::{ordered_track_item_ids, AuthoringProject, TimelineItemId};

use crate::state::authoring::{AuthoringSelection, AuthoringUiState, TimelineSelectionGesture};
use crate::ui::selection::{
    get_box_action, get_click_action, BoxAction, ClickAction, SelectionAction,
};

use super::geometry::{clip_rect, TimelineRowMetrics};
use super::{DisplayRow, RowKind};

pub(super) fn apply_item_click_selection(
    state: &mut AuthoringUiState,
    item_id: TimelineItemId,
    modifiers: egui::Modifiers,
) {
    match get_click_action(&modifiers, Some(AuthoringSelection::Item(item_id))) {
        ClickAction::Select(selection) => state.selection.replace(selection),
        ClickAction::Add(selection) => state.selection.add(selection),
        ClickAction::Remove(selection) => {
            state.selection.remove(selection);
        }
        ClickAction::Toggle(selection) => state.selection.toggle(selection),
        ClickAction::Clear | ClickAction::DoNothing => {}
    }
}

pub(super) fn prepare_item_drag_selection(
    state: &mut AuthoringUiState,
    item_id: TimelineItemId,
    modifiers: egui::Modifiers,
) -> bool {
    let selection = AuthoringSelection::Item(item_id);
    let should_drag = match SelectionAction::from_modifiers(&modifiers) {
        SelectionAction::Remove => {
            state.selection.remove(selection);
            false
        }
        SelectionAction::Add | SelectionAction::Toggle => {
            state.selection.add(selection);
            true
        }
        SelectionAction::Replace => {
            if !state.selection.contains(selection) {
                state.selection.replace(selection);
            }
            true
        }
    };
    if should_drag {
        state.selection.make_primary(selection);
    }
    should_drag
}

pub(super) fn handle_marquee_selection(
    ui: &egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    rows: &[DisplayRow],
    content_rect: Rect,
    navigation: Option<(egui::Response, bool)>,
) {
    let Some((response, handled_pan)) = navigation else {
        return;
    };
    let pointer = response
        .interact_pointer_pos()
        .or_else(|| ui.ctx().pointer_latest_pos());

    if !handled_pan
        && state.timeline.item_gesture.is_none()
        && state.timeline.keyframe_gesture.is_none()
        && state.timeline.track_gesture.is_none()
        && response.drag_started_by(egui::PointerButton::Primary)
        && !ui.input(|input| input.modifiers.alt)
    {
        if let Some(start) = pointer.filter(|point| content_rect.contains(*point)) {
            state.timeline.selection_gesture = Some(TimelineSelectionGesture {
                start,
                modifiers: ui.input(|input| input.modifiers),
            });
        }
    }

    if let Some(gesture) = state.timeline.selection_gesture {
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            state.timeline.selection_gesture = None;
            ui.ctx().request_repaint();
            return;
        }
        let current = pointer.unwrap_or(gesture.start);
        let selection_rect = Rect::from_two_pos(gesture.start, current).intersect(content_rect);
        if selection_rect.is_positive() {
            let painter = ui.painter().with_clip_rect(content_rect);
            painter.rect_filled(
                selection_rect,
                0.0,
                Color32::from_rgba_premultiplied(100, 200, 255, 30),
            );
            painter.rect_stroke(
                selection_rect,
                0.0,
                Stroke::new(1.0, Color32::from_rgb(100, 200, 255)),
                StrokeKind::Middle,
            );
            crate::qa::register_component_with_metadata(
                "timeline.marquee",
                "timeline_marquee",
                selection_rect,
                true,
                Some(serde_json::json!({
                    "start": {"x": gesture.start.x, "y": gesture.start.y},
                    "current": {"x": current.x, "y": current.y},
                })),
            );
        }
        let (released, down) = ui.input(|input| {
            (
                input.pointer.primary_released(),
                input.pointer.primary_down(),
            )
        });
        if released || !down {
            state.timeline.selection_gesture = None;
            let items = items_intersecting_rect(project, state, rows, content_rect, selection_rect)
                .into_iter()
                .map(AuthoringSelection::Item)
                .collect();
            match get_box_action(&gesture.modifiers, items) {
                BoxAction::Replace(items) => {
                    state.selection.clear();
                    for item in items {
                        state.selection.add(item);
                    }
                }
                BoxAction::Add(items) => {
                    for item in items {
                        state.selection.add(item);
                    }
                }
                BoxAction::Remove(items) => {
                    for item in items {
                        state.selection.remove(item);
                    }
                }
            }
            ui.ctx().request_repaint();
        }
        return;
    }

    // Only empty content clicks may clear selection: ruler, Track header,
    // footer and other panels have their own command owners.
    if response.clicked_by(egui::PointerButton::Primary)
        && !handled_pan
        && pointer.is_some_and(|point| content_rect.contains(point))
    {
        let modifiers = ui.input(|input| input.modifiers);
        let pointer_on_item = pointer.is_some_and(|point| {
            !items_intersecting_rect(
                project,
                state,
                rows,
                content_rect,
                Rect::from_center_size(point, egui::Vec2::splat(1.0)),
            )
            .is_empty()
        });
        if !pointer_on_item
            && matches!(
                SelectionAction::from_modifiers(&modifiers),
                SelectionAction::Replace
            )
        {
            state.selection.clear();
        }
    }
}

fn items_intersecting_rect(
    project: &AuthoringProject,
    state: &AuthoringUiState,
    rows: &[DisplayRow],
    content_rect: Rect,
    selection_rect: Rect,
) -> Vec<TimelineItemId> {
    let metrics = TimelineRowMetrics::from_view(&state.timeline);
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    for (row_index, row) in rows.iter().enumerate() {
        let (item_ids, summary) = match row.kind {
            RowKind::Track {
                track_id,
                expanded: false,
            } => (ordered_track_item_ids(project, track_id, None), true),
            RowKind::Clip { item_id, .. } => (vec![item_id], false),
            RowKind::Track { expanded: true, .. } | RowKind::Property { .. } => continue,
        };
        let row_rect = Rect::from_min_size(
            Pos2::new(
                content_rect.left(),
                super::viewport::row_top(content_rect, &state.timeline, row_index),
            ),
            egui::Vec2::new(content_rect.width(), metrics.row_height()),
        );
        for item_id in item_ids {
            let Some(item) = project.items.get(&item_id) else {
                continue;
            };
            if clip_rect(
                item.interval,
                item.layer,
                row_rect,
                &state.timeline,
                summary,
            )
            .intersects(selection_rect)
                && seen.insert(item_id)
            {
                found.push(item_id);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::panels::timeline::{display_rows, tests::fixture};

    #[test]
    fn ruler_scrub_preserves_selection_but_empty_content_click_clears_it() {
        let (project, _, item_ids) = fixture();
        let mut state = AuthoringUiState::new(project.root_timeline_id);
        let selected = AuthoringSelection::Item(item_ids[0]);
        state.selection.replace(selected);
        let context = egui::Context::default();
        let content = Rect::from_min_size(egui::pos2(200.0, 100.0), egui::vec2(800.0, 300.0));
        let canvas = Rect::from_min_max(egui::pos2(0.0, 74.0), content.max);
        for (point, should_keep) in [
            (egui::pos2(500.0, 87.0), true),
            (egui::pos2(980.0, 380.0), false),
        ] {
            for phase in 0..3 {
                let mut events = vec![egui::Event::PointerMoved(point)];
                if phase != 0 {
                    events.push(egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed: phase == 1,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                drop(context.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            Pos2::ZERO,
                            egui::vec2(1200.0, 480.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |context| {
                        egui::CentralPanel::default().show(context, |ui| {
                            let rows = display_rows(
                                &project,
                                project.root_timeline_id,
                                &state.timeline.expanded_tracks,
                                &state.timeline.expanded_items,
                                None,
                            );
                            let navigation = super::super::viewport::navigate(
                                ui,
                                content,
                                &mut state,
                                rows.len(),
                            );
                            super::super::draw_ruler(
                                ui,
                                &project.timelines[&project.root_timeline_id],
                                &mut state,
                                canvas,
                                content,
                            );
                            handle_marquee_selection(
                                ui, &project, &mut state, &rows, content, navigation,
                            );
                        });
                    },
                ));
            }
            assert_eq!(state.selection.primary(), should_keep.then_some(selected));
        }
        assert!(state.timeline.current_frame > 0);
    }

    #[test]
    fn timeline_clip_clicks_share_preview_multi_selection_modifiers() {
        let (project, _, item_ids) = fixture();
        let mut state = AuthoringUiState::new(project.root_timeline_id);

        apply_item_click_selection(&mut state, item_ids[0], egui::Modifiers::NONE);
        apply_item_click_selection(
            &mut state,
            item_ids[1],
            egui::Modifiers {
                shift: true,
                ..egui::Modifiers::NONE
            },
        );
        assert!(state
            .selection
            .contains(AuthoringSelection::Item(item_ids[0])));
        assert!(state
            .selection
            .contains(AuthoringSelection::Item(item_ids[1])));

        apply_item_click_selection(
            &mut state,
            item_ids[0],
            egui::Modifiers {
                ctrl: true,
                ..egui::Modifiers::NONE
            },
        );
        assert!(!state
            .selection
            .contains(AuthoringSelection::Item(item_ids[0])));
        assert!(state
            .selection
            .contains(AuthoringSelection::Item(item_ids[1])));
    }

    #[test]
    fn marquee_hit_testing_uses_the_same_expanded_clip_geometry_as_painting() {
        let (project, track_id, item_ids) = fixture();
        let mut state = AuthoringUiState::new(project.root_timeline_id);
        state.timeline.expanded_tracks.insert(track_id);
        let rows = display_rows(
            &project,
            project.root_timeline_id,
            &state.timeline.expanded_tracks,
            &state.timeline.expanded_items,
            state.active_instance_path.as_ref(),
        );
        let content = egui::Rect::from_min_size(egui::pos2(200.0, 100.0), egui::vec2(800.0, 300.0));
        let metrics = TimelineRowMetrics::from_view(&state.timeline);
        let selection = egui::Rect::from_min_max(
            egui::pos2(200.0, 100.0 + metrics.stride()),
            egui::pos2(900.0, 100.0 + metrics.stride() * 3.0),
        );

        let found = items_intersecting_rect(&project, &state, &rows, content, selection);
        assert_eq!(found, vec![item_ids[1], item_ids[2]]);
    }
}
