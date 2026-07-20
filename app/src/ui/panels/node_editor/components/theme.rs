use eframe::egui::{self, Color32};
use egui_snarl::ui::{PinInfo, WireStyle};
use library::model::project::{PortDataType, PortDirection, PortOwner, PortSide};
use library::model::{GeneratorContent, Node, NodeContent, Project};
use uuid::Uuid;

use crate::ui::panels::node_editor::{
    canonical_pin_definitions, node_editor_details_visible, screen_stroke_in_graph_units,
    ContainerKind, ContainerVisual, CONTAINER_HEADER_HEIGHT, CONTAINER_PORT_Y,
    EMBEDDED_PORT_LABEL_INSET, PORT_ROW_HEIGHT,
};

#[derive(Clone, Copy)]
pub(in crate::ui::panels::node_editor) struct NodePalette {
    pub(in crate::ui::panels::node_editor) body: Color32,
    pub(in crate::ui::panels::node_editor) header: Color32,
    pub(in crate::ui::panels::node_editor) accent: Color32,
}

pub(in crate::ui::panels::node_editor) const VALUE_NODE_CATEGORY_LABEL: &str = "Value";

pub(in crate::ui::panels::node_editor) fn value_operation_label(
    value: library::model::ValueContent,
) -> &'static str {
    match value {
        library::model::ValueContent::TimeModulo => "Time Modulo",
    }
}

pub(in crate::ui::panels::node_editor) fn node_palette(
    project: &Project,
    node_id: Uuid,
) -> NodePalette {
    match project.get_node(node_id).map(Node::content) {
        Some(NodeContent::Generator(GeneratorContent::Text)) => NodePalette {
            body: Color32::from_rgb(32, 36, 48),
            header: Color32::from_rgb(77, 57, 112),
            accent: Color32::from_rgb(190, 146, 245),
        },
        Some(NodeContent::Generator(GeneratorContent::Shape)) => NodePalette {
            body: Color32::from_rgb(30, 41, 42),
            header: Color32::from_rgb(42, 91, 82),
            accent: Color32::from_rgb(94, 213, 172),
        },
        Some(NodeContent::Generator(GeneratorContent::Solid)) => NodePalette {
            body: Color32::from_rgb(42, 38, 31),
            header: Color32::from_rgb(103, 76, 38),
            accent: Color32::from_rgb(238, 190, 89),
        },
        Some(NodeContent::Generator(GeneratorContent::SkSL)) => NodePalette {
            body: Color32::from_rgb(28, 41, 48),
            header: Color32::from_rgb(38, 86, 105),
            accent: Color32::from_rgb(92, 199, 226),
        },
        Some(NodeContent::Media(_)) => NodePalette {
            body: Color32::from_rgb(32, 39, 50),
            header: Color32::from_rgb(45, 77, 117),
            accent: Color32::from_rgb(100, 170, 243),
        },
        Some(NodeContent::Reference(_)) => NodePalette {
            body: Color32::from_rgb(38, 36, 49),
            header: Color32::from_rgb(74, 63, 111),
            accent: Color32::from_rgb(162, 139, 232),
        },
        Some(NodeContent::PluginOperation(_)) => NodePalette {
            body: Color32::from_rgb(42, 34, 49),
            header: Color32::from_rgb(91, 54, 112),
            accent: Color32::from_rgb(205, 139, 232),
        },
        Some(NodeContent::Value(_)) => NodePalette {
            body: Color32::from_rgb(28, 41, 46),
            header: Color32::from_rgb(39, 83, 95),
            accent: Color32::from_rgb(91, 197, 218),
        },
        Some(NodeContent::Merge) | None => NodePalette {
            body: Color32::from_rgb(38, 39, 43),
            header: Color32::from_rgb(68, 70, 79),
            accent: Color32::from_rgb(177, 182, 198),
        },
    }
}

