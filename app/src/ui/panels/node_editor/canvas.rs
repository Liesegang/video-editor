//! Node Editor canvas navigation and level-of-detail policy.
//!
//! This module deliberately knows nothing about Project graph semantics. It
//! owns only the graph-to-screen transform contract consumed by rendering,
//! interaction, and QA projection.

use super::PORT_SOCKET_SIZE;
use eframe::egui;
use egui_snarl::ui::{
    BackgroundPattern, PinPlacement, SelectionStyle, SnarlStyle, WireLayer, WireStyle,
};
use pan_zoom_ui::{
    sanitize_state, CanvasState, CanvasTheme, GridConfig, NavigationConfig, ZoomPolicy,
};

/// The previous 0.65 lower bound made an overview of a large graph impossible.
/// 0.0065 is exactly two orders of magnitude farther out while remaining far
/// enough above zero for stable inverse transforms.
pub(super) const NODE_EDITOR_MIN_SCALE: f32 = 0.0065;
pub(super) const NODE_EDITOR_MAX_SCALE: f32 = 1.25;
pub(super) const NODE_EDITOR_MAX_TRANSLATION: f32 = 10_000_000.0;
pub(super) const NODE_EDITOR_DETAIL_SCALE: f32 = 0.18;

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
        pan_zoom_ui::CanvasTransform::new(egui::Pos2::ZERO, state),
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

pub(super) fn screen_stroke_in_graph_units(screen_width: f32, scale: f32) -> f32 {
    screen_width / sanitized_node_editor_scale(scale)
}

pub(super) fn node_editor_details_visible(scale: f32) -> bool {
    sanitized_node_editor_scale(scale) >= NODE_EDITOR_DETAIL_SCALE
}

pub(super) fn node_editor_port_interactions_enabled(scale: f32) -> bool {
    node_editor_details_visible(scale)
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
                pan_zoom_ui::CanvasTransform::new(egui::Pos2::ZERO, state),
                node_editor_grid_config(),
            );
            assert!(!lines.is_empty(), "scale={scale}");
            assert!(lines.len() < 320, "scale={scale}, lines={}", lines.len());
        }
    }

    #[test]
    fn invalid_scales_are_sanitized_by_the_shared_navigation_policy() {
        assert_eq!(sanitized_node_editor_scale(f32::NAN), 1.0);
        assert_eq!(
            sanitized_node_editor_scale(NODE_EDITOR_MIN_SCALE / 2.0),
            NODE_EDITOR_MIN_SCALE
        );
        assert_eq!(
            sanitized_node_editor_scale(NODE_EDITOR_MAX_SCALE * 2.0),
            NODE_EDITOR_MAX_SCALE
        );
    }
}
