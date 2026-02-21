use crate::error::LibraryError;
use crate::rendering::renderer::{RenderOutput, TextureInfo};
use crate::rendering::skia_utils::{GpuContext, surface_to_image};
use crate::runtime::color::Color;
use crate::runtime::draw_type::{CapType, JoinType, PathEffect};
use crate::runtime::transform::Transform;
use log::trace;
use skia_safe::path_effect::PathEffect as SkPathEffect;
use skia_safe::trim_path_effect::Mode;
use skia_safe::{Matrix, Paint, PaintStyle, Point, Surface};

pub(crate) fn build_transform_matrix(transform: &Transform) -> Matrix {
    let anchor = Point::new(transform.anchor.x as f32, transform.anchor.y as f32);
    let mut matrix = Matrix::new_identity();
    matrix.pre_translate((
        transform.position.x as f32 - anchor.x,
        transform.position.y as f32 - anchor.y,
    ));
    matrix.pre_rotate(transform.rotation as f32, anchor);
    matrix.pre_scale((transform.scale.x as f32, transform.scale.y as f32), anchor);
    matrix
}

pub(crate) fn create_stroke_paint(
    color: &Color,
    width: f32,
    cap: &CapType,
    join: &JoinType,
    miter: f32,
) -> Paint {
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_color(skia_safe::Color::from_argb(
        color.a, color.r, color.g, color.b,
    ));
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
    paint
}

pub(crate) fn snapshot_surface(
    surface: &mut Surface,
    gpu_context: &mut Option<GpuContext>,
    width: u32,
    height: u32,
) -> Result<RenderOutput, LibraryError> {
    if let Some(ctx) = gpu_context.as_mut() {
        ctx.direct_context.flush_and_submit();
        if let Some(texture) = skia_safe::gpu::surfaces::get_backend_texture(
            surface,
            skia_safe::surface::BackendHandleAccess::FlushRead,
        ) {
            if let Some(gl_info) = texture.gl_texture_info() {
                return Ok(RenderOutput::Texture(TextureInfo {
                    texture_id: gl_info.id,
                    width,
                    height,
                }));
            }
        }
    }

    let image = surface_to_image(surface, width, height)?;
    Ok(RenderOutput::Image(image))
}

fn convert_path_effect(path_effect: &PathEffect) -> Result<skia_safe::PathEffect, LibraryError> {
    match path_effect {
        PathEffect::Dash { intervals, phase } => {
            let intervals: Vec<f32> = intervals.iter().map(|&x| x as f32).collect();
            Ok(
                SkPathEffect::dash(&intervals, *phase as f32).ok_or(LibraryError::render(
                    "Failed to create PathEffect".to_string(),
                ))?,
            )
        }
        PathEffect::Corner { radius } => Ok(SkPathEffect::corner_path(*radius as f32).ok_or(
            LibraryError::render("Failed to create PathEffect".to_string()),
        )?),
        PathEffect::Discrete {
            seg_length,
            deviation,
            seed,
        } => Ok(
            SkPathEffect::discrete(*seg_length as f32, *deviation as f32, *seed as u32).ok_or(
                LibraryError::render("Failed to create PathEffect".to_string()),
            )?,
        ),
        PathEffect::Trim { start, end } => {
            Ok(
                SkPathEffect::trim(*start as f32, *end as f32, Mode::Normal).ok_or(
                    LibraryError::render("Failed to create PathEffect".to_string()),
                )?,
            )
        }
    }
}

pub(crate) fn apply_path_effects(
    path_effects: &Vec<PathEffect>,
    paint: &mut Paint,
) -> Result<(), LibraryError> {
    if !path_effects.is_empty() {
        let mut composed_effect: Option<skia_safe::PathEffect> = None;
        for effect in path_effects {
            trace!("Applying path effect {:?}", effect);
            match convert_path_effect(effect) {
                Ok(sk_path_effect) => {
                    composed_effect = match composed_effect {
                        Some(e) => Some(SkPathEffect::compose(e, sk_path_effect)),
                        None => Some(sk_path_effect),
                    };
                }
                Err(e) => {
                    log::warn!("Failed to apply path effect {:?}: {}", effect, e);
                }
            }
        }
        if let Some(composed) = composed_effect {
            paint.set_path_effect(composed);
        }
    }
    Ok(())
}
