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
            let mut effector_configs = Vec::new();
            let mut decorator_configs = Vec::new();

            // Convert EffectorInstances to EffectorConfigs
            for instance in &track_clip.effectors {
                if let Some(plugin) = evaluator
                    .plugin_manager
                    .get_effector_plugin(&instance.effector_type)
                {
                    if let Some(config) = plugin.convert(evaluator, instance, eval_time) {
                        effector_configs.push(config);
                    }
                } else {
                    log::warn!(
                        "[WARN] entity_converter/text.rs: Unknown/Unsupported effector type: {}",
                        instance.effector_type
                    );
                }
            }

            // Convert DecoratorInstances to DecoratorConfigs
            for instance in &track_clip.decorators {
                if let Some(plugin) = evaluator
                    .plugin_manager
                    .get_decorator_plugin(&instance.decorator_type)
                {
                    if let Some(config) = plugin.convert(evaluator, instance, eval_time) {
                        decorator_configs.push(config);
                    }
                } else {
                    log::warn!(
                        "[WARN] entity_converter/text.rs: Unknown/Unsupported decorator type: {}",
                        instance.decorator_type
                    );
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
