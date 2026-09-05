use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use egui_phosphor::regular as icons;
use library::editor::{AuthoringKeyframeUpdate, TimelineEditorService};
use library::model::authoring::{AuthoringProject, MediaTime};
use library::model::property::KeyframeId;
use pan_zoom_ui::{
    AxisMask, CanvasState, CanvasTheme, CanvasTransform, GridAxis, GridConfig, GridLineKind,
    NavigationConfig,
};

use crate::state::authoring::{
    AuthoringSelection, AuthoringUiState, AutomationLaneId, AutomationOwner, CurveEditorState,
    CurveKeyDrag, CurveKeyframeEditor,
};
use crate::ui::automation_lanes::{
    self, component_name, component_value, lane_metadata, owner_metadata, with_component,
    AutomationChannel as CurveSeries,
};
use crate::ui::easing_menus::{
    easing_name, easing_summary, show_easing_menu, show_easing_parameters, EasingMenuQaScope,
};
use crate::ui::time_ruler::{TimeRuler, TimeRulerTick};
use crate::ui::viewport::{ViewportController, ViewportInputPolicy, ViewportState, ZoomPolicy};

const CHANNEL_WIDTH: f32 = 156.0;
const TOOLBAR_HEIGHT: f32 = 30.0;
const RULER_HEIGHT: f32 = 24.0;
const POINT_RADIUS: f32 = 5.0;
const PLOT_PADDING: Vec2 = Vec2::new(20.0, 16.0);
const MIN_TIME_ZOOM: f32 = 0.01;
const MIN_VALUE_ZOOM: f32 = 1.0e-6;
const MAX_TIME_ZOOM: f32 = 20.0;
const MAX_VALUE_ZOOM: f32 = 100_000.0;

#[cfg(test)]
mod tests;

pub fn curve_editor_panel(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
) {
    let Some(owner) = selected_owner(state) else {
        state.curve_editor.drag = None;
        state.curve_editor.keyframe_editor = None;
        ui.centered_and_justified(|ui| ui.label("Select a clip with keyframes"));
        return;
    };
    let mut series = collect_series(project, &owner);
    sync_visibility(state, &owner, &series);

    curve_toolbar(ui, state, !series.is_empty());
    let available = ui.available_rect_before_wrap();
    let channel_rect = Rect::from_min_max(
        available.min,
        Pos2::new(
            (available.min.x + CHANNEL_WIDTH).min(available.max.x),
            available.max.y,
        ),
    );
    let curve_rect = Rect::from_min_max(
        Pos2::new(channel_rect.max.x, available.min.y),
        available.max,
    );
    ui.allocate_rect(available, Sense::hover());

    channel_list(ui, state, &series, channel_rect);
    series.retain(|candidate| series_visible(state, candidate));
    curve_canvas(ui, project, state, service, &owner, &series, curve_rect);
    show_keyframe_editor(ui.ctx(), state, service);
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
                .on_hover_text("Frame visible channels and value range");
            crate::qa::register_component_with_metadata(
                "curve_editor.fit",
                "curve_editor_fit",
                fit.rect,
                has_curves,
                Some(serde_json::json!({"scope": "visible_channels"})),
            );
            if fit.clicked() {
                state.curve_editor.canvas = CanvasState::uniform(Vec2::ZERO, 1.0);
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
            if !handled.insert(candidate.id.clone()) {
                continue;
            }
            let label = target_label(candidate);
            let mut visible = !state.curve_editor.hidden_lanes.contains(&candidate.id);
            let response = ui.checkbox(&mut visible, label);
            if response.changed() {
                if visible {
                    state.curve_editor.hidden_lanes.remove(&candidate.id);
                } else {
                    state.curve_editor.hidden_lanes.insert(candidate.id.clone());
                }
            }
            crate::qa::register_component_with_metadata(
                format!("curve_editor.channel:{:?}", candidate.id),
                "curve_editor_channel",
                response.rect,
                response.enabled(),
                Some(serde_json::json!({
                    "lane": lane_metadata(&candidate.id),
                    "target": automation_lanes::target_metadata(&candidate.id.target),
                    "label": label,
                    "visible": visible,
                    "components": series
                        .iter()
                        .filter(|series| series.id == candidate.id)
                        .map(|series| component_name(series.component))
                        .collect::<Vec<_>>(),
                })),
            );
        }
    });
    ui.painter().line_segment(
        [rect.right_top(), rect.right_bottom()],
        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
    );
}

