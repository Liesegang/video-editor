use super::{EntityConverterPlugin, FrameEvaluationContext};
use crate::model::frame::entity::{FrameBounds, FrameContent, FrameObject, ImageSurface};
use crate::model::NodeContent;

#[derive(Default)]
pub struct ImageEntityConverterPlugin;

impl ImageEntityConverterPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl crate::plugin::Plugin for ImageEntityConverterPlugin {
    fn id(&self) -> &'static str {
        "image_entity_converter"
    }

    fn name(&self) -> String {
        "Image Entity Converter".to_string()
    }

    fn category(&self) -> String {
        "Converter".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EntityConverterPlugin for ImageEntityConverterPlugin {
    fn supports_kind(&self, kind: &str) -> bool {
        kind == "image"
    }

    fn get_property_definitions(
        &self,
        canvas_width: u64,
        canvas_height: u64,
        clip_width: u64,
        clip_height: u64,
    ) -> Vec<crate::model::property::PropertyDefinition> {
        use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue, Vec2};
        use ordered_float::OrderedFloat;

        vec![
            PropertyDefinition::new(
                "position",
                PropertyUiType::Vec2 {
                    suffix: "px".to_string(),
                },
                "Position",
                PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(canvas_width as f64 / 2.0),
                    y: OrderedFloat(canvas_height as f64 / 2.0),
                }),
            ),
            PropertyDefinition::new(
                "scale",
                PropertyUiType::Vec2 {
                    suffix: "%".to_string(),
                },
                "Scale",
                PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(100.0),
                    y: OrderedFloat(100.0),
                }),
            ),
            PropertyDefinition::new(
                "rotation",
                PropertyUiType::Float {
                    min: -360.0,
                    max: 360.0,
                    step: 1.0,
                    suffix: "deg".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Rotation",
                PropertyValue::Number(OrderedFloat(0.0)),
            ),
            PropertyDefinition::new(
                "anchor",
                PropertyUiType::Vec2 {
                    suffix: "px".to_string(),
                },
                "Anchor",
                PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(clip_width as f64 / 2.0),
                    y: OrderedFloat(clip_height as f64 / 2.0),
                }),
            ),
            PropertyDefinition::new(
                "opacity",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: "%".to_string(),
                    min_hard_limit: true,
                    max_hard_limit: true,
                },
                "Opacity",
                PropertyValue::Number(OrderedFloat(100.0)),
            ),
        ]
    }

    fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext,
        node: &crate::model::Node,
        time: f64,
    ) -> Option<FrameObject> {
        let props = &node.properties;
        let asset = match &node.content {
            NodeContent::Media(media) => evaluator.project.get_asset(media.asset_id),
            _ => None,
        };
        let file_path = asset
            .map(|asset| asset.path.clone())
            .or_else(|| evaluator.require_string(props, "file_path", time, "image"))?;
        let transform = evaluator.build_transform(props, time);

        let effects = evaluator.build_image_effects(&node.effects, time);
        let surface = ImageSurface {
            file_path,
            effects,
            transform: transform.clone(),
            input_color_space: None,
            output_color_space: None,
        };

        Some(FrameObject {
            source_node_id: node.id,
            source_transform: transform,
            content_bounds: asset.and_then(|asset| match (asset.width, asset.height) {
                (Some(width), Some(height)) => {
                    Some(FrameBounds::new(0.0, 0.0, width as f32, height as f32))
                }
                _ => None,
            }),
            content: FrameContent::Image { surface },
            properties: props.clone(),
        })
    }
}
