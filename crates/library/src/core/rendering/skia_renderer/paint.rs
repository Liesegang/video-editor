//! Authored-color conversion and path/text paint construction for Skia.

use log::trace;
use skia_safe::path_effect::PathEffect as SkPathEffect;
use skia_safe::trim_path_effect::Mode;
use skia_safe::{Canvas, Paint, PaintStyle, Path, PathMeasure, StrokeRec};

use crate::error::LibraryError;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{CapType, DrawStyle, JoinType, PathEffect, TrimPathUnits};
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
        apply_path_effects(path_effects, path, &mut paint)?;

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
            apply_path_effects(path_effects, path, &mut erase_paint)?;
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
            apply_path_effects(&effects_to_apply, path, &mut stroke_paint)?;
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
        apply_path_effects(&effects_to_apply, path, &mut stroke_paint)?;
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

#[derive(Debug, Clone, Copy, PartialEq)]
enum ResolvedTrim {
    Full,
    Empty { at: f32 },
    Segment { start: f32, end: f32, wraps: bool },
}

fn resolve_trim(
    start: f64,
    end: f64,
    offset: f64,
    period: f64,
) -> Result<ResolvedTrim, LibraryError> {
    if !start.is_finite() || !end.is_finite() || !offset.is_finite() {
        return Err(LibraryError::Render(
            "Trim Path values must be finite".to_string(),
        ));
    }
    if !period.is_finite() || period < 0.0 {
        return Err(LibraryError::Render(
            "Trim Path period must be finite and non-negative".to_string(),
        ));
    }
    // A zero-length geometry has no observable phase or segment. Treating the
    // Trim as an identity avoids division by zero while preserving the empty
    // input geometry.
    if period == 0.0 {
        return Ok(ResolvedTrim::Full);
    }

    // Preserve the authored span before normalizing endpoints. In particular,
    // 0 -> 1 is one full traversal while 0 -> 0 is empty, even though both
    // pairs have equal endpoints after modulo reduction.
    let raw_span = end - start;
    if raw_span.abs() >= period {
        return Ok(ResolvedTrim::Full);
    }

    let shifted_start = (start.rem_euclid(period) + offset.rem_euclid(period)).rem_euclid(period);
    let normalized_start = shifted_start / period;
    if raw_span == 0.0 {
        return Ok(ResolvedTrim::Empty {
            at: normalized_start as f32,
        });
    }

    // Trim always follows the path's forward direction. A negative authored
    // delta therefore wraps naturally instead of changing traversal direction.
    let normalized_span = raw_span.rem_euclid(period) / period;
    let unwrapped_end = normalized_start + normalized_span;
    if unwrapped_end <= 1.0 {
        Ok(ResolvedTrim::Segment {
            start: normalized_start as f32,
            end: unwrapped_end as f32,
            wraps: false,
        })
    } else {
        Ok(ResolvedTrim::Segment {
            start: normalized_start as f32,
            end: (unwrapped_end - 1.0) as f32,
            wraps: true,
        })
    }
}

fn total_path_length(path: &Path) -> Result<f64, LibraryError> {
    let mut measure = PathMeasure::new(path, false, None);
    let mut total = 0.0_f64;
    loop {
        total += f64::from(measure.length());
        if !total.is_finite() {
            return Err(LibraryError::Render(
                "Trim Path geometry has a non-finite cumulative length".to_string(),
            ));
        }
        if !measure.next_contour() {
            break;
        }
    }
    Ok(total)
}

fn path_after_effect(
    source: &Path,
    effect: &skia_safe::PathEffect,
    paint: &Paint,
) -> Result<Path, LibraryError> {
    let stroke = StrokeRec::from_paint(paint, None, None);
    let (mut builder, _) = effect
        .filter_path(source, &stroke, source.bounds())
        .ok_or_else(|| {
            LibraryError::Render(
                "failed to evaluate upstream Path Effects for Trim Length mode".to_string(),
            )
        })?;
    Ok(builder.detach())
}

fn trim_path_effect(resolved: ResolvedTrim) -> Result<Option<skia_safe::PathEffect>, LibraryError> {
    let effect = match resolved {
        // Skia intentionally returns nullptr for the 0 -> 1 identity. This is
        // not an error and must not be confused with the empty 0 -> 0 span.
        ResolvedTrim::Full => return Ok(None),
        ResolvedTrim::Empty { at } => SkPathEffect::trim(at, at, Mode::Normal),
        ResolvedTrim::Segment {
            start,
            end,
            wraps: false,
        } => SkPathEffect::trim(start, end, Mode::Normal),
        ResolvedTrim::Segment {
            start,
            end,
            wraps: true,
        } => {
            // Inverted(end, start) yields [start, 1] followed by [0, end].
            SkPathEffect::trim(end, start, Mode::Inverted)
        }
    };
    effect.map(Some).ok_or_else(|| {
        LibraryError::Render("failed to create non-identity trim PathEffect".to_string())
    })
}

