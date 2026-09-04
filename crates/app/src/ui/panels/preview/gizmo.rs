//! Production Preview transform gizmo adapted to Timeline-owned properties.
//!
//! The interaction retains the original eight resize handles, rotation
//! handle, Shift aspect lock, Alt centre scaling, parent-space inversion, and
//! one history commit on pointer release. Unlike the former Node-owned path,
//! the gesture samples and atomically updates the selected Timeline Item's
//! authored Position, Scale, and Rotation properties.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use egui::{CursorIcon, Pos2, Rect, Sense, Ui, Vec2};
use library::editor::{
    AuthoringPropertyOwner, AuthoringPropertyValueTarget, AuthoringPropertyValueUpdate,
    TimelineEditorService,
};
use library::model::authoring::{
    AuthoringProject, MediaTime, ProjectRevision, TimelineItem, TimelineItemId,
};
use library::model::frame::frame::FrameInfo;
use library::model::property::{PropertyValue, Vec2 as PropertyVec2};
use library::rendering::renderer::Affine2D;
use ordered_float::OrderedFloat;
use pan_zoom_ui::CanvasTransform;

use crate::model::ui_types::GizmoHandle;
use crate::state::authoring::{
    AuthoringSelection, AuthoringUiState, PreviewTool, PreviewTransformGesture,
};

use super::gizmo_geometry::{item_gizmo_geometry, ItemGizmoGeometry};

const HANDLE_HIT_SIZE: f32 = 15.0;
const HANDLE_RADIUS: f32 = 5.0;
const ROTATION_HANDLE_DISTANCE: f32 = 28.0;
const GIZMO_COLOR: egui::Color32 = egui::Color32::from_rgb(0, 200, 255);

#[derive(Clone, Copy)]
struct HandlePlacement {
    position: Pos2,
    handle: GizmoHandle,
    cursor: CursorIcon,
}

/// Applies the projected gesture to an immutable render snapshot so the
/// selected object itself follows the gizmo before the single release commit.
/// RenderPlan topology remains valid because only authored values change.
pub(super) fn transient_render_project(
    project: &Arc<AuthoringProject>,
    state: &AuthoringUiState,
) -> (Arc<AuthoringProject>, Option<u64>) {
    let Some(gesture) = state.preview.transform_gesture.as_ref() else {
        return (Arc::clone(project), None);
    };
    let updates = gesture_updates(gesture);
    if updates.is_empty() {
        return (Arc::clone(project), Some(transform_digest(gesture)));
    }
    match TimelineEditorService::project_authored_property_values(
        project,
        AuthoringPropertyOwner::Item(gesture.item_id),
        updates,
    ) {
        Ok(projected) => (Arc::new(projected), Some(transform_digest(gesture))),
        Err(_) => (Arc::clone(project), None),
    }
}

pub(super) fn transient_edit_digest(state: &AuthoringUiState) -> Option<u64> {
    state
        .preview
        .transform_gesture
        .as_ref()
        .map(transform_digest)
}

fn transform_digest(gesture: &PreviewTransformGesture) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    gesture.item_id.hash(&mut hasher);
    gesture.handle.hash(&mut hasher);
    gesture.local_time.hash(&mut hasher);
    gesture.position_keyframed.hash(&mut hasher);
    gesture.scale_keyframed.hash(&mut hasher);
    gesture.rotation_keyframed.hash(&mut hasher);
    gesture.projected_position.hash(&mut hasher);
    gesture.projected_scale.hash(&mut hasher);
    OrderedFloat(gesture.projected_rotation).hash(&mut hasher);
    hasher.finish()
}

pub(super) fn selected_item_id(
    project: &AuthoringProject,
    state: &AuthoringUiState,
) -> Option<TimelineItemId> {
    let AuthoringSelection::Item(item_id) = state.selection.primary()? else {
        return None;
    };
    let item = project.items.get(&item_id)?;
    project
        .tracks
        .get(&item.track_id)
        .is_some_and(|track| track.timeline_id == state.active_timeline_id)
        .then_some(item_id)
}