fn curve_canvas(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    owner: &AutomationOwner,
    series: &[CurveSeries],
    rect: Rect,
) {
    ui.painter()
        .rect_filled(rect, 0.0, CanvasTheme::default().background);
    let Some(interval) = automation_lanes::owner_interval(project, owner) else {
        return;
    };
    let duration = interval.duration.to_seconds_f64().max(1.0 / 30.0);
    let (value_min, value_max) = value_extent(series);
    let canvas_rect = Rect::from_min_max(
        Pos2::new(rect.left(), (rect.top() + RULER_HEIGHT).min(rect.bottom())),
        rect.right_bottom(),
    );
    let plot_rect = curve_plot_rect(canvas_rect);
    if !plot_rect.is_positive() {
        finish_key_drag(ui, state, service);
        return;
    }

    let mut handled_hand_tool_drag = false;
    ViewportController::new(
        ui,
        ui.make_persistent_id("curve_editor_viewport"),
        Some(egui::Key::Space),
    )
    .with_config(curve_navigation_config())
    .with_screen_origin(plot_rect.min)
    .interact_with_rect(
        plot_rect,
        &mut state.curve_editor,
        &mut handled_hand_tool_drag,
    );

    let Some(transform) = CurveTransform::new(
        plot_rect,
        duration,
        value_min,
        value_max,
        state.curve_editor.canvas,
    ) else {
        finish_key_drag(ui, state, service);
        return;
    };
    let visible_value_range = transform.visible_value_range();
    let transform_finite = transform.canvas.state.pan.is_finite()
        && transform.canvas.state.zoom.is_finite()
        && visible_value_range.is_some_and(|(min, max)| min.is_finite() && max.is_finite());
    crate::qa::register_component_with_metadata(
        "curve_editor.canvas",
        "curve_editor_canvas",
        transform.rect,
        true,
        Some(serde_json::json!({
            "owner": owner_metadata(owner),
            "curve_count": series.len(),
            "pan": {"x": transform.canvas.state.pan.x, "y": transform.canvas.state.pan.y},
            "zoom": {"x": transform.canvas.state.zoom.x, "y": transform.canvas.state.zoom.y},
            "finite": transform_finite,
            "visible_value_range": visible_value_range.map(|(min, max)| serde_json::json!({
                "min": min,
                "max": max,
                "span": max - min,
            })),
        })),
    );
    paint_grid(ui, transform);
    let ruler_rect = Rect::from_min_max(
        Pos2::new(transform.rect.left(), rect.top()),
        Pos2::new(transform.rect.right(), canvas_rect.top()),
    );
    show_curve_ruler(ui, project, state, owner, transform, ruler_rect);

    if series.is_empty() {
        ui.painter().text(
            transform.rect.center(),
            egui::Align2::CENTER_CENTER,
            "No visible keyframed channels",
            egui::FontId::proportional(13.0),
            ui.visuals().weak_text_color(),
        );
    }

    for (index, curve) in series.iter().enumerate() {
        paint_curve(ui, state, service, curve, curve_color(index), transform);
    }
    paint_playhead(ui, project, state, owner, transform, ruler_rect);
    finish_key_drag(ui, state, service);
}

fn curve_navigation_config() -> NavigationConfig {
    NavigationConfig {
        input_policy: ViewportInputPolicy::AxisModifiers,
        zoom_policy: ZoomPolicy::IndependentXY,
        pan_axes: AxisMask::BOTH,
        zoom_axes: AxisMask::BOTH,
        min_zoom: Vec2::new(MIN_TIME_ZOOM, MIN_VALUE_ZOOM),
        max_zoom: Vec2::new(MAX_TIME_ZOOM, MAX_VALUE_ZOOM),
        wheel_zoom_sensitivity: 0.2,
        ..NavigationConfig::default()
    }
}

