//! Timeline property sampling and FrameInfo item construction.

use super::*;

pub(super) fn stage_key(stage: ItemOutputStage) -> u8 {
    match stage {
        ItemOutputStage::Content => 0,
        ItemOutputStage::PostEffects => 1,
        ItemOutputStage::PostTransform => 2,
    }
}

pub(super) fn planned_source_matches(planned: PlannedSource, source: &SourceRef) -> bool {
    matches!(
        (planned, source),
        (PlannedSource::Asset, SourceRef::Asset { .. })
            | (PlannedSource::Text, SourceRef::Text { .. })
            | (PlannedSource::Shape, SourceRef::Shape { .. })
            | (PlannedSource::Solid, SourceRef::Solid { .. })
            | (PlannedSource::Module, SourceRef::Module(_))
    ) || matches!(
        (planned, source),
        (
            PlannedSource::Composition { timeline_id },
            SourceRef::Composition(instance)
        ) if timeline_id == instance.timeline_id
    )
}

pub(super) fn evaluate_property_map(
    properties: &PropertyMap,
    time: f64,
    owner: &str,
) -> Result<HashMap<String, PropertyValue>, LibraryError> {
    properties
        .iter()
        .map(|(key, property)| {
            property
                .evaluate_at(time)
                .map(|value| (key.clone(), value))
                .map_err(|error| {
                    LibraryError::Render(format!(
                        "Cannot evaluate {owner} property '{key}': {error}"
                    ))
                })
        })
        .collect()
}

pub(super) fn transform_at(properties: &PropertyMap, time: f64) -> Result<Transform, LibraryError> {
    transform_from_values(&evaluate_property_map(properties, time, "Timeline")?)
}

pub(super) fn transform_from_values(
    values: &HashMap<String, PropertyValue>,
) -> Result<Transform, LibraryError> {
    let mut transform = Transform::default();
    if let Some(value) = values.get("position") {
        let PropertyValue::Vec2(value) = value else {
            return Err(type_error("position", "Vec2"));
        };
        transform.position.x = value.x.into_inner();
        transform.position.y = value.y.into_inner();
    }
    if let Some(value) = values.get("scale") {
        let PropertyValue::Vec2(value) = value else {
            return Err(type_error("scale", "Vec2"));
        };
        transform.scale.x = value.x.into_inner();
        transform.scale.y = value.y.into_inner();
    }
    if let Some(value) = values.get("anchor") {
        let PropertyValue::Vec2(value) = value else {
            return Err(type_error("anchor", "Vec2"));
        };
        transform.anchor.x = value.x.into_inner();
        transform.anchor.y = value.y.into_inner();
    }
    if values.contains_key("rotation") {
        transform.rotation = required_number(values, "rotation", "Transform")?;
    }
    if values.contains_key("opacity") {
        transform.opacity = required_number(values, "opacity", "Transform")?;
    }
    Ok(transform)
}

pub(super) fn text_item_from_values(
    source_id: uuid::Uuid,
    text: &str,
    values: &HashMap<String, PropertyValue>,
    styles: Vec<StyleConfig>,
    ensemble: Option<crate::core::ensemble::EnsembleData>,
    current_time: f32,
) -> Result<FrameItem, LibraryError> {
    let font = values
        .get("font_family")
        .or_else(|| values.get("font"))
        .and_then(|value| match value {
            PropertyValue::String(value) => Some(value.clone()),
            _ => None,
        })
        .unwrap_or_else(|| crate::plugin::entity_converter::DEFAULT_TEXT_FONT_FAMILY.to_string());
    let size = values
        .get("size")
        .or_else(|| values.get("font_size"))
        .map(|_| {
            if values.contains_key("size") {
                required_number(values, "size", "Text item")
            } else {
                required_number(values, "font_size", "Text item")
            }
        })
        .transpose()?
        .unwrap_or(crate::plugin::entity_converter::DEFAULT_TIMELINE_TEXT_SIZE);
    let runtime_text =
        crate::core::rendering::text_layout::layout_runtime_text_shape(text, &font, size as f32);
    let content_bounds = crate::model::frame::runtime_shape::measure_text_visual_bounds(
        &runtime_text,
        &styles,
        ensemble.as_ref(),
        current_time,
    )?
    .map(|bounds| FrameBounds::new(bounds.left, bounds.top, bounds.width(), bounds.height()));
    Ok(FrameItem::Object(FrameObject {
        source_node_id: source_id,
        spatial_transform_node_id: None,
        spatial_transform: Box::default(),
        content_bounds,
        content: FrameContent::Text {
            text: text.to_string(),
            font,
            size,
            styles,
            effects: Vec::new(),
            ensemble,
            transform: Transform::default(),
        },
    }))
}

