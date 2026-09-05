//! Production Node colors, glyphs, sockets, and wires.

use eframe::egui::Color32;
use egui_phosphor::regular as icons;
use egui_snarl::ui::{PinInfo, WireStyle};
use library::model::project::PortDataType;
use library::model::{
    AssetKind, DataContent, GeneratorContent, Node, NodeContent, PathOperationContent, ValueContent,
};
use node_editor_ui::{Editor, NodePalette};
use uuid::Uuid;

/// One semantic glyph from the bundled Phosphor font plus its accessible name.
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

/// Resolve the established Node palette directly from a document Node.
///
/// The former Project graph asked a `Project` for this same Node before
/// applying the palette. A ModuleDefinition already owns the Node, so the
/// production style belongs at this lower boundary.
pub(in crate::ui::panels::node_editor) fn node_palette_for_node(
    node: Option<&Node>,
) -> NodePalette {
    match node.map(Node::content) {
        Some(NodeContent::ModuleOutput(_)) => NodePalette {
            body: Color32::from_rgb(45, 34, 37),
            header: Color32::from_rgb(103, 50, 61),
            accent: Color32::from_rgb(239, 119, 137),
        },
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

/// Resolve the established Node glyph without requiring a Project container.
pub(in crate::ui::panels::node_editor) fn node_icon_for_node<'a>(
    node: Option<&Node>,
    asset_kind: impl FnOnce(Uuid) -> Option<&'a AssetKind>,
) -> NodeEditorIcon {
    match node.map(Node::content) {
        Some(NodeContent::ModuleOutput(_)) => NodeEditorIcon::new(icons::EXPORT, "Module Output"),
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
        Some(NodeContent::Media(media)) => match asset_kind(media.asset_id) {
            Some(AssetKind::Video) => NodeEditorIcon::new(icons::FILE_VIDEO, "Video asset"),
            Some(AssetKind::Audio) => NodeEditorIcon::new(icons::FILE_AUDIO, "Audio asset"),
            Some(AssetKind::Image) => NodeEditorIcon::new(icons::FILE_IMAGE, "Image asset"),
            Some(AssetKind::Model3D) => NodeEditorIcon::new(icons::CUBE, "3D asset"),
            Some(AssetKind::Other) | None => NodeEditorIcon::new(icons::FILE, "Media asset"),
        },
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
                Some("Transition") => {
                    NodeEditorIcon::new(icons::ARROWS_MERGE, "Transition host operation")
                }
                _ => NodeEditorIcon::new(icons::WARNING, "Native design placeholder"),
            }
        }
        Some(NodeContent::Merge) => NodeEditorIcon::new(icons::ARROWS_MERGE, "Merge operation"),
        Some(NodeContent::SoundMerge) => {
            NodeEditorIcon::new(icons::WAVEFORM, "Audio Mix operation")
        }
        Some(NodeContent::SoundAnalysis(_)) => {
            NodeEditorIcon::new(icons::WAVE_SINE, "Sound analysis operation")
        }
        None => NodeEditorIcon::new(icons::QUESTION, "Missing node"),
    }
}

pub(in crate::ui::panels::node_editor) fn pin_color(data_type: PortDataType) -> Color32 {
    match data_type {
        PortDataType::Image => Color32::from_rgb(238, 207, 109),
        PortDataType::Shape => Color32::from_rgb(142, 132, 246),
        PortDataType::Style => Color32::from_rgb(224, 146, 214),
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
        PortDataType::Gradient | PortDataType::Pattern | PortDataType::Curve => {
            Color32::from_rgb(205, 120, 205)
        }
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
    connectable: bool,
) -> PinInfo {
    let color = if connectable {
        pin_color(data_type)
    } else {
        pin_color(data_type).gamma_multiply(0.32)
    };
    let visual = Editor::port_visual_style(color, connected);
    PinInfo::circle()
        .with_fill(visual.fill)
        .with_stroke(visual.stroke)
        .with_wire_color(visual.wire_color)
        .with_wire_style(WireStyle::Bezier3)
}
