use super::{EntityConverterPlugin, FrameEvaluationContext};
use crate::model::frame::entity::FrameObject;
use crate::model::frame::runtime_shape::{
    RuntimeBounds, RuntimePathShape, RuntimeShape, RuntimeShapeGeometry,
    measure_shape_visual_bounds,
};

#[derive(Default)]
pub struct ShapeEntityConverterPlugin;

impl ShapeEntityConverterPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl crate::plugin::Plugin for ShapeEntityConverterPlugin {
    fn id(&self) -> &'static str {
        "shape_entity_converter"
    }

    fn name(&self) -> String {
        "Shape Entity Converter".to_string()
    }

    fn category(&self) -> String {
        "Converter".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EntityConverterPlugin for ShapeEntityConverterPlugin {
    fn supports_kind(&self, kind: &str) -> bool {
        kind == "shape"
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
            // Shape Properties
            PropertyDefinition::new(
                "path",
                PropertyUiType::MultilineText,
                "Path Data",
                PropertyValue::String("".to_string()),
            ),
            PropertyDefinition::new(
                "path_effect",
                PropertyUiType::Dropdown {
                    options: vec![
                        "None".to_string(),
                        "Dash".to_string(),
                        "Corner".to_string(),
                        "Discrete".to_string(),
                        "Trim".to_string(),
                    ],
                },
                "Path Effect",
                PropertyValue::String("None".to_string()),
            ),
            PropertyDefinition::new(
                "path_effect_intervals",
                PropertyUiType::Text,
                "Dash Intervals",
                PropertyValue::String("8 4".to_string()),
            ),
            PropertyDefinition::new(
                "path_effect_phase",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 1000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Dash Phase",
                PropertyValue::Number(OrderedFloat(0.0)),
            ),
            PropertyDefinition::new(
                "path_effect_radius",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 1000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: true,
                    max_hard_limit: false,
                },
                "Corner Radius",
                PropertyValue::Number(OrderedFloat(8.0)),
            ),
            PropertyDefinition::new(
                "path_effect_segment_length",
                PropertyUiType::Float {
                    min: 0.1,
                    max: 1000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: true,
                    max_hard_limit: false,
                },
                "Discrete Segment",
                PropertyValue::Number(OrderedFloat(8.0)),
            ),
            PropertyDefinition::new(
                "path_effect_deviation",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 1000.0,
                    step: 1.0,
                    suffix: "px".to_string(),
                    min_hard_limit: true,
                    max_hard_limit: false,
                },
                "Discrete Deviation",
                PropertyValue::Number(OrderedFloat(2.0)),
            ),
            PropertyDefinition::new(
                "path_effect_seed",
                PropertyUiType::Integer {
                    min: 0,
                    max: i64::MAX,
                    suffix: String::new(),
                    min_hard_limit: true,
                    max_hard_limit: true,
                },
                "Discrete Seed",
                PropertyValue::Integer(0),
            ),
            PropertyDefinition::new(
                "path_effect_trim_start",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 1.0,
                    step: 0.01,
                    suffix: String::new(),
                    min_hard_limit: true,
                    max_hard_limit: true,
                },
                "Trim Start",
                PropertyValue::Number(OrderedFloat(0.0)),
            ),
            PropertyDefinition::new(
                "path_effect_trim_end",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 1.0,
                    step: 0.01,
                    suffix: String::new(),
                    min_hard_limit: true,
                    max_hard_limit: true,
                },
                "Trim End",
                PropertyValue::Number(OrderedFloat(1.0)),
            ),
        ]
    }

    fn convert_entity(
        &self,
        _evaluator: &FrameEvaluationContext,
        _node: &crate::model::Node,
        _time: f64,
    ) -> Option<FrameObject> {
        // Path generators are Shape-only. An explicit Style operation owns
        // the sole Shape -> Image boundary.
        None
    }

    fn convert_shape(
        &self,
        evaluator: &FrameEvaluationContext,
        node: &crate::model::Node,
        time: f64,
    ) -> Option<RuntimeShape> {
        let props = &node.properties;
        let path = evaluator.require_string(props, "path", time, "shape")?;
        let parsed = skia_safe::utils::parse_path::from_svg(&path)?;
        if parsed.is_empty() {
            return None;
        }
        let bounds = parsed.compute_tight_bounds();
        Some(RuntimeShape {
            source_id: node.id,
            geometry: RuntimeShapeGeometry::Path(RuntimePathShape {
                path,
                bounds: RuntimeBounds::new(bounds.left, bounds.top, bounds.right, bounds.bottom),
                path_effects: evaluator.parse_path_effects(props, time),
            }),
            transform: evaluator.build_transform(props, time),
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

        let path_str = evaluator.require_string(props, "path", eval_time, "shape")?;
        let path_effects = evaluator.parse_path_effects(props, eval_time);
        measure_shape_visual_bounds(&path_str, &[], &path_effects)
    }
}
