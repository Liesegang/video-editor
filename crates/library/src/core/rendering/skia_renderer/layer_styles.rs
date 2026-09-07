//! Shared, working-linear layer-style composition for Shape and Text.

mod mask;

use skia_safe::canvas::SaveLayerRec;
use skia_safe::{Canvas, Paint};

use crate::error::LibraryError;
use crate::model::BlendMode;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{
    BevelDirection, BevelRenderGeometry, BevelStyle, BevelTechnique, DrawStyle,
};
use crate::rendering::blend::BlendRuntime;
use crate::rendering::skia_working_surface::SkiaSurfaceContract;

pub(super) use mask::LayerMask;

pub(super) use crate::model::frame::appearance::CompositePhase;

struct EdgeSpec<'a> {
    color: &'a Color,
    opacity: f32,
    blend_mode: BlendMode,
    offset: (f32, f32),
    size: f64,
    spread: f64,
    inside: bool,
}

pub(super) struct LayerStyleRenderer<'a> {
    surface_contract: &'a SkiaSurfaceContract,
    blend_runtime: &'a mut BlendRuntime,
    render_scale: f64,
}

impl<'a> LayerStyleRenderer<'a> {
    pub(super) const fn new(
        surface_contract: &'a SkiaSurfaceContract,
        blend_runtime: &'a mut BlendRuntime,
        render_scale: f64,
    ) -> Self {
        Self {
            surface_contract,
            blend_runtime,
            render_scale,
        }
    }

