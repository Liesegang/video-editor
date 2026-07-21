use super::{EntityConverterPlugin, FrameEvaluationContext};
use crate::model::frame::entity::FrameObject;
use crate::model::frame::runtime_shape::{
    RuntimeBounds, RuntimePathPart, RuntimePathShape, RuntimeShape, RuntimeShapeGeometry,
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
        _canvas_width: u64,
        _canvas_height: u64,
        _clip_width: u64,
        _clip_height: u64,
    ) -> Vec<crate::model::property::PropertyDefinition> {
        use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue};

        vec![PropertyDefinition::new(
            "path",
            PropertyUiType::MultilineText,
            "Path Data",
            PropertyValue::String("".to_string()),
        )]
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
        let props = node.properties();
        let path = evaluator.require_string(props, "path", time, "shape")?;
        let parsed = skia_safe::utils::parse_path::from_svg(&path)?;
        if parsed.is_empty() {
            return None;
        }
        let bounds = parsed.compute_tight_bounds();
        let runtime_bounds =
            RuntimeBounds::new(bounds.left, bounds.top, bounds.right, bounds.bottom);
        let stable_id = node.id.as_u128() as u64;
        Some(RuntimeShape {
            source_id: node.id,
            geometry: RuntimeShapeGeometry::Path(RuntimePathShape {
                path: path.clone(),
                bounds: runtime_bounds,
                // Authored path effects live only on explicit Shape -> Shape
                // Path Effect operations. This Vec is render-only state.
                path_effects: Vec::new(),
                parts: vec![RuntimePathPart {
                    path,
                    bounds: runtime_bounds,
                    stable_id,
                    block_group_id: stable_id,
                    line_group_id: stable_id,
                    line_index: 0,
                    opacity: 1.0,
                }],
            }),
            spatial_transform_node_id: None,
            spatial_transform: Default::default(),
            modulation_transform: Default::default(),
            transform: Default::default(),
            effects: Vec::new(),
            effector_configs: Vec::new(),
            decorator_configs: Vec::new(),
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

        let path_str = evaluator.require_string(props, "path", eval_time, "shape")?;
        measure_shape_visual_bounds(&path_str, &[], &[])
    }
}
