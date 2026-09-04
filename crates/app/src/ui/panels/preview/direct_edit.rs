//! Preview selection and body-drag routing.
//!
//! Transform handles and gesture projection live in the restored production
//! gizmo implementation. This module owns only object hit testing, shared
//! selection semantics, and starting a body translation gesture.

use std::collections::HashSet;

use library::editor::TimelineEditorService;
use library::model::authoring::{AuthoringProject, ProjectRevision, TimelineItemId};
use library::model::frame::frame::FrameInfo;
use pan_zoom_ui::CanvasTransform;

use crate::state::authoring::{AuthoringSelection, AuthoringUiState, PreviewTool};
use crate::ui::selection::{get_click_action, ClickAction, SelectionAction};

use super::gizmo;
use super::gizmo_geometry::hit_test_item;

#[expect(
    clippy::too_many_arguments,
    reason = "direct editing keeps the rendered frame, Project revision, and canonical canvas transform explicit"
)]
pub(super) fn handle_direct_edit(
    ui: &egui::Ui,
    response: &egui::Response,
    viewport: egui::Rect,
    content_rect: egui::Rect,
    canvas: CanvasTransform,
    displayed_frame: Option<&FrameInfo>,
    revision: ProjectRevision,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
) {
    if gizmo::handle_active_gesture(ui, revision, project, state, service) {
        return;
    }

    let space_down = ui.input(|input| input.key_down(egui::Key::Space));
    if state.preview.active_tool != PreviewTool::Select || space_down {
        return;
    }
    if gizmo::interact_handles(
        ui,
        viewport,
        canvas,
        displayed_frame,
        revision,
        project,
        state,
    ) {
        return;
    }

    let pointer = response
        .interact_pointer_pos()
        .or_else(|| ui.ctx().pointer_latest_pos());
    let hovered_item = pointer.and_then(|pointer| {
        hit_test_at_pointer(
            displayed_frame,
            project,
            state,
            content_rect,
            canvas,
            pointer,
        )
    });

    if response.clicked_by(egui::PointerButton::Primary) {
        let modifiers = ui.input(|input| input.modifiers);
        apply_click_selection(state, hovered_item, modifiers);
        return;
    }
    if !response.drag_started_by(egui::PointerButton::Primary) {
        return;
    }
    let (Some(pointer), Some(item_id)) = (pointer, hovered_item) else {
        return;
    };
    let selection = AuthoringSelection::Item(item_id);
    let modifiers = ui.input(|input| input.modifiers);
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
    if !should_drag {
        return;
    }
    state.selection.make_primary(selection);
    gizmo::begin_body_gesture(
        item_id,
        pointer,
        canvas,
        displayed_frame,
        revision,
        project,
        state,
    );
}

fn hit_test_at_pointer(
    displayed_frame: Option<&FrameInfo>,
    project: &AuthoringProject,
    state: &AuthoringUiState,
    content_rect: egui::Rect,
    canvas: CanvasTransform,
    pointer: egui::Pos2,
) -> Option<TimelineItemId> {
    if !content_rect.contains(pointer) {
        return None;
    }
    let world = canvas.screen_to_world(pointer)?;
    let selectable = project
        .items
        .values()
        .filter(|item| {
            project
                .tracks
                .get(&item.track_id)
                .is_some_and(|track| track.timeline_id == state.active_timeline_id)
        })
        .map(|item| item.id)
        .collect::<HashSet<_>>();
    hit_test_item(displayed_frame?, &selectable, world)
}

fn apply_click_selection(
    state: &mut AuthoringUiState,
    hovered_item: Option<TimelineItemId>,
    modifiers: egui::Modifiers,
) {
    let hovered = hovered_item.map(AuthoringSelection::Item);
    match get_click_action(&modifiers, hovered) {
        ClickAction::Select(selection) => state.selection.replace(selection),
        ClickAction::Add(selection) => state.selection.add(selection),
        ClickAction::Remove(selection) => {
            state.selection.remove(selection);
        }
        ClickAction::Toggle(selection) => state.selection.toggle(selection),
        ClickAction::Clear => state.selection.clear(),
        ClickAction::DoNothing => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::authoring::{AuthoringProject, MediaTime, RationalRate};

    fn state() -> AuthoringUiState {
        let project = AuthoringProject::new(
            "Preview selection",
            640,
            360,
            RationalRate::new(30, 1).unwrap(),
            MediaTime::new(10, 1).unwrap(),
        )
        .unwrap();
        AuthoringUiState::new(project.root_timeline_id)
    }

    #[test]
    fn preview_click_selects_visible_item_and_plain_empty_click_clears() {
        let mut state = state();
        let item_id = TimelineItemId::new();

        apply_click_selection(&mut state, Some(item_id), egui::Modifiers::default());
        assert_eq!(
            state.selection.primary(),
            Some(AuthoringSelection::Item(item_id))
        );

        apply_click_selection(&mut state, None, egui::Modifiers::default());
        assert_eq!(state.selection.primary(), None);
    }

    #[test]
    fn preview_click_uses_shared_modifier_selection_semantics() {
        let mut state = state();
        let first = TimelineItemId::new();
        let second = TimelineItemId::new();
        state.selection.replace(AuthoringSelection::Item(first));

        apply_click_selection(
            &mut state,
            Some(second),
            egui::Modifiers {
                shift: true,
                ..egui::Modifiers::default()
            },
        );
        assert!(state.selection.contains(AuthoringSelection::Item(first)));
        assert!(state.selection.contains(AuthoringSelection::Item(second)));

        apply_click_selection(
            &mut state,
            Some(first),
            egui::Modifiers {
                ctrl: true,
                ..egui::Modifiers::default()
            },
        );
        assert!(!state.selection.contains(AuthoringSelection::Item(first)));
        assert!(state.selection.contains(AuthoringSelection::Item(second)));
    }
}
