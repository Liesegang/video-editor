//! Serializable configuration types for ensemble effectors and decorators.

use super::types::TransformData;
use crate::runtime::color::Color;

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
