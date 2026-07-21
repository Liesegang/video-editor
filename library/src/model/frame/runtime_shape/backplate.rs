//! Geometry-only Backplate evaluation.
//!
//! The operation consumes a target Shape for layout metadata and an arbitrary
//! Path Shape as the background template. It outputs only fitted path geometry;
//! color, stroke, opacity, rounded corners, and every other appearance concern
//! remain owned by the template generator and downstream Style operations.

use std::collections::BTreeMap;

use skia_safe::{Matrix, Path, PathBuilder, PathOp, Rect};
use uuid::Uuid;

use super::{
    RuntimeBounds, RuntimePathPart, RuntimePathShape, RuntimeShape, RuntimeShapeGeometry,
    RuntimeTextShape, evaluate_text_element_transforms, text_element_center, union_indices,
};
use crate::core::ensemble::decorators::{BackplateFit, BackplateTarget};
use crate::core::ensemble::types::{DecoratorConfig, EnsembleData, TransformData};
use crate::error::LibraryError;
use crate::model::frame::draw_type::PathEffect;
use crate::model::frame::transform::Transform;

#[derive(Clone)]
struct TargetRegion {
    bounds: RuntimeBounds,
    stable_id: u64,
    block_group_id: u64,
    line_group_id: u64,
    line_index: usize,
    opacity: f32,
    post_fit_transform: Option<Matrix>,
}

