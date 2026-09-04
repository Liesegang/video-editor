//! Direct canvas editing for Timeline-owned transform values.

use library::editor::{AuthoringPropertyOwner, TimelineEditorService};
use library::model::authoring::{
    AuthoringProject, MediaTime, ProjectRevision, TimelineItem, TimelineItemId,
};
use library::model::property::{PropertyValue, Vec2 as PropertyVec2};
use ordered_float::OrderedFloat;

use crate::state::authoring::{
    AuthoringSelection, AuthoringUiState, PreviewPositionGesture, PreviewTool,
};

pub(super) fn handle_position_gesture(
    ui: &egui::Ui,
    response: &egui::Response,
    content_rect: egui::Rect,
    revision: ProjectRevision,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
) {
    let selected_item_id = selected_item_id(project, state);
    let (primary_down, primary_released, escape, space_down) = ui.input(|input| {
        (
            input.pointer.primary_down(),
            input.pointer.primary_released(),
            input.key_pressed(egui::Key::Escape),
            input.key_down(egui::Key::Space),
        )
    });

    if state.preview.position_gesture.is_some() {
        let should_cancel = escape
            || state.preview.active_tool != PreviewTool::Select
            || selected_item_id
                != state
                    .preview
                    .position_gesture
                    .as_ref()
                    .map(|gesture| gesture.item_id)
            || state
                .preview
                .position_gesture
                .as_ref()
                .is_some_and(|gesture| gesture.project_revision != revision)
            || (!primary_down && !primary_released);
        if should_cancel {
            state.preview.position_gesture = None;
            return;
        }

        if let Some(pointer) = ui.ctx().pointer_latest_pos() {
            if let Some(gesture) = state.preview.position_gesture.as_mut() {
                if let Some(projected) = projected_position(
                    gesture.original_position,
                    pointer - gesture.pointer_origin,
                    gesture.zoom_origin,
                ) {
                    gesture.projected_position = projected;
                }
            }
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);

        if primary_released {
            if let Some(gesture) = state.preview.position_gesture.take() {
                if gesture.projected_position != gesture.original_position {
                    commit_position_gesture(state, service, gesture);
                }
            }
        }
        return;
    }

    if state.preview.active_tool != PreviewTool::Select
        || space_down
        || !response.drag_started_by(egui::PointerButton::Primary)
    {
        return;
    }
    let Some(pointer) = response.interact_pointer_pos() else {
        return;
    };
    if !content_rect.contains(pointer) {
        return;
    }
    let Some(item_id) = selected_item_id else {
        return;
    };
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
    let (position, keyframed) = match authored_position(item, local_time) {
        Ok(position) => position,
        Err(error) => {
            // Replacing a context-dependent evaluator would silently sever
            // automation. Refuse the gesture until an additive override model
            // is available instead of changing ownership behind the user.
            state.error = Some(error);
            return;
        }
    };
    state.preview.position_gesture = Some(PreviewPositionGesture {
        item_id,
        pointer_origin: pointer,
        zoom_origin: state.preview.zoom,
        original_position: position,
        projected_position: position,
        local_time,
        keyframed,
        project_revision: revision,
    });
}

fn selected_item_id(
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
        return Err("Move the playhead over the selected clip before dragging it".to_string());
    }
    item.time_map.local_time(item.interval, timeline_time)
}

fn authored_position(
    item: &TimelineItem,
    local_time: MediaTime,
) -> Result<(PropertyVec2, bool), String> {
    let Some(property) = item.authored_properties.get("position") else {
        return Ok((zero_position(), false));
    };
    let keyframed = match property.evaluator.as_str() {
        "constant" => false,
        "keyframe" => true,
        evaluator => {
            return Err(format!(
                "Position is controlled by '{evaluator}'. Direct drag was not applied because it would disconnect that control."
            ));
        }
    };
    let value = property
        .evaluate_at(local_time.to_seconds_f64())
        .map_err(|error| format!("Cannot sample Position: {error}"))?;
    let PropertyValue::Vec2(position) = value else {
        return Err("Authored Position is not a 2D value".to_string());
    };
    Ok((position, keyframed))
}

fn zero_position() -> PropertyVec2 {
    PropertyVec2 {
        x: OrderedFloat(0.0),
        y: OrderedFloat(0.0),
    }
}

fn projected_position(
    original: PropertyVec2,
    screen_delta: egui::Vec2,
    zoom: f32,
) -> Option<PropertyVec2> {
    if !zoom.is_finite() || zoom <= 0.0 || !screen_delta.is_finite() {
        return None;
    }
    let x = original.x.into_inner() + f64::from(screen_delta.x / zoom);
    let y = original.y.into_inner() + f64::from(screen_delta.y / zoom);
    (x.is_finite() && y.is_finite()).then_some(PropertyVec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    })
}

