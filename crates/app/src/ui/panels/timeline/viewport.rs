use egui::{Pos2, Rect};
use pan_zoom_ui::{
    apply_navigation, AxisMask, CanvasState, CanvasTransform, GridConfig, NavigationConfig,
    NavigationDelta,
};

use crate::state::authoring::{AuthoringTimelineView, AuthoringUiState};
use crate::ui::viewport::{ViewportController, ViewportInputPolicy, ViewportState, ZoomPolicy};

use super::geometry::{
    major_tick_seconds, TimelineRowMetrics, MAX_VERTICAL_ZOOM, MIN_VERTICAL_ZOOM,
};

const MIN_PIXELS_PER_SECOND: f32 = 8.0;
const MAX_PIXELS_PER_SECOND: f32 = 2_000.0;

pub(super) fn time_scale_range() -> std::ops::RangeInclusive<f32> {
    MIN_PIXELS_PER_SECOND..=MAX_PIXELS_PER_SECOND
}

fn navigation_config() -> NavigationConfig {
    NavigationConfig {
        input_policy: ViewportInputPolicy::AxisModifiers,
        zoom_policy: ZoomPolicy::IndependentXY,
        pan_axes: AxisMask::BOTH,
        zoom_axes: AxisMask::BOTH,
        min_zoom: egui::vec2(MIN_PIXELS_PER_SECOND, MIN_VERTICAL_ZOOM),
        max_zoom: egui::vec2(MAX_PIXELS_PER_SECOND, MAX_VERTICAL_ZOOM),
        ..NavigationConfig::default()
    }
}

pub(super) fn canvas_state(view: &AuthoringTimelineView) -> CanvasState {
    CanvasState::new(
        egui::vec2(-view.horizontal_scroll, -view.vertical_scroll),
        egui::vec2(view.pixels_per_second, view.vertical_zoom),
    )
}

pub(super) fn canvas_transform(
    content_rect: Rect,
    view: &AuthoringTimelineView,
) -> CanvasTransform {
    CanvasTransform::new(content_rect.min, canvas_state(view))
}

pub(super) fn grid_config(view: &AuthoringTimelineView) -> GridConfig {
    let major_seconds = major_tick_seconds(view.pixels_per_second);
    let metrics = TimelineRowMetrics::from_view(view);
    GridConfig {
        origin: Pos2::ZERO,
        minor_spacing: egui::vec2(major_seconds / 5.0, metrics.world_stride()),
        major_spacing: egui::vec2(major_seconds, metrics.world_stride() * 4.0),
        adaptive: false,
        ..GridConfig::default()
    }
}

pub(super) fn seconds_to_screen_x(
    seconds: f32,
    content_rect: Rect,
    view: &AuthoringTimelineView,
) -> f32 {
    canvas_transform(content_rect, view)
        .world_to_screen(Pos2::new(seconds, 0.0))
        .x
}

pub(super) fn screen_x_to_seconds(x: f32, content_rect: Rect, view: &AuthoringTimelineView) -> f32 {
    canvas_transform(content_rect, view)
        .screen_to_world(Pos2::new(x, content_rect.top()))
        .map_or(0.0, |world| world.x.max(0.0))
}

pub(super) fn row_top(content_rect: Rect, view: &AuthoringTimelineView, index: usize) -> f32 {
    let metrics = TimelineRowMetrics::from_view(view);
    canvas_transform(content_rect, view)
        .world_to_screen(Pos2::new(0.0, index as f32 * metrics.world_stride()))
        .y
}