/// Owns an already-started body, resize, or rotation drag. Returns true while
/// Preview pointer handling must not begin another selection gesture.
pub(super) fn handle_active_gesture(
    ui: &Ui,
    revision: ProjectRevision,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
) -> bool {
    let Some(active) = state.preview.transform_gesture.as_ref() else {
        return false;
    };
    let (primary_down, primary_released, escape) = ui.input(|input| {
        (
            input.pointer.primary_down(),
            input.pointer.primary_released(),
            input.key_pressed(egui::Key::Escape),
        )
    });
    let stale = selected_item_id(project, state) != Some(active.item_id)
        || active.project_revision != revision
        || state.preview.active_tool != PreviewTool::Select;
    if escape || stale || (!primary_down && !primary_released) {
        state.preview.transform_gesture = None;
        return true;
    }

    if let Some(pointer) = ui.ctx().pointer_latest_pos() {
        let modifiers = ui.input(|input| input.modifiers);
        if let Some(gesture) = state.preview.transform_gesture.as_mut() {
            if let Err(error) = project_gesture(gesture, pointer, modifiers) {
                state.preview.transform_gesture = None;
                state.error = Some(error);
                return true;
            }
            ui.ctx().set_cursor_icon(cursor_for_gesture(gesture.handle));
        }
    }

    if primary_released {
        if let Some(gesture) = state.preview.transform_gesture.take() {
            commit_gesture(state, service, gesture);
        }
    }
    true
}

/// Registers the original production gizmo handle hit regions and starts a
/// transform gesture before the Preview's body-drag selection path can claim
/// the pointer.
pub(super) fn interact_handles(
    ui: &Ui,
    viewport: Rect,
    canvas: CanvasTransform,
    frame: Option<&FrameInfo>,
    revision: ProjectRevision,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
) -> bool {
    if state.preview.active_tool != PreviewTool::Select {
        return false;
    }
    let Some(item_id) = selected_item_id(project, state) else {
        return false;
    };
    let Some(geometry) = frame.and_then(|frame| item_gizmo_geometry(frame, item_id)) else {
        return false;
    };
    let screen_outline = geometry
        .control_outline
        .map(|point| canvas.world_to_screen(point));
    let Some(placements) = handle_placements(screen_outline) else {
        return false;
    };
    let enabled = geometry.parent_transform.inverse().is_some();
    for placement in placements {
        let hit_rect = Rect::from_center_size(placement.position, Vec2::splat(HANDLE_HIT_SIZE));
        if !viewport.intersects(hit_rect) {
            continue;
        }
        let response = ui.interact(
            hit_rect,
            ui.make_persistent_id(("preview.gizmo.handle", item_id, placement.handle)),
            Sense::drag(),
        );
        if response.hovered() && enabled {
            ui.ctx().set_cursor_icon(placement.cursor);
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            if !enabled {
                state.error = Some(
                    "Cannot edit this transform because its parent transform is singular"
                        .to_string(),
                );
                return true;
            }
            let pointer = response
                .interact_pointer_pos()
                .or_else(|| ui.ctx().pointer_latest_pos())
                .unwrap_or(placement.position);
            begin_gesture(
                item_id,
                Some(placement.handle),
                pointer,
                canvas,
                geometry,
                revision,
                project,
                state,
            );
            return true;
        }
    }
    false
}

pub(super) fn begin_body_gesture(
    item_id: TimelineItemId,
    pointer: Pos2,
    canvas: CanvasTransform,
    frame: Option<&FrameInfo>,
    revision: ProjectRevision,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
) {
    let Some(geometry) = frame.and_then(|frame| item_gizmo_geometry(frame, item_id)) else {
        state.error = Some("The selected item's rendered bounds are unavailable".to_string());
        return;
    };
    begin_gesture(
        item_id, None, pointer, canvas, geometry, revision, project, state,
    );
}

