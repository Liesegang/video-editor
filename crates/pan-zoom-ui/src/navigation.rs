use egui::{Modifiers, Pos2, Vec2};

/// Whether zoom is constrained to one scale or may differ by axis.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ZoomPolicy {
    /// X and Y always use the same zoom factor and resulting scale.
    #[default]
    Uniform,
    /// X and Y zoom can be changed independently.
    IndependentXY,
}

/// Interpretation of wheel and trackpad input.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InputPolicy {
    /// Smooth two-axis scrolling pans and native pinch/Ctrl-wheel zooms.
    #[default]
    Trackpad,
    /// Wheel pans; Shift pans X; Ctrl/Cmd zooms X; Ctrl/Cmd+Shift zooms Y.
    AxisModifiers,
    /// A wheel step zooms uniformly instead of panning.
    UniformWheel,
}

/// A compact set of enabled axes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AxisMask {
    x: bool,
    y: bool,
}

impl AxisMask {
    pub const NONE: Self = Self::new(false, false);
    pub const X: Self = Self::new(true, false);
    pub const Y: Self = Self::new(false, true);
    pub const BOTH: Self = Self::new(true, true);

    pub const fn new(x: bool, y: bool) -> Self {
        Self { x, y }
    }

    const fn x(self) -> bool {
        self.x
    }

    const fn y(self) -> bool {
        self.y
    }

    fn filter(self, value: Vec2) -> Vec2 {
        egui::vec2(
            if self.x { value.x } else { 0.0 },
            if self.y { value.y } else { 0.0 },
        )
    }
}

/// Panel-independent navigation behavior and numeric bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavigationConfig {
    pub zoom_policy: ZoomPolicy,
    pub input_policy: InputPolicy,
    pub pan_axes: AxisMask,
    pub zoom_axes: AxisMask,
    pub min_zoom: Vec2,
    pub max_zoom: Vec2,
    /// Symmetric translation limit in screen points.
    pub max_pan: Vec2,
    pub pan_sensitivity: f32,
    /// Fractional change for one discrete wheel step (`0.1` means 10%).
    pub wheel_zoom_sensitivity: f32,
    /// Exponent applied to native pinch factors (`1.0` preserves them).
    pub pinch_zoom_sensitivity: f32,
    /// Exponential zoom change per screen point of scrub movement.
    pub scrub_zoom_sensitivity: f32,
}

impl Default for NavigationConfig {
    fn default() -> Self {
        Self {
            zoom_policy: ZoomPolicy::Uniform,
            input_policy: InputPolicy::Trackpad,
            pan_axes: AxisMask::BOTH,
            zoom_axes: AxisMask::BOTH,
            min_zoom: Vec2::splat(0.01),
            max_zoom: Vec2::splat(1_000.0),
            max_pan: Vec2::splat(f32::INFINITY),
            pan_sensitivity: 1.0,
            wheel_zoom_sensitivity: 0.1,
            pinch_zoom_sensitivity: 1.0,
            scrub_zoom_sensitivity: 0.01,
        }
    }
}

/// Generic canvas transform where
/// `screen = screen_origin + pan + world * zoom`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasState {
    pub pan: Vec2,
    pub zoom: Vec2,
}

impl CanvasState {
    pub const fn new(pan: Vec2, zoom: Vec2) -> Self {
        Self { pan, zoom }
    }

    pub fn uniform(pan: Vec2, zoom: f32) -> Self {
        Self::new(pan, Vec2::splat(zoom))
    }

    /// World coordinate under an anchor expressed relative to screen origin.
    pub fn world_at(self, anchor: Pos2) -> Vec2 {
        (anchor.to_vec2() - self.pan) / self.zoom
    }
}

/// Input sampled by a panel adapter after it has arbitrated gesture ownership.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavigationInput {
    /// Pointer position relative to the panel's chosen screen origin.
    pub anchor: Option<Pos2>,
    /// Whether wheel/pinch input belongs to this canvas.
    pub hovered: bool,
    pub modifiers: Modifiers,
    pub raw_scroll_delta: Vec2,
    pub smooth_scroll_delta: Vec2,
    /// Native multiplicative zoom factor. `1.0` means no pinch.
    pub zoom_delta: f32,
    /// Per-frame content translation already owned by the canvas adapter.
    pub drag_pan_delta: Vec2,
    /// Per-frame vertical scrub movement. Negative values zoom in.
    pub scrub_zoom_delta: f32,
}

