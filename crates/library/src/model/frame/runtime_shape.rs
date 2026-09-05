//! Render-only vector/typographic value carried by `PortDataType::Shape`.
//!
//! This is deliberately not serialized. The authoritative authored state is
//! always the Project graph; a RuntimeShape is only an evaluated value moving
//! left-to-right between graph operations for one frame.

use std::ops::Range;

use uuid::Uuid;

use crate::core::ensemble::effectors::{EffectorElementContext, evaluate_configured_transform};
use crate::core::ensemble::types::{DecoratorConfig, EffectorConfig, EnsembleData, TransformData};
use crate::error::LibraryError;
use crate::model::frame::draw_type::PathEffect;
use crate::model::frame::effect::ImageEffect;
use crate::model::frame::entity::{
    FrameBounds, FrameContent, FrameObject, FramePathPart, StyleConfig,
};
use crate::model::frame::transform::Transform;
use crate::model::path::PathValue;

mod backplate;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RuntimeBounds {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl RuntimeBounds {
    pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn width(self) -> f32 {
        (self.right - self.left).max(0.0)
    }

    pub fn height(self) -> f32 {
        (self.bottom - self.top).max(0.0)
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }

    pub fn expand(self, amount: f32) -> Self {
        self.pad((amount, amount, amount, amount))
    }

    /// Expand by `(top, right, bottom, left)` in local coordinates.
    pub fn pad(self, padding: (f32, f32, f32, f32)) -> Self {
        Self {
            left: self.left - padding.3,
            top: self.top - padding.0,
            right: self.right + padding.1,
            bottom: self.bottom + padding.2,
        }
    }