#[expect(
    clippy::too_many_arguments,
    reason = "gesture capture keeps item, pointer, viewport, frame, revision, and Project identity explicit"
)]
fn begin_gesture(
    item_id: TimelineItemId,
    handle: Option<GizmoHandle>,
    pointer: Pos2,
    canvas: CanvasTransform,
    geometry: ItemGizmoGeometry,
    revision: ProjectRevision,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
) {
    let Some(item) = project.items.get(&item_id) else {
        return;
    };
    let local_time = match item_local_time(project, state, item) {
        Ok(time) => time,
        Err(error) => {
            state.error = Some(error);
            return;
        }
    };
    let edits_scale = handle.is_some_and(|handle| handle != GizmoHandle::Rotation);
    let edits_rotation = handle == Some(GizmoHandle::Rotation);
    let edits_position = handle.is_none() || edits_scale;

    let (position, position_keyframed) = match authored_vec2(
        item,
        "position",
        local_time,
        property_vec2(
            geometry.item_transform.position.x,
            geometry.item_transform.position.y,
        ),
        edits_position,
    ) {
        Ok(value) => value,
        Err(error) => {
            state.error = Some(error);
            return;
        }
    };
    let (scale, scale_keyframed) = match authored_vec2(
        item,
        "scale",
        local_time,
        property_vec2(
            geometry.item_transform.scale.x,
            geometry.item_transform.scale.y,
        ),
        edits_scale,
    ) {
        Ok(value) => value,
        Err(error) => {
            state.error = Some(error);
            return;
        }
    };
    let (rotation, rotation_keyframed) = match authored_number(
        item,
        "rotation",
        local_time,
        geometry.item_transform.rotation,
        edits_rotation,
    ) {
        Ok(value) => value,
        Err(error) => {
            state.error = Some(error);
            return;
        }
    };

    state.error = None;
    state.preview.transform_gesture = Some(PreviewTransformGesture {
        item_id,
        handle,
        pointer_origin: pointer,
        canvas_origin: canvas,
        original_position: position,
        projected_position: position,
        original_scale: scale,
        projected_scale: scale,
        original_rotation: rotation,
        projected_rotation: rotation,
        original_visual_transform: geometry.item_transform.clone(),
        projected_visual_transform: geometry.item_transform,
        parent_transform: geometry.parent_transform,
        local_bounds: geometry.local_bounds,
        local_time,
        position_keyframed,
        scale_keyframed,
        rotation_keyframed,
        project_revision: revision,
    });
}

fn project_gesture(
    gesture: &mut PreviewTransformGesture,
    pointer: Pos2,
    modifiers: egui::Modifiers,
) -> Result<(), String> {
    gesture.projected_position = gesture.original_position;
    gesture.projected_scale = gesture.original_scale;
    gesture.projected_rotation = gesture.original_rotation;
    gesture.projected_visual_transform = gesture.original_visual_transform.clone();

    let start_world = gesture
        .canvas_origin
        .screen_to_world(gesture.pointer_origin)
        .ok_or_else(|| "Preview canvas transform is not invertible".to_string())?;
    let current_world = gesture
        .canvas_origin
        .screen_to_world(pointer)
        .ok_or_else(|| "Preview canvas transform is not invertible".to_string())?;
    let world_delta = current_world - start_world;
    let parent_delta = inverse_map_vector(gesture.parent_transform, world_delta)
        .ok_or_else(|| "Parent transform is not invertible".to_string())?;

    match gesture.handle {
        None => project_translation(gesture, parent_delta),
        Some(GizmoHandle::Rotation) => {
            project_rotation(gesture, start_world, current_world)?;
        }
        Some(handle) => project_scale(gesture, handle, parent_delta, modifiers),
    }
    Ok(())
}

fn project_translation(gesture: &mut PreviewTransformGesture, delta: Vec2) {
    gesture.projected_position = property_vec2(
        gesture.original_position.x.into_inner() + f64::from(delta.x),
        gesture.original_position.y.into_inner() + f64::from(delta.y),
    );
    gesture.projected_visual_transform.position.x =
        gesture.original_visual_transform.position.x + f64::from(delta.x);
    gesture.projected_visual_transform.position.y =
        gesture.original_visual_transform.position.y + f64::from(delta.y);
}

