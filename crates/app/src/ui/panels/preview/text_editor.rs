//! Direct Text editing projected over the rendered Timeline Item.
//!
//! Typing is rendered from a transient Project snapshot. Only acceptance
//! calls the editor service, keeping a whole typing session atomic for Undo.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use library::editor::TimelineEditorService;
use library::model::authoring::{
    AuthoringProject, MediaTime, ProjectRevision, SourceRef, TimelineItem, TimelineItemId,
};
use library::model::frame::frame::FrameInfo;
use pan_zoom_ui::CanvasTransform;

use crate::state::authoring::{AuthoringSelection, AuthoringUiState, PreviewTool};

use super::gizmo_geometry::item_gizmo_geometry;

pub(super) fn selected_text<'a>(
    project: &'a AuthoringProject,
    state: &AuthoringUiState,
) -> Option<(TimelineItemId, &'a str)> {
    let AuthoringSelection::Item(item_id) = state.selection.primary()? else {
        return None;
    };
    let item = project.items.get(&item_id)?;
    let track = project.tracks.get(&item.track_id)?;
    if track.timeline_id != state.active_timeline_id || !item_is_at_playhead(project, state, item) {
        return None;
    }
    let SourceRef::Text { text, .. } = &item.source else {
        return None;
    };
    Some((item_id, text))
}

pub(super) fn selected_text_is_editable(
    project: &AuthoringProject,
    state: &AuthoringUiState,
) -> bool {
    selected_text(project, state).is_some()
}

/// Substitute only the transient Text buffer into the render snapshot.
/// RenderPlan topology stays valid because the source kind and identity do not
/// change. The returned digest participates in Preview request identity.
pub(super) fn transient_render_project(
    project: &Arc<AuthoringProject>,
    state: &AuthoringUiState,
) -> (Arc<AuthoringProject>, Option<u64>) {
    let editor = &state.preview.text_editor;
    let Some(digest) = transient_edit_digest(state) else {
        return (Arc::clone(project), None);
    };
    let Some(item_id) = editor.target_item else {
        return (Arc::clone(project), None);
    };
    let Some(item) = project.items.get(&item_id) else {
        return (Arc::clone(project), None);
    };
    let SourceRef::Text { text, .. } = &item.source else {
        return (Arc::clone(project), None);
    };
    if *text == editor.buffer {
        return (Arc::clone(project), Some(digest));
    }

    let mut projected = project.as_ref().clone();
    let Some(item) = projected.items.get_mut(&item_id) else {
        return (Arc::clone(project), None);
    };
    let SourceRef::Text { text, .. } = &mut item.source else {
        return (Arc::clone(project), None);
    };
    text.clone_from(&editor.buffer);
    (Arc::new(projected), Some(digest))
}

pub(super) fn transient_edit_digest(state: &AuthoringUiState) -> Option<u64> {
    let editor = &state.preview.text_editor;
    if !editor.editing {
        return None;
    }
    let item_id = editor.target_item?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    item_id.hash(&mut hasher);
    editor.target_revision.hash(&mut hasher);
    editor.buffer.hash(&mut hasher);
    Some(hasher.finish())
}

