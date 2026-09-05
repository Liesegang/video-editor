//! Shared, working-linear layer-style composition for Shape and Text.

mod mask;

use skia_safe::canvas::SaveLayerRec;
use skia_safe::{
    Canvas, FilterMode, Matrix, Paint, PictureRecorder, Rect, Shader, TileMode, gradient_shader,
};

use crate::error::LibraryError;
use crate::model::BlendMode;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{
    BevelDirection, BevelStyle, BevelTechnique, DrawStyle, GradientStyle, PatternStyle,
};
use crate::model::frame::entity::StyleConfig;
use crate::model::property::{GradientGeometry, GradientSpread, PatternKind};
use crate::rendering::blend::BlendRuntime;
use crate::rendering::skia_working_surface::{self, SkiaSurfaceContract};

pub(super) use mask::LayerMask;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CompositePhase {
    Underlay,
    Overlay,
}

fn composite_phase(style: &DrawStyle) -> Option<CompositePhase> {
    match style {
        DrawStyle::DropShadow { .. } | DrawStyle::OuterGlow { .. } => {
            Some(CompositePhase::Underlay)
        }
        DrawStyle::Fill { .. } | DrawStyle::Stroke { .. } => None,
        DrawStyle::ColorOverlay { .. }
        | DrawStyle::GradientOverlay { .. }
        | DrawStyle::PatternOverlay { .. }
        | DrawStyle::InnerShadow { .. }
        | DrawStyle::InnerGlow { .. }
        | DrawStyle::Satin { .. }
        | DrawStyle::BevelEmboss { .. } => Some(CompositePhase::Overlay),
    }
}

pub(super) fn has_mask_styles(styles: &[StyleConfig]) -> bool {
    styles
        .iter()
        .any(|config| composite_phase(&config.style).is_some())
}

pub(super) fn visit_phase(
    styles: &[StyleConfig],
    phase: CompositePhase,
    mut visit: impl FnMut(&StyleConfig) -> Result<(), LibraryError>,
) -> Result<(), LibraryError> {
    for config in styles {
        if composite_phase(&config.style) == Some(phase) {
            visit(config)?;
        }
    }
    Ok(())
}

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
}

impl<'a> LayerStyleRenderer<'a> {
    pub(super) const fn new(
        surface_contract: &'a SkiaSurfaceContract,
        blend_runtime: &'a mut BlendRuntime,
    ) -> Self {
        Self {
            surface_contract,
            blend_runtime,
        }
    }

