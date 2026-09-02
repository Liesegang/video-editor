use crate::model::frame::draw_type::{DrawStyle, PathEffect};
use crate::model::frame::effect::ImageEffect;
use crate::model::frame::transform::Transform;
use serde::{Deserialize, Serialize};

use ordered_float::OrderedFloat;
use std::hash::{Hash, Hasher};

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ImageSurface {
    /// Authoritative Project Asset that owns source metadata and identity.
    /// Legacy/file-path generators may leave this absent, in which case a
    /// managed renderer must not guess the source color space.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<Uuid>,
    #[serde(rename = "file_path")]
    pub file_path: String,
    #[serde(default)]
    pub effects: Vec<ImageEffect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_color_space: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_color_space: Option<String>,
    #[serde(flatten)]
    pub transform: Transform,
}

use uuid::Uuid;

use crate::model::BlendMode;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
pub struct StyleConfig {
    pub id: Uuid,
    pub style: DrawStyle,
}

/// Declares the numeric color domain produced by an SkSL generator.
///
/// Runtime shaders do not carry an ICC/OCIO profile themselves. Keeping this
/// declaration on the evaluated frame prevents a renderer from silently
/// interpreting shader values in whichever surface happens to be active.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SkSLColorDomain {
    /// Scene-linear, straight-alpha RGBA values in the exact working space of
    /// the current Project. The renderer premultiplies RGB by alpha exactly
    /// once before the values enter Skia's premultiplied working storage.
    ProjectWorkingLinear,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type")]
pub enum FrameContent {
    Video {
        #[serde(flatten)]
        surface: ImageSurface,
        /// Source-local seconds used as the sole decode authority. The loader
        /// maps this value into the selected stream's time base and PTS.
        source_time: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stream_index: Option<usize>,
    },
    Image {
        #[serde(flatten)]
        surface: ImageSurface,
    },
    Text {
        text: String,
        font: String,
        size: f64,
        #[serde(default)]
        styles: Vec<StyleConfig>,
        #[serde(default)]
        effects: Vec<ImageEffect>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ensemble: Option<crate::core::ensemble::EnsembleData>,
        #[serde(flatten)]
        transform: Transform,
    },
    Shape {
        path: String,
        /// Exact native path geometry. `path` remains the legacy SVG
        /// presentation/fallback string, but rendering prefers this value.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        canonical_path: Option<crate::model::path::PathValue>,
        styles: Vec<StyleConfig>,
        path_effects: Vec<PathEffect>,
        #[serde(default)]
        effects: Vec<ImageEffect>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ensemble: Option<crate::core::ensemble::EnsembleData>,
        #[serde(flatten)]
        transform: Transform,
    },
    SkSL {
        shader: String,
        resolution: (f32, f32),
        color_domain: SkSLColorDomain,
        #[serde(default)]
        effects: Vec<ImageEffect>,
        #[serde(flatten)]
        transform: Transform,
    },
}

impl FrameContent {
    pub fn transform(&self) -> &Transform {
        match self {
            Self::Video { surface, .. } | Self::Image { surface } => &surface.transform,
            Self::Text { transform, .. }
            | Self::Shape { transform, .. }
            | Self::SkSL { transform, .. } => transform,
        }
    }

    pub fn transform_mut(&mut self) -> &mut Transform {
        match self {
            Self::Video { surface, .. } | Self::Image { surface } => &mut surface.transform,
            Self::Text { transform, .. }
            | Self::Shape { transform, .. }
            | Self::SkSL { transform, .. } => transform,
        }
    }
}