impl Default for NavigationInput {
    fn default() -> Self {
        Self {
            anchor: None,
            hovered: false,
            modifiers: Modifiers::NONE,
            raw_scroll_delta: Vec2::ZERO,
            smooth_scroll_delta: Vec2::ZERO,
            zoom_delta: 1.0,
            drag_pan_delta: Vec2::ZERO,
            scrub_zoom_delta: 0.0,
        }
    }
}

/// A panel-neutral navigation change. Zoom is applied before pan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavigationDelta {
    pub pan: Vec2,
    pub zoom_factor: Vec2,
    pub zoom_anchor: Option<Pos2>,
}

impl Default for NavigationDelta {
    fn default() -> Self {
        Self {
            pan: Vec2::ZERO,
            zoom_factor: Vec2::ONE,
            zoom_anchor: None,
        }
    }
}

impl NavigationDelta {
    /// Bridge a transform transition produced by an external widget into the
    /// shared navigation pipeline. Applying this delta to `current` recreates
    /// `target` while still allowing [`apply_navigation`] to enforce policy
    /// and bounds.
    pub fn between(current: CanvasState, target: CanvasState, anchor: Pos2) -> Self {
        let valid_zoom = |zoom: f32| zoom.is_finite() && zoom > 0.0;
        let factor = egui::vec2(
            if valid_zoom(current.zoom.x) && valid_zoom(target.zoom.x) {
                target.zoom.x / current.zoom.x
            } else {
                1.0
            },
            if valid_zoom(current.zoom.y) && valid_zoom(target.zoom.y) {
                target.zoom.y / current.zoom.y
            } else {
                1.0
            },
        );
        let anchor = anchor.to_vec2();
        let zoomed_pan = anchor - (anchor - current.pan) * factor;
        Self {
            pan: target.pan - zoomed_pan,
            zoom_factor: factor,
            zoom_anchor: Some(Pos2::new(anchor.x, anchor.y)),
        }
    }
}

/// Convert sampled input into a generic delta without mutating panel state.
pub fn navigation_delta(input: NavigationInput, config: NavigationConfig) -> NavigationDelta {
    let mut result = NavigationDelta {
        pan: finite_vec(input.drag_pan_delta) * finite_or(config.pan_sensitivity, 1.0),
        ..NavigationDelta::default()
    };

    if input.scrub_zoom_delta.is_finite() && input.scrub_zoom_delta != 0.0 {
        let sensitivity = finite_or(config.scrub_zoom_sensitivity, 0.01).max(0.0);
        let factor = (-input.scrub_zoom_delta * sensitivity).exp();
        multiply_zoom(&mut result, uniform_factor(factor), input.anchor, config);
    }

    if !input.hovered {
        result.pan = config.pan_axes.filter(result.pan);
        return result;
    }

    match config.input_policy {
        InputPolicy::Trackpad => {
            apply_native_zoom(&mut result, input, config);
            result.pan +=
                finite_vec(input.smooth_scroll_delta) * finite_or(config.pan_sensitivity, 1.0);
        }
        InputPolicy::AxisModifiers => apply_axis_modifiers(&mut result, input, config),
        InputPolicy::UniformWheel => {
            if native_zoom_factor(input.zoom_delta, config).is_some() {
                apply_native_zoom(&mut result, input, config);
            } else if let Some(delta) = scroll_scalar(input.raw_scroll_delta) {
                let factor = discrete_zoom_factor(delta, config.wheel_zoom_sensitivity);
                multiply_zoom(&mut result, uniform_factor(factor), input.anchor, config);
            }
        }
    }

    result.pan = config.pan_axes.filter(result.pan);
    result
}

