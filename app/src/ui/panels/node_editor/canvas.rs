//! Node Editor canvas navigation and level-of-detail policy.
//!
//! This module deliberately knows nothing about Project graph semantics. It
//! owns only the graph-to-screen transform contract consumed by rendering,
//! interaction, and QA projection.

use eframe::egui;
use egui_snarl::ui::{
    BackgroundPattern, PinPlacement, SelectionStyle, SnarlStyle, WireLayer, WireStyle,
};
use pan_zoom_ui::{
    apply_navigation, sanitize_state, CanvasState, CanvasTheme, GridConfig, NavigationConfig,
    NavigationDelta, ZoomPolicy,
};
use uuid::Uuid;

use super::PORT_SOCKET_SIZE;

/// The previous 0.65 lower bound made an overview of a large graph impossible.
/// 0.0065 is exactly two orders of magnitude farther out while remaining far
/// enough above zero for stable inverse transforms.
pub(super) const NODE_EDITOR_MIN_SCALE: f32 = 0.0065;
pub(super) const NODE_EDITOR_MAX_SCALE: f32 = 1.25;
pub(super) const NODE_EDITOR_MAX_TRANSLATION: f32 = 10_000_000.0;
pub(super) const NODE_EDITOR_DETAIL_SCALE: f32 = 0.18;
pub(super) const NODE_EDITOR_RESIZE_INTERACTION_SCALE: f32 = 0.12;

pub(super) fn node_editor_navigation_config() -> NavigationConfig {
    NavigationConfig {
        zoom_policy: ZoomPolicy::Uniform,
        min_zoom: egui::Vec2::splat(NODE_EDITOR_MIN_SCALE),
        max_zoom: egui::Vec2::splat(NODE_EDITOR_MAX_SCALE),
        max_pan: egui::Vec2::splat(NODE_EDITOR_MAX_TRANSLATION),
        ..NavigationConfig::default()
    }
}

pub(super) fn node_editor_grid_config() -> GridConfig {
    GridConfig {
        // Preserve the previous canvas' roughly 52-point adaptive grid rhythm.
        min_screen_spacing: 52.0,
        ..GridConfig::default()
    }
}

pub(super) fn paint_node_editor_canvas_grid(
    painter: &egui::Painter,
    graph_viewport: egui::Rect,
    screen_viewport: egui::Rect,
    transform: egui::emath::TSTransform,
) {
    let scale = sanitized_node_editor_scale(transform.scaling);
    let theme = CanvasTheme::default();
    painter.rect_filled(graph_viewport, 0.0, theme.background);
    let state = CanvasState::uniform(transform.translation, scale);
    for line in pan_zoom_ui::grid_lines(
        screen_viewport,
        egui::Pos2::ZERO,
        state,
        node_editor_grid_config(),
    ) {
        let grid_stroke = match line.kind {
            pan_zoom_ui::GridLineKind::Minor => theme.minor_grid,
            pan_zoom_ui::GridLineKind::Major => theme.major_grid,
            pan_zoom_ui::GridLineKind::Origin => theme.origin_grid,
        };
        let stroke = egui::Stroke::new(
            screen_stroke_in_graph_units(grid_stroke.width, scale),
            grid_stroke.color,
        );
        match line.axis {
            pan_zoom_ui::GridAxis::X => {
                painter.line_segment(
                    [
                        egui::pos2(line.world_position, graph_viewport.min.y),
                        egui::pos2(line.world_position, graph_viewport.max.y),
                    ],
                    stroke,
                );
            }
            pan_zoom_ui::GridAxis::Y => {
                painter.line_segment(
                    [
                        egui::pos2(graph_viewport.min.x, line.world_position),
                        egui::pos2(graph_viewport.max.x, line.world_position),
                    ],
                    stroke,
                );
            }
        }
    }
}

pub(super) fn sanitized_node_editor_scale(scale: f32) -> f32 {
    let mut state = CanvasState::uniform(egui::Vec2::ZERO, scale);
    sanitize_state(&mut state, node_editor_navigation_config());
    state.zoom.x
}

pub(super) fn sanitize_node_editor_transform(transform: &mut egui::emath::TSTransform) {
    let mut state = CanvasState::uniform(transform.translation, transform.scaling);
    sanitize_state(&mut state, node_editor_navigation_config());
    transform.scaling = state.zoom.x;
    transform.translation = state.pan;
}

/// Reconcile a transform transition owned by egui-snarl through the shared
/// policy without taking gesture ownership away from the widget.
pub(super) fn bridge_node_editor_transform(
    current: egui::emath::TSTransform,
    target: egui::emath::TSTransform,
) -> egui::emath::TSTransform {
    let config = node_editor_navigation_config();
    let mut current_state = CanvasState::uniform(current.translation, current.scaling);
    let mut target_state = CanvasState::uniform(target.translation, target.scaling);
    sanitize_state(&mut current_state, config);
    sanitize_state(&mut target_state, config);
    let delta = NavigationDelta::between(current_state, target_state, egui::Pos2::ZERO);
    apply_navigation(&mut current_state, delta, config);
    egui::emath::TSTransform::new(current_state.pan, current_state.zoom.x)
}

