use eframe::egui::{self, Color32};
use egui_phosphor::regular as icons;
use egui_snarl::ui::{PinInfo, WireStyle};
use library::model::project::{PortDataType, PortDirection, PortOwner, PortSide};
use library::model::{
    AssetKind, DataContent, GeneratorContent, Node, NodeContent, PathOperationContent, Project,
    ValueContent,
};
use node_editor_ui::{Editor, GroupChrome, NodePalette};
use uuid::Uuid;

use crate::ui::panels::node_editor::{
    CONTAINER_HEADER_HEIGHT, CONTAINER_PORT_Y, CONTAINER_RIGHT_PORT_ROW_HEIGHT,
    CONTAINER_RIGHT_PORT_Y, ContainerKind, ContainerVisual, EMBEDDED_PORT_LABEL_INSET,
    PORT_ROW_HEIGHT, canonical_pin_definitions, node_editor_details_visible,
    screen_stroke_in_graph_units,
};

pub(in crate::ui::panels::node_editor) const VALUE_NODE_CATEGORY_LABEL: &str = "Value";

const CONTAINER_SELECTED_OUTLINE: Color32 = Color32::from_rgb(102, 190, 255);
const CONTAINER_SELECTED_OUTLINE_SCREEN_WIDTH: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::ui::panels::node_editor) struct ContainerVisualStyle {
    pub(in crate::ui::panels::node_editor) body_fill: Color32,
    pub(in crate::ui::panels::node_editor) header_fill: Color32,
    pub(in crate::ui::panels::node_editor) outline: egui::Stroke,
    pub(in crate::ui::panels::node_editor) divider: egui::Stroke,
    pub(in crate::ui::panels::node_editor) highlight_state: &'static str,
    pub(in crate::ui::panels::node_editor) highlight_screen_width: f32,
}

/// One semantic glyph from the bundled Phosphor font plus its plain-language
/// meaning. Keeping both together prevents visual chrome from falling back to
/// arbitrary Unicode symbols and gives every glyph an accessible tooltip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) struct NodeEditorIcon {
    pub(in crate::ui::panels::node_editor) glyph: &'static str,
    pub(in crate::ui::panels::node_editor) label: &'static str,
}

impl NodeEditorIcon {
    const fn new(glyph: &'static str, label: &'static str) -> Self {
        Self { glyph, label }
    }
}

pub(in crate::ui::panels::node_editor) fn value_operation_label(
    value: library::model::ValueContent,
) -> &'static str {
    value.label()
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
        Some(NodeContent::CompositionInstance(_)) => NodePalette {
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
        Some(NodeContent::Color(_)) => NodePalette {
            body: Color32::from_rgb(48, 36, 28),
            header: Color32::from_rgb(105, 67, 37),
            accent: Color32::from_rgb(238, 159, 88),
        },
        Some(NodeContent::Data(_)) => NodePalette {
            body: Color32::from_rgb(41, 31, 48),
            header: Color32::from_rgb(84, 48, 105),
            accent: Color32::from_rgb(202, 123, 232),
        },
        Some(NodeContent::List(_)) => NodePalette {
            body: Color32::from_rgb(27, 43, 38),
            header: Color32::from_rgb(38, 88, 70),
            accent: Color32::from_rgb(87, 207, 158),
        },
        Some(NodeContent::Path(_)) => NodePalette {
            body: Color32::from_rgb(35, 31, 48),
            header: Color32::from_rgb(73, 49, 105),
            accent: Color32::from_rgb(182, 129, 232),
        },
        Some(NodeContent::NativeOperation(_)) => NodePalette {
            body: Color32::from_rgb(48, 38, 29),
            header: Color32::from_rgb(106, 72, 38),
            accent: Color32::from_rgb(238, 170, 92),
        },
        Some(NodeContent::Merge) | None => NodePalette {
            body: Color32::from_rgb(38, 39, 43),
            header: Color32::from_rgb(68, 70, 79),
            accent: Color32::from_rgb(177, 182, 198),
        },
        Some(NodeContent::SoundMerge) => NodePalette {
            body: Color32::from_rgb(38, 33, 48),
            header: Color32::from_rgb(76, 55, 104),
            accent: Color32::from_rgb(190, 145, 229),
        },
        Some(NodeContent::SoundAnalysis(_)) => NodePalette {
            body: Color32::from_rgb(27, 42, 46),
            header: Color32::from_rgb(39, 87, 96),
            accent: Color32::from_rgb(88, 207, 220),
        },
    }
}

