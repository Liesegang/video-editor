use crate::model::frame::color::Color;
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EnsembleData {
    pub enabled: bool,
    pub effector_configs: Vec<EffectorConfig>,
    pub decorator_configs: Vec<DecoratorConfig>,
    pub patches: std::collections::HashMap<usize, TransformData>,
}

impl PartialEq for EnsembleData {
    fn eq(&self, other: &Self) -> bool {
        if self.enabled != other.enabled {
            return false;
        }
        if self.effector_configs != other.effector_configs {
            return false;
        }
        if self.decorator_configs != other.decorator_configs {
            return false;
        }
        if self.patches.len() != other.patches.len() {
            return false;
        }
        for (k, v) in &self.patches {
            if other.patches.get(k) != Some(v) {
                return false;
            }
        }
        true
    }
}
impl Eq for EnsembleData {}

impl std::hash::Hash for EnsembleData {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.enabled.hash(state);
        self.effector_configs.hash(state);
        self.decorator_configs.hash(state);
        // Sort keys for deterministic hashing of HashMap
        let mut patch_keys: Vec<&usize> = self.patches.keys().collect();
        patch_keys.sort();
        for k in patch_keys {
            k.hash(state);
            self.patches.get(k).hash(state);
        }
    }
}

impl EnsembleData {
    pub fn new() -> Self {
        Self {
            enabled: false,
            effector_configs: Vec::new(),
            decorator_configs: Vec::new(),
            patches: std::collections::HashMap::new(),
        }
    }
}

impl Default for EnsembleData {
    fn default() -> Self {
        Self::new()
    }
}

/// Effector設定（シリアライズ可能）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum EffectorConfig {
    Transform {
        translate: (f32, f32),
        rotate: f32,
        scale: (f32, f32),
        target: super::target::EffectorTarget,
    },
    StepDelay {
        delay_per_element: f32,
        duration: f32,
        from_opacity: f32,
        to_opacity: f32,
        target: super::target::EffectorTarget,
    },
    Opacity {
        target_opacity: f32,
        mode: super::effectors::OpacityMode,
        target: super::target::EffectorTarget,
    },
    Randomize {
        translate_range: (f32, f32),
        rotate_range: f32,
        scale_range: (f32, f32),
        seed: u64,
        target: super::target::EffectorTarget,
    },
}

impl PartialEq for EffectorConfig {
    fn eq(&self, other: &Self) -> bool {
        use ordered_float::OrderedFloat;
        match (self, other) {
            (
                EffectorConfig::Transform {
                    translate: t1,
                    rotate: r1,
                    scale: s1,
                    target: tg1,
                },
                EffectorConfig::Transform {
                    translate: t2,
                    rotate: r2,
                    scale: s2,
                    target: tg2,
                },
            ) => {
                OrderedFloat(t1.0) == OrderedFloat(t2.0)
                    && OrderedFloat(t1.1) == OrderedFloat(t2.1)
                    && OrderedFloat(*r1) == OrderedFloat(*r2)
                    && OrderedFloat(s1.0) == OrderedFloat(s2.0)
                    && OrderedFloat(s1.1) == OrderedFloat(s2.1)
                    && tg1 == tg2
            }
            (
                EffectorConfig::StepDelay {
                    delay_per_element: d1,
                    duration: du1,
                    from_opacity: f1,
                    to_opacity: to1,
                    target: tg1,
                },
                EffectorConfig::StepDelay {
                    delay_per_element: d2,
                    duration: du2,
                    from_opacity: f2,
                    to_opacity: to2,
                    target: tg2,
                },
            ) => {
                OrderedFloat(*d1) == OrderedFloat(*d2)
                    && OrderedFloat(*du1) == OrderedFloat(*du2)
                    && OrderedFloat(*f1) == OrderedFloat(*f2)
                    && OrderedFloat(*to1) == OrderedFloat(*to2)
                    && tg1 == tg2
            }
            (
                EffectorConfig::Opacity {
                    target_opacity: o1,
                    mode: m1,
                    target: tg1,
                },
                EffectorConfig::Opacity {
                    target_opacity: o2,
                    mode: m2,
                    target: tg2,
                },
            ) => OrderedFloat(*o1) == OrderedFloat(*o2) && m1 == m2 && tg1 == tg2,
            (
                EffectorConfig::Randomize {
                    translate_range: tr1,
                    rotate_range: rr1,
                    scale_range: sr1,
                    seed: sd1,
                    target: tg1,
                },
                EffectorConfig::Randomize {
                    translate_range: tr2,
                    rotate_range: rr2,
                    scale_range: sr2,
                    seed: sd2,
                    target: tg2,
                },
            ) => {
                OrderedFloat(tr1.0) == OrderedFloat(tr2.0)
                    && OrderedFloat(tr1.1) == OrderedFloat(tr2.1)
                    && OrderedFloat(*rr1) == OrderedFloat(*rr2)
                    && OrderedFloat(sr1.0) == OrderedFloat(sr2.0)
                    && OrderedFloat(sr1.1) == OrderedFloat(sr2.1)
                    && sd1 == sd2
                    && tg1 == tg2
            }
            _ => false,
        }
    }
}
impl Eq for EffectorConfig {}

