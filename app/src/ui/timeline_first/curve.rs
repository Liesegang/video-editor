use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use egui_phosphor::regular as icons;
use library::editor::{AuthoringKeyframeUpdate, AuthoringPropertyOwner, TimelineEditorService};
use library::model::authoring::{AuthoringProject, MediaTime, SourceRef, TimelineItemId};
use library::model::property::{KeyframeId, PropertyValue};

use crate::state::authoring::{
    AuthoringSelection, AuthoringUiState, CurveKeyDrag, CurveTarget, CurveValueComponent,
};

const CHANNEL_WIDTH: f32 = 156.0;
const TOOLBAR_HEIGHT: f32 = 30.0;
const POINT_RADIUS: f32 = 5.0;

#[derive(Clone)]
struct CurvePoint {
    id: KeyframeId,
    time: MediaTime,
    value: f64,
    full_value: PropertyValue,
    easing: library::animation::EasingFunction,
}

#[derive(Clone)]
struct CurveSeries {
    target: CurveTarget,
    component: CurveValueComponent,
    label: String,
    color: Color32,
    points: Vec<CurvePoint>,
}

pub fn curve_panel(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
) {
    let Some(AuthoringSelection::Item(item_id)) = state.selection.primary() else {
        ui.centered_and_justified(|ui| ui.label("Select a clip with keyframes"));
        return;
    };
    let mut series = collect_series(project, item_id);
    sync_visibility(state, item_id, &series);

    curve_toolbar(ui, state, !series.is_empty());
    let available = ui.available_rect_before_wrap();
    let channel_rect = Rect::from_min_max(
        available.min,
        Pos2::new(
            (available.min.x + CHANNEL_WIDTH).min(available.max.x),
            available.max.y,
        ),
    );
    let graph_rect = Rect::from_min_max(
        Pos2::new(channel_rect.max.x, available.min.y),
        available.max,
    );
    ui.allocate_rect(available, Sense::hover());

    channel_list(ui, state, &series, channel_rect);
    series.retain(|candidate| series_visible(state, candidate));
    graph_canvas(ui, project, state, service, item_id, &series, graph_rect);
}

fn curve_toolbar(ui: &mut egui::Ui, state: &mut AuthoringUiState, has_curves: bool) {
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), TOOLBAR_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(egui::RichText::new(format!("{} Curve Editor", icons::CHART_LINE)).strong());
            ui.separator();
            let fit = ui
                .add_enabled(has_curves, egui::Button::new(icons::FRAME_CORNERS))
                .on_hover_text("Frame all curves");
            if fit.clicked() {
                state.curve.pan = Vec2::ZERO;
                state.curve.zoom_x = 1.0;
                state.curve.zoom_y = 1.0;
            }
            ui.label(egui::RichText::new("Drag points to edit time and value").weak());
        },
    );
}

fn channel_list(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    series: &[CurveSeries],
    rect: Rect,
) {
    ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink(6.0)), |ui| {
        ui.weak("Channels");
        if series.is_empty() {
            ui.add_space(8.0);
            ui.weak("Use ◇ beside an Inspector value to create its first keyframe.");
            return;
        }
        let mut handled = std::collections::HashSet::new();
        for candidate in series {
            if !handled.insert(candidate.target.clone()) {
                continue;
            }
            let mut visible = match &candidate.target {
                CurveTarget::AuthoredProperty(key) => {
                    state.curve.visible_authored_properties.contains(key)
                }
                CurveTarget::ModuleParameter(id) => state.curve.visible_parameters.contains(id),
            };
            if ui.checkbox(&mut visible, target_label(candidate)).changed() {
                match &candidate.target {
                    CurveTarget::AuthoredProperty(key) => {
                        if visible {
                            state.curve.visible_authored_properties.insert(key.clone());
                        } else {
                            state.curve.visible_authored_properties.remove(key);
                        }
                    }
                    CurveTarget::ModuleParameter(id) => {
                        if visible {
                            state.curve.visible_parameters.insert(*id);
                        } else {
                            state.curve.visible_parameters.remove(id);
                        }
                    }
                }
            }
        }
    });
    ui.painter().line_segment(
        [rect.right_top(), rect.right_bottom()],
        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
    );
}

