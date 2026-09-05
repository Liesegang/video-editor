//! Shared composition order and spatial support of a vector appearance stack.

use super::draw_type::{DrawStyle, PathEffect};
use super::entity::StyleConfig;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompositePhase {
    Underlay,
    Body,
    Overlay,
}

impl DrawStyle {
    pub const fn composite_phase(&self) -> CompositePhase {
        match self {
            Self::DropShadow { .. } | Self::OuterGlow { .. } => CompositePhase::Underlay,
            Self::Fill { .. } | Self::Stroke { .. } => CompositePhase::Body,
            Self::ColorOverlay { .. }
            | Self::GradientOverlay { .. }
            | Self::PatternOverlay { .. }
            | Self::InnerShadow { .. }
            | Self::InnerGlow { .. }
            | Self::Satin { .. }
            | Self::BevelEmboss { .. } => CompositePhase::Overlay,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AppearanceOutsets {
    /// Fill and Stroke extend the geometry before the alpha mask is built.
    pub body: f32,
    /// Decoration extends that composed body, not the original geometry.
    pub visual: f32,
}

pub fn appearance_outsets(styles: &[StyleConfig]) -> AppearanceOutsets {
    let mut body = 0.0_f32;
    let mut decoration = 0.0_f32;
    for config in styles {
        let outset = config.style.visual_outset();
        if config.style.composite_phase() == CompositePhase::Body {
            body = body.max(outset);
        } else {
            decoration = decoration.max(outset);
        }
    }
    AppearanceOutsets {
        body,
        visual: body + decoration,
    }
}

pub fn path_effect_outset(effects: &[PathEffect]) -> f32 {
    effects
        .iter()
        .filter_map(|effect| match effect {
            PathEffect::Discrete { deviation, .. } => Some(deviation.abs() as f32),
            PathEffect::Dash { .. } | PathEffect::Corner { .. } | PathEffect::Trim { .. } => None,
        })
        .fold(0.0, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BlendMode;
    use crate::model::frame::color::Color;

    #[test]
    fn shadow_support_extends_the_styled_body_once() {
        let styles = [
            StyleConfig {
                id: uuid::Uuid::new_v4(),
                style: DrawStyle::Fill {
                    color: Color::white(),
                    offset: 12.0,
                },
            },
            StyleConfig {
                id: uuid::Uuid::new_v4(),
                style: DrawStyle::DropShadow {
                    color: Color::black(),
                    opacity: 1.0,
                    blend_mode: BlendMode::Normal,
                    angle: 0.0,
                    distance: 9.0,
                    spread: 0.0,
                    size: 6.0,
                },
            },
        ];
        assert_eq!(
            appearance_outsets(&styles),
            AppearanceOutsets {
                body: 12.0,
                visual: 27.0
            }
        );
        let mut reordered = styles.to_vec();
        reordered.reverse();
        assert_eq!(appearance_outsets(&reordered), appearance_outsets(&styles));
    }
}
