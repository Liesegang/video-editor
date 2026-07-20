use super::{
    EntityConverterPlugin, FrameEvaluationContext, raster_source_transform_property_definitions,
};
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
        canvas_width: u64,
        canvas_height: u64,
        clip_width: u64,
        clip_height: u64,
    ) -> Vec<crate::model::property::PropertyDefinition> {
        let mut definitions = raster_source_transform_property_definitions(
            canvas_width,
            canvas_height,
            clip_width,
            clip_height,
        );
        definitions.push(crate::model::property::PropertyDefinition::new(
            "color",
            crate::model::property::PropertyUiType::Color,
            "Color",
            crate::model::property::PropertyValue::Color(
                crate::model::frame::color::Color::white(),
            ),
        ));
        definitions
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
        let transform = evaluator.build_transform(node.properties(), eval_time);
        let color = evaluator.require_color(node.properties(), "color", eval_time, "solid")?;
        let (width, height) = evaluator.evaluation_resolution();
        let path = format!("M 0 0 H {width} V {height} H 0 Z");

        Some(FrameObject {
            source_node_id: node.id,
            spatial_transform_node_id: Some(node.id),
            spatial_transform: Box::new(transform.clone()),
            content_bounds: Some(FrameBounds::new(0.0, 0.0, width as f32, height as f32)),
            content: FrameContent::Shape {
                path,
                styles: vec![StyleConfig {
                    id: node.id,
                    style: DrawStyle::Fill { color, offset: 0.0 },
                }],
                path_effects: Vec::new(),
                effects: Vec::new(),
                ensemble: None,
                transform,
            },
        })
    }
}