fn graph_canvas(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item_id: TimelineItemId,
    series: &[CurveSeries],
    rect: Rect,
) {
    crate::qa::register_component_with_metadata(
        "curve_editor.canvas",
        "curve_editor_canvas",
        rect,
        true,
        Some(serde_json::json!({"item_id": item_id, "curve_count": series.len()})),
    );
    ui.painter().rect_filled(rect, 0.0, Color32::from_gray(20));
    let Some(item) = project.items.get(&item_id) else {
        return;
    };
    let duration = item.interval.duration.to_seconds_f64().max(1.0 / 30.0);
    let (value_min, value_max) = value_extent(series);
    let transform = CurveTransform::new(rect, duration, value_min, value_max, state);
    paint_grid(ui, transform);
    handle_navigation(ui, state, rect);

    if series.is_empty() {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "No visible keyframed channels",
            egui::FontId::proportional(13.0),
            ui.visuals().weak_text_color(),
        );
    }

    for curve in series {
        paint_curve(ui, state, service, item_id, curve, transform);
    }
    paint_playhead(ui, project, state, item, transform);
    finish_key_drag(ui, state, service);
}

#[derive(Clone, Copy)]
struct CurveTransform {
    rect: Rect,
    duration: f64,
    min: f64,
    max: f64,
    pan: Vec2,
    zoom_x: f32,
    zoom_y: f32,
}

impl CurveTransform {
    fn new(rect: Rect, duration: f64, min: f64, max: f64, state: &AuthoringUiState) -> Self {
        Self {
            rect: rect.shrink2(Vec2::new(20.0, 16.0)),
            duration,
            min,
            max,
            pan: state.curve.pan,
            zoom_x: state.curve.zoom_x.max(0.1),
            zoom_y: state.curve.zoom_y.max(0.1),
        }
    }

    fn point(self, time: f64, value: f64) -> Pos2 {
        let time_ratio = (time / self.duration) as f32;
        let value_ratio = ((value - self.min) / (self.max - self.min)) as f32;
        Pos2::new(
            self.rect.left() + time_ratio * self.rect.width() * self.zoom_x + self.pan.x,
            self.rect.bottom() - value_ratio * self.rect.height() * self.zoom_y + self.pan.y,
        )
    }

    fn delta_time(self, delta_x: f32) -> f64 {
        f64::from(delta_x) * self.duration / f64::from(self.rect.width() * self.zoom_x)
    }

    fn delta_value(self, delta_y: f32) -> f64 {
        -f64::from(delta_y) * (self.max - self.min) / f64::from(self.rect.height() * self.zoom_y)
    }
}

