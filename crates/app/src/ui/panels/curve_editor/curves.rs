use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use egui_phosphor::regular as icons;
use library::editor::TimelineEditorService;
use library::model::authoring::MediaTime;

use crate::state::authoring::{
    AuthoringUiState, AutomationOwner, CurveKeyDrag, CurveKeyframeEditor,
};
use crate::ui::automation_lanes::{
    self, component_name, component_value, lane_metadata, with_component,
};
use crate::ui::easing_menus::{easing_summary, show_easing_menu, EasingMenuQaScope};

use super::{update_curve_easing, CurveSeries, CurveTransform, POINT_RADIUS};

#[cfg(test)]
mod tests;

pub(super) fn paint_curve(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    curve: &CurveSeries,
    color: Color32,
    transform: CurveTransform,
) {
    if curve.points.is_empty() {
        return;
    }
    let painter = ui.painter().with_clip_rect(transform.rect);
    let line_shape = painter.add(egui::Shape::Noop);

    for point in &curve.points {
        let (time, value) = projected_point(state, curve, point);
        let center = transform.point(time.to_seconds_f64(), value);
        if !transform.rect.contains(center) {
            continue;
        }
        let hit = Rect::from_center_size(center, Vec2::splat(POINT_RADIUS * 3.0))
            .intersect(transform.rect);
        let response = ui.interact(
            hit,
            ui.id()
                .with(("curve-key", &curve.id, point.id, curve.component)),
            Sense::click_and_drag(),
        );
        if response.drag_started_by(egui::PointerButton::Primary) {
            if let (Some(pointer_origin), Ok(source_revision)) = (
                ui.input(|input| input.pointer.press_origin()),
                service.revision(),
            ) {
                state.curve_editor.drag = Some(CurveKeyDrag {
                    source_revision,
                    lane: curve.id.clone(),
                    component: curve.component,
                    keyframe_id: point.id,
                    original_time: point.time,
                    original_value: point.full_value.clone(),
                    pointer_origin,
                    projected_time: point.time,
                    projected_value: point.full_value.clone(),
                });
                update_key_drag(ui, state, transform);
            }
        }
        let (time, value) = projected_point(state, curve, point);
        let center = transform.point(time.to_seconds_f64(), value);
        let hit = Rect::from_center_size(center, Vec2::splat(POINT_RADIUS * 3.0))
            .intersect(transform.rect);
        crate::qa::register_component_with_metadata(
            format!(
                "curve_editor.key:{}:{}",
                point.id,
                component_name(curve.component)
            ),
            "curve_editor_keyframe",
            hit,
            true,
            Some(serde_json::json!({
                "item_id": match &curve.id.owner {
                    AutomationOwner::Item(item_id) => Some(*item_id),
                    _ => None,
                },
                "target": automation_lanes::target_metadata(&curve.id.target),
                "lane": lane_metadata(&curve.id),
                "keyframe_id": point.id,
                "time": time.to_seconds_f64(),
                "value": value,
                "component": component_name(curve.component),
            })),
        );
        if response.double_clicked() {
            state.curve_editor.keyframe_editor = Some(CurveKeyframeEditor {
                lane: curve.id.clone(),
                component: curve.component,
                keyframe_id: point.id,
                time: point.time,
                value: point.full_value.clone(),
                easing: point.easing.clone(),
            });
        }
        let mut easing_update = None;
        response.context_menu(|ui| {
            let edit = ui.button(format!("{} Edit keyframe…", icons::PENCIL_SIMPLE));
            crate::qa::register_component(
                format!(
                    "curve_editor.keyframe_menu.edit:{}:{}",
                    point.id,
                    component_name(curve.component)
                ),
                "curve_editor_keyframe_action",
                edit.rect,
            );
            if edit.clicked() {
                state.curve_editor.keyframe_editor = Some(CurveKeyframeEditor {
                    lane: curve.id.clone(),
                    component: curve.component,
                    keyframe_id: point.id,
                    time: point.time,
                    value: point.full_value.clone(),
                    easing: point.easing.clone(),
                });
                ui.close();
            }
            ui.separator();
            ui.strong("Interpolation after key");
            ui.weak(easing_summary(&point.easing));
            ui.separator();
            let keyframe_id = point.id.to_string();
            show_easing_menu(
                ui,
                Some(&point.easing),
                Some(EasingMenuQaScope::new(
                    "curve_editor.keyframe_menu.easing",
                    &keyframe_id,
                )),
                |easing| easing_update = Some(easing),
            );
            if easing_update.is_some() {
                ui.close();
            }
        });
        if let Some(easing) = easing_update {
            update_curve_easing(state, service, curve.id.clone(), point.id, easing);
        }
        let fill = if response.hovered() || response.dragged() {
            Color32::WHITE
        } else {
            color
        };
        painter.rect_filled(Rect::from_center_size(center, Vec2::splat(8.0)), 1.0, fill);
        painter.rect_stroke(
            Rect::from_center_size(center, Vec2::splat(8.0)),
            1.0,
            Stroke::new(1.0, Color32::BLACK),
            StrokeKind::Inside,
        );
    }
    painter.set(
        line_shape,
        egui::Shape::line(
            curve_samples(state, curve, transform),
            Stroke::new(2.0, color),
        ),
    );
}