impl ViewportState for CurveEditorState {
    fn canvas_state(&self) -> CanvasState {
        self.canvas
    }

    fn set_canvas_state(&mut self, state: CanvasState) {
        self.canvas = state;
    }
}

fn curve_plot_rect(rect: Rect) -> Rect {
    let horizontal = PLOT_PADDING.x.min(((rect.width() - 1.0) * 0.5).max(0.0));
    let vertical = PLOT_PADDING.y.min(((rect.height() - 1.0) * 0.5).max(0.0));
    rect.shrink2(Vec2::new(horizontal, vertical))
}

#[derive(Clone, Copy)]
struct CurveTransform {
    rect: Rect,
    duration: f64,
    min: f64,
    max: f64,
    canvas: CanvasTransform,
}

impl CurveTransform {
    fn new(rect: Rect, duration: f64, min: f64, max: f64, state: CanvasState) -> Option<Self> {
        (rect.is_positive()
            && duration.is_finite()
            && duration > 0.0
            && min.is_finite()
            && max.is_finite()
            && max > min)
            .then_some(Self {
                canvas: CanvasTransform::new(rect.min, state),
                rect,
                duration,
                min,
                max,
            })
    }

    fn world_point(self, time: f64, value: f64) -> Pos2 {
        let time_ratio = (time / self.duration) as f32;
        let value_ratio = ((value - self.min) / (self.max - self.min)) as f32;
        Pos2::new(
            time_ratio * self.rect.width(),
            (1.0 - value_ratio) * self.rect.height(),
        )
    }

    fn point(self, time: f64, value: f64) -> Pos2 {
        self.canvas.world_to_screen(self.world_point(time, value))
    }

    fn world_delta(self, screen_delta: Vec2) -> Option<Vec2> {
        let start = self.canvas.screen_to_world(self.rect.min)?;
        let end = self.canvas.screen_to_world(self.rect.min + screen_delta)?;
        Some(end - start)
    }

    fn delta_time(self, delta_x: f32) -> f64 {
        self.world_delta(Vec2::new(delta_x, 0.0))
            .map_or(0.0, |delta| {
                f64::from(delta.x / self.rect.width()) * self.duration
            })
    }

    fn delta_value(self, delta_y: f32) -> f64 {
        self.world_delta(Vec2::new(0.0, delta_y))
            .map_or(0.0, |delta| {
                -f64::from(delta.y / self.rect.height()) * (self.max - self.min)
            })
    }

    fn time_at_world_x(self, world_x: f32) -> f64 {
        f64::from(world_x / self.rect.width()) * self.duration
    }

    fn value_at_world_y(self, world_y: f32) -> f64 {
        self.max - f64::from(world_y / self.rect.height()) * (self.max - self.min)
    }

    fn visible_value_range(self) -> Option<(f64, f64)> {
        let top = self
            .canvas
            .screen_to_world(self.rect.left_top())
            .map(|point| self.value_at_world_y(point.y))?;
        let bottom = self
            .canvas
            .screen_to_world(self.rect.left_bottom())
            .map(|point| self.value_at_world_y(point.y))?;
        let (min, max) = if top <= bottom {
            (top, bottom)
        } else {
            (bottom, top)
        };
        (min.is_finite() && max.is_finite()).then_some((min, max))
    }

    fn time_at_screen_x(self, screen_x: f32) -> Option<f64> {
        self.canvas
            .screen_to_world(Pos2::new(screen_x, self.rect.center().y))
            .map(|point| self.time_at_world_x(point.x))
            .filter(|time| time.is_finite())
    }