pub(super) fn shape_item(
    source_id: uuid::Uuid,
    shape: &crate::model::authoring::ShapeSource,
    styles: Vec<StyleConfig>,
) -> Result<FrameItem, LibraryError> {
    let width = direct_number(&shape.parameters, "width").unwrap_or(100.0);
    let height = direct_number(&shape.parameters, "height").unwrap_or(100.0);
    let (path, canonical_path, declared_bounds) = match shape.shape_kind {
        kind @ (crate::model::authoring::ShapeKind::Rectangle
        | crate::model::authoring::ShapeKind::Ellipse) => (
            crate::plugin::entity_converter::primitive_shape_path_data(kind, width, height)?,
            None,
            Some(FrameBounds::new(0.0, 0.0, width as f32, height as f32)),
        ),
        crate::model::authoring::ShapeKind::Path => match shape.parameters.get("path") {
            Some(PropertyValue::Path(path)) => (
                crate::model::path::write_legacy_svg_path_data(path)
                    .map_err(|error| LibraryError::Render(error.to_string()))?,
                Some(path.clone()),
                // A free Path need not start at (0, 0), and its authored
                // width/height parameters are not its painted bounds. Let the
                // shared Shape measurement calculate the actual local rect.
                None,
            ),
            _ => return Err(type_error("Shape path", "Path")),
        },
    };
    Ok(shape_object(
        source_id,
        path,
        canonical_path,
        styles,
        declared_bounds,
    ))
}

pub(super) fn solid_item(
    source_id: uuid::Uuid,
    width: u64,
    height: u64,
    color: crate::model::frame::color::Color,
    blend_mode: BlendMode,
) -> FrameItem {
    FrameItem::Group(FrameGroup {
        source_id,
        kind: FrameGroupKind::Node,
        width,
        height,
        background_color: transparent(),
        transform: Transform::default(),
        blend_mode,
        effect_time: OrderedFloat(0.0),
        effects: Vec::new(),
        items: vec![shape_object(
            source_id,
            format!("M 0 0 H {width} V {height} H 0 Z"),
            None,
            vec![fill_style(source_id, color)],
            Some(FrameBounds::new(0.0, 0.0, width as f32, height as f32)),
        )],
    })
}

fn fill_style(source_id: uuid::Uuid, color: crate::model::frame::color::Color) -> StyleConfig {
    StyleConfig {
        id: source_id,
        style: DrawStyle::Fill { color, offset: 0.0 },
    }
}

pub(super) fn shape_object(
    source_id: uuid::Uuid,
    path: String,
    canonical_path: Option<crate::model::path::PathValue>,
    styles: Vec<StyleConfig>,
    content_bounds: Option<FrameBounds>,
) -> FrameItem {
    let content_bounds = content_bounds.or_else(|| {
        crate::model::frame::runtime_shape::measure_shape_visual_bounds(&path, &styles, &[])
            .map(|(x, y, width, height)| FrameBounds::new(x, y, width, height))
    });
    FrameItem::Object(FrameObject {
        source_node_id: source_id,
        spatial_transform_node_id: None,
        spatial_transform: Box::default(),
        content_bounds,
        content: FrameContent::Shape {
            path,
            canonical_path,
            parts: Vec::new(),
            styles,
            path_effects: Vec::new(),
            effects: Vec::new(),
            ensemble: None,
            transform: Transform::default(),
        },
    })
}

pub(super) fn required_string(
    values: &HashMap<String, PropertyValue>,
    key: &str,
    owner: &str,
) -> Result<String, LibraryError> {
    match values.get(key) {
        Some(PropertyValue::String(value)) => Ok(value.clone()),
        _ => Err(type_error(&format!("{owner} {key}"), "String")),
    }
}

pub(super) fn required_number(
    values: &HashMap<String, PropertyValue>,
    key: &str,
    owner: &str,
) -> Result<f64, LibraryError> {
    match values.get(key) {
        Some(PropertyValue::Number(value)) => Ok(value.into_inner()),
        Some(PropertyValue::Integer(value)) => Ok(*value as f64),
        _ => Err(type_error(&format!("{owner} {key}"), "Number")),
    }
}

