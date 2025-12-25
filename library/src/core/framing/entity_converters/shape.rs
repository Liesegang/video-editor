//! Shape entity converter.

use crate::framing::entity_converters::{EntityConverter, FrameEvaluationContext};
use crate::model::frame::entity::{FrameContent, FrameObject};
use crate::model::project::TrackClip;
use crate::model::project::property::PropertyValue;

pub struct ShapeEntityConverter;

impl EntityConverter for ShapeEntityConverter {
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

        let path = evaluator.require_string(props, "path", eval_time, "shape")?;
        let transform = evaluator.build_transform(props, eval_time);

        let styles = evaluator.build_styles(&track_clip.styles, eval_time);

        let effects_value = evaluator
            .evaluate_property_value(props, "path_effects", eval_time)
            .unwrap_or(PropertyValue::Array(vec![]));
        let path_effects = evaluator.parse_path_effects(effects_value);
        let effects = evaluator.build_image_effects(&track_clip.effects, eval_time);

        Some(FrameObject {
            content: FrameContent::Shape {
                path,
                transform,
                styles,
                path_effects,
                effects,
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

        let path_str = evaluator.require_string(props, "path", eval_time, "shape")?;

        if let Some(path) = skia_safe::utils::parse_path::from_svg(&path_str) {
            let bounds = path.compute_tight_bounds();
            Some((bounds.left, bounds.top, bounds.width(), bounds.height()))
        } else {
            Some((0.0, 0.0, 100.0, 100.0))
        }
    }
}
