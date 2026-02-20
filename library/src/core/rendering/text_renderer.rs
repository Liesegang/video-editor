use super::paint_utils::build_transform_matrix;
use crate::core::ensemble::EnsembleData;
use crate::core::ensemble::decorators::{BackplateShape, BackplateTarget};
use crate::core::ensemble::types::{DecoratorConfig, EffectorConfig, TransformData};
use crate::model::frame::draw_type::DrawStyle;
use crate::model::frame::entity::StyleConfig;
use crate::model::frame::transform::Transform;
use skia_safe::{Canvas, Paint};

/// Render text with ensemble effectors and decorators onto a canvas.
///
/// This draws the ensemble-processed text (with per-character transforms,
/// effectors, and decorators) onto the given canvas. The caller is responsible
/// for surface creation and snapshot.
pub(crate) fn render_ensemble_text(
    canvas: &Canvas,
    text: &str,
    size: f64,
    font_name: &str,
    styles: &[StyleConfig],
    ensemble_data: &EnsembleData,
    transform: &Transform,
    current_time: f32,
) {
    log::debug!(
        "Ensemble rendering: {} effectors, {} decorators",
        ensemble_data.effector_configs.len(),
        ensemble_data.decorator_configs.len()
    );

    let matrix = build_transform_matrix(transform);
    canvas.save();
    canvas.concat(&matrix);

    // Create font
    let font_mgr = skia_safe::FontMgr::default();
    let typeface = font_mgr
        .match_family_style(font_name, skia_safe::FontStyle::default())
        .unwrap_or_else(|| {
            font_mgr
                .legacy_make_typeface(None, skia_safe::FontStyle::default())
                .unwrap()
        });
    let font = skia_safe::Font::from_typeface(typeface, size as f32);

    // Get font metrics for accurate baseline positioning
    let (_, metrics) = font.metrics();
    // metrics.ascent is negative (distance above baseline), so negate it
    let baseline_offset = -metrics.ascent;

    // Get base color from first style
    let base_color = if let Some(config) = styles.first() {
        match &config.style {
            DrawStyle::Fill { color, .. } => color.clone(),
            DrawStyle::Stroke { color, .. } => color.clone(),
        }
    } else {
        crate::model::frame::color::Color {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        }
    };

    // Text decomposition: measure each character
    let mut char_data = Vec::new();
    let mut x_pos = 0.0f32;

    for ch in text.chars() {
        let ch_str = ch.to_string();
        let (advance, _bounds) = font.measure_str(&ch_str, None);

        // Store char data
        char_data.push((ch, x_pos, advance));
        x_pos += advance;
    }

    let total_chars = char_data.len();

    // Apply effectors to build transform data for each char
    let mut char_transforms: Vec<TransformData> = vec![TransformData::identity(); total_chars];

    // Apply each effector
    for effector_config in &ensemble_data.effector_configs {
        match effector_config {
            EffectorConfig::Transform {
                translate,
                rotate,
                scale,
                ..
            } => {
                // Apply uniform transform to all chars
                for transform_data in &mut char_transforms {
                    transform_data.translate.0 += translate.0;
                    transform_data.translate.1 += translate.1;
                    transform_data.rotate += rotate;
                    transform_data.scale.0 *= scale.0;
                    transform_data.scale.1 *= scale.1;
                }
            }
            EffectorConfig::StepDelay {
                delay_per_element,
                duration,
                from_opacity,
                to_opacity,
                ..
            } => {
                // Apply step delay: animate opacity per character based on time
                for (i, transform_data) in char_transforms.iter_mut().enumerate() {
                    // Calculate when this character's animation starts
                    let char_start_time = i as f32 * delay_per_element;

                    // Calculate animation progress for this character
                    let progress = if current_time < char_start_time {
                        0.0 // Animation hasn't started yet
                    } else if current_time > char_start_time + duration {
                        1.0 // Animation completed
                    } else {
                        // Animation in progress
                        (current_time - char_start_time) / duration
                    };

                    // Interpolate opacity based on progress
                    let opacity = from_opacity + (to_opacity - from_opacity) * progress;
                    transform_data.opacity *= opacity / 100.0;
                }
            }
            EffectorConfig::Opacity { target_opacity, .. } => {
                // Apply uniform opacity
                for transform_data in &mut char_transforms {
                    transform_data.opacity *= target_opacity / 100.0;
                }
            }
            EffectorConfig::Randomize {
                translate_range,
                rotate_range,
                scale_range: _scale_range, // TODO: Implement scale randomization
                seed,
                ..
            } => {
                // Simple pseudo-random based on seed and index
                for (i, transform_data) in char_transforms.iter_mut().enumerate() {
                    let hash = (seed.wrapping_mul(31).wrapping_add(i as u64)) as f32;
                    let rand_tx = ((hash * 12.9898).sin() * 43758.5453).fract();
                    let rand_ty = ((hash * 78.233).sin() * 43758.5453).fract();
                    let rand_rot = ((hash * 39.123).sin() * 43758.5453).fract();

                    transform_data.translate.0 += (rand_tx - 0.5) * translate_range.0 * 2.0;
                    transform_data.translate.1 += (rand_ty - 0.5) * translate_range.1 * 2.0;
                    transform_data.rotate += (rand_rot - 0.5) * rotate_range * 2.0;
                }
            }
        }
    }

    // Apply patches (character-level overrides)
    for (index, patch) in &ensemble_data.patches {
        if *index < char_transforms.len() {
            char_transforms[*index] = char_transforms[*index].combine(patch);
        }
    }

    // Render decorators (backplate)
    for decorator_config in &ensemble_data.decorator_configs {
        log::warn!(
            "DEBUG: Renderer processing decorator: {:?}",
            decorator_config
        );
        match decorator_config {
            DecoratorConfig::Backplate {
                target,
                shape,
                color,
                padding,
                corner_radius,
            } => {
                log::warn!(
                    "DEBUG: Rendering Backplate - Target: {:?}, Shape: {:?}, Color: {:?}",
                    target,
                    shape,
                    color
                );

                // Helper function to draw a single backplate
                let draw_backplate = |canvas: &Canvas, rect: skia_safe::Rect| {
                    log::warn!("DEBUG: Drawing Backplate Rect: {:?}", rect);
                    let mut paint = Paint::default();
                    paint.set_color(skia_safe::Color::from_argb(
                        color.a, color.r, color.g, color.b,
                    ));
                    paint.set_anti_alias(true);

                    match shape {
                        BackplateShape::Rect => {
                            canvas.draw_rect(rect, &paint);
                        }
                        BackplateShape::RoundedRect => {
                            let rrect =
                                skia_safe::RRect::new_rect_xy(rect, *corner_radius, *corner_radius);
                            canvas.draw_rrect(rrect, &paint);
                        }
                        BackplateShape::Circle => {
                            // Draw circle centered in rect
                            let center_x = rect.center_x();
                            let center_y = rect.center_y();
                            let radius = (rect.width().min(rect.height()) / 2.0).max(0.0);
                            canvas.draw_circle((center_x, center_y), radius, &paint);
                        }
                    }
                };

                match target {
                    BackplateTarget::Char => {
                        // Draw backplate for each character individually
                        for (i, (_ch, base_x, advance)) in char_data.iter().enumerate() {
                            if let Some(ch_transform) = char_transforms.get(i) {
                                canvas.save();

                                // Apply same transform logic as character rendering
                                let char_center_x = base_x + (size as f32 / 2.0);
                                let char_center_y = 0.0; // Baseline is 0 in local coords

                                // Translate to character center
                                canvas.translate((char_center_x, char_center_y));
                                // Apply effector transforms
                                canvas.translate((
                                    ch_transform.translate.0,
                                    ch_transform.translate.1,
                                ));
                                canvas.rotate(ch_transform.rotate, None);
                                canvas.scale((ch_transform.scale.0, ch_transform.scale.1));
                                // Translate back
                                canvas.translate((-char_center_x, -char_center_y));

                                // Calculate bounds relative to baseline
                                let top = baseline_offset + metrics.ascent;
                                let bottom = baseline_offset + metrics.descent;

                                let char_rect = skia_safe::Rect::from_xywh(
                                    *base_x - padding.3,
                                    top - padding.0,
                                    *advance + padding.1 + padding.3,
                                    (bottom - top) + padding.0 + padding.2,
                                );
                                draw_backplate(canvas, char_rect);

                                canvas.restore();
                            }
                        }
                    }
                    BackplateTarget::Block | BackplateTarget::Line => {
                        // Draw backplate for entire text
                        let total_width = x_pos;
                        let top = baseline_offset + metrics.ascent;
                        let bottom = baseline_offset + metrics.descent;

                        let backplate_rect = skia_safe::Rect::from_xywh(
                            -padding.0,
                            top - padding.0,
                            total_width + padding.0 + padding.2,
                            (bottom - top) + padding.0 + padding.2,
                        );
                        draw_backplate(canvas, backplate_rect);
                    }
                    BackplateTarget::Parts => {
                        // TODO: Parts target for advanced word/sentence grouping
                        // For now, fall back to Block
                        let total_width = x_pos;
                        let top = baseline_offset + metrics.ascent;
                        let bottom = baseline_offset + metrics.descent;

                        let backplate_rect = skia_safe::Rect::from_xywh(
                            -padding.0,
                            top - padding.0,
                            total_width + padding.0 + padding.2,
                            (bottom - top) + padding.0 + padding.2,
                        );
                        draw_backplate(canvas, backplate_rect);
                    }
                }
            }
        }
    }

    // Render each character with its transform
    for (i, (ch, base_x, _advance)) in char_data.iter().enumerate() {
        let ch_transform = &char_transforms[i];

        // Apply character transform
        canvas.save();

        let char_center_x = base_x + size as f32 / 2.0;
        let char_center_y = 0.0;

        // Translate to character center
        canvas.translate((char_center_x, char_center_y));
        // Apply effector transforms
        canvas.translate((ch_transform.translate.0, ch_transform.translate.1));
        canvas.rotate(ch_transform.rotate, None);
        canvas.scale((ch_transform.scale.0, ch_transform.scale.1));
        // Translate back
        canvas.translate((-char_center_x, -char_center_y));

        // Create paint with opacity
        let mut paint = Paint::default();
        let final_alpha = (base_color.a as f32 * ch_transform.opacity).clamp(0.0, 255.0) as u8;
        paint.set_color(skia_safe::Color::from_argb(
            final_alpha,
            base_color.r,
            base_color.g,
            base_color.b,
        ));
        paint.set_anti_alias(true);

        // Draw character
        let ch_str = ch.to_string();
        // Use baseline_offset for accurate positioning to match standard text rendering
        canvas.draw_str(&ch_str, (*base_x, baseline_offset), &font, &paint);

        canvas.restore();
    }

    canvas.restore();
}