fn project_rotation(
    gesture: &mut PreviewTransformGesture,
    start_world: Pos2,
    current_world: Pos2,
) -> Result<(), String> {
    let (center_x, center_y) = gesture.parent_transform.map_point(
        gesture.original_visual_transform.position.x,
        gesture.original_visual_transform.position.y,
    );
    let center = Pos2::new(center_x as f32, center_y as f32);
    if !center.is_finite() {
        return Err("Rotation centre is not finite".to_string());
    }
    let start = start_world - center;
    let current = current_world - center;
    if start.length_sq() <= f32::EPSILON || current.length_sq() <= f32::EPSILON {
        return Ok(());
    }
    let delta = f64::from((current.y.atan2(current.x) - start.y.atan2(start.x)).to_degrees());
    gesture.projected_rotation = gesture.original_rotation + delta;
    gesture.projected_visual_transform.rotation =
        gesture.original_visual_transform.rotation + delta;
    Ok(())
}

fn project_scale(
    gesture: &mut PreviewTransformGesture,
    handle: GizmoHandle,
    parent_delta: Vec2,
    modifiers: egui::Modifiers,
) {
    let visual_rotation = gesture.original_visual_transform.rotation as f32;
    let local_delta = rotate_vec(parent_delta, -visual_rotation);
    let current_width =
        gesture.local_bounds.width() * gesture.original_visual_transform.scale.x as f32;
    let current_height =
        gesture.local_bounds.height() * gesture.original_visual_transform.scale.y as f32;
    let (sign_x, sign_y) = handle_sign(handle);
    let multiplier = if modifiers.alt { 2.0 } else { 1.0 };
    let width_delta = local_delta.x * sign_x * multiplier;
    let height_delta = local_delta.y * sign_y * multiplier;
    let mut next_width = current_width + width_delta;
    let mut next_height = current_height + height_delta;

    if modifiers.shift {
        let ratio = if current_height.abs() > f32::EPSILON {
            current_width / current_height
        } else {
            1.0
        };
        if sign_x != 0.0 && sign_y != 0.0 {
            if width_delta.abs() > height_delta.abs() {
                next_height = next_width / ratio;
            } else {
                next_width = next_height * ratio;
            }
        } else if sign_x != 0.0 {
            next_height = next_width / ratio;
        } else if sign_y != 0.0 {
            next_width = next_height * ratio;
        }
    }

    let scale_ratio_x = if current_width.abs() > f32::EPSILON {
        next_width / current_width
    } else {
        1.0
    };
    let scale_ratio_y = if current_height.abs() > f32::EPSILON {
        next_height / current_height
    } else {
        1.0
    };
    gesture.projected_scale = property_vec2(
        gesture.original_scale.x.into_inner() * f64::from(scale_ratio_x),
        gesture.original_scale.y.into_inner() * f64::from(scale_ratio_y),
    );
    gesture.projected_visual_transform.scale.x =
        gesture.original_visual_transform.scale.x * f64::from(scale_ratio_x);
    gesture.projected_visual_transform.scale.y =
        gesture.original_visual_transform.scale.y * f64::from(scale_ratio_y);

    // Preserve the opposite handle (normal resize) or the visual bounds centre
    // (Alt resize) for arbitrary Anchor values. For a centred Anchor this is
    // exactly the production gizmo's half-delta position shift.
    let bounds = gesture.local_bounds;
    let pivot = if modifiers.alt {
        bounds.center()
    } else {
        Pos2::new(
            if sign_x > 0.0 {
                bounds.left()
            } else if sign_x < 0.0 {
                bounds.right()
            } else {
                gesture.original_visual_transform.anchor.x as f32
            },
            if sign_y > 0.0 {
                bounds.top()
            } else if sign_y < 0.0 {
                bounds.bottom()
            } else {
                gesture.original_visual_transform.anchor.y as f32
            },
        )
    };
    let anchor = egui::pos2(
        gesture.original_visual_transform.anchor.x as f32,
        gesture.original_visual_transform.anchor.y as f32,
    );
    let from_anchor = pivot - anchor;
    let old_pivot_offset = egui::vec2(
        from_anchor.x * gesture.original_visual_transform.scale.x as f32,
        from_anchor.y * gesture.original_visual_transform.scale.y as f32,
    );
    let new_pivot_offset = egui::vec2(
        from_anchor.x * gesture.projected_visual_transform.scale.x as f32,
        from_anchor.y * gesture.projected_visual_transform.scale.y as f32,
    );
    let position_delta = rotate_vec(old_pivot_offset - new_pivot_offset, visual_rotation);
    gesture.projected_position = property_vec2(
        gesture.original_position.x.into_inner() + f64::from(position_delta.x),
        gesture.original_position.y.into_inner() + f64::from(position_delta.y),
    );
    gesture.projected_visual_transform.position.x =
        gesture.original_visual_transform.position.x + f64::from(position_delta.x);
    gesture.projected_visual_transform.position.y =
        gesture.original_visual_transform.position.y + f64::from(position_delta.y);
}

