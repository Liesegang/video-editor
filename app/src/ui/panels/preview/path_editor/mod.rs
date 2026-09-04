//! Direct editor for Timeline-owned canonical Path geometry.
//!
//! Dragging never mutates the Project. The overlay is an ephemeral projection
//! and release commits one `TimelineEditorService` transaction.

mod geometry;
mod interaction;
mod renderer;

use library::editor::TimelineEditorService;
use library::model::authoring::{
    AuthoringProject, MediaTime, ProjectRevision, ShapeKind, SourceRef, TimelineItemId,
};
use library::model::frame::frame::FrameInfo;
use library::model::path::PathValue;
use library::model::property::PropertyValue;
use library::model::vector::{project_path_contour, replace_path_contour};
use pan_zoom_ui::CanvasTransform;

use crate::state::authoring::{AuthoringSelection, AuthoringUiState, PreviewTool};

use geometry::item_path_transform;
use interaction::PathEditorInteraction;
use renderer::PathEditorRenderer;

const EDITED_CONTOUR: usize = 0;

pub(super) fn selected_path<'a>(
    project: &'a AuthoringProject,
    state: &AuthoringUiState,
) -> Option<(TimelineItemId, &'a PathValue)> {
    let AuthoringSelection::Item(item_id) = state.selection.primary()? else {
        return None;
    };
    let item = project.items.get(&item_id)?;
    let track = project.tracks.get(&item.track_id)?;
    if track.timeline_id != state.active_timeline_id {
        return None;
    }
    let SourceRef::Shape { shape } = &item.source else {
        return None;
    };
    if shape.shape_kind != ShapeKind::Path {
        return None;
    }
    let PropertyValue::Path(path) = shape.parameters.get("path")? else {
        return None;
    };
    Some((item_id, path))
}

pub(super) fn selected_path_is_editable(
    project: &AuthoringProject,
    state: &AuthoringUiState,
) -> bool {
    let Some((item_id, path)) = selected_path(project, state) else {
        return false;
    };
    let Some(item) = project.items.get(&item_id) else {
        return false;
    };
    let Some(timeline) = project.timelines.get(&state.active_timeline_id) else {
        return false;
    };
    let Ok(timeline_time) = MediaTime::from_frame_index(state.timeline.current_frame, timeline.fps)
    else {
        return false;
    };

    item.interval.contains(timeline_time).unwrap_or(false)
        && project_path_contour(path, EDITED_CONTOUR).is_ok()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn path_editor_overlay(
    ui: &egui::Ui,
    response: &egui::Response,
    viewport: egui::Rect,
    canvas: CanvasTransform,
    revision: ProjectRevision,
    frame: Option<&FrameInfo>,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
) {
    if state.preview.active_tool != PreviewTool::Path {
        state.preview.path_editor.cancel_drag();
        return;
    }
    let Some((item_id, canonical)) = selected_path(project, state) else {
        state.preview.path_editor.clear();
        return;
    };
    let canonical = canonical.clone();
    let mut projected = match project_path_contour(&canonical, EDITED_CONTOUR) {
        Ok(path) => path,
        Err(error) => {
            state.error = Some(error.to_string());
            state.preview.path_editor.clear();
            return;
        }
    };
    let Some(transform) = frame.and_then(|frame| item_path_transform(frame, item_id)) else {
        return;
    };

    state
        .preview
        .path_editor
        .reconcile(item_id, revision, projected.points.len());
    let result = PathEditorInteraction {
        state: &mut state.preview.path_editor,
        path: &mut projected,
        transform,
        canvas,
    }
    .handle(ui, response);

    PathEditorRenderer {
        state: &state.preview.path_editor,
        path: &projected,
        transform,
        canvas,
    }
    .draw(&ui.painter().with_clip_rect(viewport));

    crate::qa::register_component_with_metadata(
        "preview.path.editor",
        "preview_path_editor",
        viewport,
        true,
        Some(serde_json::json!({
            "item_id": item_id,
            "contour_index": EDITED_CONTOUR,
            "point_count": projected.points.len(),
            "selected_point_indices": state.preview.path_editor.selected_point_indices,
            "dragging": state.preview.path_editor.drag.is_some(),
            "captured": result.captured,
            "project_revision": revision.get(),
            "canvas_transform": {
                "pan": {"x": canvas.state.pan.x, "y": canvas.state.pan.y},
                "zoom": {"x": canvas.state.zoom.x, "y": canvas.state.zoom.y},
            },
        })),
    );

    if result.commit_requested {
        let replacement = replace_path_contour(&canonical, EDITED_CONTOUR, &projected)
            .map_err(|error| error.to_string())
            .and_then(|path| {
                service
                    .set_shape_path(item_id, path)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            });
        match replacement {
            Ok(()) => {
                state.inspector.invalidate();
                state.error = None;
                state.status = "Edited Path".to_string();
            }
            Err(error) => state.error = Some(error),
        }
    }
}