impl Hash for FrameContent {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            FrameContent::Video {
                surface,
                source_time,
                stream_index,
            } => {
                surface.hash(state);
                OrderedFloat(*source_time).hash(state);
                stream_index.hash(state);
            }
            FrameContent::Image { surface } => {
                surface.hash(state);
            }
            FrameContent::Text {
                text,
                font,
                size,
                styles,
                effects,
                ensemble,
                transform,
            } => {
                text.hash(state);
                font.hash(state);
                OrderedFloat(*size).hash(state);
                styles.hash(state);
                effects.hash(state);
                ensemble.hash(state);
                transform.hash(state);
            }
            FrameContent::Shape {
                path,
                canonical_path,
                styles,
                path_effects,
                effects,
                ensemble,
                transform,
            } => {
                path.hash(state);
                canonical_path.hash(state);
                styles.hash(state);
                path_effects.hash(state);
                effects.hash(state);
                ensemble.hash(state);
                transform.hash(state);
            }
            FrameContent::SkSL {
                shader,
                resolution,
                color_domain,
                effects,
                transform,
            } => {
                shader.hash(state);
                OrderedFloat(resolution.0).hash(state);
                OrderedFloat(resolution.1).hash(state);
                color_domain.hash(state);
                effects.hash(state);
                transform.hash(state);
            }
        }
    }
}

impl PartialEq for FrameContent {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                FrameContent::Video {
                    surface: s1,
                    source_time: t1,
                    stream_index: i1,
                },
                FrameContent::Video {
                    surface: s2,
                    source_time: t2,
                    stream_index: i2,
                },
            ) => s1 == s2 && OrderedFloat(*t1) == OrderedFloat(*t2) && i1 == i2,
            (FrameContent::Image { surface: s1 }, FrameContent::Image { surface: s2 }) => s1 == s2,
            (
                FrameContent::Text {
                    text: t1,
                    font: f1,
                    size: s1,
                    styles: st1,
                    effects: e1,
                    ensemble: en1,
                    transform: tr1,
                },
                FrameContent::Text {
                    text: t2,
                    font: f2,
                    size: s2,
                    styles: st2,
                    effects: e2,
                    ensemble: en2,
                    transform: tr2,
                },
            ) => {
                t1 == t2
                    && f1 == f2
                    && OrderedFloat(*s1) == OrderedFloat(*s2)
                    && st1 == st2
                    && e1 == e2
                    && en1 == en2
                    && tr1 == tr2
            }
            (
                FrameContent::Shape {
                    path: p1,
                    canonical_path: cp1,
                    styles: st1,
                    path_effects: pe1,
                    effects: e1,
                    ensemble: en1,
                    transform: tr1,
                },
                FrameContent::Shape {
                    path: p2,
                    canonical_path: cp2,
                    styles: st2,
                    path_effects: pe2,
                    effects: e2,
                    ensemble: en2,
                    transform: tr2,
                },
            ) => {
                p1 == p2
                    && cp1 == cp2
                    && st1 == st2
                    && pe1 == pe2
                    && e1 == e2
                    && en1 == en2
                    && tr1 == tr2
            }
            (
                FrameContent::SkSL {
                    shader: s1,
                    resolution: r1,
                    color_domain: d1,
                    effects: e1,
                    transform: tr1,
                },
                FrameContent::SkSL {
                    shader: s2,
                    resolution: r2,
                    color_domain: d2,
                    effects: e2,
                    transform: tr2,
                },
            ) => {
                s1 == s2
                    && OrderedFloat(r1.0) == OrderedFloat(r2.0)
                    && OrderedFloat(r1.1) == OrderedFloat(r2.1)
                    && d1 == d2
                    && e1 == e2
                    && tr1 == tr2
            }
            _ => false,
        }
    }
}
impl Eq for FrameContent {}

/// Evaluated local-space bounds of one rendered object.
///
/// These bounds travel with the exact `FrameObject` that reached the final
/// Composition evaluation. Preview interaction can therefore use the same
/// source and geometry as rendering instead of guessing from container order.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FrameBounds {
    pub x: OrderedFloat<f32>,
    pub y: OrderedFloat<f32>,
    pub width: OrderedFloat<f32>,
    pub height: OrderedFloat<f32>,
}

