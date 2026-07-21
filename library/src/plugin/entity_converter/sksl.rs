use super::{EntityConverterPlugin, FrameEvaluationContext};
use crate::model::frame::entity::{FrameBounds, FrameContent, FrameObject};

#[derive(Default)]
pub struct SkSLEntityConverterPlugin;

impl SkSLEntityConverterPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl crate::plugin::Plugin for SkSLEntityConverterPlugin {
    fn id(&self) -> &'static str {
        "sksl_entity_converter"
    }

    fn name(&self) -> String {
        "SkSL Entity Converter".to_string()
    }

    fn category(&self) -> String {
        "Converter".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EntityConverterPlugin for SkSLEntityConverterPlugin {
    fn supports_kind(&self, kind: &str) -> bool {
        kind == "sksl"
    }

    fn get_property_definitions(
        &self,
        canvas_width: u64,
        canvas_height: u64,
        _clip_width: u64,
        _clip_height: u64,
    ) -> Vec<crate::model::property::PropertyDefinition> {
        use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue};
        use ordered_float::OrderedFloat;

        vec![
            // Shader Properties
            PropertyDefinition::new(
                "shader",
                PropertyUiType::MultilineText,
                "Shader Code",
                PropertyValue::String("".to_string()),
            ),
            PropertyDefinition::new(
                "width",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 10000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Width",
                PropertyValue::Number(OrderedFloat(canvas_width as f64)),
            ),
            PropertyDefinition::new(
                "height",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 10000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Height",
                PropertyValue::Number(OrderedFloat(canvas_height as f64)),
            ),
        ]
    }

    fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext,
        node: &crate::model::Node,
        time: f64,
    ) -> Option<FrameObject> {
        let props = node.properties();
        let _comp_fps = evaluator.composition.fps;

        // Calculate evaluation time based on Node timeframe
        let eval_time = time;

        let shader = evaluator.require_string(props, "shader", eval_time, "sksl")?;

        let (default_width, default_height) = evaluator.evaluation_resolution();
        let res_x = evaluator.evaluate_number(props, "width", eval_time, default_width as f64);
        let res_y = evaluator.evaluate_number(props, "height", eval_time, default_height as f64);

        Some(FrameObject {
            source_node_id: node.id,
            spatial_transform_node_id: None,
            spatial_transform: Box::default(),
            content_bounds: Some(FrameBounds::new(0.0, 0.0, res_x as f32, res_y as f32)),
            content: FrameContent::SkSL {
                shader,
                resolution: (res_x as f32, res_y as f32),
                effects: Vec::new(),
                transform: Default::default(),
            },
        })
    }

    fn get_bounds(
        &self,
        evaluator: &FrameEvaluationContext,
        node: &crate::model::Node,
        time: f64,
    ) -> Option<(f32, f32, f32, f32)> {
        let props = node.properties();
        let _comp_fps = evaluator.composition.fps;

        // Calculate evaluation time based on Node timeframe
        let eval_time = time;

        let (default_width, default_height) = evaluator.evaluation_resolution();
        let width = evaluator.evaluate_number(props, "width", eval_time, default_width as f64);
        let height = evaluator.evaluate_number(props, "height", eval_time, default_height as f64);

        Some((0.0, 0.0, width as f32, height as f32))
    }
}