fn apply_axis_modifiers(
    result: &mut NavigationDelta,
    input: NavigationInput,
    config: NavigationConfig,
) {
    let command = input.modifiers.command || input.modifiers.ctrl;
    if command {
        if let Some(delta) = scroll_scalar(input.raw_scroll_delta) {
            let factor = discrete_zoom_factor(delta, config.wheel_zoom_sensitivity);
            let factor = if input.modifiers.shift {
                egui::vec2(1.0, factor)
            } else {
                egui::vec2(factor, 1.0)
            };
            multiply_zoom(result, factor, input.anchor, config);
        }
        return;
    }

    // A native pinch has no axis modifier. Supporting it here keeps independent
    // wheel controls while retaining full trackpad behavior.
    apply_native_zoom(result, input, config);

    let scroll = if input.smooth_scroll_delta != Vec2::ZERO {
        input.smooth_scroll_delta
    } else {
        input.raw_scroll_delta
    };
    if input.modifiers.shift {
        let x = if scroll.x != 0.0 { scroll.x } else { scroll.y };
        result.pan.x += finite_or(x, 0.0) * finite_or(config.pan_sensitivity, 1.0);
    } else {
        result.pan += finite_vec(scroll) * finite_or(config.pan_sensitivity, 1.0);
    }
}

fn apply_native_zoom(
    result: &mut NavigationDelta,
    input: NavigationInput,
    config: NavigationConfig,
) {
    if let Some(factor) = native_zoom_factor(input.zoom_delta, config) {
        multiply_zoom(result, uniform_factor(factor), input.anchor, config);
    }
}

fn native_zoom_factor(raw: f32, config: NavigationConfig) -> Option<f32> {
    if !raw.is_finite() || raw <= 0.0 || raw == 1.0 {
        return None;
    }
    let sensitivity = finite_or(config.pinch_zoom_sensitivity, 1.0).max(0.0);
    let factor = raw.powf(sensitivity);
    (factor.is_finite() && factor > 0.0).then_some(factor)
}

fn multiply_zoom(
    result: &mut NavigationDelta,
    mut factor: Vec2,
    anchor: Option<Pos2>,
    config: NavigationConfig,
) {
    if anchor.is_none() || !factor.x.is_finite() || !factor.y.is_finite() {
        return;
    }
    factor.x = if config.zoom_axes.x() { factor.x } else { 1.0 };
    factor.y = if config.zoom_axes.y() { factor.y } else { 1.0 };
    if config.zoom_policy == ZoomPolicy::Uniform {
        let uniform = if factor.x != 1.0 { factor.x } else { factor.y };
        factor = if config.zoom_axes != AxisMask::NONE {
            Vec2::splat(uniform)
        } else {
            Vec2::ONE
        };
    }
    result.zoom_factor = result.zoom_factor * factor;
    result.zoom_anchor = anchor;
}

fn uniform_factor(factor: f32) -> Vec2 {
    if factor.is_finite() && factor > 0.0 {
        Vec2::splat(factor)
    } else {
        Vec2::ONE
    }
}

fn scroll_scalar(scroll: Vec2) -> Option<f32> {
    let value = if scroll.y != 0.0 { scroll.y } else { scroll.x };
    (value.is_finite() && value != 0.0).then_some(value)
}

fn discrete_zoom_factor(delta: f32, sensitivity: f32) -> f32 {
    let sensitivity = finite_or(sensitivity, 0.1).clamp(0.0, 0.99);
    if delta > 0.0 {
        1.0 + sensitivity
    } else {
        1.0 - sensitivity
    }
}

/// Apply a generic delta according to the configured scale policy and bounds.
pub fn apply_navigation(
    state: &mut CanvasState,
    delta: NavigationDelta,
    config: NavigationConfig,
) -> bool {
    let before = *state;
    sanitize_state(state, config);

    let old_zoom = state.zoom;
    let mut factor = delta.zoom_factor;
    for value in [&mut factor.x, &mut factor.y] {
        if !value.is_finite() || *value <= 0.0 {
            *value = 1.0;
        }
    }
    factor.x = if config.zoom_axes.x() { factor.x } else { 1.0 };
    factor.y = if config.zoom_axes.y() { factor.y } else { 1.0 };

    let new_zoom = bounded_zoom(old_zoom * factor, config);
    if new_zoom != old_zoom {
        if let Some(anchor) = delta.zoom_anchor.filter(|anchor| anchor.is_finite()) {
            let ratio = new_zoom / old_zoom;
            let anchor = anchor.to_vec2();
            state.pan = anchor - (anchor - state.pan) * ratio;
        }
        state.zoom = new_zoom;
    }

    state.pan += config.pan_axes.filter(finite_vec(delta.pan));
    clamp_pan(&mut state.pan, config.max_pan);
    *state != before
}