    pub fn translate(self, offset: (f32, f32)) -> Self {
        Self {
            left: self.left + offset.0,
            top: self.top + offset.1,
            right: self.right + offset.0,
            bottom: self.bottom + offset.1,
        }
    }
}

/// Return the local-space bounds painted by Shape rendering.
///
/// Tight path geometry alone excludes positive fill offsets, strokes, and
/// Discrete path deviation. Keeping this next to the runtime Shape value lets
/// both conversion and the final `FrameObject` use one calculation.
pub fn measure_shape_visual_bounds(
    path_data: &str,
    styles: &[StyleConfig],
    path_effects: &[PathEffect],
) -> Option<(f32, f32, f32, f32)> {
    let path = skia_safe::utils::parse_path::from_svg(path_data)?;
    if path.is_empty() {
        return None;
    }
    let bounds = path.compute_tight_bounds();
    let outset = shape_visual_outset(styles, path_effects);

    Some((
        bounds.left - outset,
        bounds.top - outset,
        bounds.width() + outset * 2.0,
        bounds.height() + outset * 2.0,
    ))
}

fn shape_visual_outset(styles: &[StyleConfig], path_effects: &[PathEffect]) -> f32 {
    crate::model::frame::appearance::appearance_outsets(styles).visual
        + crate::model::frame::appearance::path_effect_outset(path_effects)
}

/// One Unicode grapheme element. It may contain multiple Unicode scalars, and
/// deliberately does not claim to expose shaped glyph IDs or outlines. The
/// normal non-Ensemble raster path shapes the whole source with SkParagraph.
/// The current Ensemble path re-renders each grapheme with `Font::draw_str`,
/// so it does not preserve ligatures or contextual shaping across elements.
/// TODO: carry real SkParagraph shaping-run/source mapping before applying
/// per-element transforms to complex scripts.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeTextElement {
    /// Exact source slice represented by this element.
    pub source: String,
    pub utf8_range: Range<usize>,
    pub utf16_range: Range<usize>,
    pub line_index: usize,
    pub line_element_index: usize,
    pub block_element_index: usize,
    /// Deterministic identities derived from source ranges and grouping, not
    /// transient draw order. They survive RuntimeShape fan-out clones.
    pub block_group_id: u64,
    pub line_group_id: u64,
    pub element_group_id: u64,
    pub bounds: RuntimeBounds,
    pub advance: f32,
    pub baseline: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeLine {
    pub index: usize,
    pub element_range: Range<usize>,
    pub utf8_range: Range<usize>,
    pub utf16_range: Range<usize>,
    pub group_id: u64,
    pub bounds: RuntimeBounds,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeTextShape {
    pub text: String,
    pub font: String,
    pub size: f64,
    pub elements: Vec<RuntimeTextElement>,
    pub lines: Vec<RuntimeLine>,
    pub block_group_id: u64,
    pub block_bounds: RuntimeBounds,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimePathShape {
    pub path: String,
    /// Exact canonical geometry for native PathValue sources. Legacy SVG and
    /// render-generated geometry leave this absent. The renderer always
    /// prefers this value so general conic weights never pass through SVG.
    pub canonical_path: Option<PathValue>,
    pub bounds: RuntimeBounds,
    pub path_effects: Vec<PathEffect>,
    /// Stable semantic groups retained until Style rasterizes this Shape.
    pub parts: Vec<RuntimePathPart>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimePathPart {
    pub path: String,
    /// Canonical geometry for parts that are exact, unmodified projections of
    /// an authored PathValue. Geometry-generating operations leave this None.
    pub canonical_path: Option<PathValue>,
    pub bounds: RuntimeBounds,
    pub stable_id: u64,
    pub block_group_id: u64,
    pub line_group_id: u64,
    pub line_index: usize,
    /// Target modulation retained as semantic metadata. The Style boundary
    /// carries it into one grouped renderer object without reconstructing
    /// glyph identity or multiplying individual Style opacities.
    pub opacity: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeShapeGeometry {
    Text(RuntimeTextShape),
    Path(RuntimePathShape),
}

const CONSERVATIVE_RASTER_OUTSET: f32 = 1.0;

/// Evaluate exactly the same per-element Ensemble transforms used by text
/// rasterization. Preview bounds and rendered pixels must not independently
/// interpret Effector target grouping or random seeds.
pub fn evaluate_text_element_transforms(
    text: &RuntimeTextShape,
    ensemble: &EnsembleData,
    current_time: f32,
) -> Result<Vec<TransformData>, LibraryError> {
    text.elements
        .iter()
        .map(|element| {
            let center = text_element_center(element);
            let line_element_count = text
                .lines
                .get(element.line_index)
                .map(|line| line.element_range.len())
                .unwrap_or_default();
            let line_center = text
                .lines
                .get(element.line_index)
                .map(|line| bounds_center(line.bounds))
                .unwrap_or(center);
            let mut transform = evaluate_configured_transform(
                &ensemble.effector_configs,
                current_time,
                EffectorElementContext {
                    global_index: element.block_element_index,
                    stable_id: element.element_group_id,
                    block_group_id: element.block_group_id,
                    line_group_id: element.line_group_id,
                    line_index: element.line_index,
                    line_char_index: element.line_element_index,
                    total_chars: text.elements.len(),
                    line_char_count: line_element_count,
                    line_count: text.lines.len(),
                    char_center: center,
                    line_center,
                    block_center: bounds_center(text.block_bounds),
                },
            )?;
            if let Some(patch) = ensemble.patches.get(&element.block_element_index) {
                transform = transform.combine(patch);
            }
            Ok(transform)
        })
        .collect()
}

pub(crate) fn text_element_center(element: &RuntimeTextElement) -> skia_safe::Point {
    skia_safe::Point::new(
        element.bounds.left + element.advance / 2.0,
        (element.bounds.top + element.bounds.bottom) / 2.0,
    )
}

fn bounds_center(bounds: RuntimeBounds) -> skia_safe::Point {
    skia_safe::Point::new(
        (bounds.left + bounds.right) * 0.5,
        (bounds.top + bounds.bottom) * 0.5,
    )
}

pub(crate) fn transform_bounds(
    bounds: RuntimeBounds,
    center: skia_safe::Point,
    transform: &TransformData,
) -> RuntimeBounds {
    let radians = transform.rotate.to_radians();
    let (sin, cos) = radians.sin_cos();
    let mut transformed: Option<RuntimeBounds> = None;
    for (x, y) in [
        (bounds.left, bounds.top),
        (bounds.right, bounds.top),
        (bounds.right, bounds.bottom),
        (bounds.left, bounds.bottom),
    ] {
        let x = (x - center.x) * transform.scale.0;
        let y = (y - center.y) * transform.scale.1;
        let mapped_x = center.x + transform.translate.0 + x * cos - y * sin;
        let mapped_y = center.y + transform.translate.1 + x * sin + y * cos;
        let point = RuntimeBounds::new(mapped_x, mapped_y, mapped_x, mapped_y);
        transformed = Some(transformed.map_or(point, |current| current.union(point)));
    }
    transformed.unwrap_or_default()
}

pub fn transformed_text_element_bounds(
    element: &RuntimeTextElement,
    transform: &TransformData,
) -> RuntimeBounds {
    transform_bounds(element.bounds, text_element_center(element), transform)
}

fn union_indices(
    text: &RuntimeTextShape,
    transforms: &[TransformData],
    indices: impl IntoIterator<Item = usize>,
) -> Option<RuntimeBounds> {
    indices
        .into_iter()
        .map(|index| transformed_text_element_bounds(&text.elements[index], &transforms[index]))
        .reduce(RuntimeBounds::union)
}

/// Conservative local bounds for actual Ensemble text paint. Geometry-only
/// decorators have already produced a separate Shape before this boundary;
/// frozen ABI-v1 Backplates still paint alongside their one target Shape.
pub fn measure_ensemble_text_visual_bounds(
    text: &RuntimeTextShape,
    styles: &[StyleConfig],
    ensemble: &EnsembleData,
    current_time: f32,
) -> Result<Option<RuntimeBounds>, LibraryError> {
    let transforms = evaluate_text_element_transforms(text, ensemble, current_time)?;
    let outsets = crate::model::frame::appearance::appearance_outsets(styles);
    let mut visual_bounds = text
        .elements
        .iter()
        .zip(&transforms)
        .filter(|(_, transform)| transform.opacity > 0.0)
        .map(|(element, transform)| {
            transform_bounds(
                element.bounds.expand(outsets.body),
                text_element_center(element),
                transform,
            )
        })
        .reduce(RuntimeBounds::union)
        .map(|bounds| bounds.expand((outsets.visual - outsets.body).max(0.0)));

    for decorator in &ensemble.decorator_configs {
        let DecoratorConfig::LegacyBackplate {
            target,
            color,
            padding,
            ..
        } = decorator
        else {
            return Err(LibraryError::Render(
                "geometry-only Backplate reached the paint-time renderer".to_string(),
            ));
        };
        match target {
            crate::core::ensemble::decorators::BackplateTarget::Char => {
                for (element, transform) in text.elements.iter().zip(&transforms) {
                    if transform.opacity <= 0.0 || color.a == 0 {
                        continue;
                    }
                    let bounds = transform_bounds(
                        element.bounds.pad(*padding),
                        text_element_center(element),
                        transform,
                    );
                    visual_bounds =
                        Some(visual_bounds.map_or(bounds, |current| current.union(bounds)));
                }
            }
            crate::core::ensemble::decorators::BackplateTarget::Line => {
                for line in &text.lines {
                    let indices = line.element_range.clone().collect::<Vec<_>>();
                    let opacity = indices
                        .iter()
                        .map(|index| transforms[*index].opacity)
                        .sum::<f32>()
                        / indices.len().max(1) as f32;
                    if color.a > 0
                        && opacity > 0.0
                        && let Some(bounds) = union_indices(text, &transforms, indices)
                    {
                        let bounds = bounds.pad(*padding);
                        visual_bounds =
                            Some(visual_bounds.map_or(bounds, |current| current.union(bounds)));
                    }
                }
            }
            crate::core::ensemble::decorators::BackplateTarget::Block => {
                let opacity = transforms
                    .iter()
                    .map(|transform| transform.opacity)
                    .sum::<f32>()
                    / transforms.len().max(1) as f32;
                if color.a > 0
                    && opacity > 0.0
                    && let Some(bounds) = union_indices(text, &transforms, 0..text.elements.len())
                {
                    let bounds = bounds.pad(*padding);
                    visual_bounds =
                        Some(visual_bounds.map_or(bounds, |current| current.union(bounds)));
                }
            }
            crate::core::ensemble::decorators::BackplateTarget::Parts => {
                return Err(LibraryError::Render(
                    "Ensemble BackplateTarget::Parts is not supported".to_string(),
                ));
            }
        }
    }

    Ok(visual_bounds.map(|bounds| bounds.expand(CONSERVATIVE_RASTER_OUTSET)))
}

fn measure_path_decorator_bounds(
    path: &RuntimePathShape,
    decorators: &[DecoratorConfig],
) -> Result<Option<RuntimeBounds>, LibraryError> {
    decorators
        .iter()
        .map(|decorator| {
            let DecoratorConfig::LegacyBackplate {
                target, padding, ..
            } = decorator
            else {
                return Err(LibraryError::Render(
                    "geometry-only Backplate reached the paint-time renderer".to_string(),
                ));
            };
            if *target == crate::core::ensemble::decorators::BackplateTarget::Parts {
                return Err(LibraryError::Render(
                    "Ensemble BackplateTarget::Parts is not supported".to_string(),
                ));
            }
            Ok(path.bounds.pad(*padding))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|bounds| bounds.into_iter().reduce(RuntimeBounds::union))
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeShape {
    /// Generator identity for the geometry and stable element/group metadata.
    /// Whole-Shape Transform operations must not replace this identity.
    pub source_id: Uuid,
    pub geometry: RuntimeShapeGeometry,
    /// The downstream whole-Shape Transform that owns absolute placement.
    /// `None` means the Shape has no editable absolute spatial owner.
    pub spatial_transform_node_id: Option<Uuid>,
    /// Direct transform evaluated from `spatial_transform_node_id`, or identity when
    /// the graph has no whole-Shape Transform. Element modulation may mutate
    /// `transform`, but must not change this edit baseline.
    pub spatial_transform: Transform,
    /// Component-wise modulation accumulated by Path Effectors independently
    /// from absolute placement. Keeping this separate makes
    /// `Transform -> Effector` and `Effector -> Transform` equivalent.
    pub modulation_transform: Transform,
    pub transform: Transform,
    pub effects: Vec<ImageEffect>,
    pub effector_configs: Vec<EffectorConfig>,
    pub decorator_configs: Vec<DecoratorConfig>,
}

impl RuntimeShape {
    /// Apply an absolute transform to the whole grouped Shape value.
    ///
    /// This intentionally does not enter `effector_configs`: text glyphs and
    /// path parts retain their local/group metadata and the renderer applies
    /// one root matrix around the authored anchor. Multiple absolute Transform
    /// nodes require an affine stack: non-uniform scale plus rotation can
    /// introduce skew that editable position/rotation/scale/anchor cannot
    /// represent. Reject that chain explicitly until FrameInfo and Preview
    /// carry the matrix contract together.
    pub fn set_root_transform(
        &mut self,
        source_id: Uuid,
        transform: Transform,
    ) -> Result<(), LibraryError> {
        if let Some(existing_id) = self.spatial_transform_node_id {
            return Err(LibraryError::Validation(format!(
                "Shape Transform chain {existing_id} -> {source_id} requires an affine transform stack"
            )));
        }
        self.spatial_transform_node_id = Some(source_id);
        self.spatial_transform = transform;
        self.recompose_transform();
        Ok(())
    }

    /// Compose absolute placement with optional element modulation in property
    /// space. Translation and rotation are additive, scale and opacity are
    /// multiplicative, and anchor belongs only to the absolute Transform.
    ///
    /// This is intentionally component-wise rather than matrix-order based:
    /// an Effector describes deltas to the authored root properties, so valid
    /// Shape wiring produces the same value on either side of that root.
    fn recompose_transform(&mut self) {
        self.transform = Transform {
            position: crate::model::frame::transform::Position {
                x: self.spatial_transform.position.x + self.modulation_transform.position.x,
                y: self.spatial_transform.position.y + self.modulation_transform.position.y,
            },
            scale: crate::model::frame::transform::Scale {
                x: self.spatial_transform.scale.x * self.modulation_transform.scale.x,
                y: self.spatial_transform.scale.y * self.modulation_transform.scale.y,
            },
            anchor: self.spatial_transform.anchor.clone(),
            rotation: self.spatial_transform.rotation + self.modulation_transform.rotation,
            opacity: self.spatial_transform.opacity * self.modulation_transform.opacity,
        };
    }

    pub fn apply_effector(
        &mut self,
        config: EffectorConfig,
        evaluation_time: f32,
    ) -> Result<(), LibraryError> {
        match &self.geometry {
            RuntimeShapeGeometry::Text(_) => self.effector_configs.push(config),
            RuntimeShapeGeometry::Path(path) => {
                // A path is one stable element. Until path-part grouping is an
                // authored/runtime concept, all Effector targets resolve to
                // this single element instead of fabricating glyph metadata.
                let identity = self.source_id.as_u128() as u64;
                let transform = evaluate_configured_transform(
                    &[config],
                    evaluation_time,
                    EffectorElementContext {
                        global_index: 0,
                        stable_id: identity,
                        block_group_id: identity,
                        line_group_id: identity,
                        line_index: 0,
                        line_char_index: 0,
                        total_chars: 1,
                        line_char_count: 1,
                        line_count: 1,
                        char_center: skia_safe::Point::new(
                            (path.bounds.left + path.bounds.right) * 0.5,
                            (path.bounds.top + path.bounds.bottom) * 0.5,
                        ),
                        line_center: skia_safe::Point::new(
                            (path.bounds.left + path.bounds.right) * 0.5,
                            (path.bounds.top + path.bounds.bottom) * 0.5,
                        ),
                        block_center: skia_safe::Point::new(
                            (path.bounds.left + path.bounds.right) * 0.5,
                            (path.bounds.top + path.bounds.bottom) * 0.5,
                        ),
                    },
                )?;
                self.modulation_transform.position.x += f64::from(transform.translate.0);
                self.modulation_transform.position.y += f64::from(transform.translate.1);
                self.modulation_transform.rotation += f64::from(transform.rotate);
                self.modulation_transform.scale.x *= f64::from(transform.scale.0);
                self.modulation_transform.scale.y *= f64::from(transform.scale.1);
                self.modulation_transform.opacity *= f64::from(transform.opacity);
                self.recompose_transform();
            }
        }
        Ok(())
    }

    pub fn push_decorator(&mut self, config: DecoratorConfig) {
        self.decorator_configs.push(config);
    }

    /// Append one explicit Path Effect operation to transient render state.
    /// The Vec order is the upstream-to-downstream graph order. Text remains
    /// a distinct semantic Shape geometry until a real outline-extraction
    /// operation can preserve glyph and grouping identity.
    pub fn apply_path_effect(
        &mut self,
        operation_id: Uuid,
        effect: PathEffect,
    ) -> Result<(), LibraryError> {
        match &mut self.geometry {
            RuntimeShapeGeometry::Path(path) => {
                path.path_effects.push(effect);
                Ok(())
            }
            RuntimeShapeGeometry::Text(_) => Err(LibraryError::Validation(format!(
                "Path Effect Node {operation_id} accepts only Path geometry; Text Shape source {} requires explicit outline extraction that preserves glyph grouping",
                self.source_id
            ))),
        }
    }

    /// Cross the Shape -> Image boundary as one composited vector object.
    pub fn into_styled_object(
        self,
        style: StyleConfig,
        current_time: f32,
    ) -> Result<FrameObject, LibraryError> {
        self.into_appearance_object(vec![style], current_time)
    }

    /// Cross the Shape -> Image boundary once with one ordered Appearance.
    /// All layer styles share the same composed content alpha and renderer
    /// phase ordering; evaluating each style as an independent Image would
    /// change shadow, glow, offset-fill, and partial-alpha semantics.
    pub fn into_appearance_object(
        self,
        styles: Vec<StyleConfig>,
        current_time: f32,
    ) -> Result<FrameObject, LibraryError> {
        if styles.is_empty() {
            return Err(LibraryError::Validation(
                "Appearance Stack requires at least one Style".to_string(),
            ));
        }
        let source_node_id = self.source_id;
        let spatial_transform_node_id = self.spatial_transform_node_id;
        let ensemble = if self.effector_configs.is_empty() && self.decorator_configs.is_empty() {
            None
        } else {
            Some(EnsembleData {
                enabled: true,
                effector_configs: self.effector_configs,
                decorator_configs: self.decorator_configs,
                patches: std::collections::HashMap::new(),
            })
        };
        let content_bounds = match &self.geometry {
            RuntimeShapeGeometry::Text(text) => {
                let bounds = if let Some(ensemble) = &ensemble {
                    measure_ensemble_text_visual_bounds(text, &styles, ensemble, current_time)?
                } else {
                    let outset = crate::core::rendering::text_layout::text_style_outset(&styles)
                        + CONSERVATIVE_RASTER_OUTSET;
                    Some(text.block_bounds.expand(outset))
                };
                bounds.map(|bounds| {
                    FrameBounds::new(bounds.left, bounds.top, bounds.width(), bounds.height())
                })
            }
            RuntimeShapeGeometry::Path(path) => {
                // `path.bounds` was measured from exact canonical Skia
                // geometry when available. Re-parsing the SVG fallback here
                // would silently turn weighted conics into ordinary quads.
                // Grouped parts are authoritative once present, so their
                // union also owns the declared bounds instead of trusting a
                // potentially stale aggregate-path measurement.
                let geometry_bounds = path
                    .parts
                    .iter()
                    .map(|part| part.bounds)
                    .reduce(RuntimeBounds::union)
                    .unwrap_or(path.bounds);
                let outset = shape_visual_outset(&styles, &path.path_effects);
                let mut bounds = Some(geometry_bounds.expand(outset));
                if let Some(ensemble) = &ensemble
                    && let Some(decorator_bounds) =
                        measure_path_decorator_bounds(path, &ensemble.decorator_configs)?
                {
                    bounds = Some(
                        bounds.map_or(decorator_bounds, |current| current.union(decorator_bounds)),
                    );
                }
                bounds.map(|bounds| {
                    let bounds = bounds.expand(CONSERVATIVE_RASTER_OUTSET);
                    FrameBounds::new(bounds.left, bounds.top, bounds.width(), bounds.height())
                })
            }
        };
        let content = match self.geometry {
            RuntimeShapeGeometry::Text(text) => FrameContent::Text {
                text: text.text,
                font: text.font,
                size: text.size,
                styles,
                effects: self.effects,
                ensemble,
                transform: self.transform,
            },
            RuntimeShapeGeometry::Path(path) => FrameContent::Shape {
                path: path.path,
                canonical_path: path.canonical_path,
                parts: frame_path_parts(path.parts)?,
                styles,
                path_effects: path.path_effects,
                effects: self.effects,
                ensemble,
                transform: self.transform,
            },
        };
        Ok(FrameObject {
            source_node_id,
            spatial_transform_node_id,
            spatial_transform: Box::new(self.spatial_transform),
            content_bounds,
            content,
        })
    }
}

fn frame_path_parts(parts: Vec<RuntimePathPart>) -> Result<Vec<FramePathPart>, LibraryError> {
    if parts.is_empty() || (parts.len() == 1 && (parts[0].opacity - 1.0).abs() <= f32::EPSILON) {
        return Ok(Vec::new());
    }
    parts
        .into_iter()
        .map(|part| {
            if !part.opacity.is_finite() {
                return Err(LibraryError::Validation(format!(
                    "Runtime path part {} has non-finite opacity",
                    part.stable_id
                )));
            }
            Ok(FramePathPart {
                path: part.path,
                canonical_path: part.canonical_path,
                opacity: ordered_float::OrderedFloat(part.opacity.clamp(0.0, 1.0)),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ensemble::target::EffectorTarget;
    use crate::core::ensemble::types::{EffectorConfig, EnsembleData};
    use crate::core::rendering::path_geometry::to_skia_path;
    use crate::model::frame::color::Color;
    use crate::model::frame::draw_type::DrawStyle;
    use crate::model::path::{FillRule, PathContour, PathPoint, PathSegment, PathValue};

    #[test]
    fn text_ensemble_transform_target_uses_block_line_and_character_pivots() {
        let text =
            crate::core::rendering::text_layout::layout_runtime_text_shape("AB\nCD", "Arial", 42.0);
        assert_eq!(text.lines.len(), 2);
        assert_eq!(text.elements.len(), 4);
        let transforms = |target| {
            evaluate_text_element_transforms(
                &text,
                &EnsembleData {
                    enabled: true,
                    effector_configs: vec![EffectorConfig::Transform {
                        translate: (13.0, -7.0),
                        rotate: 0.0,
                        scale: (2.0, 0.5),
                        target,
                    }],
                    decorator_configs: Vec::new(),
                    patches: Default::default(),
                },
                0.0,
            )
            .unwrap()
        };

        let block = transforms(EffectorTarget::Block);
        let line = transforms(EffectorTarget::Line);
        let character = transforms(EffectorTarget::Char);
        assert_ne!(block, line);
        assert_ne!(line, character);
        for transform in &character {
            assert_eq!(transform.translate, (13.0, -7.0));
            assert_eq!(transform.scale, (2.0, 0.5));
        }
        for runtime_line in &text.lines {
            let mapped_line = runtime_line
                .element_range
                .clone()
                .map(|index| transformed_text_element_bounds(&text.elements[index], &line[index]))
                .reduce(RuntimeBounds::union)
                .expect("line has visible elements");
            let mapped_center = bounds_center(mapped_line);
            let original_center = bounds_center(runtime_line.bounds);
            assert!((mapped_center.x - (original_center.x + 13.0)).abs() < 0.01);
            assert!((mapped_center.y - (original_center.y - 7.0)).abs() < 0.01);
        }

        let first_center = text_element_center(&text.elements[0]);
        let mapped = transformed_text_element_bounds(&text.elements[0], &block[0]);
        let mapped_center = bounds_center(mapped);
        let block_center = bounds_center(text.block_bounds);
        assert!(
            (mapped_center.x - (block_center.x + 13.0 + (first_center.x - block_center.x) * 2.0))
                .abs()
                < 0.01
        );
        assert!(
            (mapped_center.y - (block_center.y - 7.0 + (first_center.y - block_center.y) * 0.5))
                .abs()
                < 0.01
        );
    }

    #[test]
    fn ensemble_decoration_outset_is_applied_after_element_scale() {
        let text =
            crate::core::rendering::text_layout::layout_runtime_text_shape("A", "Arial", 100.0);
        let patch = TransformData {
            translate: (0.0, 0.0),
            rotate: 0.0,
            scale: (0.1, 0.1),
            opacity: 1.0,
            color_override: None,
        };
        let ensemble = EnsembleData {
            enabled: true,
            effector_configs: Vec::new(),
            decorator_configs: Vec::new(),
            patches: std::collections::HashMap::from([(0, patch)]),
        };
        let styles = vec![
            StyleConfig {
                id: Uuid::new_v4(),
                style: DrawStyle::Fill {
                    color: Color::white(),
                    offset: 0.0,
                },
            },
            StyleConfig {
                id: Uuid::new_v4(),
                style: DrawStyle::DropShadow {
                    color: Color::black(),
                    opacity: 1.0,
                    blend_mode: crate::model::BlendMode::Normal,
                    angle: 0.0,
                    distance: 50.0,
                    spread: 0.0,
                    size: 0.0,
                },
            },
        ];
        let transforms = evaluate_text_element_transforms(&text, &ensemble, 0.0).unwrap();
        let scaled_body = transformed_text_element_bounds(&text.elements[0], &transforms[0]);
        let visual = measure_ensemble_text_visual_bounds(&text, &styles, &ensemble, 0.0)
            .unwrap()
            .expect("scaled Ensemble text has visual bounds");

        assert!(
            visual.right >= scaled_body.right + 49.0,
            "Drop Shadow decoration was incorrectly scaled with the glyph: body={scaled_body:?}, visual={visual:?}"
        );
    }

    #[test]
    fn canonical_conic_bounds_do_not_use_the_quadratic_svg_fallback() -> Result<(), LibraryError> {
        let value = PathValue::new(
            FillRule::NonZero,
            vec![PathContour::new(
                PathPoint::new(0.0, 0.0),
                vec![PathSegment::conic(
                    PathPoint::new(50.0, 100.0),
                    PathPoint::new(100.0, 0.0),
                    0.2,
                )],
                false,
            )],
        )
        .map_err(|error| LibraryError::Render(error.to_string()))?;
        let direct = to_skia_path(&value)?;
        let direct_bounds = direct.compute_tight_bounds();
        let fallback = crate::model::path::encode_svg_path(&value)
            .map_err(|error| LibraryError::Render(error.to_string()))?
            .into_path_data();
        let fallback_path = skia_safe::Path::from_svg(&fallback)
            .ok_or_else(|| LibraryError::Render("invalid test SVG fallback".to_string()))?;
        let fallback_bounds = fallback_path.compute_tight_bounds();
        assert!(
            (direct_bounds.bottom - fallback_bounds.bottom).abs() > 1.0,
            "weighted conic and quadratic fallback unexpectedly share bounds"
        );

        let runtime_bounds = RuntimeBounds::new(
            direct_bounds.left,
            direct_bounds.top,
            direct_bounds.right,
            direct_bounds.bottom,
        );
        let source_id = Uuid::new_v4();
        let shape = RuntimeShape {
            source_id,
            geometry: RuntimeShapeGeometry::Path(RuntimePathShape {
                path: fallback.clone(),
                canonical_path: Some(value.clone()),
                bounds: runtime_bounds,
                path_effects: Vec::new(),
                parts: vec![RuntimePathPart {
                    path: fallback,
                    canonical_path: Some(value.clone()),
                    bounds: runtime_bounds,
                    stable_id: 7,
                    block_group_id: 7,
                    line_group_id: 7,
                    line_index: 0,
                    opacity: 0.4,
                }],
            }),
            spatial_transform_node_id: None,
            spatial_transform: Default::default(),
            modulation_transform: Default::default(),
            transform: Default::default(),
            effects: Vec::new(),
            effector_configs: Vec::new(),
            decorator_configs: Vec::new(),
        };
        let style = StyleConfig {
            id: Uuid::new_v4(),
            style: DrawStyle::Fill {
                color: Color::white(),
                offset: 0.0,
            },
        };
        let object = shape.into_styled_object(style, 0.0)?;
        let bounds = object
            .content_bounds
            .ok_or_else(|| LibraryError::Render("styled conic has no bounds".to_string()))?;
        let (_, _, _, rendered_height) = bounds.as_tuple();
        assert!((rendered_height - (direct_bounds.height() + 2.0)).abs() <= f32::EPSILON);
        let FrameContent::Shape {
            canonical_path: Some(rendered),
            parts,
            ..
        } = &object.content
        else {
            return Err(LibraryError::Render(
                "styled conic dropped canonical geometry".to_string(),
            ));
        };
        assert_eq!(rendered, &value);
        assert!(matches!(
            parts.as_slice(),
            [FramePathPart { opacity, .. }] if opacity.into_inner() == 0.4
        ));
        let FrameContent::Shape { styles, .. } = &object.content else {
            return Err(LibraryError::Render(
                "styled conic changed content type".to_string(),
            ));
        };
        assert!(matches!(
            styles.first().map(|style| &style.style),
            Some(DrawStyle::Fill { color, .. }) if color.a == 255
        ));
        Ok(())
    }
}
