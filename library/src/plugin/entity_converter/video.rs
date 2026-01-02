use super::{EntityConverterPlugin, FrameEvaluationContext};
use crate::model::frame::entity::{FrameContent, FrameObject, ImageSurface};
// use crate::model::project::TrackClip;

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
        layer: &crate::model::Layer,
        time: f64,
    ) -> Option<FrameObject> {
        let props = &layer.properties;
        let comp_fps = evaluator.composition.fps;

        // Calculate evaluation time based on Layer timeframe
        let time_since_start = time - layer.start_time.into_inner();
        let eval_time =
            time_since_start * layer.time_stretch.into_inner() + layer.trim_in.into_inner();

        let file_path = evaluator.require_string(props, "file_path", eval_time, "video")?;
        let input_color_space = evaluator.optional_string(props, "input_color_space", eval_time);
        let output_color_space = evaluator.optional_string(props, "output_color_space", eval_time);

        // Calculate source frame number based on eval_time (seconds) and FPS
        // Assuming video source FPS matches composition FPS for now, or using composition frame alignment.
        // Ideally we should know source asset FPS, but Layer doesn't carry it directly yet without Asset lookup.
        let source_frame_number = (eval_time * comp_fps).round() as i64;

        if source_frame_number < 0 {
            return None;
        }

        let transform = evaluator.build_transform(props, eval_time);
        let effects = evaluator.build_image_effects(&layer.effects, eval_time);
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
                frame_number: source_frame_number as u64,
            },
            properties: props.clone(),
        })
    }
}