pub(in crate::ui::panels::node_editor) fn node_icon(
    project: &Project,
    node_id: Uuid,
) -> NodeEditorIcon {
    match project.get_node(node_id).map(Node::content) {
        Some(NodeContent::Generator(GeneratorContent::Text)) => {
            NodeEditorIcon::new(icons::TEXT_T, "Text generator")
        }
        Some(NodeContent::Generator(GeneratorContent::Shape)) => {
            NodeEditorIcon::new(icons::POLYGON, "Shape generator")
        }
        Some(NodeContent::Generator(GeneratorContent::Solid)) => {
            NodeEditorIcon::new(icons::SQUARE, "Solid generator")
        }
        Some(NodeContent::Generator(GeneratorContent::SkSL)) => {
            NodeEditorIcon::new(icons::FUNCTION, "SkSL generator")
        }
        Some(NodeContent::Media(media)) => {
            match project.get_asset(media.asset_id).map(|asset| &asset.kind) {
                Some(AssetKind::Video) => NodeEditorIcon::new(icons::FILE_VIDEO, "Video asset"),
                Some(AssetKind::Audio) => NodeEditorIcon::new(icons::FILE_AUDIO, "Audio asset"),
                Some(AssetKind::Image) => NodeEditorIcon::new(icons::FILE_IMAGE, "Image asset"),
                Some(AssetKind::Model3D) => NodeEditorIcon::new(icons::CUBE, "3D asset"),
                Some(AssetKind::Other) | None => NodeEditorIcon::new(icons::FILE, "Media asset"),
            }
        }
        Some(NodeContent::CompositionInstance(_)) => {
            NodeEditorIcon::new(icons::ARROW_SQUARE_OUT, "Composition Instance")
        }
        Some(NodeContent::PluginOperation(operation)) => match operation.category.as_str() {
            "style" => NodeEditorIcon::new(icons::PALETTE, "Style operation"),
            "effect" => NodeEditorIcon::new(icons::SPARKLE, "Effect operation"),
            "effector" => NodeEditorIcon::new(icons::LIGHTNING, "Effector operation"),
            "path_effect" => NodeEditorIcon::new(icons::WAVE_SINE, "Path Effect operation"),
            "decorator" => NodeEditorIcon::new(icons::MAGIC_WAND, "Decorator operation"),
            _ => NodeEditorIcon::new(icons::PLUG, "Plugin operation"),
        },
        Some(NodeContent::Value(value)) => match value {
            ValueContent::Fmod => NodeEditorIcon::new(icons::PERCENT, "Fmod value operation"),
            ValueContent::Add => NodeEditorIcon::new(icons::PLUS, "Add value operation"),
            ValueContent::Subtract => NodeEditorIcon::new(icons::MINUS, "Subtract value operation"),
            ValueContent::Multiply => NodeEditorIcon::new(icons::X, "Multiply value operation"),
            ValueContent::Divide => NodeEditorIcon::new(icons::DIVIDE, "Divide value operation"),
        },
        Some(NodeContent::Color(_)) => {
            NodeEditorIcon::new(icons::PALETTE, "Lossless Color operation")
        }
        Some(NodeContent::Data(DataContent::Color)) => {
            NodeEditorIcon::new(icons::PALETTE, "Canonical color value")
        }
        Some(NodeContent::Data(DataContent::Path)) => {
            NodeEditorIcon::new(icons::WAVE_SINE, "Canonical path value")
        }
        Some(NodeContent::List(_)) => {
            NodeEditorIcon::new(icons::LIST_NUMBERS, "Ordered List operation")
        }
        Some(NodeContent::Path(PathOperationContent::Union)) => {
            NodeEditorIcon::new(icons::UNION, "Boolean Path union")
        }
        Some(NodeContent::NativeOperation(operation)) => {
            let descriptor = library::model::native_node_descriptor(&operation.catalog_id);
            match descriptor.map(|item| item.category()) {
                Some("3D") => NodeEditorIcon::new(icons::CUBE, "3D design placeholder"),
                Some("Particles") => {
                    NodeEditorIcon::new(icons::SPARKLE, "Particle design placeholder")
                }
                _ => NodeEditorIcon::new(icons::WARNING, "Native design placeholder"),
            }
        }
        Some(NodeContent::Merge) => NodeEditorIcon::new(icons::ARROWS_MERGE, "Merge operation"),
        Some(NodeContent::SoundMerge) => {
            NodeEditorIcon::new(icons::WAVEFORM, "Sound Merge operation")
        }
        Some(NodeContent::SoundAnalysis(_)) => {
            NodeEditorIcon::new(icons::WAVE_SINE, "Sound analysis operation")
        }
        None => NodeEditorIcon::new(icons::QUESTION, "Missing node"),
    }
}