fn commit_gesture(
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    gesture: PreviewTransformGesture,
) {
    let updates = gesture_updates(&gesture);
    if updates.is_empty() {
        return;
    }
    match service
        .apply_authored_property_values(AuthoringPropertyOwner::Item(gesture.item_id), updates)
    {
        Ok(_) => {
            state.inspector.invalidate();
            state.error = None;
            state.status = match gesture.handle {
                None => "Moved clip",
                Some(GizmoHandle::Rotation) => "Rotated clip",
                Some(_) => "Resized clip",
            }
            .to_string();
        }
        Err(error) => state.error = Some(error.to_string()),
    }
}

fn gesture_updates(gesture: &PreviewTransformGesture) -> Vec<AuthoringPropertyValueUpdate> {
    let mut updates = Vec::with_capacity(2);
    if gesture.projected_position != gesture.original_position {
        updates.push(AuthoringPropertyValueUpdate {
            key: "position".to_string(),
            value: PropertyValue::Vec2(gesture.projected_position),
            target: property_target(gesture.position_keyframed, gesture.local_time),
        });
    }
    if gesture.projected_scale != gesture.original_scale {
        updates.push(AuthoringPropertyValueUpdate {
            key: "scale".to_string(),
            value: PropertyValue::Vec2(gesture.projected_scale),
            target: property_target(gesture.scale_keyframed, gesture.local_time),
        });
    }
    if OrderedFloat(gesture.projected_rotation) != OrderedFloat(gesture.original_rotation) {
        updates.push(AuthoringPropertyValueUpdate {
            key: "rotation".to_string(),
            value: PropertyValue::Number(OrderedFloat(gesture.projected_rotation)),
            target: property_target(gesture.rotation_keyframed, gesture.local_time),
        });
    }
    updates
}

fn property_target(keyframed: bool, local_time: MediaTime) -> AuthoringPropertyValueTarget {
    if keyframed {
        AuthoringPropertyValueTarget::Keyframe { local_time }
    } else {
        AuthoringPropertyValueTarget::Constant
    }
}

