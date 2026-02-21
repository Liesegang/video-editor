//! Shape data types produced by text/shape clip evaluation.

use crate::evaluation::ensemble::types::TransformData;
use crate::runtime::color::Color;
use crate::runtime::draw_type::PathEffect;

/// Shape data produced by text/shape clips, consumed by fill/stroke nodes.
///
/// Effector/decorator nodes operate on `Grouped` variant to apply
/// per-element transforms and decorations before rasterization.
#[derive(Clone, Debug)]
pub enum ShapeData {
    /// Single SVG path (shape clips).
    Path {
        path_data: String,
        path_effects: Vec<PathEffect>,
    },
    /// Grouped shapes with per-element metadata (text clips decomposed to glyphs).
    Grouped {
        groups: Vec<ShapeGroup>,
        /// Global bounding box (x, y, w, h) of all groups.
        bounds: (f32, f32, f32, f32),
        /// Line-level grouping info.
        lines: Vec<LineInfo>,
        /// Font info preserved for decoration sizing.
        font_info: FontInfo,
    },
}

/// A single logical element in a grouped shape (one character).
#[derive(Clone, Debug)]
pub struct ShapeGroup {
    /// SVG path data for this element's outline.
    pub path: String,
    /// Character(s) this group represents.
    pub source_char: String,
    /// Index of this group in the overall sequence.
    pub index: usize,
    /// Which line this group belongs to.
    pub line_index: usize,
    /// Base position from text layout (x, y).
    pub base_position: (f32, f32),
    /// Bounding box (x, y, w, h) relative to base_position.
    pub bounds: (f32, f32, f32, f32),
    /// Per-element transform (accumulated by effectors).
    pub transform: TransformData,
    /// Decoration shapes (added by decorators).
    pub decorations: Vec<DecorationShape>,
}

/// Line-level grouping metadata.
#[derive(Clone, Debug)]
pub struct LineInfo {
    /// Range of group indices belonging to this line.
    pub group_range: std::ops::Range<usize>,
    /// Bounding box (x, y, w, h) for the entire line.
    pub bounds: (f32, f32, f32, f32),
}

/// Font metadata preserved for decoration sizing.
#[derive(Clone, Debug)]
pub struct FontInfo {
    pub family: String,
    pub size: f64,
}

/// A decoration shape added by decorator nodes.
#[derive(Clone, Debug)]
pub struct DecorationShape {
    /// SVG path data for the decoration.
    pub path: String,
    /// Fill color for this decoration.
    pub color: Color,
    /// Whether this decoration renders behind (true) or in front (false).
    pub behind: bool,
}

// ---------------------------------------------------------------------------
// PartialEq / Eq / Hash implementations (OrderedFloat pattern)
// ---------------------------------------------------------------------------

impl PartialEq for ShapeGroup {
    fn eq(&self, other: &Self) -> bool {
        use ordered_float::OrderedFloat;
        self.path == other.path
            && self.source_char == other.source_char
            && self.index == other.index
            && self.line_index == other.line_index
            && OrderedFloat(self.base_position.0) == OrderedFloat(other.base_position.0)
            && OrderedFloat(self.base_position.1) == OrderedFloat(other.base_position.1)
            && OrderedFloat(self.bounds.0) == OrderedFloat(other.bounds.0)
            && OrderedFloat(self.bounds.1) == OrderedFloat(other.bounds.1)
            && OrderedFloat(self.bounds.2) == OrderedFloat(other.bounds.2)
            && OrderedFloat(self.bounds.3) == OrderedFloat(other.bounds.3)
            && self.transform == other.transform
            && self.decorations == other.decorations
    }
}
impl Eq for ShapeGroup {}

impl std::hash::Hash for ShapeGroup {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        use ordered_float::OrderedFloat;
        self.path.hash(state);
        self.source_char.hash(state);
        self.index.hash(state);
        self.line_index.hash(state);
        OrderedFloat(self.base_position.0).hash(state);
        OrderedFloat(self.base_position.1).hash(state);
        OrderedFloat(self.bounds.0).hash(state);
        OrderedFloat(self.bounds.1).hash(state);
        OrderedFloat(self.bounds.2).hash(state);
        OrderedFloat(self.bounds.3).hash(state);
        self.transform.hash(state);
        self.decorations.hash(state);
    }
}

impl PartialEq for LineInfo {
    fn eq(&self, other: &Self) -> bool {
        use ordered_float::OrderedFloat;
        self.group_range == other.group_range
            && OrderedFloat(self.bounds.0) == OrderedFloat(other.bounds.0)
            && OrderedFloat(self.bounds.1) == OrderedFloat(other.bounds.1)
            && OrderedFloat(self.bounds.2) == OrderedFloat(other.bounds.2)
            && OrderedFloat(self.bounds.3) == OrderedFloat(other.bounds.3)
    }
}
impl Eq for LineInfo {}

impl std::hash::Hash for LineInfo {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        use ordered_float::OrderedFloat;
        self.group_range.hash(state);
        OrderedFloat(self.bounds.0).hash(state);
        OrderedFloat(self.bounds.1).hash(state);
        OrderedFloat(self.bounds.2).hash(state);
        OrderedFloat(self.bounds.3).hash(state);
    }
}

impl PartialEq for FontInfo {
    fn eq(&self, other: &Self) -> bool {
        use ordered_float::OrderedFloat;
        self.family == other.family && OrderedFloat(self.size) == OrderedFloat(other.size)
    }
}
impl Eq for FontInfo {}

impl std::hash::Hash for FontInfo {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        use ordered_float::OrderedFloat;
        self.family.hash(state);
        OrderedFloat(self.size).hash(state);
    }
}

impl PartialEq for DecorationShape {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.color == other.color && self.behind == other.behind
    }
}
impl Eq for DecorationShape {}

impl std::hash::Hash for DecorationShape {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.path.hash(state);
        self.color.hash(state);
        self.behind.hash(state);
    }
}
