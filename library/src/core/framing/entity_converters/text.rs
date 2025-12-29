//! Text entity converter.

use crate::framing::entity_converters::{EntityConverter, FrameEvaluationContext};
use crate::model::frame::entity::{FrameContent, FrameObject};
use crate::model::project::TrackClip;

pub struct TextEntityConverter;

impl EntityConverter for TextEntityConverter {
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

        let text = evaluator.require_string(props, "text", eval_time, "text")?;
        let font = evaluator
            .optional_string(props, "font_family", eval_time)
            .unwrap_or_else(|| "Arial".to_string());
        let size = evaluator.evaluate_number(props, "size", eval_time, 12.0);

        let styles = evaluator.build_styles(&track_clip.styles, eval_time);

        let transform = evaluator.build_transform(props, eval_time);
        let effects = evaluator.build_image_effects(&track_clip.effects, eval_time);

        // Parse Ensemble data if enabled
        let ensemble = if evaluator
            .optional_bool(props, "ensemble_enabled", eval_time)
            .unwrap_or(false)
        {
            use crate::core::ensemble::decorators::{BackplateShape, BackplateTarget};
            use crate::core::ensemble::effectors::OpacityMode;
            use crate::core::ensemble::target::EffectorTarget;
            use crate::core::ensemble::types::{DecoratorConfig, EffectorConfig};

            let mut effector_configs = Vec::new();
            let mut decorator_configs = Vec::new();

            // Transform Effector
            if evaluator
                .optional_bool(props, "ensemble_transform_enabled", eval_time)
                .unwrap_or(false)
            {
                let (translate_x, translate_y) = evaluator.evaluate_vec2(
                    props,
                    "ensemble_transform_translate",
                    "ensemble_transform_translate_x",
                    "ensemble_transform_translate_y",
                    eval_time,
                    0.0,
                    0.0,
                );

                let rotate =
                    evaluator.evaluate_number(props, "ensemble_transform_rotate", eval_time, 0.0)
                        as f32;

                let (scale_x, scale_y) = evaluator.evaluate_vec2(
                    props,
                    "ensemble_transform_scale",
                    "ensemble_transform_scale_x",
                    "ensemble_transform_scale_y",
                    eval_time,
                    100.0,
                    100.0,
                );

                effector_configs.push(EffectorConfig::Transform {
                    translate: (translate_x as f32, translate_y as f32),
                    rotate,
                    scale: (scale_x as f32 / 100.0, scale_y as f32 / 100.0),
                    target: EffectorTarget::default(),
                });
            }

            // StepDelay Effector
            if evaluator
                .optional_bool(props, "ensemble_step_delay_enabled", eval_time)
                .unwrap_or(false)
            {
                let delay_per_element = evaluator.evaluate_number(
                    props,
                    "ensemble_step_delay_per_element",
                    eval_time,
                    0.1,
                ) as f32;
                let duration = evaluator.evaluate_number(
                    props,
                    "ensemble_step_delay_duration",
                    eval_time,
                    1.0,
                ) as f32;
                let from_opacity = evaluator.evaluate_number(
                    props,
                    "ensemble_step_delay_from_opacity",
                    eval_time,
                    0.0,
                ) as f32;
                let to_opacity = evaluator.evaluate_number(
                    props,
                    "ensemble_step_delay_to_opacity",
                    eval_time,
                    100.0,
                ) as f32;

                effector_configs.push(EffectorConfig::StepDelay {
                    delay_per_element,
                    duration,
                    from_opacity,
                    to_opacity,
                    target: EffectorTarget::default(),
                });
            }

            // Opacity Effector
            if evaluator
                .optional_bool(props, "ensemble_opacity_enabled", eval_time)
                .unwrap_or(false)
            {
                let target_opacity =
                    evaluator.evaluate_number(props, "ensemble_opacity_target", eval_time, 50.0)
                        as f32;

                effector_configs.push(EffectorConfig::Opacity {
                    target_opacity,
                    mode: OpacityMode::Multiply,
                    target: EffectorTarget::default(),
                });
            }

            // Randomize Effector
            if evaluator
                .optional_bool(props, "ensemble_randomize_enabled", eval_time)
                .unwrap_or(false)
            {
                let translate_range_val = evaluator.evaluate_number(
                    props,
                    "ensemble_randomize_translate_range",
                    eval_time,
                    10.0,
                ) as f32;
                let translate_range = (translate_range_val, translate_range_val);
                let rotate_range = evaluator.evaluate_number(
                    props,
                    "ensemble_randomize_rotate_range",
                    eval_time,
                    15.0,
                ) as f32;
                let scale_range = (1.0, 1.0); // Not exposed in UI yet
                let seed =
                    evaluator.evaluate_number(props, "ensemble_randomize_seed", eval_time, 0.0)
                        as u64;

                effector_configs.push(EffectorConfig::Randomize {
                    translate_range,
                    rotate_range,
                    scale_range,
                    seed,
                    target: EffectorTarget::default(),
                });
            }

            // Backplate Decorator
            if evaluator
                .optional_bool(props, "ensemble_backplate_enabled", eval_time)
                .unwrap_or(false)
            {
                // Parse target
                let target_str = evaluator
                    .optional_string(props, "ensemble_backplate_target", eval_time)
                    .unwrap_or_else(|| "Block".to_string());
                let target = match target_str.as_str() {
                    "Char" => BackplateTarget::Char,
                    "Line" => BackplateTarget::Line,
                    "Block" => BackplateTarget::Block,
                    _ => BackplateTarget::Block,
                };

                // Parse shape
                let shape_str = evaluator
                    .optional_string(props, "ensemble_backplate_shape", eval_time)
                    .unwrap_or_else(|| "Rect".to_string());
                let shape = match shape_str.as_str() {
                    "Rect" => BackplateShape::Rect,
                    "RoundRect" => BackplateShape::RoundedRect,
                    "Circle" => BackplateShape::Circle,
                    _ => BackplateShape::Rect,
                };

                // Parse color using evaluator
                let color_val = evaluator.evaluate_color(
                    props,
                    "ensemble_backplate_color",
                    eval_time,
                    crate::model::frame::color::Color {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 128,
                    },
                );

                let color = crate::model::frame::color::Color {
                    r: (color_val.0 * 255.0) as u8,
                    g: (color_val.1 * 255.0) as u8,
                    b: (color_val.2 * 255.0) as u8,
                    a: (color_val.3 * 255.0) as u8,
                };

                let padding_val =
                    evaluator.evaluate_number(props, "ensemble_backplate_padding", eval_time, 5.0)
                        as f32;
                let padding = (padding_val, padding_val, padding_val, padding_val);
                let corner_radius =
                    evaluator.evaluate_number(props, "ensemble_backplate_radius", eval_time, 0.0)
                        as f32;

                let config = DecoratorConfig::Backplate {
                    target,
                    shape,
                    color,
                    padding,
                    corner_radius,
                };
                decorator_configs.push(config);
            }

            // Patch System
            let mut patches = std::collections::HashMap::new();
            if evaluator
                .optional_bool(props, "ensemble_patch_enabled", eval_time)
                .unwrap_or(false)
            {
                let indices_str = evaluator
                    .optional_string(props, "ensemble_patch_indices", eval_time)
                    .unwrap_or_else(|| "0".to_string());
                let indices: Vec<usize> = indices_str
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();

                if !indices.is_empty() {
                    let (translate_x, translate_y) = evaluator.evaluate_vec2(
                        props,
                        "ensemble_patch_translate",
                        "ensemble_patch_translate_x",
                        "ensemble_patch_translate_y",
                        eval_time,
                        0.0,
                        0.0,
                    );
                    let rotate =
                        evaluator.evaluate_number(props, "ensemble_patch_rotate", eval_time, 0.0)
                            as f32;
                    let (scale_x, scale_y) = evaluator.evaluate_vec2(
                        props,
                        "ensemble_patch_scale",
                        "ensemble_patch_scale_x",
                        "ensemble_patch_scale_y",
                        eval_time,
                        100.0,
                        100.0,
                    );
                    let opacity = evaluator.evaluate_number(
                        props,
                        "ensemble_patch_opacity",
                        eval_time,
                        100.0,
                    ) as f32;

                    let patch = crate::core::ensemble::types::TransformData {
                        translate: (translate_x as f32, translate_y as f32),
                        rotate,
                        scale: (scale_x as f32 / 100.0, scale_y as f32 / 100.0),
                        opacity: opacity / 100.0,
                        color_override: None,
                    };

                    for index in indices {
                        patches.insert(index, patch.clone());
                    }
                }
            }

            Some(crate::core::ensemble::EnsembleData {
                enabled: true,
                effector_configs,
                decorator_configs,
                patches,
            })
        } else {
            None
        };

        Some(FrameObject {
            content: FrameContent::Text {
                text,
                font,
                size,
                styles,
                effects,
                ensemble,
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

        let text = evaluator.require_string(props, "text", eval_time, "text")?;
        let font_name = evaluator
            .optional_string(props, "font_family", eval_time)
            .unwrap_or_else(|| "Arial".to_string());
        let size = evaluator.evaluate_number(props, "size", eval_time, 12.0);

        let font_mgr = skia_safe::FontMgr::default();
        let typeface = font_mgr
            .match_family_style(&font_name, skia_safe::FontStyle::normal())
            .unwrap_or_else(|| {
                font_mgr
                    .match_family_style("Arial", skia_safe::FontStyle::normal())
                    .expect("Failed to load default font")
            });

        let mut font = skia_safe::Font::default();
        font.set_typeface(typeface);
        font.set_size(size as f32);

        let width =
            crate::rendering::text_layout::measure_text_width(&text, &font_name, size as f32);
        let (_, metrics) = font.metrics();
        let top = 0.0;
        let height = metrics.descent - metrics.ascent;

        Some((0.0, top, width, height))
    }
}
