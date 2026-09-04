use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};
use egui_phosphor::regular as icons;
use library::editor::{AuthoringKeyframeUpdate, TimelineEditorService};
use library::model::authoring::{AuthoringProject, MediaTime, TimelineItem, TimelineItemId};

use crate::state::authoring::{AuthoringSelection, AuthoringUiState, TimelineKeyframeGesture};
use crate::ui::automation_lanes::{
    self, target_metadata, timeline_time_for_local, AutomationLane, AutomationPoint,
};

use super::geometry::frame_for_seconds;
use super::viewport::{screen_x_to_seconds, seconds_to_screen_x};

const DIAMOND_SIZE: f32 = 8.0;

pub(super) fn paint_clip_keyframe_summary(
    ui: &egui::Ui,
    project: &AuthoringProject,
    item: &TimelineItem,
    interval: library::model::authoring::TimelineInterval,
    clip_rect: Rect,
    viewport_rect: Rect,
) {
    let visible = clip_rect.intersect(viewport_rect);
    if !visible.is_positive() {
        return;
    }
    let lanes = automation_lanes::collect_item_keyframed_lanes(project, item.id);
    let keyframe_count = lanes.iter().map(|lane| lane.points.len()).sum::<usize>();
    let rate = item.time_map.playback_rate.to_f64();
    let duration = interval.duration.to_seconds_f64();
    let inner = clip_rect.shrink2(Vec2::new(6.0, 4.0));
    let painter = ui.painter().with_clip_rect(visible);
    if rate.is_finite() && rate.abs() > f64::EPSILON && duration > 0.0 {
        let lane_count = lanes.len().max(1) as f32;
        for (lane_index, lane) in lanes.iter().enumerate() {
            let y = inner.top() + inner.height() * (lane_index as f32 + 0.5) / lane_count;
            painter.line_segment(
                [Pos2::new(inner.left(), y), Pos2::new(inner.right(), y)],
                Stroke::new(1.0, Color32::from_white_alpha(32)),
            );
            for point in &lane.points {
                let offset = (point.time.to_seconds_f64()
                    - item.time_map.source_start.to_seconds_f64())
                    / rate;
                let ratio = offset / duration;
                if !(0.0..=1.0).contains(&ratio) {
                    continue;
                }
                let center = Pos2::new(clip_rect.left() + clip_rect.width() * ratio as f32, y);
                let size = 5.0;
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        Pos2::new(center.x, center.y - size * 0.5),
                        Pos2::new(center.x + size * 0.5, center.y),
                        Pos2::new(center.x, center.y + size * 0.5),
                        Pos2::new(center.x - size * 0.5, center.y),
                    ],
                    Color32::from_rgb(244, 190, 86),
                    Stroke::new(1.0, Color32::from_black_alpha(150)),
                ));
            }
        }
    }
    crate::qa::register_component_with_metadata(
        format!("timeline.keyframes:{}", item.id),
        "timeline_clip_keyframes",
        visible,
        true,
        Some(serde_json::json!({
            "item_id": item.id,
            "display_mode": "keyframes",
            "lane_count": lanes.len(),
            "keyframe_count": keyframe_count,
            "uses_shared_automation_lanes": true,
        })),
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "one property row shares the Timeline model, state, service, and clipped row geometry"
)]
pub(super) fn draw_property_row(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
    lane: &AutomationLane,
    display_row_index: usize,
    label_rect: Rect,
    row_rect: Rect,
    sidebar_rect: Rect,
    content_rect: Rect,
) {
    draw_property_label(
        ui,
        state,
        item.id,
        lane,
        display_row_index,
        label_rect,
        sidebar_rect,
    );
    draw_property_keys(
        ui,
        project,
        state,
        service,
        item,
        lane,
        display_row_index,
        row_rect,
        content_rect,
    );
}

