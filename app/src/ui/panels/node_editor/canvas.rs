//! Node Editor canvas navigation and level-of-detail policy.
//!
//! This module deliberately knows nothing about Project graph semantics. It
//! owns only the graph-to-screen transform contract consumed by rendering,
//! interaction, and QA projection.

use eframe::egui;
use egui_snarl::ui::{BackgroundPattern, PinPlacement, SnarlStyle, WireLayer, WireStyle};
use uuid::Uuid;

/// The previous 0.65 lower bound made an overview of a large graph impossible.
/// 0.0065 is exactly two orders of magnitude farther out while remaining far
/// enough above zero for stable inverse transforms.
pub(super) const NODE_EDITOR_MIN_SCALE: f32 = 0.0065;
pub(super) const NODE_EDITOR_MAX_SCALE: f32 = 1.25;
pub(super) const NODE_EDITOR_MAX_TRANSLATION: f32 = 10_000_000.0;
pub(super) const GRID_TARGET_SCREEN_SPACING: f32 = 52.0;
pub(super) const NODE_EDITOR_DETAIL_SCALE: f32 = 0.18;
pub(super) const NODE_EDITOR_RESIZE_INTERACTION_SCALE: f32 = 0.12;

pub(super) fn sanitized_node_editor_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 {
        scale.clamp(NODE_EDITOR_MIN_SCALE, NODE_EDITOR_MAX_SCALE)
    } else {
        1.0
    }
}

pub(super) fn sanitize_node_editor_transform(transform: &mut egui::emath::TSTransform) {
    transform.scaling = sanitized_node_editor_scale(transform.scaling);
    for value in [&mut transform.translation.x, &mut transform.translation.y] {
        *value = if value.is_finite() {
            value.clamp(-NODE_EDITOR_MAX_TRANSLATION, NODE_EDITOR_MAX_TRANSLATION)
        } else {
            0.0
        };
    }
}

/// Pick a 1/2/5-decade grid size in graph units. This keeps the number of
/// painted lines proportional to screen size instead of exploding at 0.0065x.
pub(super) fn adaptive_grid_spacing(scale: f32) -> f32 {
    let target = GRID_TARGET_SCREEN_SPACING / sanitized_node_editor_scale(scale);
    let decade = 10.0_f32.powf(target.log10().floor());
    let normalized = target / decade;
    let multiplier = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    (decade * multiplier).clamp(1.0, 1_000_000_000.0)
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

pub(super) fn node_editor_snarl_style() -> SnarlStyle {
    SnarlStyle {
        collapsible: Some(false),
        pin_placement: Some(PinPlacement::Edge),
        pin_size: Some(13.0),
        wire_width: Some(3.0),
        wire_style: Some(WireStyle::Bezier3),
        wire_layer: Some(WireLayer::BehindNodes),
        wire_frame_size: Some(72.0),
        bg_pattern: Some(BackgroundPattern::NoPattern),
        min_scale: Some(NODE_EDITOR_MIN_SCALE),
        max_scale: Some(NODE_EDITOR_MAX_SCALE),
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