fn convert_path_effect(
    path_effect: &PathEffect,
    source_path: &Path,
    upstream: Option<&skia_safe::PathEffect>,
    paint: &Paint,
) -> Result<Option<skia_safe::PathEffect>, LibraryError> {
    match path_effect {
        PathEffect::Dash { intervals, phase } => {
            let intervals = intervals
                .iter()
                .map(|value| *value as f32)
                .collect::<Vec<_>>();
            SkPathEffect::dash(&intervals, *phase as f32)
                .map(Some)
                .ok_or_else(|| LibraryError::Render("failed to create dash PathEffect".to_string()))
        }
        PathEffect::Corner { radius } => SkPathEffect::corner_path(*radius as f32)
            .map(Some)
            .ok_or_else(|| LibraryError::Render("failed to create corner PathEffect".to_string())),
        PathEffect::Discrete {
            seg_length,
            deviation,
            seed,
        } => SkPathEffect::discrete(*seg_length as f32, *deviation as f32, *seed as u32)
            .map(Some)
            .ok_or_else(|| {
                LibraryError::Render("failed to create discrete PathEffect".to_string())
            }),
        PathEffect::Trim {
            start,
            end,
            offset,
            units,
        } => {
            let period = match units {
                TrimPathUnits::Normalized => 1.0,
                TrimPathUnits::Length => {
                    let measured_path = match upstream {
                        Some(effect) => path_after_effect(source_path, effect, paint)?,
                        None => source_path.clone(),
                    };
                    total_path_length(&measured_path)?
                }
            };
            trim_path_effect(resolve_trim(*start, *end, *offset, period)?)
        }
    }
}

