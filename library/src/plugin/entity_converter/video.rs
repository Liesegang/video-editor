use super::{EntityConverterPlugin, FrameEvaluationContext};
use crate::model::frame::entity::{FrameContent, FrameObject, ImageSurface};

#[derive(Default)]
pub struct VideoEntityConverterPlugin;

impl VideoEntityConverterPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl crate::plugin::Plugin for VideoEntityConverterPlugin {
    fn id(&self) -> &'static str {
        "video_entity_converter"
    }

    fn name(&self) -> String {
        "Video Entity Converter".to_string()
    }

    fn category(&self) -> String {
        "Converter".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EntityConverterPlugin for VideoEntityConverterPlugin {
    fn supports_kind(&self, kind: &str) -> bool {
        kind == "video"
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
            // Video Properties
            PropertyDefinition::new(
                "input_color_space",
                PropertyUiType::Text,
                "Input Color Space",
                PropertyValue::String("".to_string()),
            ),
            PropertyDefinition::new(
                "output_color_space",
                PropertyUiType::Text,
                "Output Color Space",
                PropertyValue::String("".to_string()),
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
        let media = match &node.content {
            crate::model::NodeContent::Media(media) => Some(media),
            _ => None,
        };
        let asset = media.and_then(|media| evaluator.project.get_asset(media.asset_id));

        // Calculate evaluation time based on Node timeframe
        let eval_time = time;

        let file_path = asset
            .map(|asset| asset.path.clone())
            .or_else(|| evaluator.require_string(props, "file_path", eval_time, "video"))?;
        let input_color_space = evaluator.optional_string(props, "input_color_space", eval_time);
        let output_color_space = evaluator.optional_string(props, "output_color_space", eval_time);

        if !eval_time.is_finite() || eval_time < 0.0 {
            return None;
        }
        if asset.is_some_and(|asset| asset.duration.is_some_and(|duration| eval_time >= duration)) {
            return None;
        }
        let stream_index = media
            .and_then(|media| media.stream_index)
            .or_else(|| asset.and_then(|asset| asset.stream_index));

        let transform = evaluator.build_transform(props, eval_time);
        let effects = evaluator.build_image_effects(&node.effects, eval_time);
        let surface = ImageSurface {
            file_path,
            effects,
            transform,
            input_color_space,
            output_color_space,
        };

        Some(FrameObject {
            content: FrameContent::Video {
                surface,
                source_time: eval_time,
                stream_index,
            },
            properties: props.clone(),
        })
    }
}