pub(super) fn paint_gizmo(
    ui: &Ui,
    viewport: Rect,
    canvas: CanvasTransform,
    frame: Option<&FrameInfo>,
    project: &AuthoringProject,
    state: &AuthoringUiState,
) {
    if state.preview.active_tool != PreviewTool::Select {
        return;
    }
    let Some(frame) = frame else {
        return;
    };
    let primary = selected_item_id(project, state);
    let painter = ui.painter().with_clip_rect(viewport);

    for selection in state.selection.iter() {
        let AuthoringSelection::Item(item_id) = selection else {
            continue;
        };
        if Some(item_id) == primary {
            continue;
        }
        let Some(geometry) = item_gizmo_geometry(frame, item_id) else {
            continue;
        };
        paint_outlines(
            &painter,
            &geometry.outlines,
            canvas,
            GIZMO_COLOR.linear_multiply(0.5),
            1.0,
        );
    }

    let Some(item_id) = primary else {
        return;
    };
    let Some(geometry) = item_gizmo_geometry(frame, item_id) else {
        return;
    };
    let projected = state
        .preview
        .transform_gesture
        .as_ref()
        .filter(|gesture| gesture.item_id == item_id)
        .and_then(|gesture| geometry.projected(&gesture.projected_visual_transform));
    let (outlines, control_outline, anchor, dragging) = match projected {
        Some(projected) => (
            projected.outlines,
            projected.control_outline,
            projected.anchor,
            true,
        ),
        None => (
            geometry.outlines.clone(),
            geometry.control_outline,
            geometry.anchor,
            false,
        ),
    };
    paint_outlines(&painter, &outlines, canvas, GIZMO_COLOR, 1.25);
    let screen_outline = control_outline.map(|point| canvas.world_to_screen(point));
    paint_screen_outline(&painter, screen_outline, GIZMO_COLOR, 2.0);
    let Some(placements) = handle_placements(screen_outline) else {
        return;
    };
    let top = screen_outline[0].lerp(screen_outline[1], 0.5);
    let rotation = placements
        .iter()
        .find(|placement| placement.handle == GizmoHandle::Rotation)
        .map(|placement| placement.position)
        .unwrap_or(top);
    painter.line_segment([top, rotation], egui::Stroke::new(2.0, GIZMO_COLOR));
    for placement in placements {
        painter.circle_filled(placement.position, HANDLE_RADIUS, GIZMO_COLOR);
        let handle_rect = Rect::from_center_size(placement.position, Vec2::splat(HANDLE_HIT_SIZE));
        let name = handle_name(placement.handle);
        crate::qa::register_component_with_metadata(
            format!("preview.gizmo.handle:{name}"),
            "preview_gizmo_handle",
            handle_rect.intersect(viewport),
            geometry.parent_transform.inverse().is_some(),
            Some(serde_json::json!({
                "item_id": item_id,
                "handle": name,
                "action": "drag_preview_gizmo_handle",
            })),
        );
    }

    let screen_anchor = canvas.world_to_screen(anchor);
    if viewport.expand(8.0).contains(screen_anchor) {
        painter.circle_filled(screen_anchor, 4.0, GIZMO_COLOR);
        painter.line_segment(
            [
                screen_anchor - egui::vec2(8.0, 0.0),
                screen_anchor + egui::vec2(8.0, 0.0),
            ],
            egui::Stroke::new(1.0, egui::Color32::WHITE),
        );
        painter.line_segment(
            [
                screen_anchor - egui::vec2(0.0, 8.0),
                screen_anchor + egui::vec2(0.0, 8.0),
            ],
            egui::Stroke::new(1.0, egui::Color32::WHITE),
        );
    }
    let screen_bounds = Rect::from_points(&screen_outline);
    let qa_bounds = screen_bounds.intersect(viewport);
    let item = project.items.get(&item_id);
    let metadata = serde_json::json!({
        "item_id": item_id,
        "dragging": dragging,
        "source_kind": item.map(|item| source_kind(&item.source)),
        "outline_count": outlines.len(),
        "screen_bounds": {
            "min": {"x": screen_bounds.min.x, "y": screen_bounds.min.y},
            "max": {"x": screen_bounds.max.x, "y": screen_bounds.max.y},
        },
        "canvas_transform": {
            "pan": {"x": canvas.state.pan.x, "y": canvas.state.pan.y},
            "zoom": {"x": canvas.state.zoom.x, "y": canvas.state.zoom.y},
        },
    });
    crate::qa::register_component_with_metadata(
        "preview.gizmo.bounds",
        "preview_gizmo",
        qa_bounds,
        false,
        Some(metadata.clone()),
    );
    // Stable compatibility ID used by the existing native authoring QA. It
    // describes the same production gizmo; it is not another implementation.
    crate::qa::register_component_with_metadata(
        "preview.position_gizmo",
        "preview_position_gizmo",
        qa_bounds,
        true,
        Some(metadata),
    );
}

fn paint_outlines(
    painter: &egui::Painter,
    outlines: &[[Pos2; 4]],
    canvas: CanvasTransform,
    color: egui::Color32,
    thickness: f32,
) {
    for outline in outlines {
        paint_screen_outline(
            painter,
            outline.map(|point| canvas.world_to_screen(point)),
            color,
            thickness,
        );
    }
}

