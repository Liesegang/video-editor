//! Image entity converter.

use crate::framing::entity_converters::{EntityConverter, FrameEvaluationContext};
use crate::model::frame::entity::{FrameContent, FrameObject, ImageSurface};
use crate::model::project::TrackClip;

pub struct ImageEntityConverter;

impl EntityConverter for ImageEntityConverter {
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
