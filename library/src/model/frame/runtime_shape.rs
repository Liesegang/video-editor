//! Render-only vector/typographic value carried by `PortDataType::Shape`.
//!
//! This is deliberately not serialized. The authoritative authored state is
//! always the Project graph; a RuntimeShape is only an evaluated value moving
//! left-to-right between graph operations for one frame.

use std::ops::Range;

use uuid::Uuid;

use crate::core::ensemble::effectors::{EffectorElementContext, evaluate_configured_transform};
use crate::core::ensemble::types::{DecoratorConfig, EffectorConfig, EnsembleData};
use crate::error::LibraryError;
use crate::model::frame::draw_type::PathEffect;
use crate::model::frame::effect::ImageEffect;
use crate::model::frame::entity::{FrameContent, FrameObject, StyleConfig};
use crate::model::frame::transform::Transform;
use crate::model::property::PropertyMap;

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

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeShape {
    pub source_id: Uuid,
    pub geometry: RuntimeShapeGeometry,
    pub transform: Transform,
    pub effects: Vec<ImageEffect>,
    pub effector_configs: Vec<EffectorConfig>,
    pub decorator_configs: Vec<DecoratorConfig>,
    pub properties: PropertyMap,
}

impl RuntimeShape {
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
                self.transform.position.x += f64::from(transform.translate.0);
                self.transform.position.y += f64::from(transform.translate.1);
                self.transform.rotation += f64::from(transform.rotate);
                self.transform.scale.x *= f64::from(transform.scale.0);
                self.transform.scale.y *= f64::from(transform.scale.1);
                self.transform.opacity *= f64::from(transform.opacity);
            }
        }
        Ok(())
    }

    pub fn push_decorator(&mut self, config: DecoratorConfig) {
        self.decorator_configs.push(config);
    }

    /// Cross the Shape -> Image boundary by creating one renderer object with
    /// exactly the Style from this branch.
    pub fn into_styled_object(self, style: StyleConfig) -> FrameObject {
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
        FrameObject {
            content,
            properties: self.properties,
        }
    }
}
