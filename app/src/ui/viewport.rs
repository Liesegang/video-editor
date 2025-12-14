use eframe::egui;

pub struct ViewportConfig {
    pub allow_pan_x: bool,
    pub allow_pan_y: bool,
    pub allow_zoom_x: bool,
    pub allow_zoom_y: bool,
    pub zoom_uniform: bool, // If true, all zoom ops affect X&Y uniformly
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
            zoom_uniform: false,
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
}

impl<'a> ViewportController<'a> {
    pub fn new(ui: &'a mut egui::Ui, id: egui::Id, hand_tool_key: Option<egui::Key>) -> Self {
        Self {
            ui,
            id,
            config: ViewportConfig::default(),
            hand_tool_key,
        }
    }

    pub fn with_config(mut self, config: ViewportConfig) -> Self {
        self.config = config;
        self
    }

    #[allow(dead_code)]
    pub fn interact(
        &mut self,
        state: &mut impl ViewportState,
        handled_hand_tool_drag: &mut bool,
    ) -> (bool, egui::Response) {
        let available_rect = self.ui.available_rect_before_wrap();
        self.interact_with_rect(available_rect, state, handled_hand_tool_drag)
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

        // --- 1. Hand Tool Logic ---
        let mut _is_hand_tool_active = false;
        if let Some(key) = self.hand_tool_key {
            // Check if key is pressed (not necessarily just pressed this frame)
            if self.ui.input(|i| i.key_down(key)) {
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

        // --- 3. Wheel Zoom / Scroll ---
        if self.ui.rect_contains_pointer(rect) {
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

                if self.config.zoom_uniform {
                    // --- PREVIEW MODE (Always Uniform Zoom) ---
                    // Any scroll = Zoom
                    let delta = if scroll_delta.y != 0.0 {
                        scroll_delta.y
                    } else {
                        scroll_delta.x
                    };
                    let zoom_factor = if delta > 0.0 { 1.1 } else { 0.9 };
                    self.apply_zoom_at(state, local_pivot, egui::vec2(zoom_factor, zoom_factor));
                } else {
                    // --- TIMELINE / GRAPH MODE ---
                    // Default: Scroll Y
                    // Shift: Scroll X
                    // Ctrl: Zoom X
                    // Ctrl+Shift: Zoom Y

                    let is_ctrl = modifiers.command || modifiers.ctrl;
                    let is_shift = modifiers.shift;

                    if is_ctrl && is_shift {
                        // Zoom Y
                        let zoom_factor = if scroll_delta.y > 0.0 { 1.1 } else { 0.9 };
                        if self.config.allow_zoom_y {
                            self.apply_zoom_at(state, local_pivot, egui::vec2(1.0, zoom_factor));
                        }
                    } else if is_ctrl {
                        // Zoom X
                        let zoom_factor = if scroll_delta.y > 0.0 { 1.1 } else { 0.9 };
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
