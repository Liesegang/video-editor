//! Authored-color conversion and path/text paint construction for Skia.

use log::trace;
use skia_safe::path_effect::PathEffect as SkPathEffect;
use skia_safe::trim_path_effect::Mode;
use skia_safe::{Canvas, Paint, PaintStyle};

use crate::error::LibraryError;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{CapType, DrawStyle, JoinType, PathEffect};
use crate::rendering::skia_working_surface::{self, SkiaSurfaceContract};

pub(super) struct StrokeRenderConfig<'a> {
    pub(super) color: &'a Color,
    pub(super) width: f64,
    pub(super) offset: f64,
    pub(super) cap: &'a CapType,
    pub(super) join: &'a JoinType,
    pub(super) miter: f64,
    pub(super) dash_array: &'a [f64],
    pub(super) dash_offset: f64,
}

pub(super) struct PaintFactory<'a> {
    surface_contract: &'a SkiaSurfaceContract,
}

impl<'a> PaintFactory<'a> {
    pub(super) const fn new(surface_contract: &'a SkiaSurfaceContract) -> Self {
        Self { surface_contract }
    }

    fn stroke_paint(
        &self,
        color: &Color,
        width: f32,
        cap: &CapType,
        join: &JoinType,
        miter: f32,
    ) -> Result<Paint, LibraryError> {
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        skia_working_surface::set_paint_authored_color(
            &mut paint,
            self.surface_contract,
            color,
            1.0,
        )?;
        paint.set_style(PaintStyle::Stroke);
        paint.set_stroke_width(width);
        paint.set_stroke_cap(match cap {
            CapType::Round => skia_safe::paint::Cap::Round,
            CapType::Square => skia_safe::paint::Cap::Square,
            CapType::Butt => skia_safe::paint::Cap::Butt,
        });
        paint.set_stroke_join(match join {
            JoinType::Round => skia_safe::paint::Join::Round,
            JoinType::Bevel => skia_safe::paint::Join::Bevel,
            JoinType::Miter => skia_safe::paint::Join::Miter,
        });
        paint.set_stroke_miter(miter);
        Ok(paint)
    }

    pub(super) fn text_paint(
        &self,
        style: &DrawStyle,
        opacity: f32,
        color_override: Option<&Color>,
    ) -> Result<Paint, LibraryError> {
        match style {
            DrawStyle::Fill { color, offset } => {
                let color = color_override.unwrap_or(color);
                let mut paint = Paint::default();
                skia_working_surface::set_paint_authored_color(
                    &mut paint,
                    self.surface_contract,
                    color,
                    opacity,
                )?;
                if *offset > 0.0 {
                    paint.set_style(PaintStyle::StrokeAndFill);
                    paint.set_stroke_width((*offset * 2.0) as f32);
                    paint.set_stroke_join(skia_safe::paint::Join::Round);
                } else {
                    paint.set_style(PaintStyle::Fill);
                }
                paint.set_anti_alias(true);
                Ok(paint)
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
                let color = color_override.unwrap_or(color);
                let effective_width = (width + offset * 2.0).max(0.0);
                let mut paint =
                    self.stroke_paint(color, effective_width as f32, cap, join, *miter as f32)?;
                paint.set_alpha_f(paint.alpha_f() * opacity.clamp(0.0, 1.0));
                if !dash_array.is_empty() {
                    let intervals = dash_array
                        .iter()
                        .map(|value| *value as f32)
                        .collect::<Vec<_>>();
                    if let Some(effect) = SkPathEffect::dash(&intervals, *dash_offset as f32) {
                        paint.set_path_effect(effect);
                    }
                }
                Ok(paint)
            }
        }
    }

