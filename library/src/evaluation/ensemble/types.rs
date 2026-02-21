use crate::runtime::color::Color;
use skia_safe::{Font, Point, Rect, Size};
use std::collections::HashMap;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransformData {
    pub translate: (f32, f32),
    pub rotate: f32,
    pub scale: (f32, f32),
    pub opacity: f32,
    pub color_override: Option<Color>,
}

impl PartialEq for TransformData {
    fn eq(&self, other: &Self) -> bool {
        use ordered_float::OrderedFloat;
        OrderedFloat(self.translate.0) == OrderedFloat(other.translate.0)
            && OrderedFloat(self.translate.1) == OrderedFloat(other.translate.1)
            && OrderedFloat(self.rotate) == OrderedFloat(other.rotate)
            && OrderedFloat(self.scale.0) == OrderedFloat(other.scale.0)
            && OrderedFloat(self.scale.1) == OrderedFloat(other.scale.1)
            && OrderedFloat(self.opacity) == OrderedFloat(other.opacity)
            && self.color_override == other.color_override
    }
}
impl Eq for TransformData {}

impl std::hash::Hash for TransformData {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        use ordered_float::OrderedFloat;
        OrderedFloat(self.translate.0).hash(state);
        OrderedFloat(self.translate.1).hash(state);
        OrderedFloat(self.rotate).hash(state);
        OrderedFloat(self.scale.0).hash(state);
        OrderedFloat(self.scale.1).hash(state);
        OrderedFloat(self.opacity).hash(state);
        self.color_override.hash(state);
    }
}

impl TransformData {
    pub fn identity() -> Self {
        Self {
            translate: (0.0, 0.0),
            rotate: 0.0,
            scale: (1.0, 1.0),
            opacity: 1.0,
            color_override: None,
        }
    }

    pub fn combine(&self, other: &TransformData) -> TransformData {
        TransformData {
            translate: (
                self.translate.0 + other.translate.0,
                self.translate.1 + other.translate.1,
            ),
            rotate: self.rotate + other.rotate,
            scale: (self.scale.0 * other.scale.0, self.scale.1 * other.scale.1),
            opacity: self.opacity * other.opacity,
            color_override: other.color_override.clone().or(self.color_override.clone()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EnsembleChar {
    pub glyph_id: u16,
    pub base_pos: Point,
    pub size: Size,
    pub transform: TransformData,
}

impl EnsembleChar {
    pub fn new(glyph_id: u16, base_pos: Point, size: Size) -> Self {
        Self {
            glyph_id,
            base_pos,
            size,
            transform: TransformData::identity(),
        }
    }

    pub fn center(&self) -> Point {
        Point::new(
            self.base_pos.x + self.size.width / 2.0,
            self.base_pos.y + self.size.height / 2.0,
        )
    }
}

#[derive(Debug, Clone)]
pub struct EnsembleLine {
    pub chars: Vec<EnsembleChar>,
    pub base_bounds: Rect,
}

impl EnsembleLine {
    pub fn new(chars: Vec<EnsembleChar>, base_bounds: Rect) -> Self {
        Self { chars, base_bounds }
    }

    pub fn reset_transforms(&mut self) {
        for ch in &mut self.chars {
            ch.transform = TransformData::identity();
        }
    }
}

#[derive(Debug, Clone)]
pub struct EffectorContext {
    pub time: f32,
    pub index: usize,
    pub total: usize,
    pub line_index: usize,
    pub char_center: Point,
}

pub struct EnsembleText {
    pub raw_content: String,
    pub font: Font,
    pub base_color: Color,
    pub lines: Vec<EnsembleLine>,

    // Procedural Layer
    pub effectors: Vec<super::target::EffectorEntry>,

    // Patch Layer
    pub patches: HashMap<usize, TransformData>,

    // Decoration
    pub decorators: Vec<Box<dyn super::decorators::Decorator>>,
}

impl EnsembleText {
    pub fn new(raw_content: String, font: Font, base_color: Color) -> Self {
        Self {
            raw_content,
            font,
            base_color,
            lines: Vec::new(),
            effectors: Vec::new(),
            patches: HashMap::new(),
            decorators: Vec::new(),
        }
    }

    pub fn add_effector(&mut self, effector: Box<dyn super::effectors::Effector>) {
        self.effectors.push(super::target::EffectorEntry::new(
            effector,
            super::target::EffectorTarget::default(),
        ));
    }

    pub fn add_effector_with_target(
        &mut self,
        effector: Box<dyn super::effectors::Effector>,
        target: super::target::EffectorTarget,
    ) {
        self.effectors
            .push(super::target::EffectorEntry::new(effector, target));
    }

    pub fn add_decorator(&mut self, decorator: Box<dyn super::decorators::Decorator>) {
        self.decorators.push(decorator);
    }

    pub fn add_patch(&mut self, index: usize, transform: TransformData) {
        self.patches.insert(index, transform);
    }

    pub fn reset_all_transforms(&mut self) {
        for line in &mut self.lines {
            line.reset_transforms();
        }
    }

    pub fn total_char_count(&self) -> usize {
        self.lines.iter().map(|line| line.chars.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_identity() {
        let t = TransformData::identity();
        assert_eq!(t.translate, (0.0, 0.0));
        assert_eq!(t.rotate, 0.0);
        assert_eq!(t.scale, (1.0, 1.0));
        assert_eq!(t.opacity, 1.0);
        assert!(t.color_override.is_none());
    }

    #[test]
    fn test_transform_combine() {
        let t1 = TransformData {
            translate: (10.0, 20.0),
            rotate: 0.5,
            scale: (2.0, 3.0),
            opacity: 0.8,
            color_override: None,
        };
        let t2 = TransformData {
            translate: (5.0, 15.0),
            rotate: 0.3,
            scale: (0.5, 0.5),
            opacity: 0.5,
            color_override: Some(Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            }),
        };

        let combined = t1.combine(&t2);
        assert_eq!(combined.translate, (15.0, 35.0));
        assert_eq!(combined.rotate, 0.8);
        assert_eq!(combined.scale, (1.0, 1.5));
        assert_eq!(combined.opacity, 0.4);
        assert!(combined.color_override.is_some());
    }

    #[test]
    fn test_ensemble_char_center() {
        let ch = EnsembleChar::new(42, Point::new(10.0, 20.0), Size::new(8.0, 12.0));
        let center = ch.center();
        assert_eq!(center.x, 14.0);
        assert_eq!(center.y, 26.0);
    }
}
