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
    let border_color = Color32::from_rgb(70, 70, 80);

    if is_container {
        // Containers: border-only (no background fill), just header + stroke
        painter.rect_stroke(
            layout.node_rect,
            layout.rounding,
            Stroke::new(
                if is_selected { 2.0 * zoom } else { 1.0 * zoom },
                dim_color(
                    if is_selected {
                        theme.selection_color
                    } else {
                        border_color
                    },
                    dim,
                ),
            ),
            StrokeKind::Outside,
        );
    } else {
        // Regular nodes: filled body + outline
        let body_color = dim_color(
            if is_selected {
                theme.node_body_selected_color
            } else {
                theme.node_body_color
            },
            dim,
        );
        painter.rect_filled(layout.node_rect, layout.rounding, body_color);

        // Always draw outline for better visibility
        painter.rect_stroke(
            layout.node_rect,
            layout.rounding,
            Stroke::new(
                if is_selected { 2.0 * zoom } else { 1.0 * zoom },
                dim_color(
                    if is_selected {
                        theme.selection_color
                    } else {
                        border_color
                    },
                    dim,
                ),
            ),
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
            sw: if is_container { 0 } else { 0 },
            se: if is_container { 0 } else { 0 },
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
///
/// For containers (`is_container = true`), paired input/output pins (e.g. `image_in`/`image_out`)
/// are rendered at the same position on the right edge, acting as a single bidirectional port.
pub(crate) fn draw_pins(
    painter: &egui::Painter,
    layout: &NodeLayout,
    theme: &NodeEditorTheme,
    _node_type_id: &str,
    node_id: Uuid,
    input_pins: &[&PinInfo],
    output_pins: &[&PinInfo],
    is_container: bool,
    is_active: bool,
    zoom: f32,
    pin_screens: &mut Vec<PinScreen>,
    container_id: Option<Uuid>,
) {
    let dim = if is_active { 1.0 } else { 0.4 };
    let label_color = dim_color(theme.pin_label_color, dim);

    if is_container {
        // Container mode: pair input/output pins by base name and draw at same position.
        // Output pins define the rows; matching input pins share the position.
        for (i, out_pin) in output_pins.iter().enumerate() {
            let cy = layout.pin_start_y + i as f32 * layout.pin_row_h + layout.pin_row_h / 2.0;
            let cx = layout.screen_pos.x + layout.node_w - layout.pin_margin;
            let p = Pos2::new(cx, cy);
            let pin_color = dim_color((theme.pin_type_color)(&out_pin.data_type), dim);
            painter.circle_filled(p, layout.pin_r, pin_color);
            painter.text(
                p + Vec2::new(-layout.pin_r - 4.0 * zoom, 0.0),
                egui::Align2::RIGHT_CENTER,
                &out_pin.display_name,
                egui::FontId::proportional(10.0 * zoom),
                label_color,
            );

            // Register output PinScreen
            pin_screens.push(PinScreen {
                pos: p,
                node_id,
                name: out_pin.name.clone(),
                is_output: true,
                data_type: out_pin.data_type.clone(),
                container_id,
            });

            // Find matching input pin (e.g. "image_out" -> "image_in")
            let base = out_pin.name.trim_end_matches("_out");
            let in_name = format!("{}_in", base);
            if let Some(in_pin) = input_pins.iter().find(|p| p.name == in_name) {
                pin_screens.push(PinScreen {
                    pos: p,
                    node_id,
                    name: in_pin.name.clone(),
                    is_output: false,
                    data_type: in_pin.data_type.clone(),
                    container_id,
                });
            }
        }
    } else {
        // Regular node mode: inputs on left, outputs on right.
        for (i, pin) in input_pins.iter().enumerate() {
            let cy = layout.pin_start_y + i as f32 * layout.pin_row_h + layout.pin_row_h / 2.0;
            let cx = layout.screen_pos.x + layout.pin_margin;
            let p = Pos2::new(cx, cy);
            let pin_color = dim_color((theme.pin_type_color)(&pin.data_type), dim);
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
                data_type: pin.data_type.clone(),
                container_id,
            });
        }

        for (i, pin) in output_pins.iter().enumerate() {
            let cy = layout.pin_start_y + i as f32 * layout.pin_row_h + layout.pin_row_h / 2.0;
            let cx = layout.screen_pos.x + layout.node_w - layout.pin_margin;
            let p = Pos2::new(cx, cy);
            let pin_color = dim_color((theme.pin_type_color)(&pin.data_type), dim);
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
                data_type: pin.data_type.clone(),
                container_id,
            });
        }
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
///
/// Port pins are internal connection points inside expanded containers.
/// They are drawn at the same Y position as the external output pins
/// (merged position), with a diamond shape for visual distinction.
pub(crate) fn draw_port_pins(
    painter: &egui::Painter,
    layout: &NodeLayout,
    node_id: Uuid,
    output_pins: &[&PinInfo],
    zoom: f32,
    pin_screens: &mut Vec<PinScreen>,
    container_id: Option<Uuid>,
) {
    let port_r = layout.pin_r * 0.7;
    let port_col = Color32::from_rgb(200, 200, 100);
    let inset = 16.0 * zoom; // offset inward from the external pin

    for (i, pin) in output_pins.iter().enumerate() {
        // Same Y as external output pin (matching draw_pins layout)
        let cy = layout.pin_start_y + i as f32 * layout.pin_row_h + layout.pin_row_h / 2.0;
        let cx = layout.screen_pos.x + layout.node_w - layout.pin_margin - inset;
        let p = Pos2::new(cx, cy);

        // Draw diamond shape instead of circle for visual distinction
        let d = port_r;
        let diamond_pts = vec![
            Pos2::new(p.x, p.y - d),
            Pos2::new(p.x + d, p.y),
            Pos2::new(p.x, p.y + d),
            Pos2::new(p.x - d, p.y),
        ];
        painter.add(egui::Shape::convex_polygon(
            diamond_pts,
            port_col,
            Stroke::NONE,
        ));

        pin_screens.push(PinScreen {
            pos: p,
            node_id,
            name: pin.name.clone(),
            is_output: false,
            data_type: pin.data_type.clone(),
            container_id,
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