fn paint_screen_outline(
    painter: &egui::Painter,
    corners: [Pos2; 4],
    color: egui::Color32,
    thickness: f32,
) {
    let stroke = egui::Stroke::new(thickness, color);
    for index in 0..corners.len() {
        painter.line_segment(
            [corners[index], corners[(index + 1) % corners.len()]],
            stroke,
        );
    }
}

fn handle_placements(corners: [Pos2; 4]) -> Option<[HandlePlacement; 9]> {
    if corners.iter().any(|corner| !corner.is_finite()) {
        return None;
    }
    let top = corners[0].lerp(corners[1], 0.5);
    let bottom = corners[2].lerp(corners[3], 0.5);
    let left = corners[0].lerp(corners[3], 0.5);
    let right = corners[1].lerp(corners[2], 0.5);
    let top_edge = corners[1] - corners[0];
    if top_edge.length_sq() <= f32::EPSILON {
        return None;
    }
    let outward = egui::vec2(top_edge.y, -top_edge.x).normalized();
    let rotation = top + outward * ROTATION_HANDLE_DISTANCE;
    Some([
        placement(corners[0], GizmoHandle::TopLeft, CursorIcon::ResizeNwSe),
        placement(top, GizmoHandle::Top, CursorIcon::ResizeVertical),
        placement(corners[1], GizmoHandle::TopRight, CursorIcon::ResizeNeSw),
        placement(left, GizmoHandle::Left, CursorIcon::ResizeHorizontal),
        placement(right, GizmoHandle::Right, CursorIcon::ResizeHorizontal),
        placement(corners[3], GizmoHandle::BottomLeft, CursorIcon::ResizeNeSw),
        placement(bottom, GizmoHandle::Bottom, CursorIcon::ResizeVertical),
        placement(corners[2], GizmoHandle::BottomRight, CursorIcon::ResizeNwSe),
        placement(rotation, GizmoHandle::Rotation, CursorIcon::Grab),
    ])
}

const fn placement(position: Pos2, handle: GizmoHandle, cursor: CursorIcon) -> HandlePlacement {
    HandlePlacement {
        position,
        handle,
        cursor,
    }
}

const fn handle_name(handle: GizmoHandle) -> &'static str {
    match handle {
        GizmoHandle::TopLeft => "top_left",
        GizmoHandle::Top => "top",
        GizmoHandle::TopRight => "top_right",
        GizmoHandle::Left => "left",
        GizmoHandle::Right => "right",
        GizmoHandle::BottomLeft => "bottom_left",
        GizmoHandle::Bottom => "bottom",
        GizmoHandle::BottomRight => "bottom_right",
        GizmoHandle::Rotation => "rotation",
    }
}

fn cursor_for_gesture(handle: Option<GizmoHandle>) -> CursorIcon {
    match handle {
        None => CursorIcon::Grabbing,
        Some(GizmoHandle::Rotation) => CursorIcon::Grabbing,
        Some(GizmoHandle::TopLeft | GizmoHandle::BottomRight) => CursorIcon::ResizeNwSe,
        Some(GizmoHandle::TopRight | GizmoHandle::BottomLeft) => CursorIcon::ResizeNeSw,
        Some(GizmoHandle::Top | GizmoHandle::Bottom) => CursorIcon::ResizeVertical,
        Some(GizmoHandle::Left | GizmoHandle::Right) => CursorIcon::ResizeHorizontal,
    }
}

fn handle_sign(handle: GizmoHandle) -> (f32, f32) {
    match handle {
        GizmoHandle::TopLeft => (-1.0, -1.0),
        GizmoHandle::Top => (0.0, -1.0),
        GizmoHandle::TopRight => (1.0, -1.0),
        GizmoHandle::Left => (-1.0, 0.0),
        GizmoHandle::Right => (1.0, 0.0),
        GizmoHandle::BottomLeft => (-1.0, 1.0),
        GizmoHandle::Bottom => (0.0, 1.0),
        GizmoHandle::BottomRight => (1.0, 1.0),
        GizmoHandle::Rotation => (0.0, 0.0),
    }
}

