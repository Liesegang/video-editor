use super::paint_utils::{apply_path_effects, create_stroke_paint};
use crate::error::LibraryError;
use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{CapType, JoinType, PathEffect};
use skia_safe::{Canvas, Paint, PaintStyle};

pub fn draw_shape_fill_on_canvas(
    canvas: &Canvas,
    path: &skia_safe::Path,
    color: &Color,
    path_effects: &Vec<PathEffect>,
    offset: f64,
) -> Result<(), LibraryError> {
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_color(skia_safe::Color::from_argb(
        color.a, color.r, color.g, color.b,
    ));
    apply_path_effects(path_effects, &mut paint)?;

    if offset >= 0.0 {
        // Positive offset: Stroke and Fill to expand
        if offset > 0.0 {
            paint.set_style(PaintStyle::StrokeAndFill);
            paint.set_stroke_width((offset * 2.0) as f32);
            paint.set_stroke_join(skia_safe::paint::Join::Round);
        } else {
            paint.set_style(PaintStyle::Fill);
        }
        canvas.draw_path(path, &paint);
    } else {
        // Negative offset: Draw Fill, then Erase edges
        // 1. Draw original Fill
        paint.set_style(PaintStyle::Fill);
        canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default());
        canvas.draw_path(path, &paint);

        // 2. Erase (DstOut) the border stroke
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

pub fn draw_shape_stroke_on_canvas(
    canvas: &Canvas,
    path: &skia_safe::Path,
    color: &Color,
    path_effects: &Vec<PathEffect>,
    width: f64,
    offset: f64,
    cap: CapType,
    join: JoinType,
    miter: f64,
    dash_array: &Vec<f64>,
    dash_offset: f64,
) -> Result<(), LibraryError> {
    if width <= 0.0 {
        return Ok(());
    }

    // Prepare base stroke paint
    let mut stroke_paint = create_stroke_paint(color, width as f32, &cap, &join, miter as f32);

    // Path Effects (Dash + others)
    let mut effects_to_apply = Vec::new();
    if !dash_array.is_empty() {
        effects_to_apply.push(PathEffect::Dash {
            intervals: dash_array.clone(),
            phase: dash_offset,
        });
    }
    effects_to_apply.extend_from_slice(path_effects);

    if offset == 0.0 {
        // Standard Stroke
        stroke_paint.set_style(PaintStyle::Stroke);
        stroke_paint.set_stroke_width(width as f32);
        apply_path_effects(&effects_to_apply, &mut stroke_paint)?;
        canvas.draw_path(path, &stroke_paint);
        return Ok(());
    }

    // Offset Stroke Logic
    let outer_r = offset.abs() + width / 2.0;
    let inner_r = offset.abs() - width / 2.0;

    canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default()); // Isolate blending

    // Setup Clipping
    if offset > 0.0 {
        canvas.clip_path(path, skia_safe::ClipOp::Difference, true);
    } else {
        canvas.clip_path(path, skia_safe::ClipOp::Intersect, true);
    }

    // Apply path effects to paint before drawing
    apply_path_effects(&effects_to_apply, &mut stroke_paint)?;

    // Draw Outer (Base)
    stroke_paint.set_style(PaintStyle::Stroke);
    stroke_paint.set_stroke_width((outer_r * 2.0) as f32);
    canvas.draw_path(path, &stroke_paint);

    // Erase Inner (Hole)
    if inner_r > 0.0 {
        let mut erase_paint = stroke_paint.clone();
        erase_paint.set_blend_mode(skia_safe::BlendMode::DstOut);
        erase_paint.set_stroke_width((inner_r * 2.0) as f32);
        canvas.draw_path(path, &erase_paint);
    }

    canvas.restore();
    Ok(())
}
