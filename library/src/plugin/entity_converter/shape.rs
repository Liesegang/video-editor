use super::{EntityConverterPlugin, FrameEvaluationContext};
use crate::model::frame::entity::{FrameContent, FrameObject};
// use crate::model::project::TrackClip;

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
        let time_since_start = time - layer.start_time.into_inner();
        let eval_time =
            time_since_start * layer.time_stretch.into_inner() + layer.trim_in.into_inner();

        let path = evaluator.require_string(props, "path", eval_time, "shape")?;
        let transform = evaluator.build_transform(props, eval_time);

        let styles = evaluator.build_styles(&layer.styles, eval_time);

        // Uses the signature defined in mod.rs: parse_path_effects(&self, props: &PropertyMap, time: f64)
        let path_effects = evaluator.parse_path_effects(props, eval_time);

        let effects = evaluator.build_image_effects(&layer.effects, eval_time);

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
        layer: &crate::model::Layer,
        time: f64,
    ) -> Option<(f32, f32, f32, f32)> {
        let props = &layer.properties;
        let _comp_fps = evaluator.composition.fps;

        // Calculate evaluation time based on Layer timeframe
        let time_since_start = time - layer.start_time.into_inner();
        let eval_time =
            time_since_start * layer.time_stretch.into_inner() + layer.trim_in.into_inner();

        let path_str = evaluator.require_string(props, "path", eval_time, "shape")?;

        if let Some(path) = skia_safe::utils::parse_path::from_svg(&path_str) {
            let bounds = path.compute_tight_bounds();
            Some((bounds.left, bounds.top, bounds.width(), bounds.height()))
        } else {
            Some((0.0, 0.0, 100.0, 100.0))
        }
    }
}