fn apply_path_effects(
    path_effects: &[PathEffect],
    path: &Path,
    paint: &mut Paint,
) -> Result<(), LibraryError> {
    let mut composed_effect: Option<skia_safe::PathEffect> = None;
    for effect in path_effects {
        trace!("Applying path effect {effect:?}");
        let Some(next) = convert_path_effect(effect, path, composed_effect.as_ref(), paint)? else {
            continue;
        };
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

#[cfg(test)]
mod tests {
    use super::*;
    use skia_safe::PathBuilder;

    fn assert_near(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1.0e-5,
            "expected {expected}, got {actual}"
        );
    }

    fn assert_segment(
        resolved: ResolvedTrim,
        expected_start: f32,
        expected_end: f32,
        expected_wraps: bool,
    ) {
        let ResolvedTrim::Segment { start, end, wraps } = resolved else {
            panic!("expected Segment, got {resolved:?}");
        };
        assert_near(start, expected_start);
        assert_near(end, expected_end);
        assert_eq!(wraps, expected_wraps);
    }

    fn two_contour_path() -> Path {
        let mut builder = PathBuilder::new();
        builder.move_to((0.0, 0.0)).line_to((10.0, 0.0));
        builder.move_to((20.0, 0.0)).line_to((40.0, 0.0));
        builder.detach()
    }

    #[test]
    fn normalized_trim_preserves_full_empty_and_authored_laps() {
        assert_eq!(
            resolve_trim(0.0, 1.0, 0.0, 1.0).unwrap(),
            ResolvedTrim::Full
        );
        assert_eq!(
            resolve_trim(0.3, 1.5, 0.0, 1.0).unwrap(),
            ResolvedTrim::Full
        );
        assert!(matches!(
            resolve_trim(0.0, 0.0, 0.0, 1.0).unwrap(),
            ResolvedTrim::Empty { at } if at == 0.0
        ));

        let base = resolve_trim(0.3, 0.5, 0.0, 1.0).unwrap();
        assert_segment(base, 0.3, 0.5, false);
        assert_eq!(resolve_trim(1.3, 1.5, 0.0, 1.0).unwrap(), base);
    }

    #[test]
    fn normalized_trim_wraps_negative_values_and_offset_without_changing_length() {
        assert_segment(resolve_trim(0.8, 0.2, 0.0, 1.0).unwrap(), 0.8, 0.2, true);
        assert_segment(resolve_trim(-0.2, 0.2, 0.0, 1.0).unwrap(), 0.8, 0.2, true);
        assert_segment(resolve_trim(0.3, 0.5, 0.4, 1.0).unwrap(), 0.7, 0.9, false);
        assert_eq!(
            resolve_trim(0.3, 0.5, 1.5, 1.0).unwrap(),
            resolve_trim(0.3, 0.5, 0.5, 1.0).unwrap()
        );
    }

    #[test]
    fn length_trim_uses_cumulative_multi_contour_geometry_length() {
        let path = two_contour_path();
        let length = total_path_length(&path).unwrap();
        assert!((length - 30.0).abs() < 1.0e-5, "length={length}");

        assert_segment(
            resolve_trim(0.0, 10.0, 0.0, length).unwrap(),
            0.0,
            1.0 / 3.0,
            false,
        );
        assert_segment(
            resolve_trim(28.0, 2.0, 0.0, length).unwrap(),
            28.0 / 30.0,
            2.0 / 30.0,
            true,
        );
        assert_eq!(
            resolve_trim(5.0, 35.0, 0.0, length).unwrap(),
            ResolvedTrim::Full
        );
        assert!(matches!(
            resolve_trim(10.0, 10.0, 0.0, length).unwrap(),
            ResolvedTrim::Empty { .. }
        ));
        assert_eq!(
            resolve_trim(0.0, 10.0, 35.0, length).unwrap(),
            resolve_trim(0.0, 10.0, 5.0, length).unwrap()
        );
    }

    #[test]
    fn length_trim_measures_geometry_after_upstream_path_effects() {
        let source = two_contour_path();
        let upstream = trim_path_effect(resolve_trim(0.0, 0.5, 0.0, 1.0).unwrap())
            .unwrap()
            .expect("half-path Trim must create an effect");
        let measured = path_after_effect(&source, &upstream, &Paint::default()).unwrap();
        let length = total_path_length(&measured).unwrap();
        assert!((length - 15.0).abs() < 1.0e-5, "length={length}");
        assert_segment(
            resolve_trim(0.0, 5.0, 0.0, length).unwrap(),
            0.0,
            1.0 / 3.0,
            false,
        );
    }

    #[test]
    fn apply_path_effects_accepts_full_default_and_length_mode_cuts_ten_pixels() {
        let source = two_contour_path();
        let full = PathEffect::Trim {
            start: 0.0,
            end: 1.0,
            offset: 0.0,
            units: TrimPathUnits::Normalized,
        };
        let mut full_paint = Paint::default();
        apply_path_effects(&[full], &source, &mut full_paint).unwrap();
        assert!(
            full_paint.path_effect().is_none(),
            "full traversal is an identity, not a renderer error"
        );

        let ten_pixels = PathEffect::Trim {
            start: 0.0,
            end: 10.0,
            offset: 0.0,
            units: TrimPathUnits::Length,
        };
        let mut length_paint = Paint::default();
        apply_path_effects(&[ten_pixels], &source, &mut length_paint).unwrap();
        let effect = length_paint
            .path_effect()
            .expect("partial Length trim must attach a path effect");
        let stroke = StrokeRec::from_paint(&length_paint, None, None);
        let (mut builder, _) = effect
            .filter_path(&source, &stroke, source.bounds())
            .expect("Length trim must filter source geometry");
        let trimmed = builder.detach();
        let length = total_path_length(&trimmed).unwrap();
        assert!((length - 10.0).abs() < 1.0e-5, "length={length}");
    }

    #[test]
    fn zero_length_geometry_and_empty_span_are_safe_and_distinct_from_full() {
        let empty_path = Path::new();
        assert_eq!(total_path_length(&empty_path).unwrap(), 0.0);
        assert_eq!(
            resolve_trim(0.0, 10.0, -5.0, 0.0).unwrap(),
            ResolvedTrim::Full
        );
        assert!(trim_path_effect(ResolvedTrim::Full).unwrap().is_none());

        let source = two_contour_path();
        let effect = trim_path_effect(ResolvedTrim::Empty { at: 0.0 })
            .unwrap()
            .expect("empty span must remain an explicit path effect");
        let stroke = StrokeRec::new_fill();
        let (mut builder, _) = effect
            .filter_path(&source, &stroke, source.bounds())
            .expect("Skia must evaluate an empty Trim span");
        assert!(builder.detach().is_empty());
    }
}