fn paint_grid(ui: &egui::Ui, transform: CurveTransform) {
    let painter = ui.painter().with_clip_rect(transform.rect);
    for index in 0..=10 {
        let ratio = index as f32 / 10.0;
        let x = transform.rect.left() + ratio * transform.rect.width();
        painter.line_segment(
            [
                Pos2::new(x, transform.rect.top()),
                Pos2::new(x, transform.rect.bottom()),
            ],
            Stroke::new(
                1.0,
                Color32::from_gray(if index % 5 == 0 { 52 } else { 34 }),
            ),
        );
        if index % 2 == 0 {
            painter.text(
                Pos2::new(x + 2.0, transform.rect.bottom() - 2.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{:.2}s", transform.duration * f64::from(ratio)),
                egui::FontId::monospace(9.0),
                Color32::from_gray(115),
            );
        }
    }
    for index in 0..=8 {
        let ratio = index as f32 / 8.0;
        let y = transform.rect.bottom() - ratio * transform.rect.height();
        painter.line_segment(
            [
                Pos2::new(transform.rect.left(), y),
                Pos2::new(transform.rect.right(), y),
            ],
            Stroke::new(
                1.0,
                Color32::from_gray(if index % 4 == 0 { 52 } else { 34 }),
            ),
        );
        if index % 2 == 0 {
            painter.text(
                Pos2::new(transform.rect.left() + 3.0, y - 2.0),
                egui::Align2::LEFT_BOTTOM,
                format!(
                    "{:.2}",
                    transform.min + (transform.max - transform.min) * f64::from(ratio)
                ),
                egui::FontId::monospace(9.0),
                Color32::from_gray(115),
            );
        }
    }
}

fn paint_curve(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item_id: TimelineItemId,
    curve: &CurveSeries,
    transform: CurveTransform,
) {
    if curve.points.is_empty() {
        return;
    }
    let painter = ui.painter().with_clip_rect(transform.rect);
    let mut samples = Vec::new();
    if curve.points.len() == 1 {
        let point = &curve.points[0];
        samples.push(transform.point(0.0, point.value));
        samples.push(transform.point(transform.duration, point.value));
    } else {
        for pair in curve.points.windows(2) {
            let start = &pair[0];
            let end = &pair[1];
            for step in 0..=24 {
                let ratio = f64::from(step) / 24.0;
                let eased = start.easing.apply(ratio);
                let time = start.time.to_seconds_f64()
                    + (end.time.to_seconds_f64() - start.time.to_seconds_f64()) * ratio;
                let value = start.value + (end.value - start.value) * eased;
                samples.push(transform.point(time, value));
            }
        }
    }
    painter.add(egui::Shape::line(samples, Stroke::new(2.0, curve.color)));

    for point in &curve.points {
        let projected = state.curve.drag.as_ref().filter(|drag| {
            drag.item_id == item_id
                && drag.target == curve.target
                && drag.component == curve.component
                && drag.keyframe_id == point.id
        });
        let time = projected.map_or(point.time, |drag| drag.projected_time);
        let value = projected
            .and_then(|drag| component_value(&drag.projected_value, curve.component))
            .unwrap_or(point.value);
        let center = transform.point(time.to_seconds_f64(), value);
        if !transform.rect.contains(center) {
            continue;
        }
        let hit = Rect::from_center_size(center, Vec2::splat(POINT_RADIUS * 3.0));
        let response = ui.interact(
            hit,
            ui.id()
                .with(("curve-key", item_id, point.id, curve.component)),
            Sense::click_and_drag(),
        );
        crate::qa::register_component_with_metadata(
            format!(
                "curve_editor.key:{}:{}",
                point.id,
                component_name(curve.component)
            ),
            "curve_keyframe",
            hit,
            true,
            Some(serde_json::json!({
                "item_id": item_id,
                "keyframe_id": point.id,
                "time": time.to_seconds_f64(),
                "value": value,
                "component": component_name(curve.component),
            })),
        );
        if response.drag_started() {
            state.curve.drag = Some(CurveKeyDrag {
                item_id,
                target: curve.target.clone(),
                component: curve.component,
                keyframe_id: point.id,
                original_time: point.time,
                original_value: point.full_value.clone(),
                projected_time: point.time,
                projected_value: point.full_value.clone(),
            });
        }
        let mut easing_update = None;
        response.context_menu(|ui| {
            ui.strong("Interpolation after key");
            ui.weak(easing_name(&point.easing));
            ui.separator();
            for (label, easing) in easing_presets() {
                let selected = easing_name(&point.easing) == label;
                if ui.selectable_label(selected, label).clicked() {
                    easing_update = Some(easing);
                    ui.close();
                }
            }
        });
        if let Some(easing) = easing_update {
            update_curve_easing(
                state,
                service,
                item_id,
                curve.target.clone(),
                point.id,
                easing,
            );
        }
        if response.dragged() {
            if let Some(drag) = state.curve.drag.as_mut() {
                if drag.keyframe_id == point.id {
                    let delta = response.drag_delta();
                    let time = (drag.original_time.to_seconds_f64()
                        + transform.delta_time(delta.x))
                    .max(0.0);
                    if let Ok(time) = MediaTime::from_seconds_f64(time, 1_000_000) {
                        drag.projected_time = time;
                    }
                    let original =
                        component_value(&drag.original_value, drag.component).unwrap_or(0.0);
                    drag.projected_value = with_component(
                        drag.original_value.clone(),
                        drag.component,
                        original + transform.delta_value(delta.y),
                    );
                }
            }
        }
        let fill = if response.hovered() || response.dragged() {
            Color32::WHITE
        } else {
            curve.color
        };
        painter.rect_filled(Rect::from_center_size(center, Vec2::splat(8.0)), 1.0, fill);
        painter.rect_stroke(
            Rect::from_center_size(center, Vec2::splat(8.0)),
            1.0,
            Stroke::new(1.0, Color32::BLACK),
            StrokeKind::Inside,
        );
    }
}

fn easing_presets() -> [(&'static str, library::animation::EasingFunction); 6] {
    use library::animation::EasingFunction;
    [
        ("Linear", EasingFunction::Linear),
        ("Hold", EasingFunction::Constant),
        ("Ease In", EasingFunction::EaseInCubic),
        ("Ease Out", EasingFunction::EaseOutCubic),
        ("Ease In / Out", EasingFunction::EaseInOutCubic),
        (
            "Custom Bezier",
            EasingFunction::SimpleBezier {
                start: (0.33, 0.0),
                end: (0.67, 1.0),
            },
        ),
    ]
}

fn easing_name(easing: &library::animation::EasingFunction) -> &'static str {
    use library::animation::EasingFunction;
    match easing {
        EasingFunction::Linear => "Linear",
        EasingFunction::Constant => "Hold",
        EasingFunction::EaseInSine
        | EasingFunction::EaseInQuad
        | EasingFunction::EaseInCubic
        | EasingFunction::EaseInQuart
        | EasingFunction::EaseInQuint
        | EasingFunction::EaseInExpo
        | EasingFunction::EaseInCirc
        | EasingFunction::EaseInBack { .. }
        | EasingFunction::EaseInElastic { .. }
        | EasingFunction::EaseInBounce { .. } => "Ease In",
        EasingFunction::EaseOutSine
        | EasingFunction::EaseOutQuad
        | EasingFunction::EaseOutCubic
        | EasingFunction::EaseOutQuart
        | EasingFunction::EaseOutQuint
        | EasingFunction::EaseOutExpo
        | EasingFunction::EaseOutCirc
        | EasingFunction::EaseOutBack { .. }
        | EasingFunction::EaseOutElastic { .. }
        | EasingFunction::EaseOutBounce { .. } => "Ease Out",
        EasingFunction::EaseInOutSine
        | EasingFunction::EaseInOutQuad
        | EasingFunction::EaseInOutCubic
        | EasingFunction::EaseInOutQuart
        | EasingFunction::EaseInOutQuint
        | EasingFunction::EaseInOutExpo
        | EasingFunction::EaseInOutCirc
        | EasingFunction::EaseInOutBack { .. }
        | EasingFunction::EaseInOutElastic { .. }
        | EasingFunction::EaseInOutBounce { .. } => "Ease In / Out",
        EasingFunction::SimpleBezier { .. } | EasingFunction::Bezier { .. } => "Custom Bezier",
        EasingFunction::Expression { .. } => "Expression",
    }
}

fn update_curve_easing(
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item_id: TimelineItemId,
    target: CurveTarget,
    keyframe_id: KeyframeId,
    easing: library::animation::EasingFunction,
) {
    let update = AuthoringKeyframeUpdate {
        time: None,
        value: None,
        easing: Some(easing),
    };
    let result = match target {
        CurveTarget::AuthoredProperty(key) => service.update_authored_property_keyframe(
            AuthoringPropertyOwner::Item(item_id),
            &key,
            keyframe_id,
            update,
        ),
        CurveTarget::ModuleParameter(parameter_id) => {
            service.update_module_parameter_keyframe(item_id, parameter_id, keyframe_id, update)
        }
    };
    match result {
        Ok(_) => state.status = "Updated keyframe interpolation".to_string(),
        Err(error) => state.error = Some(error.to_string()),
    }
}

fn finish_key_drag(ui: &egui::Ui, state: &mut AuthoringUiState, service: &TimelineEditorService) {
    let (released, down, escape) = ui.input(|input| {
        (
            input.pointer.primary_released(),
            input.pointer.primary_down(),
            input.key_pressed(egui::Key::Escape),
        )
    });
    if escape || (state.curve.drag.is_some() && !down && !released) {
        state.curve.drag = None;
        return;
    }
    if !released {
        return;
    }
    let Some(drag) = state.curve.drag.take() else {
        return;
    };
    if drag.projected_time == drag.original_time && drag.projected_value == drag.original_value {
        return;
    }
    let update = AuthoringKeyframeUpdate {
        time: Some(drag.projected_time),
        value: Some(drag.projected_value),
        easing: None,
    };
    let result = match drag.target {
        CurveTarget::AuthoredProperty(key) => service.update_authored_property_keyframe(
            AuthoringPropertyOwner::Item(drag.item_id),
            &key,
            drag.keyframe_id,
            update,
        ),
        CurveTarget::ModuleParameter(parameter_id) => service.update_module_parameter_keyframe(
            drag.item_id,
            parameter_id,
            drag.keyframe_id,
            update,
        ),
    };
    if let Err(error) = result {
        state.error = Some(error.to_string());
    }
}

fn paint_playhead(
    ui: &egui::Ui,
    project: &AuthoringProject,
    state: &AuthoringUiState,
    item: &library::model::authoring::TimelineItem,
    transform: CurveTransform,
) {
    let Some(timeline) = project.timelines.get(&state.active_timeline_id) else {
        return;
    };
    let timeline_time = MediaTime::from_frame_index(state.timeline.current_frame, timeline.fps);
    let local = timeline_time.and_then(|time| item.time_map.local_time(item.interval, time));
    let Ok(local) = local else {
        return;
    };
    let x = transform.point(local.to_seconds_f64(), transform.min).x;
    if x < transform.rect.left() || x > transform.rect.right() {
        return;
    }
    ui.painter().with_clip_rect(transform.rect).line_segment(
        [
            Pos2::new(x, transform.rect.top()),
            Pos2::new(x, transform.rect.bottom()),
        ],
        Stroke::new(1.0, Color32::from_rgb(255, 94, 94)),
    );
}

fn handle_navigation(ui: &egui::Ui, state: &mut AuthoringUiState, rect: Rect) {
    if !ui
        .ctx()
        .pointer_hover_pos()
        .is_some_and(|position| rect.contains(position))
    {
        return;
    }
    let (scroll, modifiers, middle_down, pointer_delta) = ui.input(|input| {
        (
            input.smooth_scroll_delta,
            input.modifiers,
            input.pointer.button_down(egui::PointerButton::Middle),
            input.pointer.delta(),
        )
    });
    if modifiers.ctrl {
        state.curve.zoom_y = (state.curve.zoom_y * (scroll.y * 0.003).exp()).clamp(0.1, 20.0);
    } else if scroll.y != 0.0 {
        state.curve.zoom_x = (state.curve.zoom_x * (scroll.y * 0.003).exp()).clamp(0.1, 20.0);
    }
    if middle_down {
        state.curve.pan += pointer_delta;
    }
}

fn collect_series(project: &AuthoringProject, item_id: TimelineItemId) -> Vec<CurveSeries> {
    let Some(item) = project.items.get(&item_id) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    let mut color_index = 0_usize;
    let mut properties = item.authored_properties.iter().collect::<Vec<_>>();
    properties.sort_by(|left, right| left.0.cmp(right.0));
    for (key, property) in properties {
        let keyframes = property.keyframes();
        append_property_series(
            &mut result,
            CurveTarget::AuthoredProperty(key.clone()),
            key,
            keyframes.into_iter().filter_map(|keyframe| {
                MediaTime::from_seconds_f64(keyframe.time.into_inner(), 1_000_000)
                    .ok()
                    .map(|time| (keyframe.id, time, keyframe.value, keyframe.easing))
            }),
            &mut color_index,
        );
    }
    if let SourceRef::Module(invocation) = &item.source {
        let names = project
            .module_instances
            .get(&invocation.instance_id)
            .and_then(|instance| project.module_definitions.get(&instance.definition_id))
            .map(|definition| {
                definition
                    .interface
                    .parameters
                    .iter()
                    .map(|parameter| (parameter.id, parameter.name.clone()))
                    .collect::<std::collections::HashMap<_, _>>()
            })
            .unwrap_or_default();
        let mut tracks = invocation.automation_tracks.iter().collect::<Vec<_>>();
        tracks.sort_by_key(|(id, _)| **id);
        for (parameter_id, track) in tracks {
            let label = names
                .get(parameter_id)
                .cloned()
                .unwrap_or_else(|| "Published parameter".to_string());
            append_property_series(
                &mut result,
                CurveTarget::ModuleParameter(*parameter_id),
                &label,
                track
                    .keyframes
                    .iter()
                    .cloned()
                    .map(|keyframe| (keyframe.id, keyframe.time, keyframe.value, keyframe.easing)),
                &mut color_index,
            );
        }
    }
    result
}

fn append_property_series(
    output: &mut Vec<CurveSeries>,
    target: CurveTarget,
    label: &str,
    points: impl IntoIterator<
        Item = (
            KeyframeId,
            MediaTime,
            PropertyValue,
            library::animation::EasingFunction,
        ),
    >,
    color_index: &mut usize,
) {
    let points = points.into_iter().collect::<Vec<_>>();
    for component in components_for(points.first().map(|(_, _, value, _)| value)) {
        let curve_points = points
            .iter()
            .filter_map(|(id, time, full_value, easing)| {
                component_value(full_value, *component).map(|value| CurvePoint {
                    id: *id,
                    time: *time,
                    value,
                    full_value: full_value.clone(),
                    easing: easing.clone(),
                })
            })
            .collect::<Vec<_>>();
        if curve_points.is_empty() {
            continue;
        }
        output.push(CurveSeries {
            target: target.clone(),
            component: *component,
            label: if *component == CurveValueComponent::Scalar {
                label.to_string()
            } else {
                format!("{label}.{}", component_name(*component))
            },
            color: curve_color(*color_index),
            points: curve_points,
        });
        *color_index += 1;
    }
}

fn sync_visibility(state: &mut AuthoringUiState, item_id: TimelineItemId, series: &[CurveSeries]) {
    let authored = series
        .iter()
        .filter_map(|curve| match &curve.target {
            CurveTarget::AuthoredProperty(key) => Some(key.clone()),
            CurveTarget::ModuleParameter(_) => None,
        })
        .collect::<std::collections::HashSet<_>>();
    let module = series
        .iter()
        .filter_map(|curve| match &curve.target {
            CurveTarget::ModuleParameter(id) => Some(*id),
            CurveTarget::AuthoredProperty(_) => None,
        })
        .collect::<std::collections::HashSet<_>>();
    if state.curve.target_item != Some(item_id) {
        state.curve.target_item = Some(item_id);
        state.curve.pan = Vec2::ZERO;
        state.curve.zoom_x = 1.0;
        state.curve.zoom_y = 1.0;
        state.curve.visible_authored_properties = authored;
        state.curve.visible_parameters = module;
    } else {
        state
            .curve
            .visible_authored_properties
            .retain(|key| authored.contains(key));
        state
            .curve
            .visible_parameters
            .retain(|id| module.contains(id));
    }
}

fn series_visible(state: &AuthoringUiState, series: &CurveSeries) -> bool {
    match &series.target {
        CurveTarget::AuthoredProperty(key) => state.curve.visible_authored_properties.contains(key),
        CurveTarget::ModuleParameter(id) => state.curve.visible_parameters.contains(id),
    }
}

fn value_extent(series: &[CurveSeries]) -> (f64, f64) {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for value in series
        .iter()
        .flat_map(|curve| curve.points.iter().map(|point| point.value))
    {
        min = min.min(value);
        max = max.max(value);
    }
    if !min.is_finite() || !max.is_finite() {
        return (-1.0, 1.0);
    }
    if (max - min).abs() < f64::EPSILON {
        let padding = min.abs().max(1.0) * 0.25;
        return (min - padding, max + padding);
    }
    let padding = (max - min) * 0.12;
    (min - padding, max + padding)
}

fn components_for(value: Option<&PropertyValue>) -> &'static [CurveValueComponent] {
    match value {
        Some(PropertyValue::Number(_) | PropertyValue::Integer(_)) => {
            &[CurveValueComponent::Scalar]
        }
        Some(PropertyValue::Vec2(_)) => &[CurveValueComponent::X, CurveValueComponent::Y],
        Some(PropertyValue::Vec3(_)) => &[
            CurveValueComponent::X,
            CurveValueComponent::Y,
            CurveValueComponent::Z,
        ],
        Some(PropertyValue::Vec4(_)) => &[
            CurveValueComponent::X,
            CurveValueComponent::Y,
            CurveValueComponent::Z,
            CurveValueComponent::W,
        ],
        _ => &[],
    }
}

