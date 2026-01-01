use super::{EntityConverterPlugin, FrameEvaluationContext};
use crate::model::frame::entity::{FrameContent, FrameObject, ImageSurface};
use crate::model::project::TrackClip;

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
    ) -> Vec<crate::model::project::property::PropertyDefinition> {
        use crate::model::project::property::{
            PropertyDefinition, PropertyUiType, PropertyValue, Vec2,
        };
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
        track_clip: &TrackClip,
        frame_number: u64,
    ) -> Option<FrameObject> {
        let props = &track_clip.properties;
        let comp_fps = evaluator.composition.fps;

        let delta_frames = frame_number as f64 - track_clip.in_frame as f64;
        let time_offset = delta_frames / comp_fps;
        let source_start_time = track_clip.source_begin_frame as f64 / track_clip.fps;
        let eval_time = source_start_time + time_offset;

        if frame_number % 30 == 0 {
            log::info!(
                "[ImageRender] Frame: {} | ClipIn: {} | GlobalDelta: {:.4}s | EvalTime: {:.4}s",
                frame_number,
                track_clip.in_frame,
                time_offset,
                eval_time
            );
        }

        let file_path = evaluator.require_string(props, "file_path", eval_time, "image")?;
        let transform = evaluator.build_transform(props, eval_time);
        if frame_number % 30 == 0 {
            log::info!(
                "[ImageRender] Resolving transform at EvalTime {:.4}: {:?}",
                eval_time,
                transform
            );
        }
        let effects = evaluator.build_image_effects(&track_clip.effects, eval_time);
        let surface = ImageSurface {
            file_path,
            effects,
            transform,
            input_color_space: None,
            output_color_space: None,
        };

        Some(FrameObject {
            content: FrameContent::Image { surface },
            properties: props.clone(),
        })
    }
}
