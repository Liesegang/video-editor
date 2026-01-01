use super::{EntityConverterPlugin, FrameEvaluationContext};
use crate::model::frame::entity::{FrameContent, FrameObject};
use crate::model::project::TrackClip;

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
    ) -> Vec<crate::model::project::property::PropertyDefinition> {
        use crate::model::project::property::{
            PropertyDefinition, PropertyUiType, PropertyValue, Vec2,
        };
        use ordered_float::OrderedFloat;

        vec![
            // Transform Properties
            PropertyDefinition {
                name: "position".to_string(),
                label: "Position".to_string(),
                ui_type: PropertyUiType::Vec2 {
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(canvas_width as f64 / 2.0),
                    y: OrderedFloat(canvas_height as f64 / 2.0),
                }),
                category: "Transform".to_string(),
            },
            PropertyDefinition {
                name: "scale".to_string(),
                label: "Scale".to_string(),
                ui_type: PropertyUiType::Vec2 {
                    suffix: "%".to_string(),
                },
                default_value: PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(100.0),
                    y: OrderedFloat(100.0),
                }),
                category: "Transform".to_string(),
            },
            PropertyDefinition {
                name: "rotation".to_string(),
                label: "Rotation".to_string(),
                ui_type: PropertyUiType::Float {
                    min: -360.0,
                    max: 360.0,
                    step: 1.0,
                    suffix: "deg".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(0.0)),
                category: "Transform".to_string(),
            },
            PropertyDefinition {
                name: "anchor".to_string(),
                label: "Anchor".to_string(),
                ui_type: PropertyUiType::Vec2 {
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(clip_width as f64 / 2.0),
                    y: OrderedFloat(clip_height as f64 / 2.0),
                }),
                category: "Transform".to_string(),
            },
            PropertyDefinition {
                name: "opacity".to_string(),
                label: "Opacity".to_string(),
                ui_type: PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: "%".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(100.0)),
                category: "Transform".to_string(),
            },
            // Shader Properties
            PropertyDefinition {
                name: "shader".to_string(),
                label: "Shader Code".to_string(),
                ui_type: PropertyUiType::MultilineText,
                default_value: PropertyValue::String("".to_string()),
                category: "Shader".to_string(),
            },
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

        let comp_width = evaluator.composition.width as f64;
        let comp_height = evaluator.composition.height as f64;

        let res_x = comp_width;
        let res_y = comp_height;

        let transform = evaluator.build_transform(props, eval_time);
        let effects = evaluator.build_image_effects(&track_clip.effects, eval_time);

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
