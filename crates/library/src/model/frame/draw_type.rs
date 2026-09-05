use crate::model::BlendMode;
use crate::model::frame::color::Color;
use crate::model::property::{GradientGeometry, GradientSpread, PatternKind, Vec2};
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum JoinType {
    #[default]
    Round,
    Bevel,
    Miter,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum CapType {
    Round,
    #[default]
    Square,
    Butt,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum BevelStyle {
    #[default]
    InnerBevel,
    OuterBevel,
    Emboss,
    PillowEmboss,
    StrokeEmboss,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum BevelTechnique {
    #[default]
    Smooth,
    ChiselHard,
    ChiselSoft,
}

/// Geometry shared by Bevel bounds calculation and its alpha-mask renderer.
///
/// `edge_size` is the complete one-sided kernel reach: the layer-style mask
/// divides it between morphology and Gaussian blur according to `edge_spread`.
/// Keeping that contract here prevents Preview bounds from using only the
/// authored soften value while the renderer derives an additional kernel from
/// the Bevel size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BevelRenderGeometry {
    pub(crate) offset_distance: f64,
    pub(crate) edge_size: f64,
    pub(crate) edge_spread: f64,
    pub(crate) inside: bool,
}

impl BevelRenderGeometry {
    pub(crate) fn new(
        style: BevelStyle,
        technique: BevelTechnique,
        depth: f64,
        size: f64,
        soften: f64,
    ) -> Self {
        let size = size.max(0.0);
        let soften = soften.max(0.0);
        let (edge_size, edge_spread) = match technique {
            BevelTechnique::Smooth => (soften.max(size * 0.25), 0.0),
            BevelTechnique::ChiselSoft => (soften.max(size * 0.08), 0.5),
            BevelTechnique::ChiselHard => (soften.max(0.01), 1.0),
        };
        Self {
            offset_distance: size * depth.max(0.0),
            edge_size,
            edge_spread,
            inside: matches!(style, BevelStyle::InnerBevel | BevelStyle::PillowEmboss),
        }
    }

    pub(crate) fn visual_outset(self) -> f32 {
        if self.inside {
            0.0
        } else {
            (self.offset_distance + self.edge_size) as f32
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum BevelDirection {
    #[default]
    Up,
    Down,
}

/// Renderer-bound Gradient after every managed authored stop has crossed the
/// color-management boundary exactly once.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
#[serde(deny_unknown_fields)]
pub struct GradientStyleStop {
    pub offset: OrderedFloat<f64>,
    pub color: Color,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
#[serde(deny_unknown_fields)]
pub struct GradientStyle {
    pub geometry: GradientGeometry,
    pub spread: GradientSpread,
    pub stops: Vec<GradientStyleStop>,
}

/// Renderer-bound procedural Pattern. Asset-backed patterns require a future
/// explicit renderer resource owner and are intentionally not represented by
/// an unresolvable Asset ID here.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
#[serde(deny_unknown_fields)]
pub struct PatternStyle {
    pub kind: PatternKind,
    pub foreground: Color,
    pub background: Color,
    pub scale: Vec2,
    pub phase: Vec2,
    pub angle: OrderedFloat<f64>,
    pub duty: OrderedFloat<f64>,
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
    ColorOverlay {
        color: Color,
        opacity: f64,
        blend_mode: BlendMode,
    },
    GradientOverlay {
        gradient: GradientStyle,
        opacity: f64,
        blend_mode: BlendMode,
    },
    PatternOverlay {
        pattern: PatternStyle,
        opacity: f64,
        blend_mode: BlendMode,
    },
    DropShadow {
        color: Color,
        opacity: f64,
        blend_mode: BlendMode,
        angle: f64,
        distance: f64,
        spread: f64,
        size: f64,
    },
    InnerShadow {
        color: Color,
        opacity: f64,
        blend_mode: BlendMode,
        angle: f64,
        distance: f64,
        spread: f64,
        size: f64,
    },
    OuterGlow {
        color: Color,
        opacity: f64,
        blend_mode: BlendMode,
        spread: f64,
        size: f64,
    },
    InnerGlow {
        color: Color,
        opacity: f64,
        blend_mode: BlendMode,
        spread: f64,
        size: f64,
    },
    Satin {
        color: Color,
        opacity: f64,
        blend_mode: BlendMode,
        angle: f64,
        distance: f64,
        size: f64,
        invert: bool,
    },
    BevelEmboss {
        style: BevelStyle,
        technique: BevelTechnique,
        depth: f64,
        direction: BevelDirection,
        size: f64,
        soften: f64,
        angle: f64,
        altitude: f64,
        highlight_color: Color,
        highlight_opacity: f64,
        highlight_blend_mode: BlendMode,
        shadow_color: Color,
        shadow_opacity: f64,
        shadow_blend_mode: BlendMode,
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
            DrawStyle::ColorOverlay {
                color,
                opacity,
                blend_mode,
            } => {
                color.hash(state);
                OrderedFloat(*opacity).hash(state);
                blend_mode.hash(state);
            }
            DrawStyle::GradientOverlay {
                gradient,
                opacity,
                blend_mode,
            } => {
                gradient.hash(state);
                OrderedFloat(*opacity).hash(state);
                blend_mode.hash(state);
            }
            DrawStyle::PatternOverlay {
                pattern,
                opacity,
                blend_mode,
            } => {
                pattern.hash(state);
                OrderedFloat(*opacity).hash(state);
                blend_mode.hash(state);
            }
            DrawStyle::DropShadow {
                color,
                opacity,
                blend_mode,
                angle,
                distance,
                spread,
                size,
            }
            | DrawStyle::InnerShadow {
                color,
                opacity,
                blend_mode,
                angle,
                distance,
                spread,
                size,
            } => {
                color.hash(state);
                OrderedFloat(*opacity).hash(state);
                blend_mode.hash(state);
                OrderedFloat(*angle).hash(state);
                OrderedFloat(*distance).hash(state);
                OrderedFloat(*spread).hash(state);
                OrderedFloat(*size).hash(state);
            }
            DrawStyle::OuterGlow {
                color,
                opacity,
                blend_mode,
                spread,
                size,
            }
            | DrawStyle::InnerGlow {
                color,
                opacity,
                blend_mode,
                spread,
                size,
            } => {
                color.hash(state);
                OrderedFloat(*opacity).hash(state);
                blend_mode.hash(state);
                OrderedFloat(*spread).hash(state);
                OrderedFloat(*size).hash(state);
            }
            DrawStyle::Satin {
                color,
                opacity,
                blend_mode,
                angle,
                distance,
                size,
                invert,
            } => {
                color.hash(state);
                OrderedFloat(*opacity).hash(state);
                blend_mode.hash(state);
                OrderedFloat(*angle).hash(state);
                OrderedFloat(*distance).hash(state);
                OrderedFloat(*size).hash(state);
                invert.hash(state);
            }
            DrawStyle::BevelEmboss {
                style,
                technique,
                depth,
                direction,
                size,
                soften,
                angle,
                altitude,
                highlight_color,
                highlight_opacity,
                highlight_blend_mode,
                shadow_color,
                shadow_opacity,
                shadow_blend_mode,
            } => {
                style.hash(state);
                technique.hash(state);
                OrderedFloat(*depth).hash(state);
                direction.hash(state);
                OrderedFloat(*size).hash(state);
                OrderedFloat(*soften).hash(state);
                OrderedFloat(*angle).hash(state);
                OrderedFloat(*altitude).hash(state);
                highlight_color.hash(state);
                OrderedFloat(*highlight_opacity).hash(state);
                highlight_blend_mode.hash(state);
                shadow_color.hash(state);
                OrderedFloat(*shadow_opacity).hash(state);
                shadow_blend_mode.hash(state);
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
            (
                DrawStyle::ColorOverlay {
                    color: c1,
                    opacity: o1,
                    blend_mode: b1,
                },
                DrawStyle::ColorOverlay {
                    color: c2,
                    opacity: o2,
                    blend_mode: b2,
                },
            ) => c1 == c2 && OrderedFloat(*o1) == OrderedFloat(*o2) && b1 == b2,
            (
                DrawStyle::GradientOverlay {
                    gradient: g1,
                    opacity: o1,
                    blend_mode: b1,
                },
                DrawStyle::GradientOverlay {
                    gradient: g2,
                    opacity: o2,
                    blend_mode: b2,
                },
            ) => g1 == g2 && OrderedFloat(*o1) == OrderedFloat(*o2) && b1 == b2,
            (
                DrawStyle::PatternOverlay {
                    pattern: p1,
                    opacity: o1,
                    blend_mode: b1,
                },
                DrawStyle::PatternOverlay {
                    pattern: p2,
                    opacity: o2,
                    blend_mode: b2,
                },
            ) => p1 == p2 && OrderedFloat(*o1) == OrderedFloat(*o2) && b1 == b2,
            (
                DrawStyle::DropShadow {
                    color: c1,
                    opacity: o1,
                    blend_mode: b1,
                    angle: a1,
                    distance: d1,
                    spread: sp1,
                    size: sz1,
                },
                DrawStyle::DropShadow {
                    color: c2,
                    opacity: o2,
                    blend_mode: b2,
                    angle: a2,
                    distance: d2,
                    spread: sp2,
                    size: sz2,
                },
            )
            | (
                DrawStyle::InnerShadow {
                    color: c1,
                    opacity: o1,
                    blend_mode: b1,
                    angle: a1,
                    distance: d1,
                    spread: sp1,
                    size: sz1,
                },
                DrawStyle::InnerShadow {
                    color: c2,
                    opacity: o2,
                    blend_mode: b2,
                    angle: a2,
                    distance: d2,
                    spread: sp2,
                    size: sz2,
                },
            ) => {
                c1 == c2
                    && OrderedFloat(*o1) == OrderedFloat(*o2)
                    && b1 == b2
                    && OrderedFloat(*a1) == OrderedFloat(*a2)
                    && OrderedFloat(*d1) == OrderedFloat(*d2)
                    && OrderedFloat(*sp1) == OrderedFloat(*sp2)
                    && OrderedFloat(*sz1) == OrderedFloat(*sz2)
            }
            (
                DrawStyle::OuterGlow {
                    color: c1,
                    opacity: o1,
                    blend_mode: b1,
                    spread: sp1,
                    size: sz1,
                },
                DrawStyle::OuterGlow {
                    color: c2,
                    opacity: o2,
                    blend_mode: b2,
                    spread: sp2,
                    size: sz2,
                },
            )
            | (
                DrawStyle::InnerGlow {
                    color: c1,
                    opacity: o1,
                    blend_mode: b1,
                    spread: sp1,
                    size: sz1,
                },
                DrawStyle::InnerGlow {
                    color: c2,
                    opacity: o2,
                    blend_mode: b2,
                    spread: sp2,
                    size: sz2,
                },
            ) => {
                c1 == c2
                    && OrderedFloat(*o1) == OrderedFloat(*o2)
                    && b1 == b2
                    && OrderedFloat(*sp1) == OrderedFloat(*sp2)
                    && OrderedFloat(*sz1) == OrderedFloat(*sz2)
            }
            (
                DrawStyle::Satin {
                    color: c1,
                    opacity: o1,
                    blend_mode: b1,
                    angle: a1,
                    distance: d1,
                    size: sz1,
                    invert: i1,
                },
                DrawStyle::Satin {
                    color: c2,
                    opacity: o2,
                    blend_mode: b2,
                    angle: a2,
                    distance: d2,
                    size: sz2,
                    invert: i2,
                },
            ) => {
                c1 == c2
                    && OrderedFloat(*o1) == OrderedFloat(*o2)
                    && b1 == b2
                    && OrderedFloat(*a1) == OrderedFloat(*a2)
                    && OrderedFloat(*d1) == OrderedFloat(*d2)
                    && OrderedFloat(*sz1) == OrderedFloat(*sz2)
                    && i1 == i2
            }
            (
                DrawStyle::BevelEmboss {
                    style: st1,
                    technique: t1,
                    depth: d1,
                    direction: dir1,
                    size: sz1,
                    soften: so1,
                    angle: a1,
                    altitude: al1,
                    highlight_color: hc1,
                    highlight_opacity: ho1,
                    highlight_blend_mode: hb1,
                    shadow_color: sc1,
                    shadow_opacity: sho1,
                    shadow_blend_mode: sb1,
                },
                DrawStyle::BevelEmboss {
                    style: st2,
                    technique: t2,
                    depth: d2,
                    direction: dir2,
                    size: sz2,
                    soften: so2,
                    angle: a2,
                    altitude: al2,
                    highlight_color: hc2,
                    highlight_opacity: ho2,
                    highlight_blend_mode: hb2,
                    shadow_color: sc2,
                    shadow_opacity: sho2,
                    shadow_blend_mode: sb2,
                },
            ) => {
                st1 == st2
                    && t1 == t2
                    && OrderedFloat(*d1) == OrderedFloat(*d2)
                    && dir1 == dir2
                    && OrderedFloat(*sz1) == OrderedFloat(*sz2)
                    && OrderedFloat(*so1) == OrderedFloat(*so2)
                    && OrderedFloat(*a1) == OrderedFloat(*a2)
                    && OrderedFloat(*al1) == OrderedFloat(*al2)
                    && hc1 == hc2
                    && OrderedFloat(*ho1) == OrderedFloat(*ho2)
                    && hb1 == hb2
                    && sc1 == sc2
                    && OrderedFloat(*sho1) == OrderedFloat(*sho2)
                    && sb1 == sb2
            }
            _ => false,
        }
    }
}
impl Eq for DrawStyle {}

impl DrawStyle {
    /// Conservative symmetric expansion beyond the source geometry.
    ///
    /// Rendering and Preview bounds share this definition so alpha-mask
    /// styles cannot be clipped by a tighter, independently maintained box.
    pub fn visual_outset(&self) -> f32 {
        match self {
            Self::Fill { offset, .. } => offset.max(0.0) as f32,
            Self::Stroke {
                width,
                offset,
                cap,
                join,
                miter,
                ..
            } if *width > 0.0 => stroke_visual_outset(*width, *offset, cap, join, *miter),
            Self::Stroke { .. }
            | Self::ColorOverlay { .. }
            | Self::GradientOverlay { .. }
            | Self::PatternOverlay { .. }
            | Self::InnerShadow { .. }
            | Self::InnerGlow { .. }
            | Self::Satin { .. } => 0.0,
            Self::DropShadow {
                distance,
                spread,
                size,
                ..
            } => {
                let radius = size.max(0.0);
                (distance.abs() + radius + radius * spread.clamp(0.0, 1.0)) as f32
            }
            Self::OuterGlow { spread, size, .. } => {
                let radius = size.max(0.0);
                (radius + radius * spread.clamp(0.0, 1.0)) as f32
            }
            Self::BevelEmboss {
                style,
                technique,
                depth,
                size,
                soften,
                ..
            } => {
                BevelRenderGeometry::new(*style, *technique, *depth, *size, *soften).visual_outset()
            }
        }
    }
}

fn stroke_visual_outset(
    width: f64,
    offset: f64,
    cap: &CapType,
    join: &JoinType,
    miter_limit: f64,
) -> f32 {
    // Shape strokes clip negative offsets to the source silhouette, while
    // Text strokes reduce their effective width by the same amount. This
    // radius therefore conservatively covers both production paint paths.
    let radius = (width / 2.0 + offset).max(0.0);
    let join_multiplier = match join {
        JoinType::Miter => miter_limit.max(1.0),
        JoinType::Round | JoinType::Bevel => 1.0,
    };
    let cap_multiplier = match cap {
        CapType::Square => std::f64::consts::SQRT_2,
        CapType::Round | CapType::Butt => 1.0,
    };
    (radius * join_multiplier.max(cap_multiplier)) as f32
}

/// Unit space used by a Trim Path operation.
///
/// `Normalized` treats one unit as the accumulated length of all contours.
/// `Length` evaluates authored values in path-local pixels.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TrimPathUnits {
    Normalized,
    Length,
}

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
        offset: f64,
        units: TrimPathUnits,
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
            PathEffect::Trim {
                start,
                end,
                offset,
                units,
            } => {
                OrderedFloat(*start).hash(state);
                OrderedFloat(*end).hash(state);
                OrderedFloat(*offset).hash(state);
                units.hash(state);
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
            (
                PathEffect::Trim {
                    start: s1,
                    end: e1,
                    offset: o1,
                    units: u1,
                },
                PathEffect::Trim {
                    start: s2,
                    end: e2,
                    offset: o2,
                    units: u2,
                },
            ) => {
                OrderedFloat(*s1) == OrderedFloat(*s2)
                    && OrderedFloat(*e1) == OrderedFloat(*e2)
                    && OrderedFloat(*o1) == OrderedFloat(*o2)
                    && u1 == u2
            }
            _ => false,
        }
    }
}
impl Eq for PathEffect {}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke_with_bounds_policy(
        width: f64,
        offset: f64,
        cap: CapType,
        join: JoinType,
        miter: f64,
    ) -> DrawStyle {
        DrawStyle::Stroke {
            color: Color::white(),
            width,
            offset,
            cap,
            join,
            miter,
            dash_array: Vec::new(),
            dash_offset: 0.0,
        }
    }

    #[test]
    fn stroke_outset_matches_miter_and_square_cap_inflation_without_expanding_other_joins() {
        let round = stroke_with_bounds_policy(10.0, 2.0, CapType::Round, JoinType::Round, 100.0);
        let bevel = stroke_with_bounds_policy(10.0, 2.0, CapType::Butt, JoinType::Bevel, 100.0);
        let miter = stroke_with_bounds_policy(10.0, 2.0, CapType::Butt, JoinType::Miter, 4.0);
        let square = stroke_with_bounds_policy(10.0, 2.0, CapType::Square, JoinType::Round, 100.0);
        assert_eq!(round.visual_outset(), 7.0);
        assert_eq!(bevel.visual_outset(), 7.0);
        assert_eq!(miter.visual_outset(), 28.0);
        assert!((square.visual_outset() - (7.0 * std::f32::consts::SQRT_2)).abs() < f32::EPSILON);

        let inset_text_radius =
            stroke_with_bounds_policy(10.0, -2.0, CapType::Butt, JoinType::Round, 4.0);
        let disabled = stroke_with_bounds_policy(0.0, 20.0, CapType::Square, JoinType::Miter, 20.0);
        assert_eq!(inset_text_radius.visual_outset(), 3.0);
        assert_eq!(disabled.visual_outset(), 0.0);
    }

    #[test]
    fn bevel_geometry_accounts_for_derived_kernel_when_soften_and_depth_are_zero() {
        for (technique, expected_edge_size, expected_spread) in [
            (BevelTechnique::Smooth, 3.0, 0.0),
            (BevelTechnique::ChiselSoft, 0.96, 0.5),
            (BevelTechnique::ChiselHard, 0.01, 1.0),
        ] {
            let geometry =
                BevelRenderGeometry::new(BevelStyle::OuterBevel, technique, 0.0, 12.0, 0.0);
            assert!((geometry.edge_size - expected_edge_size).abs() < f64::EPSILON);
            assert!((geometry.edge_spread - expected_spread).abs() < f64::EPSILON);
            assert!((geometry.visual_outset() - expected_edge_size as f32).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn only_bevel_techniques_rendered_inside_the_source_have_no_outset() {
        for style in [BevelStyle::InnerBevel, BevelStyle::PillowEmboss] {
            let geometry = BevelRenderGeometry::new(style, BevelTechnique::Smooth, 1.5, 12.0, 0.0);
            assert!(geometry.inside);
            assert_eq!(geometry.visual_outset(), 0.0);
        }
        for style in [
            BevelStyle::OuterBevel,
            BevelStyle::Emboss,
            BevelStyle::StrokeEmboss,
        ] {
            let geometry = BevelRenderGeometry::new(style, BevelTechnique::Smooth, 1.5, 12.0, 0.0);
            assert!(!geometry.inside);
            assert_eq!(geometry.visual_outset(), 21.0);
        }
    }
}