    pub(super) fn draw(
        &mut self,
        canvas: &Canvas,
        style: &DrawStyle,
        mask: &LayerMask,
    ) -> Result<(), LibraryError> {
        match style {
            DrawStyle::Fill { .. } | DrawStyle::Stroke { .. } => Err(LibraryError::Render(
                "Fill and Stroke belong to the vector layer body".to_string(),
            )),
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
                let shader = self.gradient_shader(mask, gradient, *opacity as f32)?;
                let filter = mask.shader_tint(mask.source(), shader)?;
                self.composited(canvas, mask, *blend_mode, filter)
            }
            DrawStyle::PatternOverlay {
                pattern,
                opacity,
                blend_mode,
            } => {
                let shader = self.pattern_shader(pattern, *opacity as f32)?;
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
                let filter = mask.expanded_blur(*size, *spread)?;
                let filter = mask.offset(filter, shadow_offset(*angle, *distance))?;
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
                let filter = mask.outside(mask.expanded_blur(*size, *spread)?)?;
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
                    offset: shadow_offset(*angle, *distance),
                    size: *size,
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
                    size: *size,
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
                let mut offset = shadow_offset(*angle, *distance);
                if *invert {
                    offset = (-offset.0, -offset.1);
                }
                let first = mask.offset(mask.expanded_blur(*size, 0.0)?, offset)?;
                let second =
                    mask.offset(mask.expanded_blur(*size, 0.0)?, (-offset.0, -offset.1))?;
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
                    size: *size,
                    soften: *soften,
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
        let distance = spec.size.max(0.0) * spec.depth.max(0.0);
        let shadow = shadow_offset(spec.angle, distance);
        let (highlight_offset, shadow_offset) = match spec.direction {
            BevelDirection::Up => ((-shadow.0, -shadow.1), shadow),
            BevelDirection::Down => (shadow, (-shadow.0, -shadow.1)),
        };
        let blur = match spec.technique {
            BevelTechnique::Smooth => spec.soften.max(spec.size * 0.25),
            BevelTechnique::ChiselSoft => spec.soften.max(spec.size * 0.08),
            BevelTechnique::ChiselHard => spec.soften.max(0.01),
        };
        let spread = match spec.technique {
            BevelTechnique::Smooth => 0.0,
            BevelTechnique::ChiselSoft => 0.5,
            BevelTechnique::ChiselHard => 1.0,
        };
        let inside = matches!(
            spec.style,
            BevelStyle::InnerBevel | BevelStyle::PillowEmboss
        );
        for edge in [
            EdgeSpec {
                color: spec.highlight_color,
                opacity: spec.highlight_opacity as f32 * strength,
                blend_mode: spec.highlight_blend_mode,
                offset: highlight_offset,
                size: blur,
                spread,
                inside,
            },
            EdgeSpec {
                color: spec.shadow_color,
                opacity: spec.shadow_opacity as f32 * strength,
                blend_mode: spec.shadow_blend_mode,
                offset: shadow_offset,
                size: blur,
                spread,
                inside,
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
        canvas.save_layer(&SaveLayerRec::default().paint(&composite));
        mask.draw_filter(canvas, filter);
        canvas.restore();
        Ok(())
    }

    fn gradient_shader(
        &self,
        mask: &LayerMask,
        gradient: &GradientStyle,
        opacity: f32,
    ) -> Result<Shader, LibraryError> {
        let mut colors = Vec::with_capacity(gradient.stops.len());
        let mut color_space = None;
        for stop in &gradient.stops {
            let (color, stop_space) = skia_working_surface::authored_color4f(
                self.surface_contract,
                &stop.color,
                opacity.clamp(0.0, 1.0),
            )?;
            colors.push(color);
            color_space = color_space.or(stop_space);
        }
        let positions = gradient
            .stops
            .iter()
            .map(|stop| stop.offset.into_inner() as f32)
            .collect::<Vec<_>>();
        let bounds = mask.style_bounds();
        let point = |value: crate::model::property::Vec2| {
            (
                bounds.left + value.x.into_inner() as f32 * bounds.width(),
                bounds.top + value.y.into_inner() as f32 * bounds.height(),
            )
        };
        let tile = match gradient.spread {
            GradientSpread::Pad => TileMode::Clamp,
            GradientSpread::Repeat => TileMode::Repeat,
            GradientSpread::Reflect => TileMode::Mirror,
        };
        let interpolation = gradient_shader::Interpolation::from(gradient_shader::Flags::default());
        match gradient.geometry {
            GradientGeometry::Linear { start, end } => gradient_shader::linear_with_interpolation(
                (point(start), point(end)),
                (&colors, color_space),
                Some(positions.as_slice()),
                tile,
                interpolation,
                None,
            ),
            GradientGeometry::Radial { center, radius } => {
                gradient_shader::radial_with_interpolation(
                    (
                        point(center),
                        radius.into_inner() as f32 * bounds.width().min(bounds.height()),
                    ),
                    (&colors, color_space),
                    Some(positions.as_slice()),
                    tile,
                    interpolation,
                    None,
                )
            }
        }
        .ok_or_else(|| LibraryError::Render("Cannot create Gradient Overlay shader".to_string()))
    }

    fn pattern_shader(&self, pattern: &PatternStyle, opacity: f32) -> Result<Shader, LibraryError> {
        let width = pattern.scale.x.into_inner() as f32;
        let height = pattern.scale.y.into_inner() as f32;
        let bounds = Rect::from_wh(width, height);
        let mut recorder = PictureRecorder::new();
        let tile = recorder.begin_recording(bounds, false);
        let background = solid_paint(self.surface_contract, &pattern.background, opacity)?;
        tile.draw_rect(bounds, &background);
        let foreground = solid_paint(self.surface_contract, &pattern.foreground, opacity)?;
        let duty = pattern.duty.into_inner() as f32;
        match pattern.kind {
            PatternKind::Checker => {
                let x = width * duty;
                let y = height * duty;
                tile.draw_rect(Rect::from_xywh(0.0, 0.0, x, y), &foreground);
                tile.draw_rect(Rect::from_xywh(x, y, width - x, height - y), &foreground);
            }
            PatternKind::Stripes => {
                tile.draw_rect(Rect::from_xywh(0.0, 0.0, width * duty, height), &foreground);
            }
            PatternKind::Dots => {
                tile.draw_circle(
                    (width / 2.0, height / 2.0),
                    width.min(height) * duty / 2.0,
                    &foreground,
                );
            }
            PatternKind::Grid => {
                tile.draw_rect(Rect::from_xywh(0.0, 0.0, width * duty, height), &foreground);
                tile.draw_rect(Rect::from_xywh(0.0, 0.0, width, height * duty), &foreground);
            }
        }
        let picture = recorder
            .finish_recording_as_picture(Some(&bounds))
            .ok_or_else(|| LibraryError::Render("Cannot record Pattern Overlay".to_string()))?;
        let mut matrix = Matrix::translate((
            pattern.phase.x.into_inner() as f32,
            pattern.phase.y.into_inner() as f32,
        ));
        matrix.pre_rotate(pattern.angle.into_inner() as f32, None);
        Ok(picture.to_shader(
            Some((TileMode::Repeat, TileMode::Repeat)),
            FilterMode::Nearest,
            Some(&matrix),
            Some(&bounds),
        ))
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

fn solid_paint(
    contract: &SkiaSurfaceContract,
    color: &Color,
    opacity: f32,
) -> Result<Paint, LibraryError> {
    let mut paint = Paint::default();
    skia_working_surface::set_paint_authored_color(&mut paint, contract, color, opacity)?;
    Ok(paint)
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