pub(super) fn resolve_node_editor_transform(
    transform: &mut egui::emath::TSTransform,
    locked: Option<egui::emath::TSTransform>,
    previous: Option<egui::emath::TSTransform>,
) {
    let target = locked.unwrap_or(*transform);
    *transform = previous.map_or_else(
        || {
            let mut target = target;
            sanitize_node_editor_transform(&mut target);
            target
        },
        |previous| bridge_node_editor_transform(previous, target),
    );
}

pub(super) fn screen_stroke_in_graph_units(screen_width: f32, scale: f32) -> f32 {
    screen_width / sanitized_node_editor_scale(scale)
}

pub(super) fn node_editor_details_visible(scale: f32) -> bool {
    sanitized_node_editor_scale(scale) >= NODE_EDITOR_DETAIL_SCALE
}

pub(super) fn node_editor_port_interactions_enabled(scale: f32) -> bool {
    node_editor_details_visible(scale)
}

pub(super) fn node_editor_resize_interactions_enabled(scale: f32) -> bool {
    sanitized_node_editor_scale(scale) >= NODE_EDITOR_RESIZE_INTERACTION_SCALE
}

#[cfg(test)]
pub(super) fn node_editor_snarl_style() -> SnarlStyle {
    node_editor_snarl_style_for(&egui::Style::default())
}

pub(super) fn node_editor_snarl_style_for(style: &egui::Style) -> SnarlStyle {
    let navigation = node_editor_navigation_config();
    SnarlStyle {
        collapsible: Some(false),
        pin_placement: Some(PinPlacement::Edge),
        pin_size: Some(PORT_SOCKET_SIZE),
        wire_width: Some(3.0),
        wire_style: Some(WireStyle::Bezier3),
        wire_layer: Some(WireLayer::BehindNodes),
        wire_frame_size: Some(72.0),
        bg_pattern: Some(BackgroundPattern::NoPattern),
        bg_frame: Some(egui::Frame::canvas(style).fill(CanvasTheme::default().background)),
        // Project selection is the only visual authority. Snarl's private
        // selection is an inert renderer detail while node-editor-ui owns
        // selection and header movement, so never paint a stale second layer.
        select_style: Some(SelectionStyle {
            margin: egui::Margin::ZERO,
            rounding: egui::CornerRadius::ZERO,
            fill: egui::Color32::TRANSPARENT,
            stroke: egui::Stroke::NONE,
        }),
        min_scale: Some(navigation.min_zoom.x),
        max_scale: Some(navigation.max_zoom.x),
        ..Default::default()
    }
}

pub(super) fn node_editor_canvas_metadata(
    composition_id: Uuid,
    mut transform: egui::emath::TSTransform,
) -> serde_json::Value {
    sanitize_node_editor_transform(&mut transform);
    let scale = transform.scaling;
    serde_json::json!({
        "composition_id": composition_id,
        "scale": scale,
        "translation": {
            "x": transform.translation.x,
            "y": transform.translation.y,
        },
        "min_scale": NODE_EDITOR_MIN_SCALE,
        "max_scale": NODE_EDITOR_MAX_SCALE,
        "detail_enabled": node_editor_details_visible(scale),
        "port_interaction_enabled": node_editor_port_interactions_enabled(scale),
        "resize_interaction_enabled": node_editor_resize_interactions_enabled(scale),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_canvas_uses_shared_theme_and_bounded_grid() {
        let style = node_editor_snarl_style();
        let navigation = node_editor_navigation_config();
        assert_eq!(
            style.bg_frame.map(|frame| frame.fill),
            Some(CanvasTheme::default().background)
        );
        assert_eq!(
            style.select_style,
            Some(SelectionStyle {
                margin: egui::Margin::ZERO,
                rounding: egui::CornerRadius::ZERO,
                fill: egui::Color32::TRANSPARENT,
                stroke: egui::Stroke::NONE,
            })
        );
        assert_eq!(navigation.zoom_policy, ZoomPolicy::Uniform);
        assert_eq!(navigation.zoom_axes, pan_zoom_ui::AxisMask::BOTH);
        assert_eq!(node_editor_grid_config().min_screen_spacing, 52.0);
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1_800.0, 1_200.0));
        for scale in [NODE_EDITOR_MIN_SCALE, 0.01, 0.1, 1.0, NODE_EDITOR_MAX_SCALE] {
            let state = CanvasState::uniform(egui::vec2(347.0, -73.0), scale);
            let lines = pan_zoom_ui::grid_lines(
                viewport,
                egui::Pos2::ZERO,
                state,
                node_editor_grid_config(),
            );
            assert!(!lines.is_empty(), "scale={scale}");
            assert!(lines.len() < 320, "scale={scale}, lines={}", lines.len());
        }
    }

    #[test]
    fn external_widget_bridge_preserves_a_valid_uniform_transition() {
        let current = egui::emath::TSTransform::new(egui::vec2(120.0, -40.0), 0.5);
        let target = egui::emath::TSTransform::new(egui::vec2(87.0, 31.0), 0.75);

        assert_eq!(bridge_node_editor_transform(current, target), target);
    }
}