    /// Apply one Image -> Image style to the complete upstream alpha image.
    /// This is the single authored-order compositor used by both direct
    /// Shape/Text appearance and Module image-style groups.
    pub(super) fn compose(
        &mut self,
        canvas: &Canvas,
        style: &DrawStyle,
        mask: &LayerMask,
    ) -> Result<(), LibraryError> {
        if let DrawStyle::Opacity { opacity } = style {
            if !opacity.is_finite() || !(0.0..=1.0).contains(opacity) {
                return Err(LibraryError::Render(
                    "Image Opacity must be finite and between 0 and 1".to_string(),
                ));
            }
            canvas.save_layer_alpha_f(Some(mask.visual_bounds()), *opacity as f32);
            mask.draw_content(canvas);
            canvas.restore();
            return Ok(());
        }
        match style.composite_phase() {
            CompositePhase::Underlay => {
                self.draw(canvas, style, mask)?;
                mask.draw_content(canvas);
            }
            CompositePhase::Overlay => {
                mask.draw_content(canvas);
                self.draw(canvas, style, mask)?;
            }
            CompositePhase::Body => {
                return Err(LibraryError::Render(
                    "Fill and Stroke require a Shape input, not an Image".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub(super) fn draw(
        &mut self,
        canvas: &Canvas,
        style: &DrawStyle,
        mask: &LayerMask,
    ) -> Result<(), LibraryError> {
        match style {
            DrawStyle::Fill { .. } | DrawStyle::Stroke { .. } | DrawStyle::Opacity { .. } => {
                Err(LibraryError::Render(
                    "Fill, Stroke, and Image Opacity are handled outside alpha-mask layer styles"
                        .to_string(),
                ))
            }
            DrawStyle::ColorOverlay {
                color,
                opacity,
                blend_mode,
            } => {
                let filter =
                    mask.solid_tint(self.surface_contract, mask.source(), color, *opacity as f32)?;
                self.composited(canvas, mask, *blend_mode, filter)
            }
            DrawStyle::GradientOverlay {
                gradient,
                opacity,
                blend_mode,
            } => {
                let shader = super::paint_shader::PaintShaderFactory::new(
                    self.surface_contract,
                    self.render_scale,
                )
                .gradient(gradient, mask.style_bounds(), *opacity as f32)?;
                let filter = mask.shader_tint(mask.source(), shader)?;
                self.composited(canvas, mask, *blend_mode, filter)
            }
            DrawStyle::PatternOverlay {
                pattern,
                opacity,
                blend_mode,
            } => {
                let shader = super::paint_shader::PaintShaderFactory::new(
                    self.surface_contract,
                    self.render_scale,
                )
                .pattern(pattern, *opacity as f32)?;
                let filter = mask.shader_tint(mask.source(), shader)?;
                self.composited(canvas, mask, *blend_mode, filter)
            }
            DrawStyle::DropShadow {
                color,
                opacity,
                blend_mode,
                angle,
                distance,
                spread,
                size,
            } => {
                let filter = mask.expanded_blur(self.scaled(*size), *spread)?;
                let filter = mask.offset(filter, shadow_offset(*angle, self.scaled(*distance)))?;
                let filter =
                    mask.solid_tint(self.surface_contract, filter, color, *opacity as f32)?;
                self.composited(canvas, mask, *blend_mode, filter)
            }
            DrawStyle::OuterGlow {
                color,
                opacity,
                blend_mode,
                spread,
                size,
            } => {
                let filter = mask.outside(mask.expanded_blur(self.scaled(*size), *spread)?)?;
                let filter =
                    mask.solid_tint(self.surface_contract, filter, color, *opacity as f32)?;
                self.composited(canvas, mask, *blend_mode, filter)
            }
            DrawStyle::InnerShadow {
                color,
                opacity,
                blend_mode,
                angle,
                distance,
                spread,
                size,
            } => self.edge(
                canvas,
                mask,
                EdgeSpec {
                    color,
                    opacity: *opacity as f32,
                    blend_mode: *blend_mode,
                    offset: shadow_offset(*angle, self.scaled(*distance)),
                    size: self.scaled(*size),
                    spread: *spread,
                    inside: true,
                },
            ),
            DrawStyle::InnerGlow {
                color,
                opacity,
                blend_mode,
                spread,
                size,
            } => self.edge(
                canvas,
                mask,
                EdgeSpec {
                    color,
                    opacity: *opacity as f32,
                    blend_mode: *blend_mode,
                    offset: (0.0, 0.0),
                    size: self.scaled(*size),
                    spread: *spread,
                    inside: true,
                },
            ),
            DrawStyle::Satin {
                color,
                opacity,
                blend_mode,
                angle,
                distance,
                size,
                invert,
            } => {
                let mut offset = shadow_offset(*angle, self.scaled(*distance));
                if *invert {
                    offset = (-offset.0, -offset.1);
                }
                let first = mask.offset(mask.expanded_blur(self.scaled(*size), 0.0)?, offset)?;
                let second = mask.offset(
                    mask.expanded_blur(self.scaled(*size), 0.0)?,
                    (-offset.0, -offset.1),
                )?;
                let filter = mask.subtract(mask.source(), first)?;
                let filter = mask.subtract(filter, second)?;
                let filter =
                    mask.solid_tint(self.surface_contract, filter, color, *opacity as f32)?;
                self.composited(canvas, mask, *blend_mode, filter)
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
            } => self.bevel(
                canvas,
                mask,
                BevelSpec {
                    style: *style,
                    technique: *technique,
                    depth: *depth,
                    direction: *direction,
                    size: self.scaled(*size),
                    soften: self.scaled(*soften),
                    angle: *angle,
                    altitude: *altitude,
                    highlight_color,
                    highlight_opacity: *highlight_opacity,
                    highlight_blend_mode: *highlight_blend_mode,
                    shadow_color,
                    shadow_opacity: *shadow_opacity,
                    shadow_blend_mode: *shadow_blend_mode,
                },
            ),
        }
    }

    fn scaled(&self, value: f64) -> f64 {
        value * self.render_scale
    }

    fn edge(
        &mut self,
        canvas: &Canvas,
        mask: &LayerMask,
        spec: EdgeSpec<'_>,
    ) -> Result<(), LibraryError> {
        let filtered = if spec.inside {
            mask.eroded_blur(spec.size, spec.spread)?
        } else {
            mask.expanded_blur(spec.size, spec.spread)?
        };
        let shifted = mask.offset(filtered, spec.offset)?;
        let edge = if spec.inside {
            mask.subtract(mask.source(), shifted)?
        } else {
            mask.outside(shifted)?
        };
        let edge = mask.solid_tint(self.surface_contract, edge, spec.color, spec.opacity)?;
        self.composited(canvas, mask, spec.blend_mode, edge)
    }

    fn bevel(
        &mut self,
        canvas: &Canvas,
        mask: &LayerMask,
        spec: BevelSpec<'_>,
    ) -> Result<(), LibraryError> {
        let strength = spec.altitude.to_radians().sin().abs() as f32;
        let geometry = BevelRenderGeometry::new(
            spec.style,
            spec.technique,
            spec.depth,
            spec.size,
            spec.soften,
        );
        let shadow = shadow_offset(spec.angle, geometry.offset_distance);
        let (highlight_offset, shadow_offset) = match spec.direction {
            BevelDirection::Up => ((-shadow.0, -shadow.1), shadow),
            BevelDirection::Down => (shadow, (-shadow.0, -shadow.1)),
        };
        for edge in [
            EdgeSpec {
                color: spec.highlight_color,
                opacity: spec.highlight_opacity as f32 * strength,
                blend_mode: spec.highlight_blend_mode,
                offset: highlight_offset,
                size: geometry.edge_size,
                spread: geometry.edge_spread,
                inside: geometry.inside,
            },
            EdgeSpec {
                color: spec.shadow_color,
                opacity: spec.shadow_opacity as f32 * strength,
                blend_mode: spec.shadow_blend_mode,
                offset: shadow_offset,
                size: geometry.edge_size,
                spread: geometry.edge_spread,
                inside: geometry.inside,
            },
        ] {
            self.edge(canvas, mask, edge)?;
        }
        Ok(())
    }

    fn composited(
        &mut self,
        canvas: &Canvas,
        mask: &LayerMask,
        blend_mode: BlendMode,
        filter: skia_safe::ImageFilter,
    ) -> Result<(), LibraryError> {
        let mut composite = Paint::default();
        self.blend_runtime
            .configure_paint(&mut composite, blend_mode)?;
        let bounds = mask.visual_bounds();
        canvas.save_layer(&SaveLayerRec::default().bounds(&bounds).paint(&composite));
        mask.draw_filter(canvas, filter);
        canvas.restore();
        Ok(())
    }
}

struct BevelSpec<'a> {
    style: BevelStyle,
    technique: BevelTechnique,
    depth: f64,
    direction: BevelDirection,
    size: f64,
    soften: f64,
    angle: f64,
    altitude: f64,
    highlight_color: &'a Color,
    highlight_opacity: f64,
    highlight_blend_mode: BlendMode,
    shadow_color: &'a Color,
    shadow_opacity: f64,
    shadow_blend_mode: BlendMode,
}

fn shadow_offset(angle_degrees: f64, distance: f64) -> (f32, f32) {
    let angle = angle_degrees.to_radians();
    (
        (-angle.cos() * distance) as f32,
        (angle.sin() * distance) as f32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_direction_uses_canvas_downward_y_axis() {
        let conventional = shadow_offset(120.0, 10.0);
        assert!(conventional.0 > 0.0);
        assert!(conventional.1 > 0.0);
    }
}
