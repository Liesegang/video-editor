//! Render-only vector/typographic value carried by `PortDataType::Shape`.
//!
//! This is deliberately not serialized. The authoritative authored state is
//! always the Project graph; a RuntimeShape is only an evaluated value moving
//! left-to-right between graph operations for one frame.

use std::ops::Range;

use uuid::Uuid;

use crate::core::ensemble::decorators::BackplateTarget;
use crate::core::ensemble::effectors::{EffectorElementContext, evaluate_configured_transform};
use crate::core::ensemble::types::{DecoratorConfig, EffectorConfig, EnsembleData, TransformData};
use crate::error::LibraryError;
use crate::model::frame::draw_type::{DrawStyle, PathEffect};
use crate::model::frame::effect::ImageEffect;
use crate::model::frame::entity::{FrameBounds, FrameContent, FrameObject, StyleConfig};
use crate::model::frame::transform::Transform;

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
    let style_outset = styles.iter().fold(0.0_f32, |outset, config| {
        let candidate = match &config.style {
            DrawStyle::Fill { offset, .. } => offset.max(0.0) as f32,
            DrawStyle::Stroke { width, offset, .. } if *width > 0.0 => {
                if *offset > 0.0 {
                    (offset + width / 2.0) as f32
                } else if *offset == 0.0 {
                    (width / 2.0) as f32
                } else {
                    0.0
                }
            }
            DrawStyle::Stroke { .. } => 0.0,
        };
        outset.max(candidate)
    });
    let effect_outset = path_effects.iter().fold(0.0_f32, |outset, effect| {
        if let PathEffect::Discrete { deviation, .. } = effect {
            outset.max(deviation.abs() as f32)
        } else {
            outset
        }
    });
    let outset = style_outset + effect_outset;

    Some((
        bounds.left - outset,
        bounds.top - outset,
        bounds.width() + outset * 2.0,
        bounds.height() + outset * 2.0,
    ))
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
    pub bounds: RuntimeBounds,
    pub path_effects: Vec<PathEffect>,
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
                    char_center: center,
                },
            )?;
            if let Some(patch) = ensemble.patches.get(&element.block_element_index) {
                transform = transform.combine(patch);
            }
            Ok(transform)
        })
        .collect()
}

fn text_element_center(element: &RuntimeTextElement) -> skia_safe::Point {
    skia_safe::Point::new(
        element.bounds.left + element.advance / 2.0,
        (element.bounds.top + element.bounds.bottom) / 2.0,
    )
}

fn transform_bounds(
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

/// Conservative local bounds for the actual Ensemble text paint, including
/// per-character transforms and every Backplate target/padding mode.
pub fn measure_ensemble_text_visual_bounds(
    text: &RuntimeTextShape,
    styles: &[StyleConfig],
    ensemble: &EnsembleData,
    current_time: f32,
) -> Result<Option<RuntimeBounds>, LibraryError> {
    let transforms = evaluate_text_element_transforms(text, ensemble, current_time)?;
    let style_outset = crate::core::rendering::text_layout::text_style_outset(styles);
    let mut visual_bounds = text
        .elements
        .iter()
        .zip(&transforms)
        .filter(|(_, transform)| transform.opacity > 0.0)
        .map(|(element, transform)| {
            transform_bounds(
                element.bounds.expand(style_outset),
                text_element_center(element),
                transform,
            )
        })
        .reduce(RuntimeBounds::union);

    for decorator in &ensemble.decorator_configs {
        match decorator {
            DecoratorConfig::Backplate {
                target,
                color,
                padding,
                ..
            } => match target {
                BackplateTarget::Char => {
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
                BackplateTarget::Line => {
                    for line_index in 0..text.text.split('\n').count() {
                        let indices =
                            text.elements
                                .iter()
                                .enumerate()
                                .filter_map(|(index, element)| {
                                    (element.line_index == line_index).then_some(index)
                                });
                        let indices = indices.collect::<Vec<_>>();
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
                BackplateTarget::Block => {
                    let opacity = transforms
                        .iter()
                        .map(|transform| transform.opacity)
                        .sum::<f32>()
                        / transforms.len().max(1) as f32;
                    if color.a > 0
                        && opacity > 0.0
                        && let Some(bounds) =
                            union_indices(text, &transforms, 0..text.elements.len())
                    {
                        let bounds = bounds.pad(*padding);
                        visual_bounds =
                            Some(visual_bounds.map_or(bounds, |current| current.union(bounds)));
                    }
                }
                BackplateTarget::Parts => {
                    return Err(LibraryError::Render(
                        "Ensemble BackplateTarget::Parts is not supported".to_string(),
                    ));
                }
            },
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
        .map(|decorator| match decorator {
            DecoratorConfig::Backplate {
                target, padding, ..
            } => {
                if *target == BackplateTarget::Parts {
                    return Err(LibraryError::Render(
                        "Ensemble BackplateTarget::Parts is not supported".to_string(),
                    ));
                }
                // A RuntimePathShape is one stable element, matching renderer
                // semantics for Char/Line/Block.
                Ok(path.bounds.pad(*padding))
            }
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
                        char_center: skia_safe::Point::new(
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

    /// Cross the Shape -> Image boundary by creating one renderer object with
    /// exactly the Style from this branch.
    pub fn into_styled_object(
        self,
        style: StyleConfig,
        current_time: f32,
    ) -> Result<FrameObject, LibraryError> {
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
                    measure_ensemble_text_visual_bounds(
                        text,
                        std::slice::from_ref(&style),
                        ensemble,
                        current_time,
                    )?
                } else {
                    let outset = crate::core::rendering::text_layout::text_style_outset(
                        std::slice::from_ref(&style),
                    ) + CONSERVATIVE_RASTER_OUTSET;
                    Some(text.block_bounds.expand(outset))
                };
                bounds.map(|bounds| {
                    FrameBounds::new(bounds.left, bounds.top, bounds.width(), bounds.height())
                })
            }
            RuntimeShapeGeometry::Path(path) => {
                let mut bounds = measure_shape_visual_bounds(
                    &path.path,
                    std::slice::from_ref(&style),
                    &path.path_effects,
                )
                .map(|(x, y, width, height)| RuntimeBounds::new(x, y, x + width, y + height));
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
                styles: vec![style],
                effects: self.effects,
                ensemble,
                transform: self.transform,
            },
            RuntimeShapeGeometry::Path(path) => FrameContent::Shape {
                path: path.path,
                styles: vec![style],
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
