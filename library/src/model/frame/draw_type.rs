use crate::model::frame::color::Color;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

/// Blend mode for track compositing.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum BlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Add,
}

impl Default for BlendMode {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
pub enum JoinType {
    Round,
    Bevel,
    Miter,
}

impl Default for JoinType {
    fn default() -> Self {
        Self::Round
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
pub enum CapType {
    Round,
    Square,
    Butt,
}

impl Default for CapType {
    fn default() -> Self {
        Self::Square
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)] // Removed PartialEq, Eq
pub enum DrawStyle {
    Fill {
        color: Color,
        #[serde(default)]
        offset: f64,
    },
    Stroke {
        #[serde(default)]
        color: Color,
        #[serde(default)]
        width: f64,
        #[serde(default)]
        offset: f64,
        #[serde(default)]
        cap: CapType,
        #[serde(default)]
        join: JoinType,
        #[serde(default)]
        miter: f64,
        #[serde(default)]
        dash_array: Vec<f64>,
        #[serde(default)]
        dash_offset: f64,
    },
}

impl Hash for DrawStyle {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            DrawStyle::Fill { color, offset } => {
                color.hash(state);
                OrderedFloat(*offset).hash(state);
            }
            DrawStyle::Stroke {
                color,
                width,
                offset,
                cap,
                join,
                miter,
                dash_array,
                dash_offset,
            } => {
                color.hash(state);
                OrderedFloat(*width).hash(state);
                OrderedFloat(*offset).hash(state);
                cap.hash(state);
                join.hash(state);
                OrderedFloat(*miter).hash(state);
                // Hash dash_array
                for d in dash_array {
                    OrderedFloat(*d).hash(state);
                }
                OrderedFloat(*dash_offset).hash(state);
            }
        }
    }
}

impl Default for DrawStyle {
    fn default() -> Self {
        Self::Fill {
            color: Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            offset: 0.0,
        }
    }
}

impl PartialEq for DrawStyle {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                DrawStyle::Stroke {
                    width: w1,
                    color: c1,
                    offset: o1,
                    join: j1,
                    cap: cp1,
                    miter: m1,
                    dash_array: da1,
                    dash_offset: do1,
                },
                DrawStyle::Stroke {
                    width: w2,
                    color: c2,
                    offset: o2,
                    join: j2,
                    cap: cp2,
                    miter: m2,
                    dash_array: da2,
                    dash_offset: do2,
                },
            ) => {
                OrderedFloat(*w1) == OrderedFloat(*w2)
                    && c1 == c2
                    && OrderedFloat(*o1) == OrderedFloat(*o2)
                    && j1 == j2
                    && cp1 == cp2
                    && OrderedFloat(*m1) == OrderedFloat(*m2)
                    && da1.len() == da2.len()
                    && da1
                        .iter()
                        .zip(da2.iter())
                        .all(|(a, b)| OrderedFloat(*a) == OrderedFloat(*b))
                    && OrderedFloat(*do1) == OrderedFloat(*do2)
            }
            (
                DrawStyle::Fill {
                    color: c1,
                    offset: e1,
                },
                DrawStyle::Fill {
                    color: c2,
                    offset: e2,
                },
            ) => c1 == c2 && OrderedFloat(*e1) == OrderedFloat(*e2),
            _ => false,
        }
    }
}
impl Eq for DrawStyle {}

#[derive(Serialize, Deserialize, Debug, Clone)] // Removed PartialEq, Eq
#[serde(tag = "type")]
pub enum PathEffect {
    Dash {
        intervals: Vec<f64>,
        phase: f64,
    },
    Corner {
        radius: f64,
    },
    Discrete {
        seg_length: f64,
        deviation: f64,
        seed: u64,
    },
    Trim {
        start: f64,
        end: f64,
    },
}

impl Hash for PathEffect {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            PathEffect::Dash { intervals, phase } => {
                for i in intervals {
                    OrderedFloat(*i).hash(state);
                }
                OrderedFloat(*phase).hash(state);
            }
            PathEffect::Corner { radius } => {
                OrderedFloat(*radius).hash(state);
            }
            PathEffect::Discrete {
                seg_length,
                deviation,
                seed,
            } => {
                OrderedFloat(*seg_length).hash(state);
                OrderedFloat(*deviation).hash(state);
                seed.hash(state);
            }
            PathEffect::Trim { start, end } => {
                OrderedFloat(*start).hash(state);
                OrderedFloat(*end).hash(state);
            }
        }
    }
}

impl PartialEq for PathEffect {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                PathEffect::Dash {
                    intervals: i1,
                    phase: p1,
                },
                PathEffect::Dash {
                    intervals: i2,
                    phase: p2,
                },
            ) => {
                i1.iter()
                    .zip(i2.iter())
                    .all(|(a, b)| OrderedFloat(*a) == OrderedFloat(*b))
                    && i1.len() == i2.len()
                    && OrderedFloat(*p1) == OrderedFloat(*p2)
            }
            (PathEffect::Corner { radius: r1 }, PathEffect::Corner { radius: r2 }) => {
                OrderedFloat(*r1) == OrderedFloat(*r2)
            }
            (
                PathEffect::Discrete {
                    seg_length: s1,
                    deviation: d1,
                    seed: seed1,
                },
                PathEffect::Discrete {
                    seg_length: s2,
                    deviation: d2,
                    seed: seed2,
                },
            ) => {
                OrderedFloat(*s1) == OrderedFloat(*s2)
                    && OrderedFloat(*d1) == OrderedFloat(*d2)
                    && seed1 == seed2
            }
            (PathEffect::Trim { start: s1, end: e1 }, PathEffect::Trim { start: s2, end: e2 }) => {
                OrderedFloat(*s1) == OrderedFloat(*s2) && OrderedFloat(*e1) == OrderedFloat(*e2)
            }
            _ => false,
        }
    }
}
impl Eq for PathEffect {}
