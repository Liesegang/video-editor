use super::{EntityConverterPlugin, FrameEvaluationContext};
use crate::model::frame::entity::FrameObject;
use crate::model::frame::runtime_shape::{
    RuntimeBounds, RuntimePathPart, RuntimePathShape, RuntimeShape, RuntimeShapeGeometry,
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
        use crate::model::path::{FillRule, PathValue};
        use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue};

        vec![PropertyDefinition::new(
            "path",
            PropertyUiType::Path,
            "Path Data",
            PropertyValue::Path(PathValue::empty(FillRule::NonZero)),
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
        let path_value = evaluator.require_path_value(props, "path", time, "shape")?;
        runtime_path_shape(node.id, path_value)
            .inspect_err(|error| log::error!("{error}"))
            .ok()?
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

        let path_value = evaluator.require_path_value(props, "path", eval_time, "shape")?;
        let path = crate::core::rendering::path_geometry::to_skia_path(&path_value).ok()?;
        if path.is_empty() {
            return None;
        }
        let bounds = path.compute_tight_bounds();
        Some((bounds.left, bounds.top, bounds.width(), bounds.height()))
    }
}

/// Creates the canonical transient Path Shape from an already evaluated
/// canonical path. It is shared by both production graph runtimes.
pub(crate) fn runtime_path_shape(
    source_id: uuid::Uuid,
    path_value: crate::model::path::PathValue,
) -> Result<Option<RuntimeShape>, crate::error::LibraryError> {
    let parsed =
        crate::core::rendering::path_geometry::to_skia_path(&path_value).map_err(|error| {
            crate::error::LibraryError::Render(format!(
                "Shape Node {source_id} cannot cross the Skia path boundary: {error}"
            ))
        })?;
    if parsed.is_empty() {
        return Ok(None);
    }
    let path = crate::model::path::encode_svg_path(&path_value)
        .map_err(|error| {
            crate::error::LibraryError::Render(format!(
                "Shape Node {source_id} cannot create its SVG fallback: {error}"
            ))
        })?
        .into_path_data();
    let bounds = parsed.compute_tight_bounds();
    let runtime_bounds = RuntimeBounds::new(bounds.left, bounds.top, bounds.right, bounds.bottom);
    let stable_id = source_id.as_u128() as u64;
    Ok(Some(RuntimeShape {
        source_id,
        geometry: RuntimeShapeGeometry::Path(RuntimePathShape {
            path: path.clone(),
            canonical_path: Some(path_value.clone()),
            bounds: runtime_bounds,
            path_effects: Vec::new(),
            parts: vec![RuntimePathPart {
                path,
                canonical_path: Some(path_value),
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
    }))
}