fn component_value(value: &PropertyValue, component: CurveValueComponent) -> Option<f64> {
    match (value, component) {
        (PropertyValue::Number(value), CurveValueComponent::Scalar) => Some(value.into_inner()),
        (PropertyValue::Integer(value), CurveValueComponent::Scalar) => Some(*value as f64),
        (PropertyValue::Vec2(value), CurveValueComponent::X) => Some(value.x.into_inner()),
        (PropertyValue::Vec2(value), CurveValueComponent::Y) => Some(value.y.into_inner()),
        (PropertyValue::Vec3(value), CurveValueComponent::X) => Some(value.x.into_inner()),
        (PropertyValue::Vec3(value), CurveValueComponent::Y) => Some(value.y.into_inner()),
        (PropertyValue::Vec3(value), CurveValueComponent::Z) => Some(value.z.into_inner()),
        (PropertyValue::Vec4(value), CurveValueComponent::X) => Some(value.x.into_inner()),
        (PropertyValue::Vec4(value), CurveValueComponent::Y) => Some(value.y.into_inner()),
        (PropertyValue::Vec4(value), CurveValueComponent::Z) => Some(value.z.into_inner()),
        (PropertyValue::Vec4(value), CurveValueComponent::W) => Some(value.w.into_inner()),
        _ => None,
    }
}