fn projected_point(
    state: &AuthoringUiState,
    curve: &CurveSeries,
    point: &automation_lanes::AutomationChannelPoint,
) -> (MediaTime, f64) {
    let projected = state
        .curve_editor
        .drag
        .as_ref()
        .filter(|drag| drag.lane == curve.id && drag.keyframe_id == point.id);
    (
        projected.map_or(point.time, |drag| drag.projected_time),
        projected
            .and_then(|drag| component_value(&drag.projected_value, curve.component))
            .unwrap_or(point.value),
    )
}

fn curve_samples(
    state: &AuthoringUiState,
    curve: &CurveSeries,
    transform: CurveTransform,
) -> Vec<Pos2> {
    let mut points = curve
        .points
        .iter()
        .map(|point| {
            let (time, value) = projected_point(state, curve, point);
            (time, value, &point.easing)
        })
        .collect::<Vec<_>>();
    // A key may cross its neighbour during a drag. Display the same ordering
    // that the authoring service will commit, without mutating authored keys.
    points.sort_by_key(|point| point.0);
    let mut samples = Vec::new();
    if points.len() == 1 {
        samples.push(transform.point(0.0, points[0].1));
        samples.push(transform.point(transform.duration, points[0].1));
    } else {
        for pair in points.windows(2) {
            let (start_time, start_value, easing) = pair[0];
            let (end_time, end_value, _) = pair[1];
            for step in 0..=24 {
                let ratio = f64::from(step) / 24.0;
                let time = start_time.to_seconds_f64()
                    + (end_time.to_seconds_f64() - start_time.to_seconds_f64()) * ratio;
                let value = start_value + (end_value - start_value) * easing.apply(ratio);
                samples.push(transform.point(time, value));
            }
        }
    }
    samples
}

pub(super) fn update_key_drag(
    ui: &egui::Ui,
    state: &mut AuthoringUiState,
    transform: CurveTransform,
) {
    let Some(drag) = state.curve_editor.drag.as_mut() else {
        return;
    };
    let Some(pointer) = ui.input(|input| {
        (input.pointer.primary_down() || input.pointer.primary_released())
            .then(|| input.pointer.interact_pos())
            .flatten()
    }) else {
        return;
    };
    let mut delta = pointer - drag.pointer_origin;
    if let Some(from_global) = ui.ctx().layer_transform_from_global(ui.layer_id()) {
        delta *= from_global.scaling;
    }
    let seconds = (drag.original_time.to_seconds_f64() + transform.delta_time(delta.x)).max(0.0);
    if let Ok(time) = MediaTime::from_seconds_f64(seconds, 1_000_000) {
        drag.projected_time = time;
    }
    let original = component_value(&drag.original_value, drag.component).unwrap_or(0.0);
    drag.projected_value = with_component(
        drag.original_value.clone(),
        drag.component,
        original + transform.delta_value(delta.y),
    );
}
