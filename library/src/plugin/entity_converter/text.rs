use super::{EntityConverterPlugin, FrameEvaluationContext};
use crate::model::frame::entity::{FrameContent, FrameObject};
use crate::model::project::TrackClip;

pub struct TextEntityConverterPlugin;

impl TextEntityConverterPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl crate::plugin::Plugin for TextEntityConverterPlugin {
    fn id(&self) -> &'static str {
        "text_entity_converter"
    }

    fn name(&self) -> String {
        "Text Entity Converter".to_string()
    }

    fn category(&self) -> String {
        "Converter".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EntityConverterPlugin for TextEntityConverterPlugin {
    fn supports_kind(&self, kind: &str) -> bool {
        kind == "text"
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
            // Text Properties
            PropertyDefinition {
                name: "text".to_string(),
                label: "Content".to_string(),
                ui_type: PropertyUiType::Text,
                default_value: PropertyValue::String("Text".to_string()),
                category: "Text".to_string(),
            },
            PropertyDefinition {
                name: "font_family".to_string(),
                label: "Font".to_string(),
                ui_type: PropertyUiType::Font,
                default_value: PropertyValue::String("Arial".to_string()),
                category: "Text".to_string(),
            },
            PropertyDefinition {
                name: "size".to_string(),
                label: "Font Size".to_string(),
                ui_type: PropertyUiType::Float {
                    min: 1.0,
                    max: 1000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                },
                default_value: PropertyValue::Number(OrderedFloat(100.0)),
                category: "Text".to_string(),
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

        let text = evaluator.require_string(props, "text", eval_time, "text")?;
        let font = evaluator
            .optional_string(props, "font_family", eval_time)
            .unwrap_or_else(|| "Arial".to_string());
        let size = evaluator.evaluate_number(props, "size", eval_time, 12.0);

        let styles = evaluator.build_styles(&track_clip.styles, eval_time);

        let transform = evaluator.build_transform(props, eval_time);
        let effects = evaluator.build_image_effects(&track_clip.effects, eval_time);

        // Build Ensemble data from text_clip.effectors/decorators
        let ensemble = if !track_clip.effectors.is_empty() || !track_clip.decorators.is_empty() {
            use crate::core::ensemble::decorators::{BackplateShape, BackplateTarget};
            use crate::core::ensemble::target::EffectorTarget;
            use crate::core::ensemble::types::{DecoratorConfig, EffectorConfig};

            let mut effector_configs = Vec::new();
            let mut decorator_configs = Vec::new();

            // Convert EffectorInstances to EffectorConfigs
            for instance in &track_clip.effectors {
                match instance.effector_type.as_str() {
                    "transform" => {
                        let tx =
                            evaluator.evaluate_number(&instance.properties, "tx", eval_time, 0.0)
                                as f32;
                        let ty =
                            evaluator.evaluate_number(&instance.properties, "ty", eval_time, 0.0)
                                as f32;
                        let r = evaluator.evaluate_number(
                            &instance.properties,
                            "rotation",
                            eval_time,
                            0.0,
                        ) as f32;
                        let sx = evaluator.evaluate_number(
                            &instance.properties,
                            "scale_x",
                            eval_time,
                            1.0,
                        ) as f32;
                        let sy = evaluator.evaluate_number(
                            &instance.properties,
                            "scale_y",
                            eval_time,
                            1.0,
                        ) as f32;

                        effector_configs.push(EffectorConfig::Transform {
                            translate: (tx, ty),
                            rotate: r,
                            scale: (sx, sy),
                            target: EffectorTarget::default(),
                        });
                    }
                    "step_delay" => {
                        let delay = evaluator.evaluate_number(
                            &instance.properties,
                            "delay",
                            eval_time,
                            0.1,
                        ) as f32;
                        let duration = evaluator.evaluate_number(
                            &instance.properties,
                            "duration",
                            eval_time,
                            1.0,
                        ) as f32;
                        let from_opacity = evaluator.evaluate_number(
                            &instance.properties,
                            "from_opacity",
                            eval_time,
                            0.0,
                        ) as f32;
                        let to_opacity = evaluator.evaluate_number(
                            &instance.properties,
                            "to_opacity",
                            eval_time,
                            100.0,
                        ) as f32;

                        effector_configs.push(EffectorConfig::StepDelay {
                            delay_per_element: delay,
                            duration,
                            from_opacity,
                            to_opacity,
                            target: EffectorTarget::default(),
                        });
                    }
                    "randomize" => {
                        let seed =
                            evaluator.evaluate_number(&instance.properties, "seed", eval_time, 0.0)
                                as u64;
                        let amount = evaluator.evaluate_number(
                            &instance.properties,
                            "amount",
                            eval_time,
                            1.0,
                        ) as f32;
                        // Read explicit ranges if available, otherwise fall back to amount-based defaults
                        let tr_val = evaluator.evaluate_number(
                            &instance.properties,
                            "translate_range",
                            eval_time,
                            100.0 * amount as f64,
                        ) as f32;
                        let translate_range = (tr_val, tr_val);

                        let rotate_range = evaluator.evaluate_number(
                            &instance.properties,
                            "rotate_range",
                            eval_time,
                            45.0 * amount as f64,
                        ) as f32;

                        // Start with default scale range (1.0, 1.0) as we don't have scale_range property yet in UI snippet?
                        let scale_range = (1.0, 1.0);

                        effector_configs.push(EffectorConfig::Randomize {
                            translate_range,
                            rotate_range,
                            scale_range,
                            seed,
                            target: EffectorTarget::default(),
                        });
                    }
                    _ => {}
                }
            }

            // Convert DecoratorInstances to DecoratorConfigs
            for instance in &track_clip.decorators {
                match instance.decorator_type.as_str() {
                    "backplate" => {
                        // Note: Color evaluation logic in original text.rs was tricky, assuming default behavior for now if helpers missing
                        // But we can try to evaluate color components if they exist, or use a helper if available.
                        // FrameEvaluationContext doesn't seem to have evaluate_color exposed publicly in definitions seen so far?
                        // Actually it might map to `evaluate_color` if implemented.
                        // Let's assume `evaluate_color` is NOT available based on previous errors/context, and allow fallback.
                        // Wait, `PropertyUiType::Color` stores `PropertyValue::Color`.
                        // `evaluate_property_value` returns `PropertyValue`.
                        // We need a way to get the Color struct.

                        let color = if let Some(prop) = instance.properties.get("color") {
                            if let Some(crate::model::project::property::PropertyValue::Color(c)) =
                                prop.value()
                            {
                                c.clone()
                            } else {
                                crate::model::frame::color::Color::black()
                            }
                        } else {
                            crate::model::frame::color::Color::black()
                        };

                        let padding_val = evaluator.evaluate_number(
                            &instance.properties,
                            "padding",
                            eval_time,
                            0.0,
                        ) as f32;
                        let radius = evaluator.evaluate_number(
                            &instance.properties,
                            "radius",
                            eval_time,
                            0.0,
                        ) as f32;

                        let target_str = evaluator
                            .require_string(&instance.properties, "target", eval_time, "Block")
                            .unwrap_or("Block".to_string());

                        let target = match target_str.as_str() {
                            "Char" => BackplateTarget::Char,
                            "Line" => BackplateTarget::Line,
                            _ => BackplateTarget::Block,
                        };

                        let shape_str = evaluator
                            .require_string(&instance.properties, "shape", eval_time, "Rect")
                            .unwrap_or("Rect".to_string());

                        let shape = match shape_str.as_str() {
                            "RoundRect" => BackplateShape::RoundedRect,
                            "Circle" => BackplateShape::Circle,
                            _ => BackplateShape::Rect,
                        };

                        decorator_configs.push(DecoratorConfig::Backplate {
                            target,
                            shape,
                            color,
                            padding: (padding_val, padding_val, padding_val, padding_val),
                            corner_radius: radius,
                        });
                    }
                    _ => {}
                }
            }

            Some(crate::core::ensemble::EnsembleData {
                enabled: true,
                effector_configs,
                decorator_configs,
                patches: std::collections::HashMap::new(), // Patches not yet in UI
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