impl FrameBounds {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
            width: OrderedFloat(width.max(0.0)),
            height: OrderedFloat(height.max(0.0)),
        }
    }

    pub fn as_tuple(self) -> (f32, f32, f32, f32) {
        (
            self.x.into_inner(),
            self.y.into_inner(),
            self.width.into_inner(),
            self.height.into_inner(),
        )
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)] // Added Debug
pub struct FrameObject {
    /// Geometry/content generator identity. Style, Transform, Effect, Merge,
    /// and container wrappers deliberately do not replace it.
    pub source_node_id: Uuid,
    /// Optional Node that owns the editable absolute spatial transform.
    ///
    /// Raster generators normally own their spatial transform directly, so
    /// this is `Some(source_node_id)`. Shape-valued generators have no spatial
    /// owner until an explicit Transform operation is evaluated.
    pub spatial_transform_node_id: Option<Uuid>,
    /// Absolute transform evaluated directly from
    /// `spatial_transform_node_id`, before optional element modulation.
    pub spatial_transform: Box<Transform>,
    pub content_bounds: Option<FrameBounds>,
    pub content: FrameContent, // Renamed from entity: FrameEntity
}

/// The kind of isolated image produced by a frame group.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FrameGroupKind {
    /// A Timeline definition evaluated as an isolated visual canvas.
    Timeline,
    /// One authored Timeline item. This is a render-time boundary, not a Node.
    TimelineItem,
    Track,
    Clip,
    Composition,
    Node,
    Merge,
    /// A descriptor-backed Effect operation. Its child image is composited
    /// into one isolated layer before the operation is applied exactly once.
    Effect,
    /// A typed Image -> Image appearance operation. The complete child image
    /// is isolated once, then alpha is applied without changing geometry.
    ImageStyle,
    /// A native Image -> Image spatial operation. Its complete child image is
    /// rasterized in local space and transformed as one layer. Nested groups
    /// preserve the graph's ordered affine stack.
    ImageTransform,
    /// One evaluation of a top-level Composition definition through an
    /// independently timed Composition Instance placement.
    CompositionInstance,
    /// A graph Image connection projected into an isolated wrapper while the
    /// source item remains in its original Project containment.
    ConnectedImage,
}

/// A render-time image container derived directly from the authoritative
/// Project. Track and nested Composition output semantics require isolation:
/// children are composited first, then effects/transform/blend are applied to
/// the resulting image exactly once.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
pub struct FrameGroup {
    pub source_id: Uuid,
    pub kind: FrameGroupKind,
    pub width: u64,
    pub height: u64,
    pub background_color: crate::model::frame::color::Color,
    /// Ordered root-to-leaf transforms inherited from authored Timeline
    /// parents. Parenting does not change layer scheduling.
    #[serde(default)]
    pub inherited_transforms: Vec<Transform>,
    pub transform: Transform,
    pub blend_mode: BlendMode,
    pub effect_time: OrderedFloat<f64>,
    #[serde(default)]
    pub effects: Vec<ImageEffect>,
    /// Timeline-authored masks evaluated at this group's local time.
    #[serde(default)]
    pub masks: Vec<FrameMask>,
    #[serde(default)]
    pub items: Vec<FrameItem>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
pub struct FrameMask {
    pub path: crate::model::path::PathValue,
    pub mode: crate::model::authoring::MaskMode,
    pub inverted: bool,
    pub feather: OrderedFloat<f64>,
    pub opacity: OrderedFloat<f64>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
#[serde(tag = "item_type", content = "item")]
pub enum FrameItem {
    Object(FrameObject),
    Group(FrameGroup),
}

impl FrameItem {
    pub fn object_count(&self) -> usize {
        match self {
            Self::Object(_) => 1,
            Self::Group(group) => group.items.iter().map(Self::object_count).sum(),
        }
    }
}

pub trait ImageContent {
    fn get_surface(&self) -> Option<&ImageSurface>;
}

impl ImageContent for FrameContent {
    fn get_surface(&self) -> Option<&ImageSurface> {
        match self {
            FrameContent::Video { surface, .. } => Some(surface),
            FrameContent::Image { surface } => Some(surface),
            _ => None,
        }
    }
}