fn rotate_vec(vector: Vec2, angle_degrees: f32) -> Vec2 {
    let (sin, cos) = angle_degrees.to_radians().sin_cos();
    egui::vec2(
        vector.x * cos - vector.y * sin,
        vector.x * sin + vector.y * cos,
    )
}

fn inverse_map_vector(transform: Affine2D, vector: Vec2) -> Option<Vec2> {
    let inverse = transform.inverse()?;
    let mapped = egui::vec2(
        (inverse.scale_x * f64::from(vector.x) + inverse.skew_x * f64::from(vector.y)) as f32,
        (inverse.skew_y * f64::from(vector.x) + inverse.scale_y * f64::from(vector.y)) as f32,
    );
    mapped.is_finite().then_some(mapped)
}

fn item_local_time(
    project: &AuthoringProject,
    state: &AuthoringUiState,
    item: &TimelineItem,
) -> Result<MediaTime, String> {
    let timeline = project
        .timelines
        .get(&state.active_timeline_id)
        .ok_or_else(|| "Active Timeline is missing".to_string())?;
    let timeline_time = MediaTime::from_frame_index(state.timeline.current_frame, timeline.fps)?;
    if !item.interval.contains(timeline_time)? {
        return Err("Move the playhead over the selected clip before editing it".to_string());
    }
    item.time_map.local_time(item.interval, timeline_time)
}

fn authored_vec2(
    item: &TimelineItem,
    key: &str,
    local_time: MediaTime,
    fallback: PropertyVec2,
    required: bool,
) -> Result<(PropertyVec2, bool), String> {
    let Some(property) = item.authored_properties.get(key) else {
        return Ok((fallback, false));
    };
    let keyframed = match property.evaluator.as_str() {
        "constant" => false,
        "keyframe" => true,
        _evaluator if !required => return Ok((fallback, false)),
        evaluator => return Err(controlled_property_error(key, evaluator)),
    };
    let value = property
        .evaluate_at(local_time.to_seconds_f64())
        .map_err(|error| format!("Cannot sample {}: {error}", display_property(key)))?;
    let PropertyValue::Vec2(value) = value else {
        return Err(format!("{} is not a 2D value", display_property(key)));
    };
    Ok((value, keyframed))
}

fn authored_number(
    item: &TimelineItem,
    key: &str,
    local_time: MediaTime,
    fallback: f64,
    required: bool,
) -> Result<(f64, bool), String> {
    let Some(property) = item.authored_properties.get(key) else {
        return Ok((fallback, false));
    };
    let keyframed = match property.evaluator.as_str() {
        "constant" => false,
        "keyframe" => true,
        _evaluator if !required => return Ok((fallback, false)),
        evaluator => return Err(controlled_property_error(key, evaluator)),
    };
    let value = property
        .evaluate_at(local_time.to_seconds_f64())
        .map_err(|error| format!("Cannot sample {}: {error}", display_property(key)))?;
    let PropertyValue::Number(value) = value else {
        return Err(format!("{} is not a Number", display_property(key)));
    };
    Ok((value.into_inner(), keyframed))
}

fn controlled_property_error(key: &str, evaluator: &str) -> String {
    format!(
        "{} is controlled by '{evaluator}'. The gizmo was not applied because it would disconnect that control.",
        display_property(key)
    )
}

fn display_property(key: &str) -> &'static str {
    match key {
        "position" => "Position",
        "scale" => "Scale",
        "rotation" => "Rotation",
        _ => "Property",
    }
}

fn property_vec2(x: f64, y: f64) -> PropertyVec2 {
    PropertyVec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    }
}

fn source_kind(source: &library::model::authoring::SourceRef) -> &'static str {
    use library::model::authoring::SourceRef;
    match source {
        SourceRef::Asset { .. } => "asset",
        SourceRef::Text { .. } => "text",
        SourceRef::Shape { .. } => "shape",
        SourceRef::Solid { .. } => "solid",
        SourceRef::Composition(_) => "composition",
        SourceRef::Module(_) => "module",
    }
}

#[cfg(test)]
#[path = "gizmo_tests.rs"]
mod tests;
