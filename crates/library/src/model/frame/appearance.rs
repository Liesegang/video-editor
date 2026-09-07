//! Shared composition order and spatial support of a vector appearance stack.

use super::draw_type::{DrawStyle, PathEffect};
use super::entity::StyleConfig;

/// Conservative local-space support reserved for source-raster
/// antialiasing. Bounds evaluation and the vector renderer must use this same
/// value so direct and Node-produced Shape images allocate identical masks.
pub const SOURCE_RASTER_OUTSET: f32 = 1.0;

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
            Self::Opacity { .. }
            | Self::ColorOverlay { .. }
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
    let mut visual = 0.0_f32;
    let mut has_image = false;
    for config in styles {
        let outset = config.style.visual_outset();
        if config.style.composite_phase() == CompositePhase::Body {
            body = body.max(outset);
            visual = visual.max(outset);
            has_image = true;
        } else if has_image {
            // Each Image -> Image stage consumes the complete previous result,
            // so two outward effects can expand support cumulatively. A stage
            // before the first raster body has transparent input.
            visual += outset;
        }
    }
    AppearanceOutsets { body, visual }
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
                    paint: (Color::white()).into(),
                    opacity: 1.0,
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
        assert_eq!(
            appearance_outsets(&reordered),
            AppearanceOutsets {
                body: 12.0,
                visual: 12.0,
            }
        );
    }

    #[test]
    fn chained_image_effects_expand_the_previous_result_in_authored_order() {
        let shadow = |distance| StyleConfig {
            id: uuid::Uuid::new_v4(),
            style: DrawStyle::DropShadow {
                color: Color::black(),
                opacity: 1.0,
                blend_mode: BlendMode::Normal,
                angle: 0.0,
                distance,
                spread: 0.0,
                size: 0.0,
            },
        };
        let styles = [
            StyleConfig {
                id: uuid::Uuid::new_v4(),
                style: DrawStyle::Fill {
                    paint: (Color::white()).into(),
                    opacity: 1.0,
                    offset: 0.0,
                },
            },
            shadow(5.0),
            shadow(7.0),
        ];
        assert_eq!(
            appearance_outsets(&styles),
            AppearanceOutsets {
                body: 0.0,
                visual: 12.0,
            }
        );
    }
}
