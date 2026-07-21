//! Domain-neutral visual policy shared by the standalone editor and host adapters.

use egui::{Color32, CornerRadius, Stroke, StrokeKind};

/// Host-selected colors for one Node category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodePalette {
    pub body: Color32,
    pub header: Color32,
    pub accent: Color32,
}

/// Resolved Node shell presentation for the current frame and zoom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeVisualStyle {
    pub body_fill: Color32,
    pub header_fill: Color32,
    pub outer_stroke: Stroke,
    pub highlight_state: &'static str,
    pub highlight_screen_width: f32,
}

/// An optional glyph in a Node header. The host owns the font and meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeaderGlyph<'a> {
    pub glyph: &'a str,
    pub tooltip: &'a str,
}

/// Borrowed content and layout policy for a Node header.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeHeader<'a> {
    pub title: &'a str,
    pub title_color: Option<Color32>,
    pub leading: Option<HeaderGlyph<'a>>,
    pub trailing: Option<HeaderGlyph<'a>>,
    pub accent: Color32,
    pub min_width: f32,
    pub title_width: f32,
    pub row_height: f32,
    pub details_visible: bool,
}

/// Colors and geometry for one group/container shell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupChrome {
    pub body_fill: Color32,
    pub header_fill: Color32,
    pub outline: Stroke,
    pub divider: Stroke,
    pub header_height: f32,
    pub corner_radius: u8,
    pub details_visible: bool,
}

/// Direction-independent socket colors used by generic and host layouts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PortVisualStyle {
    pub fill: Color32,
    pub stroke: Stroke,
    pub wire_color: Color32,
}

/// Borrowed text layout for a port row owned by an external layout engine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PortLabel<'a> {
    pub text: &'a str,
    pub width: f32,
    pub row_height: f32,
    pub align: egui::Align,
    pub details_visible: bool,
}

const SELECTED_OUTLINE: Color32 = Color32::from_rgb(102, 190, 255);
const SELECTED_OUTLINE_SCREEN_WIDTH: f32 = 3.0;

pub(crate) fn node_visual_style(
    palette: NodePalette,
    inactive: bool,
    selected: bool,
    scale: f32,
) -> NodeVisualStyle {
    let body_fill = if inactive {
        palette.body.gamma_multiply(0.42)
    } else {
        palette.body
    };
    let base_header = if inactive {
        palette.header.gamma_multiply(0.42)
    } else {
        palette.header
    };
    if selected {
        return NodeVisualStyle {
            body_fill,
            header_fill: mix_color(base_header, SELECTED_OUTLINE, 0.48),
            outer_stroke: Stroke::new(
                screen_stroke_width(SELECTED_OUTLINE_SCREEN_WIDTH, scale),
                SELECTED_OUTLINE,
            ),
            highlight_state: "selected",
            highlight_screen_width: SELECTED_OUTLINE_SCREEN_WIDTH,
        };
    }

    let stroke_color = if inactive {
        palette.accent.gamma_multiply(0.48)
    } else {
        palette.accent
    };
    let stroke_width = if scale.max(f32::EPSILON) >= 0.18 {
        1.25
    } else {
        screen_stroke_width(1.1, scale)
    };
    NodeVisualStyle {
        body_fill,
        header_fill: base_header,
        outer_stroke: Stroke::new(stroke_width, stroke_color),
        highlight_state: "none",
        highlight_screen_width: stroke_width * scale.max(f32::EPSILON),
    }
}

pub(crate) fn node_frame(style: NodeVisualStyle) -> egui::Frame {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(9, 8))
        .corner_radius(10)
        .fill(style.body_fill)
        .stroke(style.outer_stroke)
}

pub(crate) fn node_header_frame(style: NodeVisualStyle) -> egui::Frame {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(9, 7))
        .corner_radius(CornerRadius {
            nw: 9,
            ne: 9,
            sw: 3,
            se: 3,
        })
        .fill(style.header_fill)
}

pub(crate) fn show_node_header(ui: &mut egui::Ui, header: NodeHeader<'_>) -> egui::Response {
    ui.set_min_width(header.min_width);
    if !header.details_visible {
        return ui.allocate_response(
            egui::vec2(header.min_width, header.row_height),
            egui::Sense::hover(),
        );
    }

    ui.horizontal(|ui| {
        if let Some(icon) = header.leading {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(icon.glyph)
                        .color(header.accent)
                        .strong(),
                )
                .selectable(false),
            )
            .on_hover_text(icon.tooltip);
        }
        let mut title = egui::RichText::new(header.title).strong();
        if let Some(color) = header.title_color {
            title = title.color(color);
        }
        ui.add_sized(
            [header.title_width, header.row_height],
            egui::Label::new(title).selectable(false).truncate(),
        );
        if let Some(status) = header.trailing {
            ui.add(
                egui::Label::new(egui::RichText::new(status.glyph).color(header.accent))
                    .selectable(false),
            )
            .on_hover_text(status.tooltip);
        }
    })
    .response
}

