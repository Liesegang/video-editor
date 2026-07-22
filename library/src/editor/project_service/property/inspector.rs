use super::super::lifecycle::ProjectManager;
use super::super::node::DEFAULT_TEXT_FONT;
use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue};
use crate::model::{GeneratorContent, NodeContent};
use crate::plugin::entity_converter::measure_text_size;

impl ProjectManager {
    pub fn get_inspector_definitions(
        &self,
        comp_id: uuid::Uuid,
        _track_id: uuid::Uuid,
        node_id: uuid::Uuid,
    ) -> Vec<PropertyDefinition> {
        let project = self
            .project
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let (node, canvas_width, canvas_height) =
            if let Some(comp) = project.compositions.iter().find(|c| c.id == comp_id) {
                if let Some(node) = project.get_node(node_id) {
                    (node.clone(), comp.width, comp.height)
                } else {
                    return Vec::new();
                }
            } else {
                return Vec::new();
            };

        // Resolve clip dimensions
        let (clip_width, clip_height): (u64, u64) = match node.content() {
            NodeContent::Media(m) => {
                // If asset is loaded, get dimensions
                if let Some(asset) = project.assets.iter().find(|a| a.id == m.asset_id) {
                    (
                        asset.width.unwrap_or(100) as u64,
                        asset.height.unwrap_or(100) as u64,
                    )
                } else {
                    (100, 100)
                }
            }
            NodeContent::Generator(GeneratorContent::Shape) => {
                let w = node.properties().get_f64("width").unwrap_or(100.0) as u64;
                let h = node.properties().get_f64("height").unwrap_or(100.0) as u64;
                (w, h)
            }
            NodeContent::Generator(GeneratorContent::Text) => {
                let size = node.properties().get_f64("size").unwrap_or(100.0) as f32;
                let text = node.properties().get_string("text").unwrap_or_default();
                let font = node
                    .properties()
                    .get_string("font_family")
                    .unwrap_or_else(|| DEFAULT_TEXT_FONT.to_string());
                let (w, h) = measure_text_size(&text, &font, size);
                (w.round() as u64, h.round() as u64)
            }
            NodeContent::Generator(GeneratorContent::SkSL) => (canvas_width, canvas_height),
            _ => (100, 100),
        };

        // Key for entity converter: "video", "image", "text", "shape", "sksl"
        // In Trinity, LayerContent doesn't store "Kind" string.
        // We infer key from content.
        let kind_key = match node.content() {
            NodeContent::Media(m) => {
                if let Some(asset) = project.assets.iter().find(|a| a.id == m.asset_id) {
                    match asset.kind {
                        crate::model::asset::AssetKind::Video => "video",
                        crate::model::asset::AssetKind::Image => "image",
                        crate::model::asset::AssetKind::Audio => "audio",
                        _ => "unknown",
                    }
                } else {
                    "video" // default fallback?
                }
            }
            NodeContent::Generator(GeneratorContent::Shape) => "shape",
            NodeContent::Generator(GeneratorContent::Text) => "text",
            NodeContent::Generator(GeneratorContent::SkSL) => "sksl",
            NodeContent::Generator(GeneratorContent::Solid) => "solid",
            _ => "unknown",
        };

        let converter = self.plugin_manager.get_entity_converter(kind_key);

        let mut definitions = match node.content() {
            NodeContent::Value(value) => value.property_definitions().to_vec(),
            NodeContent::Data(data) => data.property_definitions().to_vec(),
            NodeContent::List(operation) => operation.property_definitions().to_vec(),
            _ => converter.map_or_else(Vec::new, |converter| {
                converter.get_property_definitions(
                    canvas_width,
                    canvas_height,
                    clip_width,
                    clip_height,
                )
            }),
        };

        if kind_key == "video" {
            let colorspaces =
                crate::editor::color_service::ColorSpaceManager::get_available_colorspaces();
            if !colorspaces.is_empty() {
                definitions.push(PropertyDefinition::new(
                    "input_color_space",
                    PropertyUiType::Dropdown {
                        options: colorspaces.clone(),
                    },
                    "Input Color Space",
                    PropertyValue::String("".to_string()),
                ));
                definitions.push(PropertyDefinition::new(
                    "output_color_space",
                    PropertyUiType::Dropdown {
                        options: colorspaces,
                    },
                    "Output Color Space",
                    PropertyValue::String("".to_string()),
                ));
            }
        }

        definitions
    }
}