impl std::hash::Hash for EffectorConfig {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        use ordered_float::OrderedFloat;
        std::mem::discriminant(self).hash(state);
        match self {
            EffectorConfig::Transform {
                translate,
                rotate,
                scale,
                target,
            } => {
                OrderedFloat(translate.0).hash(state);
                OrderedFloat(translate.1).hash(state);
                OrderedFloat(*rotate).hash(state);
                OrderedFloat(scale.0).hash(state);
                OrderedFloat(scale.1).hash(state);
                target.hash(state);
            }
            EffectorConfig::StepDelay {
                delay_per_element,
                duration,
                from_opacity,
                to_opacity,
                target,
            } => {
                OrderedFloat(*delay_per_element).hash(state);
                OrderedFloat(*duration).hash(state);
                OrderedFloat(*from_opacity).hash(state);
                OrderedFloat(*to_opacity).hash(state);
                target.hash(state);
            }
            EffectorConfig::Opacity {
                target_opacity,
                mode,
                target,
            } => {
                OrderedFloat(*target_opacity).hash(state);
                mode.hash(state);
                target.hash(state);
            }
            EffectorConfig::Randomize {
                translate_range,
                rotate_range,
                scale_range,
                seed,
                target,
            } => {
                OrderedFloat(translate_range.0).hash(state);
                OrderedFloat(translate_range.1).hash(state);
                OrderedFloat(*rotate_range).hash(state);
                OrderedFloat(scale_range.0).hash(state);
                OrderedFloat(scale_range.1).hash(state);
                seed.hash(state);
                target.hash(state);
            }
        }
    }
}

/// Decorator設定（シリアライズ可能）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum DecoratorConfig {
    Backplate {
        target: super::decorators::BackplateTarget,
        shape: super::decorators::BackplateShape,
        color: Color,
        padding: (f32, f32, f32, f32),
        corner_radius: f32,
    },
}

impl PartialEq for DecoratorConfig {
    fn eq(&self, other: &Self) -> bool {
        use ordered_float::OrderedFloat;
        match (self, other) {
            (
                DecoratorConfig::Backplate {
                    target: t1,
                    shape: s1,
                    color: c1,
                    padding: p1,
                    corner_radius: cr1,
                },
                DecoratorConfig::Backplate {
                    target: t2,
                    shape: s2,
                    color: c2,
                    padding: p2,
                    corner_radius: cr2,
                },
            ) => {
                t1 == t2
                    && s1 == s2
                    && c1 == c2
                    && OrderedFloat(p1.0) == OrderedFloat(p2.0)
                    && OrderedFloat(p1.1) == OrderedFloat(p2.1)
                    && OrderedFloat(p1.2) == OrderedFloat(p2.2)
                    && OrderedFloat(p1.3) == OrderedFloat(p2.3)
                    && OrderedFloat(*cr1) == OrderedFloat(*cr2)
            }
        }
    }
}
impl Eq for DecoratorConfig {}

impl std::hash::Hash for DecoratorConfig {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        use ordered_float::OrderedFloat;
        std::mem::discriminant(self).hash(state);
        match self {
            DecoratorConfig::Backplate {
                target,
                shape,
                color,
                padding,
                corner_radius,
            } => {
                target.hash(state);
                shape.hash(state);
                color.hash(state);
                OrderedFloat(padding.0).hash(state);
                OrderedFloat(padding.1).hash(state);
                OrderedFloat(padding.2).hash(state);
                OrderedFloat(padding.3).hash(state);
                OrderedFloat(*corner_radius).hash(state);
            }
        }
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