fn draw_property_label(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    item_id: TimelineItemId,
    lane: &AutomationLane,
    display_row_index: usize,
    rect: Rect,
    sidebar_rect: Rect,
) {
    let visible = rect.intersect(sidebar_rect);
    if !visible.is_positive() {
        return;
    }
    let response = ui
        .interact(
            visible,
            ui.id()
                .with(("timeline-property-label", item_id, &lane.target)),
            Sense::click(),
        )
        .on_hover_text(format!("{} automation", lane.label));
    if response.clicked() {
        state.selection.replace(AuthoringSelection::Item(item_id));
    }
    if response.hovered() || state.selection.contains(AuthoringSelection::Item(item_id)) {
        ui.painter().with_clip_rect(sidebar_rect).rect_filled(
            rect,
            0.0,
            if response.hovered() {
                Color32::from_gray(39)
            } else {
                Color32::from_gray(34)
            },
        );
    }
    let painter = ui.painter().with_clip_rect(sidebar_rect);
    painter.text(
        Pos2::new(rect.left() + 42.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        icons::DIAMOND,
        egui::FontId::proportional(9.0),
        Color32::from_rgb(244, 190, 86),
    );
    painter.text(
        Pos2::new(rect.left() + 57.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &lane.label,
        egui::FontId::proportional(11.5),
        ui.visuals().text_color(),
    );
    crate::qa::register_component_with_metadata(
        property_component_id(item_id, lane),
        "timeline_property_label",
        visible,
        true,
        Some(serde_json::json!({
            "item_id": item_id,
            "target": target_metadata(&lane.target),
            "label": lane.label,
            "keyframe_count": lane.points.len(),
            "keyframe_ids": lane.points.iter().map(|point| point.id.to_string()).collect::<Vec<_>>(),
            "display_row_index": display_row_index,
        })),
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "key hit testing needs canonical Timeline and row geometry"
)]
fn draw_property_keys(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    _service: &TimelineEditorService,
    item: &TimelineItem,
    lane: &AutomationLane,
    display_row_index: usize,
    row_rect: Rect,
    content_rect: Rect,
) {
    let visible = row_rect.intersect(content_rect);
    if !visible.is_positive() {
        return;
    }
    let row_response = ui.interact(
        visible,
        ui.id()
            .with(("timeline-property-row", item.id, &lane.target)),
        Sense::click(),
    );
    if row_response.clicked() {
        if let (Some(pointer), Some(timeline)) = (
            row_response.interact_pointer_pos(),
            project.timelines.get(&state.active_timeline_id),
        ) {
            state.selection.replace(AuthoringSelection::Item(item.id));
            let seconds = screen_x_to_seconds(pointer.x, content_rect, &state.timeline);
            state
                .timeline
                .seek_frame(frame_for_seconds(seconds, timeline.fps));
        }
    }

    let start_x = seconds_to_screen_x(
        item.interval.start.to_seconds_f64() as f32,
        content_rect,
        &state.timeline,
    );
    let end_x = item.interval.end().ok().map_or(start_x, |time| {
        seconds_to_screen_x(time.to_seconds_f64() as f32, content_rect, &state.timeline)
    });
    let painter = ui.painter().with_clip_rect(content_rect);
    painter.line_segment(
        [
            Pos2::new(start_x, row_rect.center().y),
            Pos2::new(end_x, row_rect.center().y),
        ],
        Stroke::new(1.0, Color32::from_gray(67)),
    );
    for point in &lane.points {
        draw_keyframe(
            ui,
            project,
            state,
            item,
            lane,
            point,
            display_row_index,
            row_rect,
            content_rect,
        );
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "one key gesture shares its lane and Timeline row geometry"
)]
fn draw_keyframe(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    item: &TimelineItem,
    lane: &AutomationLane,
    point: &AutomationPoint,
    display_row_index: usize,
    row_rect: Rect,
    content_rect: Rect,
) {
    let projected_time = state
        .timeline
        .keyframe_gesture
        .as_ref()
        .filter(|gesture| {
            gesture.item_id == item.id
                && gesture.target == lane.target
                && gesture.keyframe_id == point.id
        })
        .map(|gesture| gesture.projected_time);
    let dragging = projected_time.is_some();
    let local_time = projected_time.unwrap_or(point.time);
    let Some(timeline_time) = timeline_time_for_local(project, item.id, local_time) else {
        return;
    };
    let center = Pos2::new(
        seconds_to_screen_x(
            timeline_time.to_seconds_f64() as f32,
            content_rect,
            &state.timeline,
        ),
        row_rect.center().y,
    );
    if center.x < content_rect.left() - DIAMOND_SIZE
        || center.x > content_rect.right() + DIAMOND_SIZE
    {
        return;
    }
    let hit =
        Rect::from_center_size(center, Vec2::splat(DIAMOND_SIZE * 2.5)).intersect(content_rect);
    if !hit.is_positive() {
        return;
    }
    let response = ui
        .interact(
            hit,
            ui.id()
                .with(("timeline-property-key", item.id, &lane.target, point.id)),
            Sense::click_and_drag(),
        )
        .on_hover_text(format!(
            "{} at {:.3}s - drag to retime",
            lane.label,
            timeline_time.to_seconds_f64()
        ));
    if response.clicked() || response.drag_started() {
        state.selection.replace(AuthoringSelection::Item(item.id));
        if let Some(timeline) = project.timelines.get(&state.active_timeline_id) {
            state.timeline.seek_frame(
                timeline_time
                    .checked_frame_index(timeline.fps)
                    .unwrap_or_default(),
            );
        }
    }
    if response.drag_started() && state.timeline.keyframe_gesture.is_none() {
        let pointer_origin_x = ui
            .input(|input| input.pointer.press_origin())
            .map_or(center.x, |pointer| pointer.x);
        state.timeline.keyframe_gesture = Some(TimelineKeyframeGesture {
            item_id: item.id,
            target: lane.target.clone(),
            keyframe_id: point.id,
            pointer_origin_x,
            original_time: point.time,
            projected_time: point.time,
        });
    }
    let active = response.hovered() || response.dragged();
    let size = if active { 9.0 } else { DIAMOND_SIZE };
    ui.painter()
        .with_clip_rect(content_rect)
        .add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(center.x, center.y - size * 0.5),
                Pos2::new(center.x + size * 0.5, center.y),
                Pos2::new(center.x, center.y + size * 0.5),
                Pos2::new(center.x - size * 0.5, center.y),
            ],
            if active {
                Color32::WHITE
            } else {
                Color32::from_rgb(244, 190, 86)
            },
            Stroke::new(1.0, Color32::BLACK),
        ));
    crate::qa::register_component_with_metadata(
        format!("timeline.keyframe:{}:{}", item.id, point.id),
        "timeline_property_keyframe",
        hit,
        true,
        Some(serde_json::json!({
            "item_id": item.id,
            "target": target_metadata(&lane.target),
            "keyframe_id": point.id,
            "local_time": local_time.to_seconds_f64(),
            "timeline_time": timeline_time.to_seconds_f64(),
            "display_row_index": display_row_index,
            "dragging": dragging,
        })),
    );
}

