use super::{EntityConverterPlugin, FrameEvaluationContext};
use crate::model::frame::entity::FrameObject;
use crate::model::frame::runtime_shape::{
    RuntimeBounds, RuntimePathPart, RuntimePathShape, RuntimeShape, RuntimeShapeGeometry,
};

/// Produces the canonical local-space contour for a parameterized Timeline or
/// Module primitive. Both authoring surfaces must use this exact geometry so
/// promoting a Clip to a Node Clip cannot change its pixels or gizmo bounds.
pub(crate) fn primitive_shape_path_data(
    kind: crate::model::authoring::ShapeKind,
    width: f64,
    height: f64,
) -> Result<String, crate::error::LibraryError> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err(crate::error::LibraryError::Validation(
            "Primitive Shape width and height must be positive and finite".to_string(),
        ));
    }
    match kind {
        crate::model::authoring::ShapeKind::Rectangle => {
            Ok(format!("M 0 0 H {width} V {height} H 0 Z"))
        }
        crate::model::authoring::ShapeKind::Ellipse => Ok(format!(
            "M {width} {} A {} {} 0 1 1 0 {} A {} {} 0 1 1 {width} {} Z",
            height / 2.0,
            width / 2.0,
            height / 2.0,
            height / 2.0,
            width / 2.0,
            height / 2.0,
            height / 2.0
        )),
        crate::model::authoring::ShapeKind::Path => Err(crate::error::LibraryError::Validation(
            "A free Path is not a parameterized primitive".to_string(),
        )),
    }
}

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
