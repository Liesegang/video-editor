use eframe::egui;

/// How wheel and trackpad navigation is interpreted inside a viewport.
///
/// Keep this explicit per panel: Timeline and Graph use axis/modifier based
/// navigation, while Preview follows the native trackpad convention of
/// two-dimensional scrolling plus a separate pinch gesture.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ViewportInputPolicy {
    /// Unmodified wheel input pans, Shift pans X, Ctrl/Cmd zooms X, and
    /// Ctrl/Cmd+Shift zooms Y.
    #[default]
    AxisModifiers,
    /// Any wheel direction changes X and Y zoom uniformly.
    UniformWheel,
    /// Smooth two-axis scrolling pans and native pinch/Ctrl-wheel zooms
    /// uniformly around the pointer.
    Trackpad,
}

pub struct ViewportConfig {
    pub allow_pan_x: bool,
    pub allow_pan_y: bool,
    pub allow_zoom_x: bool,
    pub allow_zoom_y: bool,
    pub input_policy: ViewportInputPolicy,
    pub min_zoom: f32,
    pub max_zoom: f32,
}

impl Default for ViewportConfig {
    fn default() -> Self {
        Self {
            allow_pan_x: true,
            allow_pan_y: true,
            allow_zoom_x: true,
            allow_zoom_y: true,
            input_policy: ViewportInputPolicy::default(),
            min_zoom: 0.01,
            max_zoom: 1000.0,
        }
    }
}

pub trait ViewportState {
    // Pan is essentially "Scroll Offset" in pixels.
    // (0,0) means top-left of content is at top-left of viewport.
    fn get_pan(&self) -> egui::Vec2;
    fn set_pan(&mut self, pan: egui::Vec2);

    // Zoom is scale factor. 1.0 = 100%.
    fn get_zoom(&self) -> egui::Vec2;
    fn set_zoom(&mut self, zoom: egui::Vec2);
}

pub struct ViewportController<'a> {
    pub ui: &'a mut egui::Ui,
    pub id: egui::Id,
    pub config: ViewportConfig,
    pub hand_tool_key: Option<egui::Key>,
    pub pan_tool_active: bool,
    pub zoom_tool_active: bool,
}

impl<'a> ViewportController<'a> {
    pub fn new(ui: &'a mut egui::Ui, id: egui::Id, hand_tool_key: Option<egui::Key>) -> Self {
        Self {
            ui,
            id,
            config: ViewportConfig::default(),
            hand_tool_key,
            pan_tool_active: false,
            zoom_tool_active: false,
        }
    }