pub(super) fn update_key_projection(
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    pointer_x: f32,
) {
    let Some(gesture) = state.timeline.keyframe_gesture.as_ref() else {
        return;
    };
    let Some(original_timeline) =
        timeline_time_for_local(project, gesture.item_id, gesture.original_time)
    else {
        return;
    };
    let Some(timeline) = project.timelines.get(&state.active_timeline_id) else {
        return;
    };
    let delta_x = pointer_x - gesture.pointer_origin_x;
    let seconds = original_timeline.to_seconds_f64()
        + f64::from(delta_x) / f64::from(state.timeline.pixels_per_second);
    let frame = (seconds * timeline.fps.to_f64()).round().max(0.0) as i64;
    let Ok(timeline_time) = MediaTime::from_frame_index(frame, timeline.fps) else {
        return;
    };
    let Some(local_time) =
        automation_lanes::local_time_for_timeline(project, gesture.item_id, timeline_time)
    else {
        return;
    };
    if let Some(gesture) = state.timeline.keyframe_gesture.as_mut() {
        gesture.projected_time = local_time;
    }
}

pub(super) fn finish_keyframe_gesture(
    ui: &egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
) {
    let (released, down, escape) = ui.input(|input| {
        (
            input.pointer.primary_released(),
            input.pointer.primary_down(),
            input.key_pressed(egui::Key::Escape),
        )
    });
    if escape || (state.timeline.keyframe_gesture.is_some() && !down && !released) {
        state.timeline.keyframe_gesture = None;
        ui.ctx().request_repaint();
        return;
    }
    if !released {
        return;
    }
    let Some(gesture) = state.timeline.keyframe_gesture.take() else {
        return;
    };
    ui.ctx().request_repaint();
    if !gesture.changed() {
        return;
    }
    let result = automation_lanes::update_keyframe(
        service,
        gesture.item_id,
        &gesture.target,
        gesture.keyframe_id,
        AuthoringKeyframeUpdate {
            time: Some(gesture.projected_time),
            value: None,
            easing: None,
        },
    );
    match result {
        Ok(()) => state.status = "Retimed keyframe".to_string(),
        Err(error) => state.error = Some(error.to_string()),
    }
}