fn with_component(
    mut value: PropertyValue,
    component: CurveValueComponent,
    replacement: f64,
) -> PropertyValue {
    match (&mut value, component) {
        (PropertyValue::Number(number), CurveValueComponent::Scalar) => {
            *number = ordered_float::OrderedFloat(replacement);
        }
        (PropertyValue::Integer(integer), CurveValueComponent::Scalar) => {
            *integer = replacement.round() as i64;
        }
        (PropertyValue::Vec2(vector), CurveValueComponent::X) => {
            vector.x = ordered_float::OrderedFloat(replacement);
        }
        (PropertyValue::Vec2(vector), CurveValueComponent::Y) => {
            vector.y = ordered_float::OrderedFloat(replacement);
        }
        (PropertyValue::Vec3(vector), CurveValueComponent::X) => {
            vector.x = ordered_float::OrderedFloat(replacement);
        }
        (PropertyValue::Vec3(vector), CurveValueComponent::Y) => {
            vector.y = ordered_float::OrderedFloat(replacement);
        }
        (PropertyValue::Vec3(vector), CurveValueComponent::Z) => {
            vector.z = ordered_float::OrderedFloat(replacement);
        }
        (PropertyValue::Vec4(vector), CurveValueComponent::X) => {
            vector.x = ordered_float::OrderedFloat(replacement);
        }
        (PropertyValue::Vec4(vector), CurveValueComponent::Y) => {
            vector.y = ordered_float::OrderedFloat(replacement);
        }
        (PropertyValue::Vec4(vector), CurveValueComponent::Z) => {
            vector.z = ordered_float::OrderedFloat(replacement);
        }
        (PropertyValue::Vec4(vector), CurveValueComponent::W) => {
            vector.w = ordered_float::OrderedFloat(replacement);
        }
        _ => {}
    }
    value
}

fn target_label(series: &CurveSeries) -> &str {
    series
        .label
        .split_once('.')
        .map_or(series.label.as_str(), |(label, _)| label)
}

fn component_name(component: CurveValueComponent) -> &'static str {
    match component {
        CurveValueComponent::Scalar => "value",
        CurveValueComponent::X => "x",
        CurveValueComponent::Y => "y",
        CurveValueComponent::Z => "z",
        CurveValueComponent::W => "w",
    }
}

fn curve_color(index: usize) -> Color32 {
    [
        Color32::from_rgb(238, 102, 102),
        Color32::from_rgb(104, 189, 255),
        Color32::from_rgb(117, 214, 135),
        Color32::from_rgb(244, 190, 86),
        Color32::from_rgb(194, 129, 245),
        Color32::from_rgb(80, 213, 200),
    ][index % 6]
}