#[allow(
    clippy::too_many_arguments,
    reason = "The text overlay combines the shared canvas transform, rendered frame, authored snapshot, and deferred editor service at one immediate-mode interaction boundary"
)]
pub(super) fn text_editor_overlay(
    ui: &mut egui::Ui,
    viewport: egui::Rect,
    canvas: CanvasTransform,
    revision: ProjectRevision,
    frame: Option<&FrameInfo>,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
) {
    if state.preview.active_tool != PreviewTool::Text {
        accept_if_active(state, service);
        return;
    }

    let Some((item_id, authored_text)) = selected_text(project, state) else {
        cancel(state, "Select a visible Text clip to edit it in Preview");
        return;
    };
    if state.preview.text_editor.editing
        && (state.preview.text_editor.target_item != Some(item_id)
            || state.preview.text_editor.target_revision != Some(revision))
    {
        cancel(
            state,
            "Text edit was cancelled because its Timeline source changed",
        );
        return;
    }
    if !state.preview.text_editor.editing {
        state
            .preview
            .text_editor
            .begin(item_id, revision, authored_text);
    }

    let Some(rect) = frame
        .and_then(|frame| item_gizmo_geometry(frame, item_id))
        .and_then(|geometry| editor_rect(&geometry.outlines, canvas, viewport))
    else {
        return;
    };

    let widget_id = ui.make_persistent_id(("preview-text-editor", item_id));
    let font_size = editor_font_size(project.items.get(&item_id), rect);
    let response = ui.put(
        rect,
        egui::TextEdit::multiline(&mut state.preview.text_editor.buffer)
            .id(widget_id)
            .frame(false)
            .text_color(egui::Color32::TRANSPARENT)
            .font(egui::FontId::proportional(font_size))
            .desired_width(rect.width()),
    );
    if state.preview.text_editor.request_focus {
        response.request_focus();
        state.preview.text_editor.request_focus = false;
    }

    crate::qa::register_component_with_metadata(
        "preview.text.editor",
        "preview_text_editor",
        rect,
        true,
        Some(serde_json::json!({
            "item_id": item_id,
            "editing": true,
            "changed": state.preview.text_editor.changed(),
            "buffer": state.preview.text_editor.buffer,
            "project_revision": revision.get(),
        })),
    );

    let (escape, accept_shortcut) = ui.input(|input| {
        (
            input.key_pressed(egui::Key::Escape),
            input.key_pressed(egui::Key::Enter)
                && (input.modifiers.command || input.modifiers.ctrl),
        )
    });
    if escape {
        state.preview.text_editor.finish();
        state.preview.active_tool = PreviewTool::Select;
        state.status = "Cancelled Text edit".to_string();
        state.error = None;
    } else if accept_shortcut || (response.lost_focus() && !response.changed()) {
        accept_if_active(state, service);
        state.preview.active_tool = PreviewTool::Select;
    } else if response.changed() {
        ui.ctx().request_repaint();
    }
}

fn accept_if_active(state: &mut AuthoringUiState, service: &TimelineEditorService) {
    if !state.preview.text_editor.editing {
        return;
    }
    let target = state.preview.text_editor.target_item;
    let changed = state.preview.text_editor.changed();
    let text = state.preview.text_editor.buffer.clone();
    state.preview.text_editor.finish();
    if !changed {
        return;
    }
    let Some(item_id) = target else {
        return;
    };
    match service.set_text(item_id, text) {
        Ok(_) => {
            state.inspector.invalidate();
            state.error = None;
            state.status = "Edited Text".to_string();
        }
        Err(error) => state.error = Some(error.to_string()),
    }
}

fn cancel(state: &mut AuthoringUiState, message: &str) {
    state.preview.text_editor.finish();
    state.preview.active_tool = PreviewTool::Select;
    state.error = Some(message.to_string());
}

fn item_is_at_playhead(
    project: &AuthoringProject,
    state: &AuthoringUiState,
    item: &TimelineItem,
) -> bool {
    let Some(timeline) = project.timelines.get(&state.active_timeline_id) else {
        return false;
    };
    let Ok(time) = MediaTime::from_frame_index(state.timeline.current_frame, timeline.fps) else {
        return false;
    };
    item.interval.contains(time).unwrap_or(false)
}

fn editor_rect(
    outlines: &[[egui::Pos2; 4]],
    canvas: CanvasTransform,
    viewport: egui::Rect,
) -> Option<egui::Rect> {
    let points = outlines
        .iter()
        .flatten()
        .map(|point| canvas.world_to_screen(*point))
        .collect::<Vec<_>>();
    if points.is_empty() {
        return None;
    }
    let mut rect = egui::Rect::from_points(&points);
    if rect.width() < 48.0 {
        rect.max.x = rect.min.x + 48.0;
    }
    if rect.height() < 24.0 {
        rect.max.y = rect.min.y + 24.0;
    }
    let clipped = rect.intersect(viewport);
    clipped.is_positive().then_some(clipped)
}

fn editor_font_size(item: Option<&TimelineItem>, rect: egui::Rect) -> f32 {
    let authored = item
        .and_then(|item| item.authored_properties.get("size"))
        .and_then(|property| property.value())
        .and_then(|value| match value {
            library::model::property::PropertyValue::Number(value) => {
                Some(value.into_inner() as f32)
            }
            _ => None,
        })
        .unwrap_or(48.0);
    authored.min(rect.height()).clamp(8.0, 256.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_digest_includes_the_target_identity() {
        let mut state = AuthoringUiState::new(library::model::authoring::TimelineId::new());
        let first = TimelineItemId::new();
        let second = TimelineItemId::new();
        state.preview.text_editor.editing = true;
        state.preview.text_editor.target_item = Some(first);
        state.preview.text_editor.buffer = "same".to_string();
        let first_digest = transient_edit_digest(&state).expect("first digest");
        state.preview.text_editor.target_item = Some(second);
        let second_digest = transient_edit_digest(&state).expect("second digest");

        assert_ne!(first_digest, second_digest);
    }
}