    pub fn with_config(mut self, config: ViewportConfig) -> Self {
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

    pub fn interact_with_rect(
        &mut self,
        rect: egui::Rect,
        state: &mut impl ViewportState,
        handled_hand_tool_drag: &mut bool,
    ) -> (bool, egui::Response) {
        let mut changed = false;

        // Use the provided rect for interaction
        let response = self
            .ui
            .interact(rect, self.id, egui::Sense::click_and_drag());

        // --- 0. Zoom Tool Logic ---
        if self.zoom_tool_active {
            // Set Cursor
            self.ui
                .output_mut(|o| o.cursor_icon = egui::CursorIcon::ZoomIn); // Or generic Zoom if available

            if response.dragged_by(egui::PointerButton::Primary) {
                // Scrubby Zoom
                // Drag Up (Neg Y) -> Zoom In
                // Drag Down (Pos Y) -> Zoom Out
                let delta = response.drag_delta();

                // Sensitivity
                let sensitivity = 0.01;
                let zoom_change = 1.0 - (delta.y * sensitivity);

                if zoom_change != 1.0 {
                    let pivot = response.interact_pointer_pos().unwrap_or(rect.center());
                    let local_pivot = pivot - rect.min; // Relative to rect, as apply_zoom_at expects logic derived from screen-rect.min

                    self.apply_zoom_at(
                        state,
                        egui::Pos2::new(local_pivot.x, local_pivot.y),
                        egui::vec2(zoom_change, zoom_change),
                    );
                    changed = true;
                    // Mark handled?
                }

                // Don't process other tools if zooming
                return (changed, response);
            }
        }

        // --- 1. Hand Tool Logic ---
        let mut _is_hand_tool_active = false;

        let key_active = if let Some(key) = self.hand_tool_key {
            self.ui.input(|i| i.key_down(key))
        } else {
            false
        };

        if key_active || self.pan_tool_active {
            _is_hand_tool_active = true;

            // Set initial cursor (can be overridden by dragging)
            self.ui
                .output_mut(|o| o.cursor_icon = egui::CursorIcon::Grab);

            if response.dragged_by(egui::PointerButton::Primary) {
                let delta = response.drag_delta();
                if delta != egui::Vec2::ZERO {
                    self.apply_pan(state, -delta);
                    changed = true;

                    // Mark as handled to prevent 'Short Press' action on release
                    *handled_hand_tool_drag = true;

                    self.ui
                        .output_mut(|o| o.cursor_icon = egui::CursorIcon::Grabbing);
                }
            }
        }

        // --- 2. Middle Mouse Pan ---
        if response.dragged_by(egui::PointerButton::Middle) {
            let delta = response.drag_delta();
            if delta != egui::Vec2::ZERO {
                self.apply_pan(state, -delta); // Invert delta for "dragging content" feel
                changed = true;
                self.ui
                    .output_mut(|o| o.cursor_icon = egui::CursorIcon::Grabbing);
            }
        }

        // --- 3. Wheel / Trackpad navigation ---
        if self.ui.rect_contains_pointer(rect) {
            if self.config.input_policy == ViewportInputPolicy::Trackpad {
                // `zoom_delta` contains native `Event::Zoom` gestures and
                // Ctrl/Cmd-wheel zoom. egui deliberately excludes the latter
                // from `smooth_scroll_delta`, so reading both cannot apply the
                // same wheel event as a zoom and a pan. A real trackpad may
                // still report independent pan and pinch deltas in one frame;
                // both are useful and are applied in that order.
                let (zoom_delta, pan_delta, pointer_pos) = self.ui.input(|input| {
                    (
                        input.zoom_delta(),
                        input.smooth_scroll_delta,
                        input.pointer.hover_pos().unwrap_or(rect.center()),
                    )
                });
                let local_pivot = pointer_pos - rect.min;
                let local_pivot = egui::pos2(local_pivot.x, local_pivot.y);

                if zoom_delta.is_finite() && zoom_delta > 0.0 && zoom_delta != 1.0 {
                    self.apply_zoom_at(state, local_pivot, egui::Vec2::splat(zoom_delta));
                    changed = true;
                }
                if pan_delta != egui::Vec2::ZERO {
                    // InputState deltas describe content movement, while
                    // ViewportState stores a camera/scroll offset.
                    self.apply_pan(state, -pan_delta);
                    changed = true;
                }

                return (changed, response);
            }

            let scroll_delta = self.ui.input(|i| i.raw_scroll_delta);
            // egui scroll delta: Y is vertical scroll. X is horizontal.
            // Usually Y is dominant on simple mouse wheels.

            // Only process if there is scroll interaction
            if scroll_delta != egui::Vec2::ZERO {
                changed = true;

                let modifiers = self.ui.input(|i| i.modifiers);
                let pointer_pos = self
                    .ui
                    .input(|i| i.pointer.hover_pos())
                    .unwrap_or(rect.center());
                // Relative position within the viewport (not content space)
                // This is needed for "Zoom around mouse"
                // Screen Point P = (World Point W * Zoom) - Pan
                // W = (P + Pan) / Zoom
                // New Pan = W * NewZoom - P
                //         = ((P + Pan) / OldZoom) * NewZoom - P
                //         = (P + Pan) * (NewZoom / OldZoom) - P

                // Determine Action
                let local_pivot =
                    egui::pos2(pointer_pos.x - rect.min.x, pointer_pos.y - rect.min.y);

                if self.config.input_policy == ViewportInputPolicy::UniformWheel {
                    // --- ALWAYS UNIFORM WHEEL ZOOM ---
                    // Any scroll = Zoom
                    let delta = if scroll_delta.y != 0.0 {
                        scroll_delta.y
                    } else {
                        scroll_delta.x
                    };
                    let zoom_factor = if delta > 0.0 { 1.1 } else { 0.9 };
                    self.apply_zoom_at(state, local_pivot, egui::vec2(zoom_factor, zoom_factor));
                } else {
                    // --- AXIS / MODIFIER MODE ---
                    // Default: Scroll Y
                    // Shift: Scroll X
                    // Ctrl: Zoom X
                    // Ctrl+Shift: Zoom Y

                    let is_ctrl = modifiers.command || modifiers.ctrl;
                    let is_shift = modifiers.shift;

                    if is_ctrl && is_shift {
                        // Zoom Y
                        let delta = if scroll_delta.y != 0.0 {
                            scroll_delta.y
                        } else {
                            scroll_delta.x
                        };
                        let zoom_factor = if delta > 0.0 { 1.1 } else { 0.9 };
                        if self.config.allow_zoom_y {
                            self.apply_zoom_at(state, local_pivot, egui::vec2(1.0, zoom_factor));
                        }
                    } else if is_ctrl {
                        // Zoom X
                        let delta = if scroll_delta.y != 0.0 {
                            scroll_delta.y
                        } else {
                            scroll_delta.x
                        };
                        let zoom_factor = if delta > 0.0 { 1.1 } else { 0.9 };
                        if self.config.allow_zoom_x {
                            self.apply_zoom_at(state, local_pivot, egui::vec2(zoom_factor, 1.0));
                        }
                    } else if is_shift {
                        // Pan X (Horizontal Scroll)
                        // Map scroll Y to Pan X usually, or take X if trackpad
                        let pan_x = if scroll_delta.x != 0.0 {
                            scroll_delta.x
                        } else {
                            scroll_delta.y
                        };
                        // Scroll up/down typically means move view up/down. moves CONTENT down/up.
                        // Pan increase = move view down/right (scroll down/right).
                        // If I scroll "down" (delta.y negative?), I want to go down.
                        // egui: scroll down is typically POSITIVE delta in some contexts, negative in others?
                        // raw_scroll_delta: standard mouse wheel down is NEGATIVE Y? No, checking docs...
                        // Usually up is positive.
                        // If I scroll UP, I want to see TOP. Pan decreases.
                        // So pan -= delta.
                        if self.config.allow_pan_x {
                            // Let's assume scroll_delta is "content movement".
                            // If scroll UP (pos), content moves DOWN.
                            // If we apply pan -= delta:
                            // Pan decreases. View moves UP/LEFT. Content moves DOWN/RIGHT. Matches.
                            // But usually shift+scroll wheel controls X scroll.
                            self.apply_pan(state, egui::vec2(-pan_x, 0.0));
                        }
                    } else {
                        // Default: Pan Y (Vertical Scroll)
                        if self.config.allow_pan_y {
                            self.apply_pan(state, -scroll_delta);
                        }
                    }
                }
            }
        }

        (changed, response)
    }

    fn apply_pan(&self, state: &mut impl ViewportState, delta: egui::Vec2) {
        let mut pan = state.get_pan();
        if self.config.allow_pan_x {
            pan.x += delta.x;
        }
        if self.config.allow_pan_y {
            pan.y += delta.y;
        }
        state.set_pan(pan);
    }

    fn apply_zoom_at(&self, state: &mut impl ViewportState, pivot: egui::Pos2, factor: egui::Vec2) {
        let old_zoom = state.get_zoom();
        let mut new_zoom = old_zoom * factor;

        // Clamp
        new_zoom.x = new_zoom.x.clamp(self.config.min_zoom, self.config.max_zoom);
        new_zoom.y = new_zoom.y.clamp(self.config.min_zoom, self.config.max_zoom);

        if !self.config.allow_zoom_x {
            new_zoom.x = old_zoom.x;
        }
        if !self.config.allow_zoom_y {
            new_zoom.y = old_zoom.y;
        }

        if new_zoom == old_zoom {
            return;
        }

        // Adjust Pan to keep pivot stable
        // Formula: NewPan = (Pivot + OldPan) * (NewZoom / OldZoom) - Pivot
        // Wait, standard logic:
        // World = (Screen + Pan) / Zoom
        // Pivot is Screen coord.
        // W = (Pivot + OldPan) / OldZoom
        // We want W to be at Pivot after zoom:
        // W = (Pivot + NewPan) / NewZoom
        // (Pivot + OldPan) / OldZoom = (Pivot + NewPan) / NewZoom
        // Pivot + NewPan = ((Pivot + OldPan) / OldZoom) * NewZoom
        // NewPan = (Pivot + OldPan) * (NewZoom / OldZoom) - Pivot

        let old_pan = state.get_pan();
        // Be careful with Vec2 division, it's component-wise
        let ratio = new_zoom / old_zoom;
        let _p_vec = pivot.to_vec2(); // Pivot as vector from origin

        // Note: Pivot is in UI coordinates (absolute screen).
        // But Pan is usually relative to the "Content Top-Left" in standard ScrollArea?
        // OR Pan is the offset applied to translation.
        // Let's assume: Content Point C drawn at Screen Point S
        // S = (C * Zoom) - Pan  <-- This is standard "Camera" pan.
        // OR S = (C - Pan) * Zoom
        // We need to know the Model used by panels.

        // Case 1: Preview Panel
        // transform = translate(pan) * scale(zoom) ? No, usually translate then scale or scale then translate.
        // Preview usually: Pan is translation. Zoom is scale.
        // If I Pan (20, 20), content shifts by (20, 20).
        // So S = C * Zoom + Pan.
        // Let's check Preview implementation later.

        // Case 2: Timeline
        // Scroll Offset.
        // S = (C - ScrollOffset) * Zoom. (Usually local coords).

        // This discrepancy is TRICKY.
        // "Pan" in Preview might be "Offset of Image". S = Image * Z + Pan.
        // "Scroll" in Timeline is "Offset of View". S = (Time * Scale) - Scroll.

        // If Pan = ScrollOffset:
        // S = C * Zoom - Pan
        // W = (S + Pan) / Zoom
        // NewPan = (S + Pan) * (NewZ/OldZ) - S

        // If Pan = ImageOffset (Preview):
        // S = C * Zoom + Pan
        // W = (S - Pan) / Zoom
        // NewPan = S - (S - Pan) * (NewZ/OldZ)

        // We need `ViewportState` to clarify or handle this?
        // Or we standardize.
        // User wants "Unified Logic".
        // Timeline uses `scroll_offset`. Positive scroll = view moved right (seeing later time).
        // So S = T * Zoom - Scroll.

        // Preview uses `pan`.
        // `app/src/ui/panels/preview/mod.rs` checks:
        // `let transform = TSTransform::from_translation(self.pan) * TSTransform::from_scale(self.zoom);`
        // So S = C * Zoom + Pan. (Scale then Translate).
        // This is opposite direction!

        // Controller needs to know this?
        // Or we implement `ViewportState` such that `get_pan()` always returns "Scroll Offset" style?
        // Preview `pan` is effectively negative scroll offset?
        // If I drag image Right, Pan increases. I see Left part of image.
        // If I scroll Timeline Right (increase offset), I see Right part of timeline.

        // We should normalize in the Adapter impls!

        // DECISION: ViewportState works with "View Position" (Scroll Offset).
        // Positive Pan X = Camera moved Right = Content moves LEFT.
        // Preview Adapter: get_pan() returns -view.pan. set_pan(p) sets view.pan = -p.
        // Timeline Adapter: get_pan() returns scroll_offset. set_pan(p) sets scroll_offset = p.

        // Wait, "Drag Pan".
        // If I drag Mouse RIGHT (Delta > 0).
        // Paper moves RIGHT.
        // Content moves RIGHT.
        // Camera moves LEFT.
        // Scroll Offset DECREASES.
        // Pan -= Delta. (Matches my previous code).

        // So if ViewportState is Scroll Offset:
        // Preview Adapter:
        //   Scroll Offset = -ImagePosition.
        //   get_pan() -> -view.pan
        //   set_pan(p) -> view.pan = -p

        // Let's verify Formula for Scroll Offset model.
        // S = C * Zoom - Pan
        // Piv = C * OldZ - OldPan
        // C = (Piv + OldPan) / OldZ
        // New Piv = C * NewZ - NewPan = Piv (We want Pivot to stay)
        // Piv = ((Piv + OldPan) / OldZ) * NewZ - NewPan
        // NewPan = ((Piv + OldPan) / OldZ) * NewZ - Piv
        // NewPan = (Piv + OldPan) * (NewZ/OldZ) - Piv.
        // This matches the formula derived earlier!

        let new_pan_x = (pivot.x + old_pan.x) * ratio.x - pivot.x;
        let new_pan_y = (pivot.y + old_pan.y) * ratio.y - pivot.y;

        state.set_zoom(new_zoom);
        state.set_pan(egui::vec2(new_pan_x, new_pan_y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: egui::Rect =
        egui::Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(300.0, 230.0));

    #[derive(Clone, Copy, Debug)]
    struct TestViewportState {
        pan: egui::Vec2,
        zoom: egui::Vec2,
    }

    impl ViewportState for TestViewportState {
        fn get_pan(&self) -> egui::Vec2 {
            self.pan
        }

        fn set_pan(&mut self, pan: egui::Vec2) {
            self.pan = pan;
        }

        fn get_zoom(&self) -> egui::Vec2 {
            self.zoom
        }

        fn set_zoom(&mut self, zoom: egui::Vec2) {
            self.zoom = zoom;
        }
    }

    fn run_frame(
        context: &egui::Context,
        state: &mut TestViewportState,
        policy: ViewportInputPolicy,
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
                    .with_config(ViewportConfig {
                        input_policy: policy,
                        ..Default::default()
                    });
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
            ViewportInputPolicy::Trackpad,
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
    fn trackpad_policy_pans_both_axes_and_pinches_around_pointer() {
        let context = egui::Context::default();
        let pointer = egui::pos2(140.0, 120.0);
        let pivot = pointer - VIEWPORT.min;
        let mut state = TestViewportState {
            pan: egui::vec2(12.0, -8.0),
            zoom: egui::Vec2::splat(2.0),
        };
        warm_pointer(&context, &mut state, pointer);

        assert!(run_frame(
            &context,
            &mut state,
            ViewportInputPolicy::Trackpad,
            1,
            egui::Modifiers::NONE,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(4.0, -3.0),
                modifiers: egui::Modifiers::NONE,
            }],
        ));
        assert_eq!(state.pan, egui::vec2(8.0, -5.0));
        assert_eq!(state.zoom, egui::Vec2::splat(2.0));

        let world_before = (pivot + state.pan) / state.zoom;
        assert!(run_frame(
            &context,
            &mut state,
            ViewportInputPolicy::Trackpad,
            2,
            egui::Modifiers::NONE,
            vec![egui::Event::Zoom(1.5)],
        ));
        assert_eq!(state.zoom, egui::Vec2::splat(3.0));
        let world_after = (pivot + state.pan) / state.zoom;
        assert_near(world_after.x, world_before.x);
        assert_near(world_after.y, world_before.y);
    }

    #[test]
    fn trackpad_policy_applies_independent_pinch_and_pan_without_double_counting() {
        let context = egui::Context::default();
        let pointer = egui::pos2(140.0, 120.0);
        let pivot = pointer - VIEWPORT.min;
        let mut state = TestViewportState {
            pan: egui::vec2(10.0, 20.0),
            zoom: egui::Vec2::splat(1.0),
        };
        warm_pointer(&context, &mut state, pointer);

        let pan_delta = egui::vec2(2.0, 3.0);
        let factor = 1.25;
        assert!(run_frame(
            &context,
            &mut state,
            ViewportInputPolicy::Trackpad,
            1,
            egui::Modifiers::NONE,
            vec![
                egui::Event::Zoom(factor),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: pan_delta,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        ));
        let zoomed_pan = (pivot + egui::vec2(10.0, 20.0)) * factor - pivot;
        assert_near(state.pan.x, zoomed_pan.x - pan_delta.x);
        assert_near(state.pan.y, zoomed_pan.y - pan_delta.y);
        assert_eq!(state.zoom, egui::Vec2::splat(factor));

        let command = egui::Modifiers::COMMAND;
        let pan_before = state.pan;
        let zoom_before = state.zoom;
        assert!(run_frame(
            &context,
            &mut state,
            ViewportInputPolicy::Trackpad,
            2,
            command,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 4.0),
                modifiers: command,
            }],
        ));
        assert!(state.zoom.x > zoom_before.x);
        let ratio = state.zoom / zoom_before;
        let zoom_only_pan = (pivot + pan_before) * ratio - pivot;
        assert_near(state.pan.x, zoom_only_pan.x);
        assert_near(state.pan.y, zoom_only_pan.y);
    }