pub(super) fn navigate(
    ui: &mut egui::Ui,
    content_rect: Rect,
    state: &mut AuthoringUiState,
    row_count: usize,
) {
    if state.timeline.item_gesture.is_some()
        || state.timeline.keyframe_gesture.is_some()
        || state.timeline.library_drag.is_some()
    {
        return;
    }

    let metrics = TimelineRowMetrics::from_view(&state.timeline);
    let content_height = metrics.content_height(row_count);
    let max_vertical_scroll = (content_height - content_rect.height()).max(0.0);
    state.timeline.vertical_scroll = state
        .timeline
        .vertical_scroll
        .clamp(0.0, max_vertical_scroll);
    let mut viewport = TimelineViewportState {
        horizontal_scroll: &mut state.timeline.horizontal_scroll,
        vertical_scroll: &mut state.timeline.vertical_scroll,
        pixels_per_second: &mut state.timeline.pixels_per_second,
        vertical_zoom: &mut state.timeline.vertical_zoom,
        world_content_height: row_count as f32 * metrics.world_stride(),
        viewport_height: content_rect.height(),
    };
    let config = navigation_config();
    let mut handled_pan = false;
    let mut controller = ViewportController::new(
        ui,
        ui.make_persistent_id("authoring_timeline_viewport"),
        None,
    )
    .with_config(config)
    .with_screen_origin(content_rect.min);
    let _ = controller.interact_with_rect(content_rect, &mut viewport, &mut handled_pan);
}

/// Apply explicit View Scale controls through the same navigation policy,
/// anchor math, row bounds, and `CanvasState` used by wheel/pinch input.
pub(super) fn set_view_scale(
    view: &mut AuthoringTimelineView,
    content_rect: Rect,
    row_count: usize,
    pixels_per_second: f32,
    vertical_zoom: f32,
) {
    let current = canvas_state(view);
    let target = egui::vec2(pixels_per_second, vertical_zoom);
    let zoom_factor = egui::vec2(
        target.x / current.zoom.x.max(f32::EPSILON),
        target.y / current.zoom.y.max(f32::EPSILON),
    );
    let local_anchor = content_rect.center() - content_rect.min;
    let mut updated = current;
    apply_navigation(
        &mut updated,
        NavigationDelta {
            pan: egui::Vec2::ZERO,
            zoom_factor,
            zoom_anchor: Some(egui::pos2(local_anchor.x, local_anchor.y)),
        },
        navigation_config(),
    );
    let metrics = TimelineRowMetrics::from_view(view);
    let mut viewport = TimelineViewportState {
        horizontal_scroll: &mut view.horizontal_scroll,
        vertical_scroll: &mut view.vertical_scroll,
        pixels_per_second: &mut view.pixels_per_second,
        vertical_zoom: &mut view.vertical_zoom,
        world_content_height: row_count as f32 * metrics.world_stride(),
        viewport_height: content_rect.height(),
    };
    viewport.set_canvas_state(updated);
}

struct TimelineViewportState<'a> {
    horizontal_scroll: &'a mut f32,
    vertical_scroll: &'a mut f32,
    pixels_per_second: &'a mut f32,
    vertical_zoom: &'a mut f32,
    world_content_height: f32,
    viewport_height: f32,
}

