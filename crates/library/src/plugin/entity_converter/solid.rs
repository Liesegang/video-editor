use super::{EntityConverterPlugin, FrameEvaluationContext};
use crate::model::frame::draw_type::DrawStyle;
use crate::model::frame::entity::{FrameBounds, FrameContent, FrameObject, StyleConfig};
use crate::model::{GeneratorContent, NodeContent};

#[derive(Default)]
pub struct SolidEntityConverterPlugin;

impl SolidEntityConverterPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl crate::plugin::Plugin for SolidEntityConverterPlugin {
    fn id(&self) -> &'static str {
        "solid_entity_converter"
    }

    fn name(&self) -> String {
        "Solid Entity Converter".to_string()
    }

    fn category(&self) -> String {
        "Converter".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EntityConverterPlugin for SolidEntityConverterPlugin {
    fn supports_kind(&self, kind: &str) -> bool {
        kind == "solid"
    }

    fn get_property_definitions(
        &self,
        _canvas_width: u64,
        _canvas_height: u64,
        _clip_width: u64,
        _clip_height: u64,
    ) -> Vec<crate::model::property::PropertyDefinition> {
        let white = crate::model::property::ColorValue::from_straight_srgba8(
            &crate::model::frame::color::Color::white(),
        );
        vec![crate::model::property::PropertyDefinition::new(
            "color",
            crate::model::property::PropertyUiType::ColorValue,
            "Color",
            crate::model::property::PropertyValue::ColorValue(white),
        )]
    }

    fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext,
        node: &crate::model::Node,
        time: f64,
    ) -> Option<FrameObject> {
        let NodeContent::Generator(GeneratorContent::Solid) = node.content() else {
            return None;
        };
        let eval_time = time;
        let color_value =
            evaluator.require_color_value(node.properties(), "color", eval_time, "solid")?;
        let color = crate::color_management::to_renderer_srgba8(&color_value)
            .inspect_err(|error| {
                log::error!(
                    "Solid Node {} cannot cross the legacy renderer color boundary: {error}",
                    node.id
                );
            })
            .ok()?;
        let (width, height) = evaluator.evaluation_resolution();
        let path = format!("M 0 0 H {width} V {height} H 0 Z");

        Some(FrameObject {
            source_node_id: node.id,
            spatial_transform_node_id: None,
            spatial_transform: Box::default(),
            content_bounds: Some(FrameBounds::new(0.0, 0.0, width as f32, height as f32)),
            content: FrameContent::Shape {
                path,
                canonical_path: None,
                parts: Vec::new(),
                styles: vec![StyleConfig {
                    id: node.id,
                    style: DrawStyle::Fill { color, offset: 0.0 },
                }],
                path_effects: Vec::new(),
                effects: Vec::new(),
                ensemble: None,
                transform: Default::default(),
            },
        })
    }
}
