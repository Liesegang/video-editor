//! Video entity converter.

use crate::framing::entity_converters::{EntityConverter, FrameEvaluationContext};
use crate::model::frame::entity::{FrameContent, FrameObject, ImageSurface};
use crate::model::project::TrackClip;

pub struct VideoEntityConverter;

impl EntityConverter for VideoEntityConverter {
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

        let file_path = evaluator.require_string(props, "file_path", eval_time, "video")?;
        let input_color_space = evaluator.optional_string(props, "input_color_space", eval_time);
        let output_color_space = evaluator.optional_string(props, "output_color_space", eval_time);

        let source_delta_frames = time_offset * track_clip.fps;
        let source_frame_number =
            track_clip.source_begin_frame + source_delta_frames.round() as i64;

        if source_frame_number < 0 {
            return None;
        }

        let transform = evaluator.build_transform(props, eval_time);
        let effects = evaluator.build_image_effects(&track_clip.effects, eval_time);
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
