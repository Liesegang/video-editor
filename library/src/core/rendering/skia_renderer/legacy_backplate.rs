//! Frozen paint-time behavior for externally built ABI-v1 Decorators.
//!
//! New built-in and ABI-v2 Backplates are resolved to geometry before Style
//! and never enter this module.

use skia_safe::{Canvas, Paint, Path, Rect};

use crate::core::ensemble::decorators::{BackplateShape, BackplateTarget};
use crate::core::ensemble::types::{DecoratorConfig, TransformData};
use crate::error::LibraryError;
use crate::model::frame::runtime_shape::{RuntimeTextShape, transformed_text_element_bounds};

pub(super) fn draw_text_backplates(
    canvas: &Canvas,
    text: &RuntimeTextShape,
    transforms: &[TransformData],
    decorators: &[DecoratorConfig],
) -> Result<(), LibraryError> {
    for decorator in decorators {
        let DecoratorConfig::LegacyBackplate {
            target,
            shape,
            color,
            padding,
            corner_radius,
        } = decorator
        else {
            return Err(LibraryError::Render(
                "geometry-only Backplate reached the paint-time renderer".to_string(),
            ));
        };
        match target {
            BackplateTarget::Char => {
                for (element, transform) in text.elements.iter().zip(transforms) {
                    if transform.opacity <= 0.0 {
                        continue;
                    }
                    let center = super::Point::new(
                        element.bounds.left + element.advance / 2.0,
                        (element.bounds.top + element.bounds.bottom) / 2.0,
                    );
                    canvas.save();
                    canvas.translate((center.x, center.y));
                    canvas.translate(transform.translate);
                    canvas.rotate(transform.rotate, None);
                    canvas.scale(transform.scale);
                    canvas.translate((-center.x, -center.y));
                    draw_backplate(
                        canvas,
                        padded_rect(
                            Rect::new(
                                element.bounds.left,
                                element.bounds.top,
                                element.bounds.right,
                                element.bounds.bottom,
                            ),
                            *padding,
                        ),
                        *shape,
                        color,
                        *corner_radius,
                        transform.opacity,
                    );
                    canvas.restore();
                }
            }
            BackplateTarget::Line => {
                for line in &text.lines {
                    let indices = line.element_range.clone().collect::<Vec<_>>();
                    if let Some(bounds) = union_text_bounds(text, transforms, &indices) {
                        let opacity = indices
                            .iter()
                            .map(|index| transforms[*index].opacity)
                            .sum::<f32>()
                            / indices.len().max(1) as f32;
                        draw_backplate(
                            canvas,
                            padded_rect(bounds, *padding),
                            *shape,
                            color,
                            *corner_radius,
                            opacity,
                        );
                    }
                }
            }
            BackplateTarget::Block => {
                let indices = (0..text.elements.len()).collect::<Vec<_>>();
                if let Some(bounds) = union_text_bounds(text, transforms, &indices) {
                    let opacity = transforms
                        .iter()
                        .map(|transform| transform.opacity)
                        .sum::<f32>()
                        / transforms.len().max(1) as f32;
                    draw_backplate(
                        canvas,
                        padded_rect(bounds, *padding),
                        *shape,
                        color,
                        *corner_radius,
                        opacity,
                    );
                }
            }
            BackplateTarget::Parts => {
                return Err(LibraryError::Render(
                    "Ensemble BackplateTarget::Parts is not supported".to_string(),
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn draw_path_backplates(
    canvas: &Canvas,
    path: &Path,
    decorators: &[DecoratorConfig],
) -> Result<(), LibraryError> {
    let bounds = path.compute_tight_bounds();
    for decorator in decorators {
        let DecoratorConfig::LegacyBackplate {
            target,
            shape,
            color,
            padding,
            corner_radius,
        } = decorator
        else {
            return Err(LibraryError::Render(
                "geometry-only Backplate reached the paint-time renderer".to_string(),
            ));
        };
        if *target == BackplateTarget::Parts {
            return Err(LibraryError::Render(
                "Ensemble BackplateTarget::Parts is not supported".to_string(),
            ));
        }
        draw_backplate(
            canvas,
            padded_rect(bounds, *padding),
            *shape,
            color,
            *corner_radius,
            1.0,
        );
    }
    Ok(())
}

fn union_text_bounds(
    text: &RuntimeTextShape,
    transforms: &[TransformData],
    indices: &[usize],
) -> Option<Rect> {
    indices
        .iter()
        .map(|index| {
            let bounds =
                transformed_text_element_bounds(&text.elements[*index], &transforms[*index]);
            Rect::new(bounds.left, bounds.top, bounds.right, bounds.bottom)
        })
        .reduce(|current, next| {
            Rect::new(
                current.left.min(next.left),
                current.top.min(next.top),
                current.right.max(next.right),
                current.bottom.max(next.bottom),
            )
        })
}

fn padded_rect(bounds: Rect, padding: (f32, f32, f32, f32)) -> Rect {
    Rect::new(
        bounds.left - padding.3,
        bounds.top - padding.0,
        bounds.right + padding.1,
        bounds.bottom + padding.2,
    )
}

fn draw_backplate(
    canvas: &Canvas,
    bounds: Rect,
    shape: BackplateShape,
    color: &crate::model::frame::color::Color,
    corner_radius: f32,
    opacity: f32,
) {
    let mut paint = Paint::default();
    paint.set_color(skia_safe::Color::from_argb(
        (f32::from(color.a) * opacity).clamp(0.0, 255.0) as u8,
        color.r,
        color.g,
        color.b,
    ));
    paint.set_anti_alias(true);
    match shape {
        BackplateShape::Rect => canvas.draw_rect(bounds, &paint),
        BackplateShape::RoundedRect => canvas.draw_rrect(
            skia_safe::RRect::new_rect_xy(bounds, corner_radius, corner_radius),
            &paint,
        ),
        BackplateShape::Circle => canvas.draw_circle(
            (bounds.center_x(), bounds.center_y()),
            (bounds.width().min(bounds.height()) * 0.5).max(0.0),
            &paint,
        ),
    };
}
