//! Direct Text editing projected over the rendered Timeline Item.
//!
//! Typing is rendered from a transient Project snapshot. Only acceptance
//! calls the editor service, keeping a whole typing session atomic for Undo.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use library::editor::TimelineEditorService;
use library::model::authoring::{
    AuthoringProject, MediaTime, ProjectRevision, SourceRef, TimelineItem, TimelineItemId,
};
use library::model::frame::frame::FrameInfo;
use library::plugin::PluginManager;
use pan_zoom_ui::CanvasTransform;

use crate::state::authoring::{AuthoringSelection, AuthoringUiState, PreviewTool};
use crate::state::text_editor::TextToolClick;
use crate::ui::clip_creation::{create_basic_clip, BasicClipKind, BasicClipPlacement};

use super::gizmo_geometry::{hit_test_item, item_gizmo_geometry};

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

/// Route the Text tool through the rendered canvas: edit the top-most Text at
/// the click or create one at the click when no Text occupies that point.
#[expect(
    clippy::too_many_arguments,
    reason = "Text hit/edit/create is one atomic Preview interaction requiring rendered geometry, canonical viewport state, plugin registry, and authoring transaction"
)]
pub(super) fn handle_tool_click(
    ui: &egui::Ui,
    response: &egui::Response,
    content_rect: egui::Rect,
    canvas: CanvasTransform,
    frame: Option<&FrameInfo>,
    revision: ProjectRevision,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
) -> bool {
    let canvas_clicked = response.clicked_by(egui::PointerButton::Primary);
    if ui.input(|input| {
        input.pointer.button_pressed(egui::PointerButton::Primary)
            || (input.pointer.button_clicked(egui::PointerButton::Primary) && !canvas_clicked)
    }) {
        // A newer click anywhere in the workspace supersedes a deferred
        // canvas intent, including returning to this panel through its tab.
        state.preview.text_editor.pending_click = None;
    }
    if state.preview.active_tool != PreviewTool::Text
        || egui::Popup::is_any_open(ui.ctx())
        || ui.input(|input| input.key_pressed(egui::Key::Escape))
    {
        state.preview.text_editor.pending_click = None;
        return false;
    }
    if canvas_clicked {
        state.preview.text_editor.pending_click = response
            .interact_pointer_pos()
            .filter(|pointer| content_rect.contains(*pointer))
            .and_then(|pointer| canvas.screen_to_world(pointer))
            .map(|position| TextToolClick {
                position,
                revision,
                timeline_id: state.active_timeline_id,
                instance_path: state.active_instance_path.clone(),
                frame_number: state.timeline.current_frame,
                selection: state.selection.primary(),
            });
    }
    let Some(click) = state.preview.text_editor.pending_click.as_ref() else {
        return false;
    };
    if click.revision != revision
        || click.timeline_id != state.active_timeline_id
        || click.instance_path != state.active_instance_path
        || click.frame_number != state.timeline.current_frame
        || click.selection != state.selection.primary()
        || service.revision().ok() != Some(revision)
    {
        state.preview.text_editor.pending_click = None;
        return false;
    }
    // A frame without evaluated geometry cannot distinguish a blank click
    // from a hit on existing Text. Retain the intent until the normal Preview
    // result arrives, without reinterpreting it through a later camera.
    let Some(frame) = frame else {
        ui.ctx().request_repaint();
        return true;
    };
    let world = click.position;
    state.preview.text_editor.pending_click = None;
    let defer_overlay = state.preview.text_editor.editing;
    let refreshed;
    let (project, revision) = if defer_overlay {
        if !accept_if_active(state, service) {
            return true;
        }
        let Ok((snapshot, revision)) = service.snapshot_with_revision() else {
            return true;
        };
        refreshed = snapshot;
        (refreshed.as_ref(), revision)
    } else {
        (project, revision)
    };
    let selectable = project
        .items
        .values()
        .filter(|item| {
            matches!(item.source, SourceRef::Text { .. })
                && project
                    .tracks
                    .get(&item.track_id)
                    .is_some_and(|track| track.timeline_id == state.active_timeline_id)
                && item_is_at_playhead(project, state, item)
        })
        .map(|item| item.id)
        .collect::<HashSet<_>>();
    if let Some(item_id) = hit_test_item(frame, &selectable, world) {
        state.selection.replace(AuthoringSelection::Item(item_id));
        if let Some(SourceRef::Text { text, .. }) =
            project.items.get(&item_id).map(|item| &item.source)
        {
            state.preview.text_editor.begin(item_id, revision, text);
        }
        return defer_overlay;
    }
    match create_basic_clip(
        project,
        state.active_timeline_id,
        state,
        service,
        plugins,
        BasicClipKind::Text,
        BasicClipPlacement {
            position: Some([f64::from(world.x), f64::from(world.y)]),
            ..Default::default()
        },
    ) {
        Ok(item_id) => {
            state.selection.replace(AuthoringSelection::Item(item_id));
            if let Ok(revision) = service.revision() {
                state.preview.text_editor.begin(item_id, revision, "Text");
            }
            state.inspector.invalidate();
            state.status = "Created Text clip".to_string();
            state.error = None;
            true
        }
        Err(error) => {
            state.error = Some(error);
            false
        }
    }
}