pub(super) fn property_component_id(item_id: TimelineItemId, lane: &AutomationLane) -> String {
    let suffix = match &lane.target {
        crate::state::authoring::AutomationTarget::AuthoredProperty(key) => {
            format!("property:{key}")
        }
        crate::state::authoring::AutomationTarget::ModuleParameter(id) => {
            format!("parameter:{id}")
        }
        crate::state::authoring::AutomationTarget::AttachmentParameter { attachment_id, key } => {
            format!("attachment:{attachment_id}:{key}")
        }
    };
    format!("timeline.property:{item_id}:{suffix}")
}

#[cfg(test)]
mod tests {
    use library::model::authoring::{RationalRate, TimeMap, TimelineInterval};

    use super::*;

    #[test]
    fn frame_snapping_uses_the_host_timeline_rate() {
        let mut project = AuthoringProject::new(
            "snap",
            640,
            360,
            RationalRate::new(30, 1).unwrap(),
            MediaTime::new(10, 1).unwrap(),
        )
        .unwrap();
        let track_id = project.timelines[&project.root_timeline_id].track_order[0];
        let item_id = TimelineItemId::new();
        project.items.insert(
            item_id,
            TimelineItem {
                id: item_id,
                track_id,
                name: "item".to_string(),
                source: library::model::authoring::SourceRef::Solid {
                    color: library::model::frame::color::Color::black(),
                },
                interval: TimelineInterval::new(
                    MediaTime::new(1, 1).unwrap(),
                    MediaTime::new(5, 1).unwrap(),
                )
                .unwrap(),
                time_map: TimeMap::default(),
                layer: 0,
                parent: None,
                blend_mode: library::model::BlendMode::Normal,
                authored_properties: Default::default(),
            },
        );
        let mut state = AuthoringUiState::new(project.root_timeline_id);
        state.timeline.pixels_per_second = 100.0;
        state.timeline.keyframe_gesture = Some(TimelineKeyframeGesture {
            item_id,
            target: crate::state::authoring::AutomationTarget::AuthoredProperty(
                "position".to_string(),
            ),
            keyframe_id: library::model::property::KeyframeId::new(),
            pointer_origin_x: 20.0,
            original_time: MediaTime::zero(),
            projected_time: MediaTime::zero(),
        });

        update_key_projection(&project, &mut state, 74.0);

        let projected = state.timeline.keyframe_gesture.unwrap().projected_time;
        assert_eq!(projected, MediaTime::new(8, 15).unwrap());
    }
}
