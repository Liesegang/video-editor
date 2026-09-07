//! Render-only vector/typographic value carried by `PortDataType::Shape`.
//!
//! This is deliberately not serialized. The authoritative authored state is
//! always the Project graph; a RuntimeShape is only an evaluated value moving
//! left-to-right between graph operations for one frame.

use std::ops::Range;

use uuid::Uuid;

use crate::core::ensemble::effectors::{
    EffectorElementContext, SpacingSequence, evaluate_configured_transform,
};
use crate::core::ensemble::types::{DecoratorConfig, EffectorConfig, EnsembleData, TransformData};
use crate::error::LibraryError;
use crate::model::frame::appearance::SOURCE_RASTER_OUTSET;
use crate::model::frame::draw_type::PathEffect;
use crate::model::frame::effect::ImageEffect;
use crate::model::frame::entity::{
    FrameBounds, FrameContent, FrameObject, FramePathPart, StyleConfig,
};
use crate::model::frame::transform::{Position, Scale, Transform};
use crate::model::path::PathValue;
use crate::rendering::renderer::Affine2D;

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

/// One atomic shaped element in logical source order. Graphemes crossed by a
/// shaping cluster (for example a ligature) form one element and animate
/// together. Its exact source ranges identify the whole element; font and
/// glyph resources stay render-local, and rendering never reshapes its source.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeTextElement {
    /// Exact source slice represented by this element.
    pub source: String,
    pub utf8_range: Range<usize>,
    pub utf16_range: Range<usize>,
    pub line_index: usize,
    pub line_element_index: usize,
    pub block_element_index: usize,
    /// Visual spacing units derived from the same shaped glyphs as rendering.
    /// These do not change logical source ranges, animation order, or patch IDs.
    pub line_spacing: SpacingSequence,
    pub block_spacing: SpacingSequence,
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