pub(super) fn required_color(
    values: &HashMap<String, PropertyValue>,
    key: &str,
    owner: &str,
) -> Result<crate::model::frame::color::Color, LibraryError> {
    match values.get(key) {
        Some(PropertyValue::Color(value)) => Ok(value.clone()),
        Some(PropertyValue::ColorValue(value)) => {
            crate::color_management::to_renderer_srgba8(value)
                .map_err(|error| LibraryError::Render(error.to_string()))
        }
        _ => Err(type_error(&format!("{owner} {key}"), "Color")),
    }
}

pub(super) fn direct_number(values: &HashMap<String, PropertyValue>, key: &str) -> Option<f64> {
    match values.get(key) {
        Some(PropertyValue::Number(value)) => Some(value.into_inner()),
        Some(PropertyValue::Integer(value)) => Some(*value as f64),
        _ => None,
    }
}

pub(super) fn type_error(owner: &str, expected: &str) -> LibraryError {
    LibraryError::Validation(format!("{owner} must evaluate to {expected}"))
}

pub(super) fn neutralize_root_blend(item: &mut FrameItem) {
    if let FrameItem::Group(group) = item {
        group.blend_mode = BlendMode::Normal;
    }
}

pub(super) fn transparent() -> crate::model::frame::color::Color {
    crate::model::frame::color::Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    }
}

#[cfg(test)]
mod shape_bounds_tests {
    use std::collections::HashMap;

    use super::*;
    use crate::model::authoring::{ShapeKind, ShapeSource};
    use crate::model::frame::entity::FrameItem;
    use crate::model::path::{FillRule, PathContour, PathPoint, PathSegment, PathValue};

    #[test]
    fn free_path_frame_bounds_keep_the_authored_local_origin() {
        let path = PathValue::new(
            FillRule::NonZero,
            vec![PathContour::new(
                PathPoint::new(120.0, 80.0),
                vec![
                    PathSegment::line(PathPoint::new(280.0, 80.0)),
                    PathSegment::line(PathPoint::new(280.0, 170.0)),
                    PathSegment::line(PathPoint::new(120.0, 170.0)),
                ],
                true,
            )],
        )
        .expect("Path");
        let shape = ShapeSource {
            shape_kind: ShapeKind::Path,
            // Deliberately conflicting presentation hints: Path geometry is
            // authoritative for its actual painted local bounds.
            parameters: HashMap::from([
                ("path".to_string(), PropertyValue::Path(path)),
                ("width".to_string(), PropertyValue::from(10.0)),
                ("height".to_string(), PropertyValue::from(20.0)),
            ]),
            appearance_operations: Vec::new(),
        };

        let source_id = uuid::Uuid::new_v4();
        let FrameItem::Object(object) = shape_item(
            source_id,
            &shape,
            vec![fill_style(
                source_id,
                crate::model::frame::color::Color::white(),
            )],
        )
        .unwrap() else {
            panic!("Shape object");
        };
        let bounds = object.content_bounds.expect("measured Path bounds");
        assert_eq!(bounds.as_tuple(), (120.0, 80.0, 160.0, 90.0));
    }

    #[test]
    fn ellipse_path_and_declared_gizmo_bounds_share_the_same_origin() {
        let shape = ShapeSource {
            shape_kind: ShapeKind::Ellipse,
            parameters: HashMap::from([
                ("width".to_string(), PropertyValue::from(160.0)),
                ("height".to_string(), PropertyValue::from(80.0)),
            ]),
            appearance_operations: Vec::new(),
        };
        let source_id = uuid::Uuid::new_v4();
        let FrameItem::Object(object) = shape_item(
            source_id,
            &shape,
            vec![fill_style(
                source_id,
                crate::model::frame::color::Color::white(),
            )],
        )
        .unwrap() else {
            panic!("Shape object");
        };
        let FrameContent::Shape {
            path,
            styles,
            path_effects,
            ..
        } = &object.content
        else {
            panic!("Ellipse content");
        };
        let painted = crate::model::frame::runtime_shape::measure_shape_visual_bounds(
            path,
            styles,
            path_effects,
        )
        .expect("painted ellipse bounds");
        let declared = object.content_bounds.expect("declared ellipse bounds");
        let declared = declared.as_tuple();
        for (actual, expected) in [painted.0, painted.1, painted.2, painted.3]
            .into_iter()
            .zip([declared.0, declared.1, declared.2, declared.3])
        {
            assert!((actual - expected).abs() <= 0.01, "{actual} != {expected}");
        }
    }
}