pub(in crate::ui::panels::node_editor) fn container_icon(owner: PortOwner) -> NodeEditorIcon {
    match owner {
        PortOwner::Composition(_) => {
            NodeEditorIcon::new(icons::PROJECTOR_SCREEN, "Composition container")
        }
        PortOwner::Track(_) => NodeEditorIcon::new(icons::STACK, "Track container"),
        PortOwner::Clip(_) => NodeEditorIcon::new(icons::FILM_STRIP, "Clip container"),
        PortOwner::Node(_) => NodeEditorIcon::new(icons::CIRCLE, "Node"),
    }
}

pub(in crate::ui::panels::node_editor) fn paint_container_backdrop(
    painter: &egui::Painter,
    container: &ContainerVisual,
    inactive: bool,
    selected: bool,
    scale: f32,
) {
    let rect = container.rect();
    let style = container_visual_style(container.kind, inactive, selected, scale);
    Editor::paint_group_backdrop(
        painter,
        rect,
        GroupChrome {
            body_fill: style.body_fill,
            header_fill: style.header_fill,
            outline: egui::Stroke::NONE,
            divider: egui::Stroke::NONE,
            header_height: CONTAINER_HEADER_HEIGHT,
            corner_radius: 8,
            details_visible: false,
        },
    );
}