/// Repair non-finite state and enforce the configured uniform/XY policy.
pub fn sanitize_state(state: &mut CanvasState, config: NavigationConfig) -> bool {
    let before = *state;
    state.pan = finite_vec(state.pan);
    state.zoom = bounded_zoom(state.zoom, config);
    clamp_pan(&mut state.pan, config.max_pan);
    *state != before
}

fn bounded_zoom(zoom: Vec2, config: NavigationConfig) -> Vec2 {
    let min = egui::vec2(
        valid_positive(config.min_zoom.x, 0.01),
        valid_positive(config.min_zoom.y, 0.01),
    );
    let max = egui::vec2(
        valid_max(config.max_zoom.x, min.x),
        valid_max(config.max_zoom.y, min.y),
    );
    if config.zoom_policy == ZoomPolicy::Uniform {
        let lower = min.x.max(min.y);
        let upper = max.x.min(max.y).max(lower);
        let scalar = valid_positive(zoom.x, 1.0).clamp(lower, upper);
        Vec2::splat(scalar)
    } else {
        egui::vec2(
            valid_positive(zoom.x, 1.0).clamp(min.x, max.x),
            valid_positive(zoom.y, 1.0).clamp(min.y, max.y),
        )
    }
}

fn clamp_pan(pan: &mut Vec2, max_pan: Vec2) {
    let limit_x = valid_limit(max_pan.x);
    let limit_y = valid_limit(max_pan.y);
    pan.x = pan.x.clamp(-limit_x, limit_x);
    pan.y = pan.y.clamp(-limit_y, limit_y);
}

fn valid_limit(value: f32) -> f32 {
    if value.is_nan() || value <= 0.0 {
        f32::INFINITY
    } else {
        value
    }
}