pub(crate) fn show_port_label(ui: &mut egui::Ui, label: PortLabel<'_>) -> egui::Response {
    if !label.details_visible {
        return ui.allocate_response(
            egui::vec2(label.width, label.row_height),
            egui::Sense::hover(),
        );
    }
    ui.add_sized(
        [label.width, label.row_height],
        egui::Label::new(label.text)
            .selectable(false)
            .truncate()
            .halign(label.align),
    )
}

pub(crate) fn port_visual_style(color: Color32, connected: bool) -> PortVisualStyle {
    PortVisualStyle {
        fill: if connected {
            color
        } else {
            color.gamma_multiply(0.32)
        },
        stroke: Stroke::new(if connected { 2.0 } else { 1.25 }, color),
        wire_color: color,
    }
}

pub(crate) fn paint_group_backdrop(painter: &egui::Painter, rect: egui::Rect, chrome: GroupChrome) {
    let header = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(rect.width(), chrome.header_height.min(rect.height())),
    );
    painter.rect_filled(
        rect,
        CornerRadius::same(chrome.corner_radius),
        chrome.body_fill,
    );
    painter.rect_filled(
        header,
        CornerRadius {
            nw: chrome.corner_radius,
            ne: chrome.corner_radius,
            sw: 2,
            se: 2,
        },
        chrome.header_fill,
    );
}

pub(crate) fn paint_group_foreground(
    painter: &egui::Painter,
    rect: egui::Rect,
    chrome: GroupChrome,
) {
    painter.rect_stroke(
        rect,
        CornerRadius::same(chrome.corner_radius),
        chrome.outline,
        StrokeKind::Inside,
    );
    if chrome.details_visible {
        let y = rect.top() + chrome.header_height.min(rect.height());
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            chrome.divider,
        );
    }
}

fn screen_stroke_width(screen_width: f32, scale: f32) -> f32 {
    screen_width / scale.max(f32::EPSILON)
}

fn mix_color(base: Color32, tint: Color32, tint_weight: f32) -> Color32 {
    fn channel(base: u8, tint: u8, tint_weight: f32) -> u8 {
        (base as f32 * (1.0 - tint_weight) + tint as f32 * tint_weight)
            .round()
            .clamp(0.0, 255.0) as u8
    }
    Color32::from_rgba_premultiplied(
        channel(base.r(), tint.r(), tint_weight),
        channel(base.g(), tint.g(), tint_weight),
        channel(base.b(), tint.b(), tint_weight),
        channel(base.a(), tint.a(), tint_weight),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> NodePalette {
        NodePalette {
            body: Color32::from_rgb(30, 40, 50),
            header: Color32::from_rgb(60, 70, 80),
            accent: Color32::from_rgb(150, 160, 170),
        }
    }

    #[test]
    fn selected_outline_stays_three_screen_points_at_every_lod() {
        for scale in [0.0065, 0.18, 1.0, 1.25] {
            let selected = node_visual_style(palette(), false, true, scale);
            assert_eq!(selected.highlight_state, "selected");
            assert!((selected.highlight_screen_width - 3.0).abs() < f32::EPSILON);
            assert!((selected.outer_stroke.width * scale - 3.0).abs() < 0.001);
            assert_eq!(selected.outer_stroke.color, SELECTED_OUTLINE);
        }
    }

    #[test]
    fn selection_preserves_inactive_body_while_tinting_header_and_outline() {
        let inactive = node_visual_style(palette(), true, false, 1.0);
        let selected_inactive = node_visual_style(palette(), true, true, 1.0);
        assert_eq!(selected_inactive.body_fill, inactive.body_fill);
        assert_ne!(selected_inactive.header_fill, inactive.header_fill);
        assert_ne!(selected_inactive.outer_stroke, inactive.outer_stroke);
        assert_eq!(selected_inactive.highlight_state, "selected");
    }

    #[test]
    fn unconnected_port_keeps_type_colored_outline_and_dimmed_fill() {
        let color = Color32::from_rgb(100, 150, 200);
        let visual = port_visual_style(color, false);
        assert_eq!(visual.stroke.color, color);
        assert_eq!(visual.wire_color, color);
        assert_ne!(visual.fill, color);
    }
}