    pub(super) fn draw_shape_fill(
        &self,
        canvas: &Canvas,
        path: &skia_safe::Path,
        color: &Color,
        path_effects: &[PathEffect],
        offset: f64,
    ) -> Result<(), LibraryError> {
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        skia_working_surface::set_paint_authored_color(
            &mut paint,
            self.surface_contract,
            color,
            1.0,
        )?;
        apply_path_effects(path_effects, &mut paint)?;

        if offset >= 0.0 {
            if offset > 0.0 {
                paint.set_style(PaintStyle::StrokeAndFill);
                paint.set_stroke_width((offset * 2.0) as f32);
                paint.set_stroke_join(skia_safe::paint::Join::Round);
            } else {
                paint.set_style(PaintStyle::Fill);
            }
            canvas.draw_path(path, &paint);
        } else {
            paint.set_style(PaintStyle::Fill);
            canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default());
            canvas.draw_path(path, &paint);

            let mut erase_paint = Paint::default();
            erase_paint.set_anti_alias(true);
            erase_paint.set_style(PaintStyle::Stroke);
            erase_paint.set_stroke_width((-offset * 2.0) as f32);
            erase_paint.set_stroke_join(skia_safe::paint::Join::Round);
            erase_paint.set_blend_mode(skia_safe::BlendMode::DstOut);
            apply_path_effects(path_effects, &mut erase_paint)?;
            canvas.draw_path(path, &erase_paint);
            canvas.restore();
        }
        Ok(())
    }

    pub(super) fn draw_shape_stroke(
        &self,
        canvas: &Canvas,
        path: &skia_safe::Path,
        path_effects: &[PathEffect],
        config: StrokeRenderConfig<'_>,
    ) -> Result<(), LibraryError> {
        let StrokeRenderConfig {
            color,
            width,
            offset,
            cap,
            join,
            miter,
            dash_array,
            dash_offset,
        } = config;
        if width <= 0.0 {
            return Ok(());
        }

        let mut stroke_paint = self.stroke_paint(color, width as f32, cap, join, miter as f32)?;
        let mut effects_to_apply = path_effects.to_vec();
        if !dash_array.is_empty() {
            effects_to_apply.push(PathEffect::Dash {
                intervals: dash_array.to_vec(),
                phase: dash_offset,
            });
        }
        if offset == 0.0 {
            stroke_paint.set_style(PaintStyle::Stroke);
            stroke_paint.set_stroke_width(width as f32);
            apply_path_effects(&effects_to_apply, &mut stroke_paint)?;
            canvas.draw_path(path, &stroke_paint);
            return Ok(());
        }

        let outer_radius = offset.abs() + width / 2.0;
        let inner_radius = offset.abs() - width / 2.0;
        canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default());
        if offset > 0.0 {
            canvas.clip_path(path, skia_safe::ClipOp::Difference, true);
        } else {
            canvas.clip_path(path, skia_safe::ClipOp::Intersect, true);
        }
        apply_path_effects(&effects_to_apply, &mut stroke_paint)?;
        stroke_paint.set_style(PaintStyle::Stroke);
        stroke_paint.set_stroke_width((outer_radius * 2.0) as f32);
        canvas.draw_path(path, &stroke_paint);
        if inner_radius > 0.0 {
            let mut erase_paint = stroke_paint.clone();
            erase_paint.set_blend_mode(skia_safe::BlendMode::DstOut);
            erase_paint.set_stroke_width((inner_radius * 2.0) as f32);
            canvas.draw_path(path, &erase_paint);
        }
        canvas.restore();
        Ok(())
    }
}

fn convert_path_effect(path_effect: &PathEffect) -> Result<skia_safe::PathEffect, LibraryError> {
    match path_effect {
        PathEffect::Dash { intervals, phase } => {
            let intervals = intervals
                .iter()
                .map(|value| *value as f32)
                .collect::<Vec<_>>();
            SkPathEffect::dash(&intervals, *phase as f32)
                .ok_or_else(|| LibraryError::Render("failed to create dash PathEffect".to_string()))
        }
        PathEffect::Corner { radius } => SkPathEffect::corner_path(*radius as f32)
            .ok_or_else(|| LibraryError::Render("failed to create corner PathEffect".to_string())),
        PathEffect::Discrete {
            seg_length,
            deviation,
            seed,
        } => SkPathEffect::discrete(*seg_length as f32, *deviation as f32, *seed as u32)
            .ok_or_else(|| {
                LibraryError::Render("failed to create discrete PathEffect".to_string())
            }),
        PathEffect::Trim { start, end } => {
            SkPathEffect::trim(*start as f32, *end as f32, Mode::Normal)
                .ok_or_else(|| LibraryError::Render("failed to create trim PathEffect".to_string()))
        }
    }
}

fn apply_path_effects(path_effects: &[PathEffect], paint: &mut Paint) -> Result<(), LibraryError> {
    let mut composed_effect: Option<skia_safe::PathEffect> = None;
    for effect in path_effects {
        trace!("Applying path effect {effect:?}");
        let next = convert_path_effect(effect)?;
        composed_effect = Some(match composed_effect {
            // compose(outer, inner) evaluates graph upstream first.
            Some(upstream) => SkPathEffect::compose(next, upstream),
            None => next,
        });
    }
    if let Some(composed) = composed_effect {
        paint.set_path_effect(composed);
    }
    Ok(())
}
