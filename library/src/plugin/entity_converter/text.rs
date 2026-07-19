use super::{EntityConverterPlugin, FrameEvaluationContext};
use crate::core::rendering::text_layout::{
    layout_runtime_text_shape, measure_text_layout, text_style_outset,
};
use crate::model::frame::entity::FrameObject;
use crate::model::frame::runtime_shape::{RuntimeShape, RuntimeShapeGeometry};

#[derive(Default)]
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
        _evaluator: &FrameEvaluationContext,
        _node: &crate::model::Node,
        _time: f64,
    ) -> Option<FrameObject> {
        // Text is Shape-only. Rasterization is owned by an explicit Style
        // operation and this legacy image-converter slot must stay closed.
        None
    }

    fn convert_shape(
        &self,
        evaluator: &FrameEvaluationContext,
        node: &crate::model::Node,
        time: f64,
    ) -> Option<RuntimeShape> {
        let props = &node.properties;
        let text = evaluator.require_string(props, "text", time, "text")?;
        let font = evaluator
            .optional_string(props, "font_family", time)
            .unwrap_or_else(|| "Arial".to_string());
        let size = evaluator.evaluate_number(props, "size", time, 12.0);
        if !size.is_finite() || size <= 0.0 {
            log::warn!(
                "Text Node {} has invalid size {size}; producing NoOutput",
                node.id
            );
            return None;
        }
        let transform = evaluator.build_transform(props, time);
        Some(RuntimeShape {
            source_id: node.id,
            geometry: RuntimeShapeGeometry::Text(layout_runtime_text_shape(
                &text,
                &font,
                size as f32,
            )),
            source_transform: transform.clone(),
            transform,
            effects: evaluator.build_image_effects(&node.effects, time),
            effector_configs: Vec::new(),
            decorator_configs: Vec::new(),
            properties: props.clone(),
        })
    }

    fn get_bounds(
        &self,
        evaluator: &FrameEvaluationContext,
        node: &crate::model::Node,
        time: f64,
    ) -> Option<(f32, f32, f32, f32)> {
        let props = &node.properties;
        let _comp_fps = evaluator.composition.fps;

        // Calculate evaluation time based on Node timeframe
        let eval_time = time;

        let text = evaluator.require_string(props, "text", eval_time, "text")?;
        let font_name = evaluator
            .optional_string(props, "font_family", eval_time)
            .unwrap_or_else(|| "Arial".to_string());
        let size = evaluator.evaluate_number(props, "size", eval_time, 12.0);

        let metrics = measure_text_layout(&text, &font_name, size as f32);
        let outset = text_style_outset(&[]);

        Some((
            -outset,
            -outset,
            metrics.width + outset * 2.0,
            metrics.height + outset * 2.0,
        ))
    }
}

pub fn measure_text_size(text: &str, primary_font_name: &str, size: f32) -> (f32, f32) {
    let metrics = measure_text_layout(text, primary_font_name, size);
    log::debug!(
        "measure_text_size: text='{}' font='{}' size={} -> w={} h={}",
        text,
        primary_font_name,
        size,
        metrics.width,
        metrics.height
    );
    (metrics.width, metrics.height)
}
