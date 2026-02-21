use super::{EntityConverterPlugin, FrameEvaluationContext};
use crate::project::clip::TrackClip;
use crate::runtime::entity::{FrameContent, FrameObject};

pub struct SkSLEntityConverterPlugin;

impl SkSLEntityConverterPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl crate::plugin::Plugin for SkSLEntityConverterPlugin {
    fn id(&self) -> &'static str {
        "sksl_entity_converter"
    }

    fn name(&self) -> String {
        "SkSL Entity Converter".to_string()
    }

    fn category(&self) -> String {
        "Converter".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EntityConverterPlugin for SkSLEntityConverterPlugin {
    fn supports_kind(&self, kind: &str) -> bool {
        kind == "sksl"
    }

    fn get_property_definitions(
        &self,
        canvas_width: u64,
        canvas_height: u64,
        clip_width: u64,
        clip_height: u64,
    ) -> Vec<crate::project::property::PropertyDefinition> {
        use crate::project::property::{PropertyDefinition, PropertyUiType, PropertyValue, Vec2};
        use ordered_float::OrderedFloat;

        vec![
            // Transform Properties
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
            // Shader Properties
            PropertyDefinition::new(
                "shader",
                PropertyUiType::MultilineText,
                "Shader Code",
                PropertyValue::String("".to_string()),
            ),
            PropertyDefinition::new(
                "width",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 10000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Width",
                PropertyValue::Number(OrderedFloat(canvas_width as f64)),
            ),
            PropertyDefinition::new(
                "height",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 10000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Height",
                PropertyValue::Number(OrderedFloat(canvas_height as f64)),
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

        let shader = evaluator.require_string(props, "shader", eval_time, "sksl")?;

        let res_x = evaluator.evaluate_number(
            props,
            "width",
            eval_time,
            evaluator.composition.width as f64,
        );
        let res_y = evaluator.evaluate_number(
            props,
            "height",
            eval_time,
            evaluator.composition.height as f64,
        );

        let transform = evaluator.build_transform(props, eval_time);
        let effects = evaluator.build_clip_effects(track_clip, eval_time);

        Some(FrameObject {
            content: FrameContent::SkSL {
                shader,
                resolution: (res_x as f32, res_y as f32),
                effects,
                transform,
            },
            properties: props.clone(),
        })
    }

    fn get_bounds(
        &self,
        evaluator: &FrameEvaluationContext,
        track_clip: &TrackClip,
        frame_number: u64,
    ) -> Option<(f32, f32, f32, f32)> {
        let props = &track_clip.properties;
        let comp_fps = evaluator.composition.fps;

        let delta_frames = frame_number as f64 - track_clip.in_frame as f64;
        let time_offset = delta_frames / comp_fps;
        let source_start_time = track_clip.source_begin_frame as f64 / track_clip.fps;
        let eval_time = source_start_time + time_offset;

        let width = evaluator.evaluate_number(
            props,
            "width",
            eval_time,
            evaluator.composition.width as f64,
        );
        let height = evaluator.evaluate_number(
            props,
            "height",
            eval_time,
            evaluator.composition.height as f64,
        );

        Some((0.0, 0.0, width as f32, height as f32))
    }
}
