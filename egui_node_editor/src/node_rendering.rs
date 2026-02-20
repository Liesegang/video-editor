//! Node layout computation and drawing primitives, extracted from draw_node().

use egui::{self, Color32, Pos2, Rect, Stroke, StrokeKind, Vec2};
use uuid::Uuid;

use crate::theme::NodeEditorTheme;
use crate::types::PinInfo;
use crate::widget::PinScreen;

/// Pre-computed layout for a node.
pub(crate) struct NodeLayout {
    pub screen_pos: Pos2,
    pub node_rect: Rect,
    pub header_h: f32,
    pub pin_row_h: f32,
    pub pin_r: f32,
    pub pin_margin: f32,
    pub rounding: f32,
    pub node_w: f32,
    pub pin_start_y: f32,
}

/// Draw the node body and selection outline.
pub(crate) fn draw_node_chrome(
    painter: &egui::Painter,
    layout: &NodeLayout,
    theme: &NodeEditorTheme,
    node_type_id: &str,
    display_name: &str,
    is_container: bool,
    is_expanded: bool,
    is_selected: bool,
    is_active: bool,
    zoom: f32,
) {
    let dim = if is_active { 1.0 } else { 0.4 };

    // Body
    let body_color = dim_color(
        if is_selected {
            theme.node_body_selected_color
        } else {
            theme.node_body_color
        },
        dim,
    );
    painter.rect_filled(layout.node_rect, layout.rounding, body_color);

    if is_selected {
        painter.rect_stroke(
            layout.node_rect,
            layout.rounding,
            Stroke::new(2.0 * zoom, theme.selection_color),
            StrokeKind::Outside,
        );
    }

    // Header
    let header_rect =
        Rect::from_min_size(layout.screen_pos, Vec2::new(layout.node_w, layout.header_h));
    painter.rect_filled(
        header_rect,
        egui::CornerRadius {
            nw: layout.rounding as u8,
            ne: layout.rounding as u8,
            sw: 0,
            se: 0,
        },
        dim_color((theme.header_color)(node_type_id), dim),
    );

    let header_text = if is_container {
        format!(
            "{} {}",
            if is_expanded { "\u{25BC}" } else { "\u{25B6}" },
            display_name
        )
    } else {
        display_name.to_string()
    };
    painter.text(
        header_rect.center(),
        egui::Align2::CENTER_CENTER,
        &header_text,
        egui::FontId::proportional(12.0 * zoom),
        dim_color(Color32::WHITE, dim),
    );
}

/// Draw input and output pins, pushing PinScreen entries.
pub(crate) fn draw_pins(
    painter: &egui::Painter,
    layout: &NodeLayout,
    theme: &NodeEditorTheme,
    node_type_id: &str,
    node_id: Uuid,
    input_pins: &[&PinInfo],
    output_pins: &[&PinInfo],
    is_active: bool,
    zoom: f32,
    pin_screens: &mut Vec<PinScreen>,
) {
    let dim = if is_active { 1.0 } else { 0.4 };
    let pin_color = dim_color((theme.pin_color)(node_type_id), dim);
    let label_color = dim_color(theme.pin_label_color, dim);

    for (i, pin) in input_pins.iter().enumerate() {
        let cy = layout.pin_start_y + i as f32 * layout.pin_row_h + layout.pin_row_h / 2.0;
        let cx = layout.screen_pos.x + layout.pin_margin;
        let p = Pos2::new(cx, cy);
        painter.circle_filled(p, layout.pin_r, pin_color);
        painter.text(
            p + Vec2::new(layout.pin_r + 4.0 * zoom, 0.0),
            egui::Align2::LEFT_CENTER,
            &pin.display_name,
            egui::FontId::proportional(10.0 * zoom),
            label_color,
        );
        pin_screens.push(PinScreen {
            pos: p,
            node_id,
            name: pin.name.clone(),
            is_output: false,
        });
    }

    for (i, pin) in output_pins.iter().enumerate() {
        let cy = layout.pin_start_y + i as f32 * layout.pin_row_h + layout.pin_row_h / 2.0;
        let cx = layout.screen_pos.x + layout.node_w - layout.pin_margin;
        let p = Pos2::new(cx, cy);
        painter.circle_filled(p, layout.pin_r, pin_color);
        painter.text(
            p + Vec2::new(-layout.pin_r - 4.0 * zoom, 0.0),
            egui::Align2::RIGHT_CENTER,
            &pin.display_name,
            egui::FontId::proportional(10.0 * zoom),
            label_color,
        );
        pin_screens.push(PinScreen {
            pos: p,
            node_id,
            name: pin.name.clone(),
            is_output: true,
        });
    }
}

/// Draw the resize handle triangle for containers.
pub(crate) fn draw_resize_handle(painter: &egui::Painter, node_rect: Rect, zoom: f32) {
    let handle_size = 8.0 * zoom;
    let handle_rect = Rect::from_min_size(
        Pos2::new(node_rect.max.x - handle_size, node_rect.max.y - handle_size),
        Vec2::new(handle_size, handle_size),
    );
    let pts = [
        handle_rect.right_bottom(),
        Pos2::new(handle_rect.left(), handle_rect.bottom()),
        Pos2::new(handle_rect.right(), handle_rect.top()),
    ];
    painter.add(egui::Shape::convex_polygon(
        pts.to_vec(),
        Color32::from_rgb(100, 100, 120),
        Stroke::NONE,
    ));
}

/// Draw port pins for expanded containers (bridge pins).
pub(crate) fn draw_port_pins(
    painter: &egui::Painter,
    layout: &NodeLayout,
    node_id: Uuid,
    output_pins: &[&PinInfo],
    zoom: f32,
    pin_screens: &mut Vec<PinScreen>,
) {
    let port_y = layout.node_rect.max.y - 16.0 * zoom;
    let port_r = layout.pin_r * 0.8;
    let port_col = Color32::from_rgb(200, 200, 100);

    for pin in output_pins {
        let p = Pos2::new(
            layout.screen_pos.x + layout.node_w - layout.pin_margin - 16.0 * zoom,
            port_y,
        );
        painter.circle_filled(p, port_r, port_col);
        painter.text(
            p + Vec2::new(-port_r - 3.0 * zoom, 0.0),
            egui::Align2::RIGHT_CENTER,
            &format!("{} \u{2192}", pin.display_name),
            egui::FontId::proportional(9.0 * zoom),
            Color32::from_rgb(200, 200, 100),
        );
        pin_screens.push(PinScreen {
            pos: p,
            node_id,
            name: pin.name.clone(),
            is_output: false,
        });
    }
}

/// Dim a color by a factor (1.0 = no change, 0.0 = black).
pub(crate) fn dim_color(color: Color32, factor: f32) -> Color32 {
    if factor >= 1.0 {
        return color;
    }
    Color32::from_rgba_unmultiplied(
        (color.r() as f32 * factor) as u8,
        (color.g() as f32 * factor) as u8,
        (color.b() as f32 * factor) as u8,
        color.a(),
    )
}