pub(in crate::ui::panels::node_editor) fn paint_container_foreground(
    painter: &egui::Painter,
    project: &Project,
    container: &ContainerVisual,
    inactive: bool,
    selected: bool,
    scale: f32,
) {
    let rect = container.rect();
    let detailed = node_editor_details_visible(scale);
    let style = container_visual_style(container.kind, inactive, selected, scale);
    Editor::paint_group_foreground(
        painter,
        rect,
        GroupChrome {
            body_fill: Color32::TRANSPARENT,
            header_fill: Color32::TRANSPARENT,
            outline: style.outline,
            divider: style.divider,
            header_height: CONTAINER_HEADER_HEIGHT,
            corner_radius: 8,
            details_visible: detailed,
        },
    );
    if !detailed {
        return;
    }
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
    }
    for (index, definition) in canonical_pin_definitions(
        project,
        container.owner,
        PortDirection::Output,
        PortSide::Right,
    )
    .iter()
    .enumerate()
    {
        painter.text(
            egui::pos2(
                rect.right() - 42.0,
                rect.top()
                    + CONTAINER_RIGHT_PORT_Y
                    + index as f32 * CONTAINER_RIGHT_PORT_ROW_HEIGHT,
            ),
            egui::Align2::RIGHT_CENTER,
            definition.name.to_uppercase(),
            egui::FontId::proportional(10.0),
            pin_color(definition.data_type).gamma_multiply(if inactive { 0.45 } else { 0.9 }),
        );
    }

    if let PortOwner::Clip(clip_id) = container.owner {
        if let Some(clip) = project.get_clip(clip_id) {
            painter.text(
                rect.right_top() + egui::vec2(-12.0, 35.0),
                egui::Align2::RIGHT_TOP,
                format!(
                    "{:.2}s  ·  {:.2}s  ·  x{:.2}",
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

pub(in crate::ui::panels::node_editor) fn container_visual_style(
    kind: ContainerKind,
    inactive: bool,
    selected: bool,
    scale: f32,
) -> ContainerVisualStyle {
    let detailed = node_editor_details_visible(scale);
    let mut body_fill = match kind {
        ContainerKind::Composition => Color32::from_rgba_premultiplied(25, 43, 67, 70),
        ContainerKind::Track => Color32::from_rgba_premultiplied(48, 43, 61, 64),
        ContainerKind::Clip => Color32::from_rgba_premultiplied(38, 60, 47, 66),
    };
    let mut header_fill = match kind {
        ContainerKind::Composition => Color32::from_rgba_premultiplied(38, 66, 100, 220),
        ContainerKind::Track => Color32::from_rgba_premultiplied(73, 61, 91, 220),
        ContainerKind::Clip => Color32::from_rgba_premultiplied(52, 88, 64, 220),
    };
    let (normal_screen_width, accent) = match kind {
        ContainerKind::Composition => (
            if detailed { 2.0 } else { 1.4 },
            Color32::from_rgb(74, 137, 207),
        ),
        ContainerKind::Track => (
            if detailed { 1.5 } else { 1.15 },
            Color32::from_rgb(143, 116, 196),
        ),
        ContainerKind::Clip => (
            if detailed { 1.5 } else { 1.15 },
            Color32::from_rgb(95, 174, 121),
        ),
    };
    if inactive {
        body_fill = body_fill.gamma_multiply(0.35);
        header_fill = header_fill.gamma_multiply(0.42);
    }

    let (outline, highlight_state, highlight_screen_width) = if selected {
        header_fill = mix_color(header_fill, CONTAINER_SELECTED_OUTLINE, 0.48);
        (
            egui::Stroke::new(
                screen_stroke_in_graph_units(CONTAINER_SELECTED_OUTLINE_SCREEN_WIDTH, scale),
                CONTAINER_SELECTED_OUTLINE,
            ),
            "selected",
            CONTAINER_SELECTED_OUTLINE_SCREEN_WIDTH,
        )
    } else {
        let color = if inactive {
            accent.gamma_multiply(0.5)
        } else {
            accent
        };
        let width = if detailed {
            normal_screen_width
        } else {
            screen_stroke_in_graph_units(normal_screen_width, scale)
        };
        (
            egui::Stroke::new(width, color),
            "none",
            width * scale.max(f32::EPSILON),
        )
    };
    ContainerVisualStyle {
        body_fill,
        header_fill,
        outline,
        divider: egui::Stroke::new(1.0, outline.color.gamma_multiply(0.82)),
        highlight_state,
        highlight_screen_width,
    }
}

pub(in crate::ui::panels::node_editor) fn container_highlight_metadata(
    style: ContainerVisualStyle,
) -> serde_json::Value {
    serde_json::json!({
        "state": style.highlight_state,
        "outer_stroke": {
            "color": [
                style.outline.color.r(),
                style.outline.color.g(),
                style.outline.color.b(),
                style.outline.color.a(),
            ],
            "width_graph": style.outline.width,
            "width_screen": style.highlight_screen_width,
        },
        "header_fill": [
            style.header_fill.r(),
            style.header_fill.g(),
            style.header_fill.b(),
            style.header_fill.a(),
        ],
    })
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

pub(in crate::ui::panels::node_editor) fn pin_color(data_type: PortDataType) -> Color32 {
    match data_type {
        PortDataType::Image => Color32::from_rgb(238, 207, 109),
        PortDataType::Shape => Color32::from_rgb(142, 132, 246),
        PortDataType::Audio => Color32::from_rgb(100, 200, 100),
        PortDataType::Spectrum => Color32::from_rgb(88, 207, 220),
        PortDataType::String => Color32::from_rgb(100, 220, 220),
        PortDataType::Path => Color32::from_rgb(100, 150, 255),
        PortDataType::Numeric | PortDataType::Number | PortDataType::Integer => {
            Color32::from_rgb(255, 100, 100)
        }
        PortDataType::Color => Color32::from_rgb(220, 120, 220),
        PortDataType::Vec2 => Color32::from_rgb(120, 170, 255),
        PortDataType::Vec3 => Color32::from_rgb(105, 195, 235),
        PortDataType::Vec4 => Color32::from_rgb(145, 145, 245),
        PortDataType::Boolean => Color32::from_rgb(220, 160, 100),
        PortDataType::Enum => Color32::from_rgb(225, 154, 91),
        PortDataType::Asset | PortDataType::Material => Color32::from_rgb(105, 145, 180),
        PortDataType::Gradient | PortDataType::Curve => Color32::from_rgb(205, 120, 205),
        PortDataType::ParticleSystem => Color32::from_rgb(105, 205, 145),
        PortDataType::Geometry3D
        | PortDataType::Object3D
        | PortDataType::Object3DList
        | PortDataType::Camera3D
        | PortDataType::PointSource
        | PortDataType::Instance3D => Color32::from_rgb(105, 165, 225),
        PortDataType::Effector3D
        | PortDataType::EffectorStack
        | PortDataType::Field3D
        | PortDataType::FieldStack => Color32::from_rgb(185, 125, 225),
        PortDataType::MotionBehavior => Color32::from_rgb(235, 145, 105),
        PortDataType::Any => Color32::from_rgb(200, 200, 200),
        PortDataType::List => Color32::from_rgb(87, 207, 158),
    }
}

pub(in crate::ui::panels::node_editor) fn pin_info(
    data_type: PortDataType,
    connected: bool,
) -> PinInfo {
    let color = pin_color(data_type);
    let visual = Editor::port_visual_style(color, connected);
    PinInfo::circle()
        .with_fill(visual.fill)
        .with_stroke(visual.stroke)
        .with_wire_color(visual.wire_color)
        .with_wire_style(WireStyle::Bezier3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_container_kind_has_selected_header_and_screen_stable_outline() {
        for kind in [
            ContainerKind::Composition,
            ContainerKind::Track,
            ContainerKind::Clip,
        ] {
            for scale in [0.0065, 0.18, 1.0] {
                let normal = container_visual_style(kind, false, false, scale);
                let selected = container_visual_style(kind, false, true, scale);
                assert_eq!(selected.highlight_state, "selected");
                assert_ne!(selected.header_fill, normal.header_fill);
                assert_ne!(selected.outline, normal.outline);
                assert!(
                    (selected.outline.width * scale - CONTAINER_SELECTED_OUTLINE_SCREEN_WIDTH)
                        .abs()
                        < 0.001
                );
            }
        }
    }

    #[test]
    fn selected_inactive_container_keeps_inactive_body_semantics() {
        let inactive = container_visual_style(ContainerKind::Clip, true, false, 1.0);
        let selected = container_visual_style(ContainerKind::Clip, true, true, 1.0);
        assert_eq!(selected.body_fill, inactive.body_fill);
        assert_ne!(selected.header_fill, inactive.header_fill);
    }
}
