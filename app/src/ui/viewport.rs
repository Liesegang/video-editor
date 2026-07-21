//! Thin egui adapter for the model-free `pan-zoom-ui` navigation engine.

use eframe::egui;
use pan_zoom_ui::{
    apply_navigation, navigation_delta, CanvasState, NavigationConfig, NavigationInput,
};

pub use pan_zoom_ui::{InputPolicy as ViewportInputPolicy, ZoomPolicy};

/// Adapter implemented by panel-owned view state.
pub trait ViewportState {
    fn canvas_state(&self) -> CanvasState;
    fn set_canvas_state(&mut self, state: CanvasState);
}

pub struct ViewportController<'a> {
    ui: &'a mut egui::Ui,
    id: egui::Id,
    config: NavigationConfig,
    hand_tool_key: Option<egui::Key>,
    pan_tool_active: bool,
    zoom_tool_active: bool,
    screen_origin: Option<egui::Pos2>,
}

impl<'a> ViewportController<'a> {
    pub fn new(ui: &'a mut egui::Ui, id: egui::Id, hand_tool_key: Option<egui::Key>) -> Self {
        Self {
            ui,
            id,
            config: NavigationConfig::default(),
            hand_tool_key,
            pan_tool_active: false,
            zoom_tool_active: false,
            screen_origin: None,
        }
    }

    pub fn with_config(mut self, config: NavigationConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_pan_tool_active(mut self, active: bool) -> Self {
        self.pan_tool_active = active;
        self
    }

    pub fn with_zoom_tool_active(mut self, active: bool) -> Self {
        self.zoom_tool_active = active;
        self
    }

    /// Screen coordinate corresponding to world zero before panel pan.
    pub fn with_screen_origin(mut self, origin: egui::Pos2) -> Self {
        self.screen_origin = Some(origin);
        self
    }

    pub fn interact_with_rect(
        &mut self,
        rect: egui::Rect,
        state: &mut impl ViewportState,
        handled_hand_tool_drag: &mut bool,
    ) -> (bool, egui::Response) {
        let response = self
            .ui
            .interact(rect, self.id, egui::Sense::click_and_drag());
        let key_active = self
            .hand_tool_key
            .is_some_and(|key| self.ui.input(|input| input.key_down(key)));
        let pointer_delta = self.ui.input(|input| input.pointer.delta());
        let primary_pan = (key_active || self.pan_tool_active)
            && response.dragged_by(egui::PointerButton::Primary)
            && !self.zoom_tool_active;
        let middle_pan = response.dragged_by(egui::PointerButton::Middle);
        let scrub_zoom = self.zoom_tool_active
            && response.dragged_by(egui::PointerButton::Primary)
            && !primary_pan;

        if key_active || self.pan_tool_active {
            self.ui.output_mut(|output| {
                output.cursor_icon = if primary_pan {
                    egui::CursorIcon::Grabbing
                } else {
                    egui::CursorIcon::Grab
                };
            });
        }
        if middle_pan {
            self.ui
                .output_mut(|output| output.cursor_icon = egui::CursorIcon::Grabbing);
        }
        if self.zoom_tool_active {
            self.ui
                .output_mut(|output| output.cursor_icon = egui::CursorIcon::ZoomIn);
        }
        if primary_pan && pointer_delta != egui::Vec2::ZERO {
            *handled_hand_tool_drag = true;
        }

        let screen_origin = self.screen_origin.unwrap_or(rect.min);
        let hovered = self.ui.rect_contains_pointer(rect);
        let input = self.ui.input(|input| NavigationInput {
            anchor: input.pointer.hover_pos().map(|position| {
                let local = position - screen_origin;
                egui::pos2(local.x, local.y)
            }),
            hovered,
            modifiers: input.modifiers,
            raw_scroll_delta: input.raw_scroll_delta,
            smooth_scroll_delta: input.smooth_scroll_delta,
            zoom_delta: input.zoom_delta(),
            drag_pan_delta: if primary_pan || middle_pan {
                pointer_delta
            } else {
                egui::Vec2::ZERO
            },
            scrub_zoom_delta: if scrub_zoom { pointer_delta.y } else { 0.0 },
        });
        let delta = navigation_delta(input, self.config);
        let mut canvas_state = state.canvas_state();
        let changed = apply_navigation(&mut canvas_state, delta, self.config);
        if changed {
            state.set_canvas_state(canvas_state);
        }
        (changed, response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: egui::Rect =
        egui::Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(300.0, 230.0));

    #[derive(Clone, Copy, Debug)]
    struct TestViewportState(CanvasState);

    impl ViewportState for TestViewportState {
        fn canvas_state(&self) -> CanvasState {
            self.0
        }

        fn set_canvas_state(&mut self, state: CanvasState) {
            self.0 = state;
        }
    }

    fn run_frame(
        context: &egui::Context,
        state: &mut TestViewportState,
        config: NavigationConfig,
        frame: usize,
        modifiers: egui::Modifiers,
        events: Vec<egui::Event>,
    ) -> bool {
        let mut changed = false;
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(360.0, 280.0),
                )),
                time: Some(frame as f64 / 60.0),
                modifiers,
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let mut handled_hand_tool_drag = false;
                    let mut controller = ViewportController::new(
                        ui,
                        ui.make_persistent_id("viewport-policy-test"),
                        None,
                    )
                    .with_config(config);
                    changed = controller
                        .interact_with_rect(VIEWPORT, state, &mut handled_hand_tool_drag)
                        .0;
                });
            },
        ));
        changed
    }

    fn warm_pointer(context: &egui::Context, state: &mut TestViewportState, point: egui::Pos2) {
        assert!(!run_frame(
            context,
            state,
            NavigationConfig::default(),
            0,
            egui::Modifiers::NONE,
            vec![egui::Event::PointerMoved(point)],
        ));
    }

    fn assert_near(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 1.0e-4,
            "{actual} was not near {expected}"
        );
    }

    #[test]
    fn trackpad_pan_and_pinch_reach_panel_adapter_once() {
        let context = egui::Context::default();
        let pointer = egui::pos2(140.0, 120.0);
        let anchor = pointer - VIEWPORT.min;
        let mut state = TestViewportState(CanvasState::uniform(egui::vec2(-12.0, 8.0), 2.0));
        warm_pointer(&context, &mut state, pointer);

        assert!(run_frame(
            &context,
            &mut state,
            NavigationConfig::default(),
            1,
            egui::Modifiers::NONE,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(4.0, -3.0),
                modifiers: egui::Modifiers::NONE,
            }],
        ));
        assert_eq!(state.0.pan, egui::vec2(-8.0, 5.0));

        let world_before = state.0.world_at(egui::pos2(anchor.x, anchor.y));
        assert!(run_frame(
            &context,
            &mut state,
            NavigationConfig::default(),
            2,
            egui::Modifiers::NONE,
            vec![egui::Event::Zoom(1.5)],
        ));
        let world_after = state.0.world_at(egui::pos2(anchor.x, anchor.y));
        assert_near(world_after.x, world_before.x);
        assert_near(world_after.y, world_before.y);
        assert_eq!(state.0.zoom, egui::Vec2::splat(3.0));
    }
}