fn valid_positive(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

fn valid_max(value: f32, minimum: f32) -> f32 {
    if !value.is_nan() && value >= minimum {
        value
    } else {
        minimum
    }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

fn finite_vec(value: Vec2) -> Vec2 {
    egui::vec2(finite_or(value.x, 0.0), finite_or(value.y, 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_near(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 1.0e-4,
            "{actual} was not near {expected}"
        );
    }

    #[test]
    fn uniform_zoom_uses_one_scale() {
        let mut state = CanvasState::uniform(egui::vec2(4.0, -8.0), 2.0);
        let changed = apply_navigation(
            &mut state,
            NavigationDelta {
                zoom_factor: egui::vec2(1.5, 7.0),
                zoom_anchor: Some(Pos2::ZERO),
                ..NavigationDelta::default()
            },
            NavigationConfig::default(),
        );

        assert!(changed);
        assert_eq!(state.zoom, Vec2::splat(3.0));
    }

    #[test]
    fn independent_xy_zoom_changes_one_axis() {
        let mut state = CanvasState::new(Vec2::ZERO, egui::vec2(100.0, 2.0));
        let config = NavigationConfig {
            zoom_policy: ZoomPolicy::IndependentXY,
            min_zoom: Vec2::splat(0.001),
            ..NavigationConfig::default()
        };
        apply_navigation(
            &mut state,
            NavigationDelta {
                zoom_factor: egui::vec2(1.25, 1.0),
                zoom_anchor: Some(egui::pos2(30.0, 40.0)),
                ..NavigationDelta::default()
            },
            config,
        );

        assert_eq!(state.zoom, egui::vec2(125.0, 2.0));
    }

    #[test]
    fn zoom_preserves_world_coordinate_under_anchor() {
        let anchor = egui::pos2(180.0, 75.0);
        let mut state = CanvasState::new(egui::vec2(12.0, -9.0), egui::vec2(3.0, 5.0));
        let before = state.world_at(anchor);
        let config = NavigationConfig {
            zoom_policy: ZoomPolicy::IndependentXY,
            ..NavigationConfig::default()
        };
        apply_navigation(
            &mut state,
            NavigationDelta {
                zoom_factor: egui::vec2(1.5, 0.5),
                zoom_anchor: Some(anchor),
                ..NavigationDelta::default()
            },
            config,
        );
        let after = state.world_at(anchor);

        assert_near(after.x, before.x);
        assert_near(after.y, before.y);
    }

    #[test]
    fn pan_delta_respects_sensitivity_and_axis_mask() {
        let input = NavigationInput {
            hovered: false,
            drag_pan_delta: egui::vec2(8.0, -3.0),
            ..NavigationInput::default()
        };
        let config = NavigationConfig {
            pan_axes: AxisMask::X,
            pan_sensitivity: 0.5,
            ..NavigationConfig::default()
        };
        let delta = navigation_delta(input, config);
        let mut state = CanvasState::uniform(Vec2::ZERO, 1.0);

        apply_navigation(&mut state, delta, config);

        assert_eq!(state.pan, egui::vec2(4.0, 0.0));
    }

    #[test]
    fn trackpad_combines_native_pinch_and_smooth_pan_once() {
        let anchor = egui::pos2(60.0, 40.0);
        let start_pan = egui::vec2(10.0, -4.0);
        let config = NavigationConfig::default();
        let delta = navigation_delta(
            NavigationInput {
                anchor: Some(anchor),
                hovered: true,
                smooth_scroll_delta: egui::vec2(3.0, -2.0),
                zoom_delta: 1.25,
                ..NavigationInput::default()
            },
            config,
        );
        let mut state = CanvasState::uniform(start_pan, 2.0);
        apply_navigation(&mut state, delta, config);

        let zoomed_pan = anchor.to_vec2() - (anchor.to_vec2() - start_pan) * 1.25;
        assert_eq!(state.zoom, Vec2::splat(2.5));
        assert_eq!(state.pan, zoomed_pan + egui::vec2(3.0, -2.0));
    }

    #[test]
    fn external_widget_delta_reconstructs_its_target_transform() {
        let current = CanvasState::uniform(egui::vec2(10.0, 20.0), 2.0);
        let target = CanvasState::uniform(egui::vec2(-7.0, 31.0), 3.0);
        let mut resolved = current;
        apply_navigation(
            &mut resolved,
            NavigationDelta::between(current, target, egui::pos2(50.0, 80.0)),
            NavigationConfig::default(),
        );

        assert_near(resolved.pan.x, target.pan.x);
        assert_near(resolved.pan.y, target.pan.y);
        assert_eq!(resolved.zoom, target.zoom);
    }

    #[test]
    fn axis_policy_keeps_ctrl_wheel_single_axis_and_accepts_native_pinch() {
        let config = NavigationConfig {
            zoom_policy: ZoomPolicy::IndependentXY,
            input_policy: InputPolicy::AxisModifiers,
            ..NavigationConfig::default()
        };
        let ctrl_wheel = navigation_delta(
            NavigationInput {
                anchor: Some(Pos2::ZERO),
                hovered: true,
                modifiers: Modifiers::COMMAND,
                raw_scroll_delta: egui::vec2(0.0, 4.0),
                // Ctrl-wheel is also represented here by egui. It must not
                // be applied a second time by the axis policy.
                zoom_delta: 1.1,
                ..NavigationInput::default()
            },
            config,
        );
        assert_eq!(ctrl_wheel.zoom_factor, egui::vec2(1.1, 1.0));

        let ctrl_shift_wheel = navigation_delta(
            NavigationInput {
                anchor: Some(Pos2::ZERO),
                hovered: true,
                modifiers: Modifiers {
                    shift: true,
                    ..Modifiers::COMMAND
                },
                raw_scroll_delta: egui::vec2(0.0, 4.0),
                zoom_delta: 1.1,
                ..NavigationInput::default()
            },
            config,
        );
        assert_eq!(ctrl_shift_wheel.zoom_factor, egui::vec2(1.0, 1.1));

        let pinch = navigation_delta(
            NavigationInput {
                anchor: Some(Pos2::ZERO),
                hovered: true,
                zoom_delta: 1.2,
                ..NavigationInput::default()
            },
            config,
        );
        assert_eq!(pinch.zoom_factor, Vec2::splat(1.2));
    }
}
