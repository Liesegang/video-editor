//! SkSL entity converter.

use crate::framing::entity_converters::{EntityConverter, FrameEvaluationContext};
use crate::model::frame::entity::{FrameContent, FrameObject};
use crate::model::project::TrackClip;

pub struct SkSLEntityConverter;

impl EntityConverter for SkSLEntityConverter {
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
