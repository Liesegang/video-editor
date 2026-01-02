use skia_safe::FontMgr;
use skia_safe::textlayout::{FontCollection, ParagraphBuilder, ParagraphStyle, TextStyle};

use super::{EntityConverterPlugin, FrameEvaluationContext};
use crate::model::frame::entity::{FrameContent, FrameObject};
// use crate::model::project::TrackClip;

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
    ) -> Vec<crate::model::property::PropertyDefinition> {
        use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue, Vec2};
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
            // Text Properties
            PropertyDefinition::new(
                "text",
                PropertyUiType::Text,
                "Content",
                PropertyValue::String("Text".to_string()),
            ),
            PropertyDefinition::new(
                "font_family",
                PropertyUiType::Font,
                "Font",
                PropertyValue::String("Arial".to_string()),
            ),
            PropertyDefinition::new(
                "size",
                PropertyUiType::Float {
                    min: 1.0,
                    max: 1000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Font Size",
                PropertyValue::Number(OrderedFloat(100.0)),
            ),
        ]
    }

    fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext,
        layer: &crate::model::Layer,
        time: f64,
    ) -> Option<FrameObject> {
        let props = &layer.properties;
        let _comp_fps = evaluator.composition.fps;

        // Calculate evaluation time based on Layer timeframe
        // eval_time = (global_time - start_time) * stretch + trim_in
        let time_since_start = time - layer.start_time.into_inner();
        let eval_time =
            time_since_start * layer.time_stretch.into_inner() + layer.trim_in.into_inner();

        let text = evaluator.require_string(props, "text", eval_time, "text")?;
        let font = evaluator
            .optional_string(props, "font_family", eval_time)
            .unwrap_or_else(|| "Arial".to_string());
        let size = evaluator.evaluate_number(props, "size", eval_time, 12.0);

        let styles = evaluator.build_styles(&layer.styles, eval_time);

        let transform = evaluator.build_transform(props, eval_time);
        let effects = evaluator.build_image_effects(&layer.effects, eval_time);

        // Build Ensemble data from layer.effectors/decorators
        let ensemble = if !layer.effects.is_empty() || !layer.styles.is_empty() {
            let mut effector_configs = Vec::new();
            let mut decorator_configs = Vec::new();

            // Convert EffectorInstances to EffectorConfigs
            // Convert EffectConfigs to EffectorConfigs (Text Animators)
            for instance in &layer.effects {
                if let Some(plugin) = evaluator
                    .plugin_manager
                    .get_effector_plugin(&instance.effect_type)
                {
                    if let Some(config) = plugin.convert(evaluator, instance, eval_time) {
                        effector_configs.push(config);
                    }
                }
                // Note: Image effects are handled separately in build_image_effects
            }

            // Convert StyleInstances to DecoratorConfigs (Backplates)
            for instance in &layer.styles {
                if let Some(plugin) = evaluator
                    .plugin_manager
                    .get_decorator_plugin(&instance.style_type)
                {
                    if let Some(config) = plugin.convert(evaluator, instance, eval_time) {
                        decorator_configs.push(config);
                    }
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
        layer: &crate::model::Layer,
        time: f64,
    ) -> Option<(f32, f32, f32, f32)> {
        let props = &layer.properties;
        let _comp_fps = evaluator.composition.fps;

        // Calculate evaluation time based on Layer timeframe
        let time_since_start = time - layer.start_time.into_inner();
        let eval_time =
            time_since_start * layer.time_stretch.into_inner() + layer.trim_in.into_inner();

        let text = evaluator.require_string(props, "text", eval_time, "text")?;
        let font_name = evaluator
            .optional_string(props, "font_family", eval_time)
            .unwrap_or_else(|| "Arial".to_string());
        let size = evaluator.evaluate_number(props, "size", eval_time, 12.0);

        let (width, height) = measure_text_size(&text, &font_name, size as f32);

        Some((0.0, 0.0, width, height))
    }
}

pub fn measure_text_size(text: &str, primary_font_name: &str, size: f32) -> (f32, f32) {
    let mut font_collection = FontCollection::new();
    font_collection.set_default_font_manager(FontMgr::default(), None);

    let mut text_style = TextStyle::new();
    text_style.set_font_families(&[primary_font_name]);
    text_style.set_font_size(size);

    let mut paragraph_style = ParagraphStyle::new();
    paragraph_style.set_text_style(&text_style);

    let mut builder = ParagraphBuilder::new(&paragraph_style, font_collection);

    builder.add_text(text);

    let mut paragraph = builder.build();
    paragraph.layout(f32::MAX);

    let width = paragraph.max_intrinsic_width();
    let height = paragraph.height();

    log::info!(
        "measure_text_size: text='{}' font='{}' size={} -> w={} h={}",
        text,
        primary_font_name,
        size,
        width,
        height
    );

    (width, height)
}