impl ViewportState for TimelineViewportState<'_> {
    fn canvas_state(&self) -> CanvasState {
        CanvasState::new(
            egui::vec2(-*self.horizontal_scroll, -*self.vertical_scroll),
            egui::vec2(*self.pixels_per_second, *self.vertical_zoom),
        )
    }

    fn set_canvas_state(&mut self, state: CanvasState) {
        *self.horizontal_scroll = (-state.pan.x).max(0.0);
        *self.pixels_per_second = state
            .zoom
            .x
            .clamp(MIN_PIXELS_PER_SECOND, MAX_PIXELS_PER_SECOND);
        *self.vertical_zoom = state.zoom.y.clamp(MIN_VERTICAL_ZOOM, MAX_VERTICAL_ZOOM);
        let max_vertical_scroll =
            (self.world_content_height * *self.vertical_zoom - self.viewport_height).max(0.0);
        *self.vertical_scroll = (-state.pan.y).clamp(0.0, max_vertical_scroll);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: Rect = Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(300.0, 230.0));

    fn pointer_button(position: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn run_navigation_frame(
        context: &egui::Context,
        state: &mut AuthoringUiState,
        frame: usize,
        events: Vec<egui::Event>,
    ) {
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(360.0, 280.0))),
                time: Some(frame as f64 / 60.0),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    navigate(ui, VIEWPORT, state, 20);
                });
            },
        ));
    }

    #[test]
    fn screen_coordinates_round_trip_through_the_shared_canvas_state() {
        let view = AuthoringTimelineView {
            horizontal_scroll: 125.0,
            pixels_per_second: 80.0,
            ..AuthoringTimelineView::default()
        };
        let rect = Rect::from_min_size(egui::pos2(200.0, 50.0), egui::Vec2::new(900.0, 400.0));

        let screen = seconds_to_screen_x(3.25, rect, &view);
        assert!((screen_x_to_seconds(screen, rect, &view) - 3.25).abs() < f32::EPSILON);
    }

    #[test]
    fn grid_and_content_share_the_same_panned_time_origin() {
        let view = AuthoringTimelineView {
            horizontal_scroll: 80.0,
            pixels_per_second: 80.0,
            ..AuthoringTimelineView::default()
        };
        let rect = Rect::from_min_size(Pos2::new(200.0, 50.0), egui::Vec2::new(900.0, 400.0));
        let lines =
            pan_zoom_ui::grid_lines(rect, canvas_transform(rect, &view), grid_config(&view));
        let one_second = lines
            .iter()
            .find(|line| {
                line.axis == pan_zoom_ui::GridAxis::X
                    && (line.world_position - 1.0).abs() < f32::EPSILON
            })
            .expect("one-second grid line");

        assert!(
            (one_second.screen_position - seconds_to_screen_x(1.0, rect, &view)).abs() < 1.0e-4
        );
    }

    #[test]
    fn primary_background_drag_does_not_pan_the_timeline() {
        let context = egui::Context::default();
        let timeline_id = library::model::authoring::TimelineId::new();
        let mut state = AuthoringUiState::new(timeline_id);
        state.timeline.horizontal_scroll = 120.0;
        state.timeline.vertical_scroll = 32.0;
        state.timeline.pixels_per_second = 96.0;
        let start = egui::pos2(140.0, 120.0);
        let end = egui::pos2(210.0, 165.0);

        run_navigation_frame(
            &context,
            &mut state,
            0,
            vec![egui::Event::PointerMoved(start)],
        );
        run_navigation_frame(&context, &mut state, 1, vec![pointer_button(start, true)]);
        run_navigation_frame(
            &context,
            &mut state,
            2,
            vec![egui::Event::PointerMoved(end)],
        );
        run_navigation_frame(&context, &mut state, 3, vec![pointer_button(end, false)]);

        assert_eq!(state.timeline.horizontal_scroll, 120.0);
        assert_eq!(state.timeline.vertical_scroll, 32.0);
        assert_eq!(state.timeline.pixels_per_second, 96.0);
    }

    #[test]
    fn zoomed_pan_reaches_the_new_bottom_instead_of_the_old_bound() {
        let mut horizontal_scroll = 0.0;
        let mut vertical_scroll = 174.0;
        let mut pixels_per_second = 80.0;
        let mut vertical_zoom = 0.55;
        let mut viewport = TimelineViewportState {
            horizontal_scroll: &mut horizontal_scroll,
            vertical_scroll: &mut vertical_scroll,
            pixels_per_second: &mut pixels_per_second,
            vertical_zoom: &mut vertical_zoom,
            world_content_height: 680.0,
            viewport_height: 200.0,
        };

        let new_bottom = 680.0 * 0.605 - 200.0;
        viewport.set_canvas_state(CanvasState::new(
            egui::vec2(0.0, -new_bottom),
            egui::vec2(80.0, 0.605),
        ));

        assert_eq!(vertical_zoom, 0.605);
        assert!((vertical_scroll - new_bottom).abs() < 1.0e-4);
        assert!(
            vertical_scroll > 174.0,
            "new zoom must expand the scroll bound"
        );
    }

    #[test]
    fn explicit_view_scale_preserves_the_center_world_anchor() {
        let mut view = AuthoringTimelineView {
            horizontal_scroll: 60.0,
            vertical_scroll: 40.0,
            ..AuthoringTimelineView::default()
        };
        let anchor = VIEWPORT.center() - VIEWPORT.min;
        let before = canvas_state(&view).world_at(egui::pos2(anchor.x, anchor.y));

        set_view_scale(&mut view, VIEWPORT, 20, 120.0, 1.5);

        let after = canvas_state(&view).world_at(egui::pos2(anchor.x, anchor.y));
        assert!((before.x - after.x).abs() < 1.0e-4);
        assert!((before.y - after.y).abs() < 1.0e-4);
    }
}