impl RuntimeTextShape {
    /// Map a shaped glyph's UTF-8 cluster start to its atomic source element.
    pub(crate) fn element_index_at_utf8(&self, start: usize) -> Option<usize> {
        let index = self
            .elements
            .partition_point(|element| element.utf8_range.end <= start);
        self.elements
            .get(index)
            .filter(|element| element.utf8_range.contains(&start))
            .map(|_| index)
    }
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

/// Evaluate exactly the same per-element Ensemble transforms used by text
/// rasterization. Preview bounds and rendered pixels must not independently
/// interpret Effector target grouping or random seeds.
pub fn evaluate_text_element_transforms(
    text: &RuntimeTextShape,
    ensemble: &EnsembleData,
    current_time: f32,
) -> Result<Vec<TransformData>, LibraryError> {
    if ensemble
        .effector_configs
        .iter()
        .any(|config| config.target() == crate::core::ensemble::target::EffectorTarget::Parts)
    {
        return Err(LibraryError::Render(
            "Ensemble EffectorTarget::Parts is not supported".to_string(),
        ));
    }
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
                    line_spacing: element.line_spacing,
                    block_spacing: element.block_spacing,
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

/// The same authored transform convention drives glyph paint and its bounds.
pub(crate) fn text_element_affine(center: skia_safe::Point, transform: &TransformData) -> Affine2D {
    Affine2D::from(&Transform {
        position: Position {
            x: f64::from(center.x) + f64::from(transform.translate.0),
            y: f64::from(center.y) + f64::from(transform.translate.1),
        },
        anchor: Position {
            x: f64::from(center.x),
            y: f64::from(center.y),
        },
        scale: Scale {
            x: f64::from(transform.scale.0),
            y: f64::from(transform.scale.1),
        },
        rotation: f64::from(transform.rotate),
        opacity: f64::from(transform.opacity),
    })
}

pub(crate) fn transform_bounds(
    bounds: RuntimeBounds,
    center: skia_safe::Point,
    transform: &TransformData,
) -> RuntimeBounds {
    let affine = text_element_affine(center, transform);
    let mut transformed: Option<RuntimeBounds> = None;
    for (x, y) in [
        (bounds.left, bounds.top),
        (bounds.right, bounds.top),
        (bounds.right, bounds.bottom),
        (bounds.left, bounds.bottom),
    ] {
        let (x, y) = affine.map_point(f64::from(x), f64::from(y));
        let point = RuntimeBounds::new(x as f32, y as f32, x as f32, y as f32);
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

    if let Some(decorator_bounds) =
        measure_text_decorator_bounds(text, &transforms, &ensemble.decorator_configs)?
    {
        visual_bounds =
            Some(visual_bounds.map_or(decorator_bounds, |current| current.union(decorator_bounds)));
    }

    Ok(visual_bounds.map(|bounds| bounds.expand(SOURCE_RASTER_OUTSET)))
}

/// Measure the local bounds of the Text body that crosses the Shape -> Image
/// boundary. Direct Timeline Text and Text produced by a Module must use this
/// same interpretation of Ensemble transforms and Appearance outsets.
pub fn measure_text_visual_bounds(
    text: &RuntimeTextShape,
    styles: &[StyleConfig],
    ensemble: Option<&EnsembleData>,
    current_time: f32,
) -> Result<Option<RuntimeBounds>, LibraryError> {
    if text.elements.is_empty() {
        return Ok(None);
    }
    match ensemble.filter(|ensemble| ensemble.enabled) {
        Some(ensemble) => measure_ensemble_text_visual_bounds(text, styles, ensemble, current_time),
        None => {
            let outset = crate::core::rendering::text_layout::text_style_outset(styles)
                + SOURCE_RASTER_OUTSET;
            Ok(Some(text.block_bounds.expand(outset)))
        }
    }
}

pub(crate) fn measure_text_decorator_bounds(
    text: &RuntimeTextShape,
    transforms: &[TransformData],
    decorators: &[DecoratorConfig],
) -> Result<Option<RuntimeBounds>, LibraryError> {
    let mut decorator_bounds = None;
    for decorator in decorators {
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
                for (element, transform) in text.elements.iter().zip(transforms) {
                    if transform.opacity <= 0.0 || color.a == 0 {
                        continue;
                    }
                    let bounds = transform_bounds(
                        element.bounds.pad(*padding),
                        text_element_center(element),
                        transform,
                    );
                    decorator_bounds = Some(
                        decorator_bounds
                            .map_or(bounds, |current: RuntimeBounds| current.union(bounds)),
                    );
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
                        && let Some(bounds) = union_indices(text, transforms, indices)
                    {
                        let bounds = bounds.pad(*padding);
                        decorator_bounds = Some(
                            decorator_bounds
                                .map_or(bounds, |current: RuntimeBounds| current.union(bounds)),
                        );
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
                    && let Some(bounds) = union_indices(text, transforms, 0..text.elements.len())
                {
                    let bounds = bounds.pad(*padding);
                    decorator_bounds = Some(
                        decorator_bounds
                            .map_or(bounds, |current: RuntimeBounds| current.union(bounds)),
                    );
                }
            }
            crate::core::ensemble::decorators::BackplateTarget::Parts => {
                return Err(LibraryError::Render(
                    "Ensemble BackplateTarget::Parts is not supported".to_string(),
                ));
            }
        }
    }
    Ok(decorator_bounds)
}

pub(crate) fn measure_path_decorator_bounds(
    path_bounds: RuntimeBounds,
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
            Ok(path_bounds.pad(*padding))
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
                        line_spacing: SpacingSequence { index: 0, total: 1 },
                        block_spacing: SpacingSequence { index: 0, total: 1 },
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

    /// Rasterize Shape content with the supplied authored style sequence.
    /// Production graph evaluation normally supplies one style per
    /// Shape -> Image stage; the ordered slice is retained for shared vector
    /// rendering callers and preserves the sequence without phase sorting.
    pub fn into_appearance_object(
        self,
        styles: Vec<StyleConfig>,
        current_time: f32,
    ) -> Result<FrameObject, LibraryError> {
        if styles.is_empty() {
            return Err(LibraryError::Validation(
                "Shape rasterization requires at least one Style".to_string(),
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
                let bounds =
                    measure_text_visual_bounds(text, &styles, ensemble.as_ref(), current_time)?;
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
                        measure_path_decorator_bounds(geometry_bounds, &ensemble.decorator_configs)?
                {
                    bounds = Some(
                        bounds.map_or(decorator_bounds, |current| current.union(decorator_bounds)),
                    );
                }
                bounds.map(|bounds| {
                    let bounds = bounds.expand(SOURCE_RASTER_OUTSET);
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
mod tests;