pub(in crate::ui::panels::node_editor) fn node_icon(
    project: &Project,
    node_id: Uuid,
) -> &'static str {
    match project.get_node(node_id).map(Node::content) {
        Some(NodeContent::Generator(GeneratorContent::Text)) => "T",
        Some(NodeContent::Generator(GeneratorContent::Shape)) => "◇",
        Some(NodeContent::Generator(GeneratorContent::Solid)) => "■",
        Some(NodeContent::Generator(GeneratorContent::SkSL)) => "ƒ",
        Some(NodeContent::Media(_)) => "▶",
        Some(NodeContent::Reference(_)) => "↗",
        Some(NodeContent::PluginOperation(operation)) => match operation.category.as_str() {
            "style" => "◐",
            "effect" => "✦",
            "effector" => "↯",
            "decorator" => "⌁",
            _ => "P",
        },
        Some(NodeContent::Value(_)) => "%",
        Some(NodeContent::Merge) => "⋈",
        None => "?",
    }
}

pub(in crate::ui::panels::node_editor) fn container_icon(owner: PortOwner) -> &'static str {
    match owner {
        PortOwner::Composition(_) => "◉",
        PortOwner::Track(_) => "≡",
        PortOwner::Clip(_) => "▱",
        PortOwner::Node(_) => "●",
    }
}

pub(in crate::ui::panels::node_editor) fn paint_container_backdrop(
    painter: &egui::Painter,
    container: &ContainerVisual,
    inactive: bool,
) {
    let rect = container.rect();
    let mut fill = match container.kind {
        ContainerKind::Composition => Color32::from_rgba_premultiplied(25, 43, 67, 70),
        ContainerKind::Track => Color32::from_rgba_premultiplied(48, 43, 61, 64),
        ContainerKind::Clip => Color32::from_rgba_premultiplied(38, 60, 47, 66),
    };
    if inactive {
        fill = fill.gamma_multiply(0.35);
    }
    let radius = egui::CornerRadius::same(8);
    painter.rect_filled(rect, radius, fill);
    let mut header_fill = match container.kind {
        ContainerKind::Composition => Color32::from_rgba_premultiplied(38, 66, 100, 220),
        ContainerKind::Track => Color32::from_rgba_premultiplied(73, 61, 91, 220),
        ContainerKind::Clip => Color32::from_rgba_premultiplied(52, 88, 64, 220),
    };
    if inactive {
        header_fill = header_fill.gamma_multiply(0.42);
    }
    let header = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(rect.width(), CONTAINER_HEADER_HEIGHT.min(rect.height())),
    );
    painter.rect_filled(
        header,
        egui::CornerRadius {
            nw: 8,
            ne: 8,
            sw: 2,
            se: 2,
        },
        header_fill,
    );
}

