//! Direct editor for Timeline-owned canonical Path geometry.
//!
//! Dragging never mutates the Project. The overlay is an ephemeral projection
//! and release commits one `TimelineEditorService` transaction.

mod geometry;
mod interaction;
mod renderer;

use library::editor::TimelineEditorService;
use library::model::authoring::{
    AuthoringProject, ProjectRevision, ShapeKind, SourceRef, TimelineItemId,
};
use library::model::frame::frame::FrameInfo;
use library::model::path::PathValue;
use library::model::property::PropertyValue;
use library::model::vector::{project_path_contour, replace_path_contour};
use library::plugin::PluginManager;
use pan_zoom_ui::CanvasTransform;

use crate::state::authoring::{AuthoringSelection, AuthoringUiState, PreviewTool};
use crate::state::path_editor::ShapeCreationGesture;
use crate::ui::clip_creation::{create_basic_clip, BasicClipKind, BasicClipPlacement};

use geometry::item_path_transform;
use interaction::PathEditorInteraction;
use renderer::PathEditorRenderer;

const EDITED_CONTOUR: usize = 0;
const MIN_SHAPE_DRAG_POINTS: f32 = 3.0;
const PEN_CLOSE_RADIUS_POINTS: f32 = 10.0;

#[expect(
    clippy::too_many_arguments,
    reason = "the shared Preview creation overlay needs pointer geometry, canonical viewport transform, Project state, plugin registry, and one authoring transaction"
)]
pub(super) fn creation_overlay(
    ui: &egui::Ui,
    response: &egui::Response,
    viewport: egui::Rect,
    canvas: CanvasTransform,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
) {
    let tool = state.preview.active_tool;
    if !matches!(
        tool,
        PreviewTool::Pen | PreviewTool::Rectangle | PreviewTool::Ellipse
    ) {
        state.preview.path_editor.shape_creation = None;
        state.preview.path_editor.pen_points.clear();
        return;
    }
    let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
    if escape {
        state.preview.path_editor.shape_creation = None;
        state.preview.path_editor.pen_points.clear();
        state.status = "Cancelled Shape creation".to_string();
        return;
    }
    if egui::Popup::is_any_open(ui.ctx()) {
        return;
    }
    let pointer = response
        .interact_pointer_pos()
        .or_else(|| ui.ctx().pointer_latest_pos());
    let world = pointer
        .filter(|pointer| viewport.contains(*pointer))
        .and_then(|pointer| canvas.screen_to_world(pointer));

    if tool == PreviewTool::Pen {
        handle_pen_creation(
            ui, response, viewport, canvas, world, project, state, service, plugins,
        );
        return;
    }

    let (pressed, down, released) = ui.input(|input| {
        (
            input.pointer.primary_pressed(),
            input.pointer.primary_down(),
            input.pointer.primary_released(),
        )
    });
    if pressed {
        if let Some(world) = world {
            state.preview.path_editor.shape_creation = Some(ShapeCreationGesture {
                start_world: [world.x, world.y],
                current_world: [world.x, world.y],
            });
        }
    } else if down {
        if let (Some(gesture), Some(world)) = (&mut state.preview.path_editor.shape_creation, world)
        {
            gesture.current_world = [world.x, world.y];
        }
    }
    let Some(gesture) = state.preview.path_editor.shape_creation.clone() else {
        return;
    };
    paint_creation_rect(ui, viewport, canvas, &gesture, tool == PreviewTool::Ellipse);
    crate::qa::register_component_with_metadata(
        "preview.shape.creation",
        "preview_shape_creation",
        viewport,
        true,
        Some(serde_json::json!({
            "tool": if tool == PreviewTool::Rectangle { "rectangle" } else { "ellipse" },
            "start_world": gesture.start_world,
            "current_world": gesture.current_world,
            "drag_active": true,
        })),
    );
    if !released {
        return;
    }
    state.preview.path_editor.shape_creation = None;
    let start_screen =
        canvas.world_to_screen(egui::pos2(gesture.start_world[0], gesture.start_world[1]));
    if !start_screen.is_finite() {
        return;
    }
    let end_screen = canvas.world_to_screen(egui::pos2(
        gesture.current_world[0],
        gesture.current_world[1],
    ));
    if start_screen.distance(end_screen) < MIN_SHAPE_DRAG_POINTS {
        return;
    }
    let min = [
        gesture.start_world[0].min(gesture.current_world[0]),
        gesture.start_world[1].min(gesture.current_world[1]),
    ];
    let size = [
        f64::from((gesture.current_world[0] - gesture.start_world[0]).abs()),
        f64::from((gesture.current_world[1] - gesture.start_world[1]).abs()),
    ];
    let kind = if tool == PreviewTool::Rectangle {
        BasicClipKind::Rectangle
    } else {
        BasicClipKind::Ellipse
    };
    finish_creation(
        project,
        state,
        service,
        plugins,
        kind,
        BasicClipPlacement {
            position: Some([f64::from(min[0]), f64::from(min[1])]),
            size: Some(size),
            path: None,
        },
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "Pen creation owns the Preview pointer, shared transform, Project snapshot, and atomic editor transaction"
)]
fn handle_pen_creation(
    ui: &egui::Ui,
    response: &egui::Response,
    viewport: egui::Rect,
    canvas: CanvasTransform,
    world: Option<egui::Pos2>,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
) {
    let double_clicked = response.double_clicked_by(egui::PointerButton::Primary);
    let clicked = response.clicked_by(egui::PointerButton::Primary);
    if let Some(world) = world.filter(|_| clicked || double_clicked) {
        let candidate = [world.x, world.y];
        let close = state
            .preview
            .path_editor
            .pen_points
            .first()
            .copied()
            .filter(|_| state.preview.path_editor.pen_points.len() >= 3)
            .is_some_and(|first| {
                canvas
                    .world_to_screen(egui::pos2(first[0], first[1]))
                    .distance(canvas.world_to_screen(world))
                    <= PEN_CLOSE_RADIUS_POINTS
            });
        if close {
            commit_pen_path(project, state, service, plugins, true);
        } else {
            let duplicate_last = state
                .preview
                .path_editor
                .pen_points
                .last()
                .is_some_and(|last| egui::pos2(last[0], last[1]).distance(world) <= f32::EPSILON);
            if !duplicate_last {
                state.preview.path_editor.pen_points.push(candidate);
            }
            if double_clicked && state.preview.path_editor.pen_points.len() >= 2 {
                commit_pen_path(project, state, service, plugins, false);
            }
        }
    }
    let points = state
        .preview
        .path_editor
        .pen_points
        .iter()
        .map(|point| canvas.world_to_screen(egui::pos2(point[0], point[1])))
        .collect::<Vec<_>>();
    if !points.is_empty() {
        let painter = ui.painter().with_clip_rect(viewport);
        if points.len() > 1 {
            painter.add(egui::Shape::line(
                points.clone(),
                egui::Stroke::new(1.5, ui.visuals().selection.stroke.color),
            ));
        }
        for point in &points {
            painter.circle_filled(*point, 3.5, ui.visuals().selection.stroke.color);
        }
    }
    crate::qa::register_component_with_metadata(
        "preview.pen.creation",
        "preview_pen_creation",
        viewport,
        true,
        Some(serde_json::json!({
            "point_count": state.preview.path_editor.pen_points.len(),
            "points_world": state.preview.path_editor.pen_points,
            "complete_actions": ["double_click", "click_first_point"],
        })),
    );
}