/// Substitute only the transient Text buffer into the render snapshot.
/// RenderPlan topology stays valid because the source kind and identity do not
/// change. The returned digest participates in Preview request identity.
pub(super) fn transient_render_project(
    project: &Arc<AuthoringProject>,
    revision: ProjectRevision,
    state: &AuthoringUiState,
) -> Result<(Arc<AuthoringProject>, Option<u64>), String> {
    let editor = &state.preview.text_editor;
    let Some(digest) = transient_edit_digest(state, revision) else {
        return Ok((Arc::clone(project), None));
    };
    let Some(item_id) = editor.target_item else {
        return Ok((Arc::clone(project), None));
    };
    let Some(item) = project.items.get(&item_id) else {
        return Ok((Arc::clone(project), None));
    };
    let SourceRef::Text { text, .. } = &item.source else {
        return Ok((Arc::clone(project), None));
    };
    if *text == editor.buffer {
        return Ok((Arc::clone(project), Some(digest)));
    }

    TimelineEditorService::project_text(project, item_id, editor.buffer.clone())
        .map(|projected| (Arc::new(projected), Some(digest)))
        .map_err(|error| format!("Preview Text: {error}"))
}

pub(super) fn transient_edit_digest(
    state: &AuthoringUiState,
    revision: ProjectRevision,
) -> Option<u64> {
    let editor = &state.preview.text_editor;
    if !editor.editing || editor.target_revision != Some(revision) {
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
    if !state.preview.text_editor.editing {
        return;
    }
    if state.preview.text_editor.target_revision != Some(revision) {
        cancel(
            state,
            "Text edit was cancelled because its Timeline source changed",
        );
        return;
    }
    let selected = selected_text(project, state).map(|(item_id, _)| item_id);
    if state.preview.active_tool != PreviewTool::Text
        || selected != state.preview.text_editor.target_item
    {
        accept_if_active(state, service);
        ui.ctx().request_repaint();
        return;
    }
    let Some(item_id) = selected else { return };
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
        ui.ctx().request_repaint();
        return;
    }
    if let Some(geometry) = frame.and_then(|frame| item_gizmo_geometry(frame, item_id)) {
        let points = geometry
            .outlines
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        state.preview.text_editor.layout_bounds = Some(egui::Rect::from_points(&points));
    }
    let rect = state
        .preview
        .text_editor
        .layout_bounds
        .and_then(|bounds| editor_rect(bounds, canvas, viewport));
    let Some(rect) = rect else {
        if accept_shortcut {
            accept_if_active(state, service);
            state.preview.active_tool = PreviewTool::Select;
            ui.ctx().request_repaint();
        }
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

    if accept_shortcut || response.lost_focus() {
        accept_if_active(state, service);
        state.preview.active_tool = PreviewTool::Select;
        ui.ctx().request_repaint();
    } else if response.changed() {
        ui.ctx().request_repaint();
    }
}

fn accept_if_active(state: &mut AuthoringUiState, service: &TimelineEditorService) -> bool {
    if !state.preview.text_editor.editing {
        return true;
    }
    if service.revision().ok() != state.preview.text_editor.target_revision {
        cancel(
            state,
            "Text edit was cancelled because its Timeline source changed",
        );
        return false;
    }
    let target = state.preview.text_editor.target_item;
    let changed = state.preview.text_editor.changed();
    let text = state.preview.text_editor.buffer.clone();
    state.preview.text_editor.finish();
    if !changed {
        return true;
    }
    let Some(item_id) = target else {
        return false;
    };
    match service.set_text(item_id, text) {
        Ok(_) => {
            state.inspector.invalidate();
            state.error = None;
            state.status = "Edited Text".to_string();
            true
        }
        Err(error) => {
            state.error = Some(error.to_string());
            false
        }
    }
}

fn cancel(state: &mut AuthoringUiState, message: &str) {
    state.preview.text_editor.finish();
    if state.preview.active_tool == PreviewTool::Text {
        state.preview.active_tool = PreviewTool::Select;
    }
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
    bounds: egui::Rect,
    canvas: CanvasTransform,
    viewport: egui::Rect,
) -> Option<egui::Rect> {
    let mut rect = egui::Rect::from_min_max(
        canvas.world_to_screen(bounds.min),
        canvas.world_to_screen(bounds.max),
    );
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
mod tests;
