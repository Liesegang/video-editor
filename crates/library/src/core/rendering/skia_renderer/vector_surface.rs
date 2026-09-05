//! Physical allocation for transient backend-native vector layers.
//!
//! The authored geometry and masks stay in local coordinates. Direct drawing
//! uses a tight, target-clipped Surface whose integer origin is translated out
//! of its Canvas. Raster boundaries keep the complete current target because
//! external Image Effects intentionally operate in target coordinates.

use skia_safe::{Point, Rect, Surface};

use super::{Affine2D, LibraryError, SkiaRenderer};

const DEVICE_PIXEL_GUARD: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VectorSurfaceMode {
    Target,
    Content,
}

pub(super) struct NativeLayer {
    pub(super) surface: Surface,
    pub(super) origin: Point,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SurfaceRegion {
    left: u32,
    top: u32,
    width: u32,
    height: u32,
}

impl SkiaRenderer {
    pub(super) fn create_vector_surface(
        &mut self,
        mode: VectorSurfaceMode,
        local_visual: Rect,
        transform: Affine2D,
    ) -> Result<NativeLayer, LibraryError> {
        let target = self.current_target_dimensions();
        let region = match mode {
            VectorSurfaceMode::Target => SurfaceRegion {
                left: 0,
                top: 0,
                width: target.0.max(1),
                height: target.1.max(1),
            },
            VectorSurfaceMode::Content => resolve_content_region(local_visual, transform, target)?,
        };
        let mut surface = self.create_layer_surface(region.width, region.height)?;
        surface.canvas().clear(skia_safe::Color::TRANSPARENT);
        let origin = Point::new(region.left as f32, region.top as f32);
        surface.canvas().translate((-origin.x, -origin.y));
        Ok(NativeLayer { surface, origin })
    }
}

fn resolve_content_region(
    local_visual: Rect,
    transform: Affine2D,
    target: (u32, u32),
) -> Result<SurfaceRegion, LibraryError> {
    let local = [
        (local_visual.left as f64, local_visual.top as f64),
        (local_visual.right as f64, local_visual.top as f64),
        (local_visual.right as f64, local_visual.bottom as f64),
        (local_visual.left as f64, local_visual.bottom as f64),
    ];
    if local
        .iter()
        .flat_map(|(x, y)| [x, y])
        .any(|value| !value.is_finite())
    {
        return Err(LibraryError::Render(
            "Vector layer has non-finite local bounds".to_string(),
        ));
    }
    if target.0 == 0 || target.1 == 0 || local_visual.is_empty() {
        return Ok(transparent_region());
    }

    let mapped = local.map(|(x, y)| transform.map_point(x, y));
    if mapped
        .iter()
        .flat_map(|(x, y)| [x, y])
        .any(|value| !value.is_finite())
    {
        return Err(LibraryError::Render(
            "Vector layer transform produced non-finite target bounds".to_string(),
        ));
    }

    let min_x = mapped
        .iter()
        .map(|point| point.0)
        .fold(f64::INFINITY, f64::min);
    let min_y = mapped
        .iter()
        .map(|point| point.1)
        .fold(f64::INFINITY, f64::min);
    let max_x = mapped
        .iter()
        .map(|point| point.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let max_y = mapped
        .iter()
        .map(|point| point.1)
        .fold(f64::NEG_INFINITY, f64::max);
    let target_width = target.0 as f64;
    let target_height = target.1 as f64;
    let left = (min_x.floor() - DEVICE_PIXEL_GUARD).clamp(0.0, target_width);
    let top = (min_y.floor() - DEVICE_PIXEL_GUARD).clamp(0.0, target_height);
    let right = (max_x.ceil() + DEVICE_PIXEL_GUARD).clamp(0.0, target_width);
    let bottom = (max_y.ceil() + DEVICE_PIXEL_GUARD).clamp(0.0, target_height);
    if right <= left || bottom <= top {
        return Ok(transparent_region());
    }

    let left = left as u32;
    let top = top as u32;
    let right = right as u32;
    let bottom = bottom as u32;
    Ok(SurfaceRegion {
        left,
        top,
        width: right - left,
        height: bottom - top,
    })
}

const fn transparent_region() -> SurfaceRegion {
    SurfaceRegion {
        left: 0,
        top: 0,
        width: 1,
        height: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_region_is_integer_aligned_guarded_and_target_clipped() {
        let region = resolve_content_region(
            Rect::new(10.25, 20.5, 30.75, 40.25),
            Affine2D::IDENTITY,
            (100, 80),
        )
        .expect("content region");
        assert_eq!(
            region,
            SurfaceRegion {
                left: 8,
                top: 18,
                width: 25,
                height: 25,
            }
        );

        let clipped = resolve_content_region(
            Rect::new(-20.0, -10.0, 8.0, 9.0),
            Affine2D::IDENTITY,
            (100, 80),
        )
        .expect("clipped content region");
        assert_eq!(
            clipped,
            SurfaceRegion {
                left: 0,
                top: 0,
                width: 10,
                height: 11,
            }
        );
    }

    #[test]
    fn content_region_maps_all_affine_corners_before_rounding() {
        let region = resolve_content_region(
            Rect::new(2.0, 3.0, 12.0, 9.0),
            Affine2D {
                scale_x: 1.5,
                skew_x: -0.5,
                translate_x: 20.0,
                skew_y: 0.25,
                scale_y: 0.75,
                translate_y: 10.0,
            },
            (100, 80),
        )
        .expect("affine content region");
        assert_eq!(
            region,
            SurfaceRegion {
                left: 16,
                top: 10,
                width: 23,
                height: 12,
            }
        );
    }

    #[test]
    fn empty_and_offscreen_content_use_a_bounded_transparent_surface() {
        assert_eq!(
            resolve_content_region(Rect::new_empty(), Affine2D::IDENTITY, (100, 80))
                .expect("empty region"),
            transparent_region()
        );
        assert_eq!(
            resolve_content_region(
                Rect::new(2.0, 3.0, 12.0, 9.0),
                Affine2D::translate(500.0, -400.0),
                (100, 80),
            )
            .expect("offscreen region"),
            transparent_region()
        );
    }

    #[test]
    fn non_finite_content_bounds_are_rejected() {
        let error = resolve_content_region(
            Rect::new(0.0, 0.0, f32::INFINITY, 10.0),
            Affine2D::IDENTITY,
            (100, 80),
        )
        .expect_err("non-finite local bounds must fail");
        assert!(error.to_string().contains("non-finite local bounds"));

        let error = resolve_content_region(
            Rect::new(f32::NAN, 0.0, 10.0, 10.0),
            Affine2D::IDENTITY,
            (100, 80),
        )
        .expect_err("NaN local bounds must fail before the empty check");
        assert!(error.to_string().contains("non-finite local bounds"));

        let error = resolve_content_region(
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Affine2D {
                translate_x: f64::NAN,
                ..Affine2D::IDENTITY
            },
            (100, 80),
        )
        .expect_err("non-finite mapped bounds must fail");
        assert!(error.to_string().contains("non-finite target bounds"));
    }
}