fn commit_pen_path(
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    closed: bool,
) {
    let points = std::mem::take(&mut state.preview.path_editor.pen_points);
    let Some(origin) = points.first().copied() else {
        return;
    };
    if points.len() < 2 {
        return;
    }
    let local = points
        .iter()
        .map(|point| {
            [
                f64::from(point[0] - origin[0]),
                f64::from(point[1] - origin[1]),
            ]
        })
        .collect::<Vec<_>>();
    let path = library::model::path::PathValue::new(
        library::model::path::FillRule::NonZero,
        vec![library::model::path::PathContour::new(
            library::model::path::PathPoint::new(local[0][0], local[0][1]),
            local[1..]
                .iter()
                .map(|point| {
                    library::model::path::PathSegment::line(library::model::path::PathPoint::new(
                        point[0], point[1],
                    ))
                })
                .collect(),
            closed,
        )],
    );
    let path = match path {
        Ok(path) => path,
        Err(error) => {
            state.error = Some(error.to_string());
            return;
        }
    };
    let (min_x, max_x) = points
        .iter()
        .map(|point| point[0])
        .fold((f32::INFINITY, f32::NEG_INFINITY), |bounds, value| {
            (bounds.0.min(value), bounds.1.max(value))
        });
    let (min_y, max_y) = points
        .iter()
        .map(|point| point[1])
        .fold((f32::INFINITY, f32::NEG_INFINITY), |bounds, value| {
            (bounds.0.min(value), bounds.1.max(value))
        });
    finish_creation(
        project,
        state,
        service,
        plugins,
        BasicClipKind::Path,
        BasicClipPlacement {
            position: Some([f64::from(origin[0]), f64::from(origin[1])]),
            size: Some([
                f64::from((max_x - min_x).max(1.0)),
                f64::from((max_y - min_y).max(1.0)),
            ]),
            path: Some(path),
        },
    );
    state.preview.active_tool = PreviewTool::Path;
}