    #[test]
    fn legacy_axis_and_uniform_wheel_policies_keep_their_navigation_contracts() {
        let pointer = egui::pos2(140.0, 120.0);

        let axis_context = egui::Context::default();
        let mut axis_state = TestViewportState {
            pan: egui::Vec2::ZERO,
            zoom: egui::Vec2::ONE,
        };
        warm_pointer(&axis_context, &mut axis_state, pointer);
        assert!(run_frame(
            &axis_context,
            &mut axis_state,
            ViewportInputPolicy::AxisModifiers,
            1,
            egui::Modifiers::NONE,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(4.0, -3.0),
                modifiers: egui::Modifiers::NONE,
            }],
        ));
        assert_eq!(axis_state.pan, egui::vec2(-4.0, 3.0));
        assert_eq!(axis_state.zoom, egui::Vec2::ONE);

        let uniform_context = egui::Context::default();
        let mut uniform_state = TestViewportState {
            pan: egui::Vec2::ZERO,
            zoom: egui::Vec2::ONE,
        };
        warm_pointer(&uniform_context, &mut uniform_state, pointer);
        assert!(run_frame(
            &uniform_context,
            &mut uniform_state,
            ViewportInputPolicy::UniformWheel,
            1,
            egui::Modifiers::NONE,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 4.0),
                modifiers: egui::Modifiers::NONE,
            }],
        ));
        assert_eq!(uniform_state.zoom, egui::Vec2::splat(1.1));
    }
}