pub(in crate::ui::panels::node_editor) fn paint_container_foreground(
    painter: &egui::Painter,
    project: &Project,
    container: &ContainerVisual,
    inactive: bool,
    scale: f32,
) {
    let rect = container.rect();
    let detailed = node_editor_details_visible(scale);
    let mut stroke = match container.kind {
        ContainerKind::Composition => egui::Stroke::new(
            if detailed {
                2.0
            } else {
                screen_stroke_in_graph_units(1.4, scale)
            },
            Color32::from_rgb(74, 137, 207),
        ),
        ContainerKind::Track => egui::Stroke::new(
            if detailed {
                1.5
            } else {
                screen_stroke_in_graph_units(1.15, scale)
            },
            Color32::from_rgb(143, 116, 196),
        ),
        ContainerKind::Clip => egui::Stroke::new(
            if detailed {
                1.5
            } else {
                screen_stroke_in_graph_units(1.15, scale)
            },
            Color32::from_rgb(95, 174, 121),
        ),
    };
    if inactive {
        stroke.color = stroke.color.gamma_multiply(0.5);
    }
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        stroke,
        egui::StrokeKind::Inside,
    );
    if !detailed {
        return;
    }
    let header_bottom = rect.top() + CONTAINER_HEADER_HEIGHT.min(rect.height());
    painter.line_segment(
        [
            egui::pos2(rect.left(), header_bottom),
            egui::pos2(rect.right(), header_bottom),
        ],
        egui::Stroke::new(1.0, stroke.color.gamma_multiply(0.82)),
    );
    painter.text(
        rect.right_top() + egui::vec2(-12.0, 10.0),
        egui::Align2::RIGHT_TOP,
        match container.kind {
            ContainerKind::Composition => "COMPOSITION",
            ContainerKind::Track => "TRACK",
            ContainerKind::Clip => "CLIP",
        },
        egui::FontId::proportional(11.0),
        Color32::from_white_alpha(155),
    );

    if !container.collapsed {
        for (index, definition) in canonical_pin_definitions(
            project,
            container.owner,
            PortDirection::Output,
            PortSide::Left,
        )
        .iter()
        .enumerate()
        {
            painter.text(
                egui::pos2(
                    rect.left() + EMBEDDED_PORT_LABEL_INSET,
                    rect.top() + CONTAINER_PORT_Y + index as f32 * PORT_ROW_HEIGHT,
                ),
                egui::Align2::LEFT_CENTER,
                &definition.name,
                egui::FontId::proportional(11.0),
                pin_color(definition.data_type).gamma_multiply(if inactive { 0.45 } else { 0.9 }),
            );
        }
        painter.text(
            egui::pos2(
                rect.right() - EMBEDDED_PORT_LABEL_INSET,
                rect.top() + CONTAINER_HEADER_HEIGHT * 0.5,
            ),
            egui::Align2::RIGHT_CENTER,
            "IMAGE OUT",
            egui::FontId::proportional(10.0),
            pin_color(PortDataType::Image).gamma_multiply(if inactive { 0.45 } else { 0.9 }),
        );
    }

    if let PortOwner::Clip(clip_id) = container.owner {
        if let Some(clip) = project.get_clip(clip_id) {
            painter.text(
                rect.right_top() + egui::vec2(-12.0, 35.0),
                egui::Align2::RIGHT_TOP,
                format!(
                    "{:.2}s  ·  {:.2}s  ·  ×{:.2}",
                    clip.start_time.into_inner(),
                    clip.duration.into_inner(),
                    clip.time_stretch.into_inner()
                ),
                egui::FontId::proportional(10.0),
                Color32::from_white_alpha(if inactive { 75 } else { 135 }),
            );
        }
    }
}

pub(in crate::ui::panels::node_editor) fn pin_color(data_type: PortDataType) -> Color32 {
    match data_type {
        PortDataType::Image => Color32::from_rgb(238, 207, 109),
        PortDataType::Shape => Color32::from_rgb(142, 132, 246),
        PortDataType::Audio => Color32::from_rgb(100, 200, 100),
        PortDataType::String => Color32::from_rgb(100, 220, 220),
        PortDataType::Path => Color32::from_rgb(100, 150, 255),
        PortDataType::Number | PortDataType::Integer => Color32::from_rgb(255, 100, 100),
        PortDataType::Color => Color32::from_rgb(220, 120, 220),
        PortDataType::Vec2 => Color32::from_rgb(120, 170, 255),
        PortDataType::Vec3 => Color32::from_rgb(105, 195, 235),
        PortDataType::Vec4 => Color32::from_rgb(145, 145, 245),
        PortDataType::Boolean => Color32::from_rgb(220, 160, 100),
        PortDataType::Any => Color32::from_rgb(200, 200, 200),
    }
}

pub(in crate::ui::panels::node_editor) fn pin_info(
    data_type: PortDataType,
    connected: bool,
) -> PinInfo {
    let color = pin_color(data_type);
    let fill = if connected {
        color
    } else {
        color.gamma_multiply(0.32)
    };
    PinInfo::circle()
        .with_fill(fill)
        .with_stroke(egui::Stroke::new(if connected { 2.0 } else { 1.25 }, color))
        .with_wire_color(color)
        .with_wire_style(WireStyle::Bezier3)
}