impl RuntimeShape {
    /// Consume this target Shape and fit `background` geometry to the selected
    /// semantic groups. The output source is the Backplate operation Node, but
    /// root placement remains owned by the target's explicit Transform Node.
    pub fn into_backplate_geometry(
        self,
        operation_id: Uuid,
        background: RuntimeShape,
        config: DecoratorConfig,
        current_time: f32,
    ) -> Result<Self, LibraryError> {
        let DecoratorConfig::Backplate {
            target,
            padding,
            offset,
            fit,
        } = config
        else {
            return Err(LibraryError::Validation(
                "legacy Backplate config cannot execute the two-Shape contract".to_string(),
            ));
        };
        if target == BackplateTarget::Parts {
            return Err(LibraryError::Validation(
                "Backplate target Parts requires authored path-part semantics".to_string(),
            ));
        }
        validate_layout(padding, offset)?;

        let (template_path, template_path_effects) = background_path(&background)?;
        if template_path.is_empty() {
            return Err(LibraryError::Validation(
                "Backplate background Shape must not be empty".to_string(),
            ));
        }
        let template_path = apply_background_transform(&template_path, &background.transform)?;
        let template_bounds = template_path.compute_tight_bounds();
        if !template_bounds.is_finite()
            || template_bounds.width() <= f32::EPSILON
            || template_bounds.height() <= f32::EPSILON
        {
            return Err(LibraryError::Validation(
                "Backplate background Shape must have finite non-zero bounds".to_string(),
            ));
        }

        let regions = target_regions(&self, target, current_time)?;
        if regions.is_empty() {
            return Err(LibraryError::Validation(
                "Backplate target Shape has no addressable geometry".to_string(),
            ));
        }
        let background_opacity = finite_f32(background.transform.opacity, "opacity")?;
        let mut parts = Vec::with_capacity(regions.len());
        for region in regions {
            let destination = region.bounds.pad(padding).translate(offset);
            let fitted = fit_template(
                &template_path,
                template_bounds,
                destination,
                fit,
                region.post_fit_transform.as_ref(),
            )?;
            let bounds = fitted.compute_tight_bounds();
            let bounds = RuntimeBounds::new(bounds.left, bounds.top, bounds.right, bounds.bottom);
            parts.push(RuntimePathPart {
                path: fitted.to_svg(),
                bounds,
                stable_id: region.stable_id,
                block_group_id: region.block_group_id,
                line_group_id: region.line_group_id,
                line_index: region.line_index,
                opacity: region.opacity * background_opacity,
            });
        }

        let bounds = parts
            .iter()
            .map(|part| part.bounds)
            .reduce(RuntimeBounds::union)
            .ok_or_else(|| {
                LibraryError::Validation("Backplate produced no finite geometry".to_string())
            })?;
        let path = parts
            .iter()
            .map(|part| part.path.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        Ok(Self {
            source_id: operation_id,
            geometry: RuntimeShapeGeometry::Path(RuntimePathShape {
                path,
                bounds,
                path_effects: template_path_effects,
                parts,
            }),
            spatial_transform_node_id: self.spatial_transform_node_id,
            spatial_transform: self.spatial_transform,
            modulation_transform: self.modulation_transform,
            transform: self.transform,
            effects: background.effects,
            // Target element modulation was baked into fitted geometry. A
            // downstream Effector can start a new explicit Shape operation.
            effector_configs: Vec::new(),
            decorator_configs: Vec::new(),
        })
    }
}

fn background_path(background: &RuntimeShape) -> Result<(Path, Vec<PathEffect>), LibraryError> {
    match &background.geometry {
        RuntimeShapeGeometry::Path(template) => {
            let path = Path::from_svg(&template.path).ok_or_else(|| {
                LibraryError::Validation(
                    "Backplate background Shape has invalid path data".to_string(),
                )
            })?;
            Ok((path, template.path_effects.clone()))
        }
        RuntimeShapeGeometry::Text(template) => {
            let mut paragraph = crate::core::rendering::text_layout::build_text_paragraph(
                &template.text,
                &template.font,
                template.size as f32,
                None,
            );
            let mut builder = PathBuilder::new();
            for line in 0..template.lines.len() {
                let (unconverted_glyphs, path) = paragraph.get_path_at(line);
                if unconverted_glyphs != 0 {
                    return Err(LibraryError::Validation(format!(
                        "Backplate background Text Shape contains {unconverted_glyphs} glyphs without vector outlines"
                    )));
                }
                builder.add_path(&path);
            }
            let path = builder.detach();
            if path.is_empty() {
                return Err(LibraryError::Validation(
                    "Backplate background Text Shape has no vector outline".to_string(),
                ));
            }
            Ok((path, Vec::new()))
        }
    }
}

fn apply_background_transform(path: &Path, transform: &Transform) -> Result<Path, LibraryError> {
    let affine = crate::core::rendering::renderer::Affine2D::from(transform);
    let values = [
        affine.scale_x,
        affine.skew_x,
        affine.translate_x,
        affine.skew_y,
        affine.scale_y,
        affine.translate_y,
    ];
    if values
        .iter()
        .any(|value| !value.is_finite() || !(*value as f32).is_finite())
    {
        return Err(LibraryError::Validation(
            "Backplate background Shape transform must be finite".to_string(),
        ));
    }
    let matrix = Matrix::new_all(
        values[0] as f32,
        values[1] as f32,
        values[2] as f32,
        values[3] as f32,
        values[4] as f32,
        values[5] as f32,
        0.0,
        0.0,
        1.0,
    );
    path.try_make_transform(&matrix).ok_or_else(|| {
        LibraryError::Validation(
            "Backplate background Shape transform produced invalid geometry".to_string(),
        )
    })
}

fn finite_f32(value: f64, field: &str) -> Result<f32, LibraryError> {
    let converted = value as f32;
    if value.is_finite() && converted.is_finite() {
        Ok(converted)
    } else {
        Err(LibraryError::Validation(format!(
            "Backplate background Shape {field} must fit the finite f32 render contract"
        )))
    }
}

fn validate_layout(padding: (f32, f32, f32, f32), offset: (f32, f32)) -> Result<(), LibraryError> {
    if [
        padding.0, padding.1, padding.2, padding.3, offset.0, offset.1,
    ]
    .into_iter()
    .any(|value| !value.is_finite())
    {
        return Err(LibraryError::Validation(
            "Backplate padding and offset must be finite".to_string(),
        ));
    }
    Ok(())
}

fn target_regions(
    shape: &RuntimeShape,
    target: BackplateTarget,
    current_time: f32,
) -> Result<Vec<TargetRegion>, LibraryError> {
    match &shape.geometry {
        RuntimeShapeGeometry::Text(text) => text_target_regions(shape, text, target, current_time),
        RuntimeShapeGeometry::Path(path) => Ok(path_target_regions(path, target)),
    }
}

fn text_target_regions(
    shape: &RuntimeShape,
    text: &RuntimeTextShape,
    target: BackplateTarget,
    current_time: f32,
) -> Result<Vec<TargetRegion>, LibraryError> {
    let ensemble = EnsembleData {
        enabled: true,
        effector_configs: shape.effector_configs.clone(),
        decorator_configs: Vec::new(),
        patches: Default::default(),
    };
    let transforms = evaluate_text_element_transforms(text, &ensemble, current_time)?;
    Ok(match target {
        BackplateTarget::Char => text
            .elements
            .iter()
            .zip(&transforms)
            .filter(|(_, transform)| transform.opacity > 0.0)
            .map(|(element, transform)| TargetRegion {
                bounds: element.bounds,
                stable_id: element.element_group_id,
                block_group_id: element.block_group_id,
                line_group_id: element.line_group_id,
                line_index: element.line_index,
                opacity: transform.opacity,
                post_fit_transform: Some(element_transform_matrix(element, transform)),
            })
            .collect(),
        BackplateTarget::Line => text
            .lines
            .iter()
            .filter_map(|line| {
                let indices = line.element_range.clone().collect::<Vec<_>>();
                let bounds = union_indices(text, &transforms, indices.iter().copied())?;
                let opacity = indices
                    .iter()
                    .map(|index| transforms[*index].opacity)
                    .sum::<f32>()
                    / indices.len().max(1) as f32;
                (opacity > 0.0).then_some(TargetRegion {
                    bounds,
                    stable_id: line.group_id,
                    block_group_id: text.block_group_id,
                    line_group_id: line.group_id,
                    line_index: line.index,
                    opacity,
                    post_fit_transform: None,
                })
            })
            .collect(),
        BackplateTarget::Block => {
            let indices = 0..text.elements.len();
            let Some(bounds) = union_indices(text, &transforms, indices) else {
                return Ok(Vec::new());
            };
            let opacity = transforms
                .iter()
                .map(|transform| transform.opacity)
                .sum::<f32>()
                / transforms.len().max(1) as f32;
            if opacity <= 0.0 {
                Vec::new()
            } else {
                vec![TargetRegion {
                    bounds,
                    stable_id: text.block_group_id,
                    block_group_id: text.block_group_id,
                    line_group_id: text.block_group_id,
                    line_index: 0,
                    opacity,
                    post_fit_transform: None,
                }]
            }
        }
        BackplateTarget::Parts => Vec::new(),
    })
}

fn path_target_regions(path: &RuntimePathShape, target: BackplateTarget) -> Vec<TargetRegion> {
    match target {
        BackplateTarget::Char => path.parts.iter().map(region_from_part).collect(),
        BackplateTarget::Line => {
            let mut groups: BTreeMap<(usize, u64), Vec<&RuntimePathPart>> = BTreeMap::new();
            for part in &path.parts {
                groups
                    .entry((part.line_index, part.line_group_id))
                    .or_default()
                    .push(part);
            }
            groups
                .into_iter()
                .filter_map(|((line_index, line_group_id), parts)| {
                    reduce_path_parts(&parts).map(|(bounds, opacity)| TargetRegion {
                        bounds,
                        stable_id: line_group_id,
                        block_group_id: parts[0].block_group_id,
                        line_group_id,
                        line_index,
                        opacity,
                        post_fit_transform: None,
                    })
                })
                .collect()
        }
        BackplateTarget::Block => reduce_path_parts(&path.parts.iter().collect::<Vec<_>>())
            .map(|(bounds, opacity)| {
                let part = &path.parts[0];
                vec![TargetRegion {
                    bounds,
                    stable_id: part.block_group_id,
                    block_group_id: part.block_group_id,
                    line_group_id: part.block_group_id,
                    line_index: 0,
                    opacity,
                    post_fit_transform: None,
                }]
            })
            .unwrap_or_default(),
        BackplateTarget::Parts => Vec::new(),
    }
}

fn region_from_part(part: &RuntimePathPart) -> TargetRegion {
    TargetRegion {
        bounds: part.bounds,
        stable_id: part.stable_id,
        block_group_id: part.block_group_id,
        line_group_id: part.line_group_id,
        line_index: part.line_index,
        opacity: part.opacity,
        post_fit_transform: None,
    }
}

fn reduce_path_parts(parts: &[&RuntimePathPart]) -> Option<(RuntimeBounds, f32)> {
    let bounds = parts
        .iter()
        .map(|part| part.bounds)
        .reduce(RuntimeBounds::union)?;
    let opacity = parts.iter().map(|part| part.opacity).sum::<f32>() / parts.len() as f32;
    Some((bounds, opacity))
}

fn element_transform_matrix(
    element: &super::RuntimeTextElement,
    transform: &TransformData,
) -> Matrix {
    let center = text_element_center(element);
    let radians = transform.rotate.to_radians();
    let (sin, cos) = radians.sin_cos();
    let sx = transform.scale.0;
    let sy = transform.scale.1;
    Matrix::new_all(
        sx * cos,
        -sy * sin,
        center.x + transform.translate.0 - center.x * sx * cos + center.y * sy * sin,
        sx * sin,
        sy * cos,
        center.y + transform.translate.1 - center.x * sx * sin - center.y * sy * cos,
        0.0,
        0.0,
        1.0,
    )
}

fn fit_template(
    template: &Path,
    source: skia_safe::Rect,
    destination: RuntimeBounds,
    fit: BackplateFit,
    post_fit_transform: Option<&Matrix>,
) -> Result<Path, LibraryError> {
    let width = destination.width();
    let height = destination.height();
    if width <= f32::EPSILON
        || height <= f32::EPSILON
        || ![
            destination.left,
            destination.top,
            destination.right,
            destination.bottom,
        ]
        .into_iter()
        .all(f32::is_finite)
    {
        return Err(LibraryError::Validation(
            "Backplate padding produced non-positive or non-finite bounds".to_string(),
        ));
    }

    let direct_x = width / source.width();
    let direct_y = height / source.height();
    let (scale_x, scale_y) = match fit {
        BackplateFit::Stretch => (direct_x, direct_y),
        BackplateFit::Contain => {
            let scale = direct_x.min(direct_y);
            (scale, scale)
        }
        BackplateFit::Cover => {
            let scale = direct_x.max(direct_y);
            (scale, scale)
        }
    };
    let translate_x = (destination.left + destination.right) * 0.5 - source.center_x() * scale_x;
    let translate_y = (destination.top + destination.bottom) * 0.5 - source.center_y() * scale_y;
    let fit_matrix = Matrix::scale_translate((scale_x, scale_y), (translate_x, translate_y));
    let mut fitted = template.try_make_transform(&fit_matrix).ok_or_else(|| {
        LibraryError::Validation("Backplate fit produced invalid path geometry".to_string())
    })?;
    if fit == BackplateFit::Cover {
        let clip = Path::rect(
            Rect::new(
                destination.left,
                destination.top,
                destination.right,
                destination.bottom,
            ),
            None,
        );
        fitted = fitted
            .op(&clip, PathOp::Intersect)
            .ok_or_else(|| LibraryError::Validation("Backplate Cover crop failed".to_string()))?;
    }
    if let Some(post) = post_fit_transform {
        fitted = fitted.try_make_transform(post).ok_or_else(|| {
            LibraryError::Validation(
                "Backplate target transform produced invalid geometry".to_string(),
            )
        })?;
    }
    Ok(fitted)
}
