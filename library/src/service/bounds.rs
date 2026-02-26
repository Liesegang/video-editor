//! Source content bounds calculation.
//!
//! Provides bounds (x, y, width, height) for source content such as text, shape,
//! and SkSL, primarily used for gizmo overlay in the preview panel.

use crate::plugin::{EvaluationContext, PropertyEvaluatorRegistry};
use crate::project::property::PropertyMap;
use crate::project::source::{SourceData, SourceKind};
use crate::rendering::text_renderer::measure_text_size;

/// Calculate the content bounds (x, y, w, h) for a source at the given frame.
///
/// Returns `None` if the source kind doesn't support bounds calculation
/// or if required properties are missing.
pub fn get_source_content_bounds(
    source: &SourceData,
    comp_fps: f64,
    frame_number: u64,
    property_evaluators: &PropertyEvaluatorRegistry,
) -> Option<(f32, f32, f32, f32)> {
    let eval_time = source_eval_time(source, comp_fps, frame_number);
    let props = &source.properties;

    match source.kind {
        SourceKind::Text => {
            let text = eval_string(props, "text", eval_time, comp_fps, property_evaluators)?;
            let font_name = eval_optional_string(
                props,
                "font_family",
                eval_time,
                comp_fps,
                property_evaluators,
            )
            .unwrap_or_else(|| "Arial".to_string());
            let size = eval_number(
                props,
                "size",
                eval_time,
                comp_fps,
                property_evaluators,
                12.0,
            );
            let (width, height) = measure_text_size(&text, &font_name, size as f32);
            Some((0.0, 0.0, width, height))
        }
        SourceKind::Shape => {
            let path_str = eval_string(props, "path", eval_time, comp_fps, property_evaluators)?;
            if let Some(path) = skia_safe::utils::parse_path::from_svg(&path_str) {
                let bounds = path.compute_tight_bounds();
                Some((bounds.left, bounds.top, bounds.width(), bounds.height()))
            } else {
                Some((0.0, 0.0, 100.0, 100.0))
            }
        }
        SourceKind::SkSL => {
            let width = eval_number(
                props,
                "width",
                eval_time,
                comp_fps,
                property_evaluators,
                1920.0,
            );
            let height = eval_number(
                props,
                "height",
                eval_time,
                comp_fps,
                property_evaluators,
                1080.0,
            );
            Some((0.0, 0.0, width as f32, height as f32))
        }
        _ => None,
    }
}

fn source_eval_time(source: &SourceData, comp_fps: f64, frame_number: u64) -> f64 {
    let delta_frames = frame_number as f64 - source.in_frame as f64;
    let time_offset = delta_frames / comp_fps;
    let source_start_time = source.source_begin_frame as f64 / source.fps;
    source_start_time + time_offset
}

fn eval_number(
    props: &PropertyMap,
    key: &str,
    time: f64,
    fps: f64,
    evaluators: &PropertyEvaluatorRegistry,
    default: f64,
) -> f64 {
    if let Some(prop) = props.get(key) {
        let ctx = EvaluationContext {
            property_map: props,
            fps,
        };
        let val = evaluators.evaluate(prop, time, &ctx);
        val.get_as::<f64>().unwrap_or(default)
    } else {
        default
    }
}

fn eval_string(
    props: &PropertyMap,
    key: &str,
    time: f64,
    fps: f64,
    evaluators: &PropertyEvaluatorRegistry,
) -> Option<String> {
    if let Some(prop) = props.get(key) {
        let ctx = EvaluationContext {
            property_map: props,
            fps,
        };
        let val = evaluators.evaluate(prop, time, &ctx);
        val.get_as::<String>()
    } else {
        None
    }
}

fn eval_optional_string(
    props: &PropertyMap,
    key: &str,
    time: f64,
    fps: f64,
    evaluators: &PropertyEvaluatorRegistry,
) -> Option<String> {
    eval_string(props, key, time, fps, evaluators)
}