fn finish_creation(
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    kind: BasicClipKind,
    placement: BasicClipPlacement,
) {
    match create_basic_clip(
        project,
        state.active_timeline_id,
        state,
        service,
        plugins,
        kind,
        placement,
    ) {
        Ok(item_id) => {
            state.selection.replace(AuthoringSelection::Item(item_id));
            state.inspector.invalidate();
            state.status = format!(
                "Created {} clip",
                if kind == BasicClipKind::Path {
                    "Path"
                } else if kind == BasicClipKind::Rectangle {
                    "Rectangle"
                } else {
                    "Ellipse"
                }
            );
            state.error = None;
        }
        Err(error) => state.error = Some(error),
    }
}

fn paint_creation_rect(
    ui: &egui::Ui,
    viewport: egui::Rect,
    canvas: CanvasTransform,
    gesture: &ShapeCreationGesture,
    ellipse: bool,
) {
    let start = canvas.world_to_screen(egui::pos2(gesture.start_world[0], gesture.start_world[1]));
    let current = canvas.world_to_screen(egui::pos2(
        gesture.current_world[0],
        gesture.current_world[1],
    ));
    let rect = egui::Rect::from_two_pos(start, current);
    let painter = ui.painter().with_clip_rect(viewport);
    let stroke = egui::Stroke::new(1.5, ui.visuals().selection.stroke.color);
    if ellipse {
        const ELLIPSE_SEGMENTS: usize = 48;
        let center = rect.center();
        let radius = rect.size() * 0.5;
        let points = (0..=ELLIPSE_SEGMENTS)
            .map(|index| {
                let angle = std::f32::consts::TAU * index as f32 / ELLIPSE_SEGMENTS as f32;
                center + egui::vec2(radius.x * angle.cos(), radius.y * angle.sin())
            })
            .collect();
        painter.add(egui::Shape::line(points, stroke));
    } else {
        painter.rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Middle);
    }
}

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

#[allow(
    clippy::too_many_arguments,
    reason = "The path overlay combines the shared canvas transform, rendered frame, authored snapshot, and deferred editor service at one immediate-mode interaction boundary"
)]
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