    fn grid_config(self) -> GridConfig {
        GridConfig {
            minor_spacing: Vec2::new(self.rect.width() / 10.0, self.rect.height() / 8.0),
            major_spacing: Vec2::new(self.rect.width() / 2.0, self.rect.height() / 2.0),
            ..GridConfig::default()
        }
    }
}

fn paint_grid(ui: &egui::Ui, transform: CurveTransform) {
    let painter = ui.painter().with_clip_rect(transform.rect);
    let config = transform.grid_config();
    pan_zoom_ui::paint_canvas(
        &painter,
        transform.rect,
        transform.canvas,
        config,
        CanvasTheme::default(),
    );
    let label_color = ui.visuals().weak_text_color();
    for line in pan_zoom_ui::grid_lines(transform.rect, transform.canvas, config)
        .into_iter()
        .filter(|line| line.kind != GridLineKind::Minor)
    {
        match line.axis {
            GridAxis::X => {}
            GridAxis::Y => {
                painter.text(
                    Pos2::new(transform.rect.left() + 3.0, line.screen_position - 2.0),
                    egui::Align2::LEFT_BOTTOM,
                    format_curve_value(transform.value_at_world_y(line.world_position)),
                    egui::FontId::monospace(9.0),
                    label_color,
                );
            }
        };
    }
}

fn show_curve_ruler(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    owner: &AutomationOwner,
    transform: CurveTransform,
    ruler_rect: Rect,
) {
    if !ruler_rect.is_positive() {
        return;
    }
    let ticks = pan_zoom_ui::grid_lines(transform.rect, transform.canvas, transform.grid_config())
        .into_iter()
        .filter(|line| line.axis == GridAxis::X && line.kind != GridLineKind::Minor)
        .map(|line| {
            let seconds = transform.time_at_world_x(line.world_position);
            TimeRulerTick {
                x: line.screen_position,
                label: format!("{seconds:.2}s"),
            }
        })
        .collect::<Vec<_>>();
    let response = TimeRuler::new("authoring_curve_editor_ruler", "curve_editor.ruler", &ticks)
        .show(ui, ruler_rect);
    if !(response.clicked() || response.dragged()) {
        return;
    }
    let Some(local_seconds) = response
        .interact_pointer_pos()
        .and_then(|pointer| transform.time_at_screen_x(pointer.x))
        .map(|seconds| seconds.clamp(0.0, transform.duration))
    else {
        return;
    };
    let Ok(local_time) = MediaTime::from_seconds_f64(local_seconds, 1_000_000) else {
        return;
    };
    let Some(timeline_time) = automation_lanes::timeline_time_for_local(project, owner, local_time)
    else {
        return;
    };
    let Some(timeline) = project.timelines.get(&state.active_timeline_id) else {
        return;
    };
    let frame = (timeline_time.to_seconds_f64() * timeline.fps.to_f64()).round() as i64;
    state.timeline.seek_frame(frame);
}