fn commit_position_gesture(
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    gesture: PreviewPositionGesture,
) {
    let value = PropertyValue::Vec2(gesture.projected_position);
    let result = if gesture.keyframed {
        service
            .upsert_authored_property_keyframe(
                AuthoringPropertyOwner::Item(gesture.item_id),
                "position".to_string(),
                gesture.local_time,
                value,
                None,
            )
            .map(|_| ())
    } else {
        service
            .set_authored_property_constant(
                AuthoringPropertyOwner::Item(gesture.item_id),
                "position".to_string(),
                value,
            )
            .map(|_| ())
    };
    match result {
        Ok(()) => {
            state.inspector.invalidate();
            state.error = None;
            state.status = if gesture.keyframed {
                "Updated Position keyframe".to_string()
            } else {
                "Moved clip".to_string()
            };
        }
        Err(error) => state.error = Some(error.to_string()),
    }
}

pub(super) fn paint_position_gizmo(
    ui: &egui::Ui,
    viewport: egui::Rect,
    content_rect: egui::Rect,
    project: &AuthoringProject,
    state: &AuthoringUiState,
) {
    if state.preview.active_tool != PreviewTool::Select {
        return;
    }
    let Some(item_id) = selected_item_id(project, state) else {
        return;
    };
    let (position, keyframed, dragging) = if let Some(gesture) = state
        .preview
        .position_gesture
        .as_ref()
        .filter(|gesture| gesture.item_id == item_id)
    {
        (gesture.projected_position, gesture.keyframed, true)
    } else {
        let Some(item) = project.items.get(&item_id) else {
            return;
        };
        let Ok(local_time) = item_local_time(project, state, item) else {
            return;
        };
        let Ok((position, keyframed)) = authored_position(item, local_time) else {
            return;
        };
        (position, keyframed, false)
    };
    let offset = egui::vec2(
        position.x.into_inner() as f32,
        position.y.into_inner() as f32,
    ) * state.preview.zoom;
    let bounds = content_rect.translate(offset);
    let visible = bounds.intersect(viewport);
    let painter = ui.painter().with_clip_rect(viewport);
    if visible.is_positive() {
        painter.rect_stroke(
            bounds,
            0.0,
            egui::Stroke::new(1.5, egui::Color32::from_rgb(86, 177, 255)),
            egui::StrokeKind::Inside,
        );
    }
    let origin = bounds.min;
    if viewport.expand(8.0).contains(origin) {
        painter.circle_filled(origin, 4.0, egui::Color32::from_rgb(86, 177, 255));
        painter.line_segment(
            [origin - egui::vec2(8.0, 0.0), origin + egui::vec2(8.0, 0.0)],
            egui::Stroke::new(1.0, egui::Color32::WHITE),
        );
        painter.line_segment(
            [origin - egui::vec2(0.0, 8.0), origin + egui::vec2(0.0, 8.0)],
            egui::Stroke::new(1.0, egui::Color32::WHITE),
        );
    }
    let qa_rect = if visible.is_positive() {
        visible
    } else {
        egui::Rect::from_center_size(origin, egui::Vec2::splat(1.0))
    };
    crate::qa::register_component_with_metadata(
        "timeline_first.preview.position_gizmo",
        "timeline_first_preview_position_gizmo",
        qa_rect,
        true,
        Some(serde_json::json!({
            "item_id": item_id,
            "position": {
                "x": position.x.into_inner(),
                "y": position.y.into_inner(),
            },
            "keyframed": keyframed,
            "dragging": dragging,
        })),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::authoring::{AuthoringProject, RationalRate, SourceRef, TimelineInterval};
    use library::model::frame::color::Color;

    fn editable_item() -> (TimelineEditorService, AuthoringUiState, TimelineItemId) {
        let project = AuthoringProject::new(
            "Preview direct edit",
            640,
            360,
            RationalRate::new(30, 1).expect("fixture rate"),
            MediaTime::new(10, 1).expect("fixture duration"),
        )
        .expect("fixture Project");
        let timeline_id = project.root_timeline_id;
        let track_id = project
            .timelines
            .get(&timeline_id)
            .and_then(|timeline| timeline.track_order.first())
            .copied()
            .expect("fixture Track");
        let service = TimelineEditorService::new(project).expect("fixture service");
        let (item_id, _) = service
            .add_item(
                track_id,
                "Movable Solid".to_string(),
                SourceRef::Solid {
                    color: Color {
                        r: 255,
                        g: 255,
                        b: 255,
                        a: 255,
                    },
                },
                TimelineInterval::new(
                    MediaTime::zero(),
                    MediaTime::new(5, 1).expect("fixture item duration"),
                )
                .expect("fixture interval"),
                0,
            )
            .expect("fixture item");
        (service, AuthoringUiState::new(timeline_id), item_id)
    }

    fn position(x: f64, y: f64) -> PropertyVec2 {
        PropertyVec2 {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
        }
    }

    fn gesture(
        service: &TimelineEditorService,
        item_id: TimelineItemId,
        original_position: PropertyVec2,
        projected_position: PropertyVec2,
        local_time: MediaTime,
        keyframed: bool,
    ) -> PreviewPositionGesture {
        PreviewPositionGesture {
            item_id,
            pointer_origin: egui::Pos2::ZERO,
            zoom_origin: 1.0,
            original_position,
            projected_position,
            local_time,
            keyframed,
            project_revision: service.revision().expect("fixture revision"),
        }
    }

    #[test]
    fn projection_uses_the_immutable_origin_and_canvas_zoom() {
        let original = position(12.0, -4.0);
        let projected =
            projected_position(original, egui::vec2(40.0, -10.0), 2.0).expect("finite projection");

        assert_eq!(projected.x.into_inner(), 32.0);
        assert_eq!(projected.y.into_inner(), -9.0);
        assert_eq!(original.x.into_inner(), 12.0);
        assert_eq!(original.y.into_inner(), -4.0);
    }

    #[test]
    fn constant_drag_commits_one_atomic_project_revision() {
        let (service, mut state, item_id) = editable_item();
        let before = service.revision().expect("revision before commit");

        commit_position_gesture(
            &mut state,
            &service,
            gesture(
                &service,
                item_id,
                position(0.0, 0.0),
                position(24.0, -8.0),
                MediaTime::zero(),
                false,
            ),
        );

        assert_eq!(
            service.revision().expect("revision after commit").get(),
            before.get() + 1
        );
        let project = service.snapshot().expect("updated Project");
        let item = project.items.get(&item_id).expect("updated item");
        assert_eq!(
            authored_position(item, MediaTime::zero()).expect("authored Position"),
            (position(24.0, -8.0), false)
        );
        assert_eq!(state.status, "Moved clip");
        assert!(state.error.is_none());
    }

    #[test]
    fn keyframed_drag_upserts_only_the_captured_local_time() {
        let (service, mut state, item_id) = editable_item();
        service
            .set_authored_property_constant(
                AuthoringPropertyOwner::Item(item_id),
                "position".to_string(),
                PropertyValue::Vec2(position(3.0, 4.0)),
            )
            .expect("constant Position");
        service
            .upsert_authored_property_keyframe(
                AuthoringPropertyOwner::Item(item_id),
                "position".to_string(),
                MediaTime::zero(),
                PropertyValue::Vec2(position(3.0, 4.0)),
                None,
            )
            .expect("initial keyframe");
        let local_time = MediaTime::new(3, 2).expect("exact local time");
        let before = service.revision().expect("revision before commit");

        commit_position_gesture(
            &mut state,
            &service,
            gesture(
                &service,
                item_id,
                position(3.0, 4.0),
                position(40.0, 50.0),
                local_time,
                true,
            ),
        );

        assert_eq!(
            service.revision().expect("revision after commit").get(),
            before.get() + 1
        );
        let project = service.snapshot().expect("updated Project");
        let item = project.items.get(&item_id).expect("updated item");
        assert_eq!(
            authored_position(item, local_time).expect("authored Position"),
            (position(40.0, 50.0), true)
        );
        assert_eq!(state.status, "Updated Position keyframe");
        assert!(state.error.is_none());
    }

    #[test]
    fn context_dependent_position_is_refused_instead_of_detached() {
        let (_, _, item_id) = editable_item();
        let mut item = TimelineItem {
            id: item_id,
            track_id: library::model::authoring::TimelineTrackId::new(),
            name: "Bound".to_string(),
            source: SourceRef::Solid {
                color: Color::black(),
            },
            interval: TimelineInterval::new(
                MediaTime::zero(),
                MediaTime::new(1, 1).expect("fixture duration"),
            )
            .expect("fixture interval"),
            time_map: library::model::authoring::TimeMap::default(),
            layer: 0,
            parent: None,
            authored_properties: library::model::property::PropertyMap::new(),
        };
        item.authored_properties.set(
            "position".to_string(),
            library::model::property::Property::expression(
                "external_signal".to_string(),
                PropertyValue::Vec2(position(1.0, 2.0)),
            ),
        );

        let error = authored_position(&item, MediaTime::zero())
            .expect_err("context-dependent control must not become a constant");
        assert!(error.contains("would disconnect that control"));
        assert_eq!(
            item.authored_properties
                .get("position")
                .expect("Position")
                .evaluator,
            "expression"
        );
    }
}