fn format_curve_value(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude >= 1_000_000.0 || (magnitude > 0.0 && magnitude < 0.001) {
        format!("{value:.2e}")
    } else if magnitude >= 1_000.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

fn paint_curve(
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
    painter.add(egui::Shape::line(samples, Stroke::new(2.0, color)));

    for point in &curve.points {
        let projected = state.curve_editor.drag.as_ref().filter(|drag| {
            drag.lane == curve.id
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
        let hit = Rect::from_center_size(center, Vec2::splat(POINT_RADIUS * 3.0))
            .intersect(transform.rect);
        let response = ui.interact(
            hit,
            ui.id()
                .with(("curve-key", &curve.id, point.id, curve.component)),
            Sense::click_and_drag(),
        );
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
        if response.drag_started() {
            state.curve_editor.drag = Some(CurveKeyDrag {
                lane: curve.id.clone(),
                component: curve.component,
                keyframe_id: point.id,
                original_time: point.time,
                original_value: point.full_value.clone(),
                projected_time: point.time,
                projected_value: point.full_value.clone(),
            });
        }
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
        if response.dragged() {
            if let Some(drag) = state.curve_editor.drag.as_mut() {
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
}

fn show_keyframe_editor(
    context: &egui::Context,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
) {
    let Some(mut draft) = state.curve_editor.keyframe_editor.take() else {
        return;
    };
    let mut open = true;
    let mut apply = false;
    let mut cancel = false;
    crate::ui::widgets::modal::Modal::dialog("Edit Keyframe", 420.0)
        .open(&mut open)
        .show(context, |ui| {
            if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                cancel = true;
            }
            egui::Grid::new("curve_keyframe_editor_values")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Time");
                    let mut seconds = draft.time.to_seconds_f64();
                    let time = ui.add(
                        egui::DragValue::new(&mut seconds)
                            .speed(0.01)
                            .range(0.0..=f64::MAX)
                            .suffix(" s"),
                    );
                    crate::qa::register_component(
                        "curve_editor.keyframe_dialog.time",
                        "keyframe_dialog_control",
                        time.rect,
                    );
                    if time.changed() {
                        if let Ok(value) = MediaTime::from_seconds_f64(seconds, 1_000_000) {
                            draft.time = value;
                        }
                    }
                    ui.end_row();

                    ui.label("Value");
                    let mut value = component_value(&draft.value, draft.component).unwrap_or(0.0);
                    let value_response = ui.add(egui::DragValue::new(&mut value).speed(0.1));
                    crate::qa::register_component(
                        "curve_editor.keyframe_dialog.value",
                        "keyframe_dialog_control",
                        value_response.rect,
                    );
                    if value_response.changed() {
                        draft.value = with_component(draft.value.clone(), draft.component, value);
                    }
                    ui.end_row();

                    ui.label("Interpolation");
                    egui::ComboBox::from_id_salt("curve_keyframe_editor_easing")
                        .selected_text(easing_name(&draft.easing))
                        .show_ui(ui, |ui| {
                            let current = draft.easing.clone();
                            show_easing_menu(ui, Some(&current), None, |easing| {
                                draft.easing = easing;
                            });
                        });
                    ui.end_row();
                });
            ui.separator();
            show_easing_parameters(ui, &mut draft.easing);
            crate::ui::dialogs::dialog_footer(ui, |ui| {
                let apply_response = crate::ui::dialogs::dialog_button(
                    ui,
                    "Apply",
                    crate::ui::dialogs::DialogButtonRole::Primary,
                );
                crate::qa::register_component(
                    "curve_editor.keyframe_dialog.apply",
                    "keyframe_dialog_button",
                    apply_response.rect,
                );
                apply = apply_response.clicked();
                let cancel_response = crate::ui::dialogs::dialog_button(
                    ui,
                    "Cancel",
                    crate::ui::dialogs::DialogButtonRole::Secondary,
                );
                cancel = cancel_response.clicked();
            });
        });
    if apply {
        let update = AuthoringKeyframeUpdate {
            time: Some(draft.time),
            value: Some(draft.value),
            easing: Some(draft.easing),
        };
        match automation_lanes::update_keyframe(service, &draft.lane, draft.keyframe_id, update) {
            Ok(_) => state.status = "Updated keyframe".to_string(),
            Err(error) => state.error = Some(error.to_string()),
        }
    } else if open && !cancel {
        state.curve_editor.keyframe_editor = Some(draft);
    }
}

fn update_curve_easing(
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    lane: AutomationLaneId,
    keyframe_id: KeyframeId,
    easing: library::animation::EasingFunction,
) {
    let update = AuthoringKeyframeUpdate {
        time: None,
        value: None,
        easing: Some(easing),
    };
    let result = automation_lanes::update_keyframe(service, &lane, keyframe_id, update);
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
    if escape || (state.curve_editor.drag.is_some() && !down && !released) {
        state.curve_editor.drag = None;
        return;
    }
    if !released {
        return;
    }
    let Some(drag) = state.curve_editor.drag.take() else {
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
    let result = automation_lanes::update_keyframe(service, &drag.lane, drag.keyframe_id, update);
    if let Err(error) = result {
        state.error = Some(error.to_string());
    }
}

fn paint_playhead(
    ui: &egui::Ui,
    project: &AuthoringProject,
    state: &AuthoringUiState,
    owner: &AutomationOwner,
    transform: CurveTransform,
    ruler_rect: Rect,
) {
    let Some(timeline) = project.timelines.get(&state.active_timeline_id) else {
        return;
    };
    let Ok(timeline_time) = MediaTime::from_frame_index(state.timeline.current_frame, timeline.fps)
    else {
        return;
    };
    let Some(interval) = automation_lanes::owner_interval(project, owner) else {
        return;
    };
    let Ok(end) = interval.end() else {
        return;
    };
    if timeline_time < interval.start || timeline_time > end {
        return;
    }
    let local = automation_lanes::local_time_for_timeline(project, owner, timeline_time);
    let Some(local) = local else {
        return;
    };
    let Some(x) = visible_playhead_x(transform, local.to_seconds_f64()) else {
        return;
    };
    ui.painter().with_clip_rect(transform.rect).line_segment(
        [
            Pos2::new(x, transform.rect.top()),
            Pos2::new(x, transform.rect.bottom()),
        ],
        Stroke::new(1.0, Color32::from_rgb(255, 94, 94)),
    );
    if ruler_rect.is_positive() {
        let painter = ui.painter().with_clip_rect(ruler_rect);
        painter.line_segment(
            [
                Pos2::new(x, ruler_rect.top()),
                Pos2::new(x, ruler_rect.bottom()),
            ],
            Stroke::new(1.0, Color32::from_rgb(255, 94, 94)),
        );
        painter.circle_filled(
            Pos2::new(x, ruler_rect.bottom() - 3.0),
            4.0,
            Color32::from_rgb(255, 94, 94),
        );
    }
}

/// Returns a playhead position only while its local time is visible in the
/// plot. Keeping this policy separate from painting prevents the shared
/// Timeline cursor from leaking into the channel list or outside the current
/// Curve Editor time window.
fn visible_playhead_x(transform: CurveTransform, local_seconds: f64) -> Option<f32> {
    if !local_seconds.is_finite() {
        return None;
    }
    let x = transform.point(local_seconds, transform.min).x;
    (x >= transform.rect.left() && x <= transform.rect.right()).then_some(x)
}

fn selected_owner(state: &AuthoringUiState) -> Option<AutomationOwner> {
    match state.selection.primary()? {
        AuthoringSelection::Item(item_id) => Some(AutomationOwner::Item(item_id)),
        AuthoringSelection::Transition(transition_id) => Some(automation_lanes::transition_owner(
            transition_id,
            state.active_instance_path.as_ref(),
        )),
        _ => None,
    }
}

fn collect_series(project: &AuthoringProject, owner: &AutomationOwner) -> Vec<CurveSeries> {
    automation_lanes::numeric_channels(&automation_lanes::collect_lanes(project, owner))
}

fn sync_visibility(state: &mut AuthoringUiState, owner: &AutomationOwner, series: &[CurveSeries]) {
    let lanes = series
        .iter()
        .map(|curve| curve.id.clone())
        .collect::<std::collections::HashSet<_>>();
    if state.curve_editor.target_owner.as_ref() != Some(owner) {
        state.curve_editor.target_owner = Some(owner.clone());
        state.curve_editor.canvas = CanvasState::uniform(Vec2::ZERO, 1.0);
        state.curve_editor.hidden_lanes.clear();
        state.curve_editor.drag = None;
        state.curve_editor.keyframe_editor = None;
    } else {
        state
            .curve_editor
            .hidden_lanes
            .retain(|lane| lanes.contains(lane));
    }
}

fn series_visible(state: &AuthoringUiState, series: &CurveSeries) -> bool {
    !state.curve_editor.hidden_lanes.contains(&series.id)
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

fn target_label(series: &CurveSeries) -> &str {
    series
        .label
        .split_once('.')
        .map_or(series.label.as_str(), |(label, _)| label)
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
